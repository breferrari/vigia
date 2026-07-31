//! The watch and coalesce engine: I1.
//!
//! > Redraw is event-driven, never a fixed timer. No filesystem event and no
//! > git index change means no work.
//!
//! The whole invariant reduces to one line of code, [`Watcher::next_tick`]'s
//! unbounded `recv()`. An idle monitor is a blocked thread, not a sleeping one,
//! so "zero wakeups" is a property of the design rather than a number to tune
//! towards. Every timeout in this module is inside a burst, never outside one.
//!
//! Two things stop that from being enough on its own. A recursive watch over a
//! Rust worktree sees every write `cargo build` makes to `target/`, so events
//! are filtered against the same gitignore rules the diff uses. And an agent
//! saving twelve files produces twelve events for one logical change, so
//! accepted events are coalesced into a single tick.
//!
//! A tick also **names the files written in it**, which is what follow mode and
//! the glance history both read (`SPEC.md` §11.1, I5 and I10). That is not extra
//! work: the gitignore filter already resolves every event against the worktree
//! root, so the paths are a by-product of a decision this module was making
//! anyway. The alternative was deriving recency by `stat`-ing every changed
//! file, which is the cost [#19](https://github.com/breferrari/vigia/issues/19)
//! records as breaching I9 at scale, for an answer the event was carrying all
//! along.
//!
//! The set a burst reports is **capped**, at the same [`HISTORY_PATHS`] that
//! bounds the store it feeds. A bulk operation can put ten thousand writes
//! inside one hundred-millisecond burst, and a tick that carried all of them
//! would hand I10 a bound to enforce after the allocation had already happened.
//! What the cap refuses is counted rather than dropped silently.

use std::collections::HashSet;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use notify::{EventKind, RecursiveMode, Watcher as _};

use crate::error::{Error, Result};
use crate::history::HISTORY_PATHS;

/// How the watch loop folds a burst of events into one refresh.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WatchOptions {
    /// How long the tree must be quiet before a burst counts as finished.
    ///
    /// Charged only once per burst, so it is latency on the first frame after
    /// an edit and nothing at all while idle.
    pub quiet: Duration,
    /// The longest a tick may be held back while events keep arriving.
    ///
    /// Without this, a process writing continuously would starve the display
    /// forever, since the tree would never fall quiet.
    pub max_delay: Duration,
}

impl Default for WatchOptions {
    fn default() -> Self {
        Self {
            // One frame. Long enough to fold a multi-file save, short enough
            // to stay under the eye's threshold for "instant".
            quiet: Duration::from_millis(16),
            max_delay: Duration::from_millis(100),
        }
    }
}

/// A coalesced signal that the working tree changed.
///
/// Not `Copy`, because it owns the paths. One `Vec` per burst, holding paths
/// that only exist while something is being edited, and never more than
/// [`HISTORY_PATHS`] of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tick {
    /// Accepted events folded into this tick.
    pub events: u32,
    /// How long the tick was held back to coalesce.
    pub coalesced_for: Duration,
    /// The distinct files written in this tick, spelled the way
    /// [`crate::FileChange::path`] spells them.
    ///
    /// **The write that landed last is the last element**, which is what
    /// [`Tick::newest`] reads and what I5 follows. The order of everything
    /// before it is unspecified: I10 bumps a bucket per path and does not care
    /// which order they arrive in, so imposing one would be a promise with no
    /// reader and a sort with no purpose.
    ///
    /// Empty when the burst wrote no file the view could move to, which is an
    /// ordinary outcome rather than a failure: a write to `.git/index` is a real
    /// change and produces a real tick, because the index is the left-hand side
    /// of every diff drawn, but it is not somewhere to scroll.
    pub paths: Vec<String>,
    /// How many further paths this burst touched past [`HISTORY_PATHS`].
    ///
    /// Reported rather than dropped quietly. A bulk operation is exactly when a
    /// reader most wants to know the picture is partial, and a cap that lied by
    /// omission would make the sparkline understate the busiest moment a session
    /// ever has.
    pub dropped: u32,
}

impl Tick {
    /// The write that landed **last** in this tick.
    ///
    /// This is what I5 follows, and `SPEC.md` §11.1 is the rule it obeys. It is
    /// the last element of [`Tick::paths`] rather than a field of its own,
    /// because two fields holding one fact are two things that can disagree, and
    /// the one that would be wrong is the one the viewport jumps to.
    ///
    /// `None` for a tick that named no followable file. See [`Tick::paths`].
    pub fn newest(&self) -> Option<&str> {
        self.paths.last().map(String::as_str)
    }
}

/// The paths accumulated while a burst is still being coalesced.
///
/// A set rather than a list, because the same file saved four times inside one
/// burst is one change to the reader and I10 samples per tick. The write that
/// landed last is tracked alongside it, and is the one path the cap may never
/// refuse: it is where the viewport is about to move.
#[derive(Debug, Default)]
struct Burst {
    seen: HashSet<String>,
    newest: Option<String>,
    dropped: u32,
}

impl Burst {
    /// Record a followable path, which by construction is the newest so far.
    fn push(&mut self, path: String) {
        if !self.seen.contains(&path) {
            if self.seen.len() >= HISTORY_PATHS {
                // Displace something rather than refuse this one. Refusing
                // would eventually refuse the newest path, and losing that is
                // losing follow mode's answer at the exact moment it had one.
                // Which path goes is arbitrary and says so: past the cap this
                // is a bulk operation, where no individual membership is
                // information a reader could act on.
                let victim = self.seen.iter().next().cloned();
                if let Some(victim) = victim {
                    self.seen.remove(&victim);
                }
                self.dropped += 1;
            }
            self.seen.insert(path.clone());
        }
        self.newest = Some(path);
    }

    /// The paths, with the newest moved to the end.
    fn finish(mut self) -> (Vec<String>, u32) {
        let Some(newest) = self.newest else {
            return (self.seen.into_iter().collect(), self.dropped);
        };
        self.seen.remove(&newest);
        let mut paths: Vec<String> = self.seen.into_iter().collect();
        paths.push(newest);
        (paths, self.dropped)
    }
}

/// What one accepted message meant.
///
/// Two answers rather than one, because they are independent: an index write
/// is relevant and unfollowable, and both facts have to survive the call.
#[derive(Debug, Default)]
struct Accepted {
    /// The event named something the display depends on.
    relevant: bool,
    /// Where the view should move, if this event said.
    newest: Option<String>,
}

/// Counters, so I1 can be asserted rather than believed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WatchStats {
    /// Times the loop woke because a message arrived.
    pub wakeups: u64,
    /// Events discarded before they could cost a redraw.
    pub filtered: u64,
    /// Ticks emitted.
    pub ticks: u64,
}

/// What travels on the watch channel.
enum Message {
    Event(Box<notify::Event>),
    /// Sent by a [`Stop`] handle to unblock `next_tick` from another thread.
    Stop,
}

/// Unblocks a [`Watcher`] that is waiting.
///
/// A monitor blocks indefinitely by design, so quitting needs a way in from
/// another thread. Cloneable and cheap; sending on a dead channel is a no-op.
#[derive(Clone)]
pub struct Stop {
    tx: Sender<Message>,
}

impl Stop {
    /// Wake the watcher and make its current `next_tick` return `None`.
    pub fn stop(&self) {
        let _ = self.tx.send(Message::Stop);
    }
}

/// Watches a working tree and emits one tick per burst of real change.
pub struct Watcher<'repo> {
    rx: Receiver<Message>,
    tx: Sender<Message>,
    /// Dropping this stops the OS watch, so it must outlive the receiver.
    _backend: notify::RecommendedWatcher,
    excludes: gix::AttributeStack<'repo>,
    /// Prefixes an event path may carry for the same worktree. See [`roots_of`].
    roots: Vec<PathBuf>,
    options: WatchOptions,
    stats: WatchStats,
    delivered: Arc<AtomicU64>,
}

impl<'repo> Watcher<'repo> {
    pub(crate) fn new(
        repo: &'repo gix::Repository,
        workdir: &Path,
        options: WatchOptions,
    ) -> Result<Self> {
        // Everything that reads the repository happens before the watch is
        // armed, so the watcher never observes its own construction. Reading
        // `.git/index` and the gitignore files is enough to register on
        // backends that report reads or attribute touches, and Linux and macOS
        // both do.
        //
        // An empty index is the correct fallback: a repository with no commits
        // still has gitignore files, and they are what the filter needs.
        let index = repo
            .index_or_empty()
            .map_err(|e| Error::Watch(Box::new(e)))?;
        let excludes = repo
            .excludes(
                &index,
                None,
                gix::worktree::stack::state::ignore::Source::WorktreeThenIdMappingIfNotSkipped,
            )
            .map_err(|e| Error::Watch(Box::new(e)))?;

        let roots = roots_of(workdir);

        let (tx, rx) = mpsc::channel();

        let delivered = Arc::new(AtomicU64::new(0));
        let counter = Arc::clone(&delivered);
        let sender = tx.clone();
        let mut backend = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            counter.fetch_add(1, Ordering::Relaxed);
            // A watch error is not worth killing a monitor over. Dropping
            // it costs at most one missed frame, and the next real event
            // resynchronises us.
            if let Ok(event) = res {
                let _ = sender.send(Message::Event(Box::new(event)));
            }
        })
        .map_err(|e| Error::Watch(Box::new(e)))?;

        backend
            .watch(workdir, RecursiveMode::Recursive)
            .map_err(|e| Error::Watch(Box::new(e)))?;

        Ok(Self {
            rx,
            tx,
            _backend: backend,
            excludes,
            roots,
            options,
            stats: WatchStats::default(),
            delivered,
        })
    }

    /// A handle that can wake this watcher from another thread.
    pub fn stopper(&self) -> Stop {
        Stop {
            tx: self.tx.clone(),
        }
    }

    /// Counters for the loop itself.
    pub fn stats(&self) -> WatchStats {
        self.stats
    }

    /// Raw events the OS has delivered since the watcher started, accepted and
    /// filtered alike.
    ///
    /// This is a non-vacuity guard, not a measure of idle cost. A test that
    /// asserts some class of write produces no tick needs to know the OS
    /// reported the write at all, or it would pass just as happily against a
    /// watcher that was silently broken.
    ///
    /// It is deliberately not asserted to be zero on an idle tree: inotify and
    /// FSEvents both report reads and attribute touches, so a quiet tree is not
    /// a silent one, and I1 is a claim about work done rather than about
    /// packets received.
    pub fn delivered(&self) -> u64 {
        self.delivered.load(Ordering::Relaxed)
    }

    /// Block until the working tree changes, then return one coalesced tick.
    ///
    /// Returns `None` once a [`Stop`] handle fires or the backend goes away.
    ///
    /// The first `recv()` has no timeout, and that is the whole of I1. Giving
    /// it one, however generous, would turn this into a polling loop and make
    /// the idle cost non-zero.
    pub fn next_tick(&mut self) -> Option<Tick> {
        loop {
            let message = self.rx.recv().ok()?;
            self.stats.wakeups += 1;
            let accepted = self.accept(message)?;
            if !accepted.relevant {
                continue;
            }

            let started = Instant::now();
            let hard_deadline = started + self.options.max_delay;
            let mut quiet_until = started + self.options.quiet;
            let mut events = 1;
            let mut burst = Burst::default();
            if let Some(path) = accepted.newest {
                burst.push(path);
            }

            // Inside a burst, and only inside it, waiting is bounded.
            loop {
                let now = Instant::now();
                let wake_at = quiet_until.min(hard_deadline);
                let Some(wait) = wake_at.checked_duration_since(now) else {
                    break;
                };

                match self.rx.recv_timeout(wait) {
                    Ok(message) => {
                        self.stats.wakeups += 1;
                        match self.accept(message) {
                            // Stopped mid-burst: report what we already have,
                            // so the caller draws before it shuts down.
                            None => break,
                            Some(accepted) if accepted.relevant => {
                                events += 1;
                                // Only an event that named a file adds one. An
                                // agent that edits and then stages ends its
                                // burst on an index write, and letting that
                                // blank the target would lose follow mode's
                                // answer at the exact moment it had one.
                                if let Some(path) = accepted.newest {
                                    burst.push(path);
                                }
                                quiet_until = Instant::now() + self.options.quiet;
                            }
                            Some(_) => {}
                        }
                    }
                    Err(RecvTimeoutError::Timeout) => break,
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            }

            self.stats.ticks += 1;
            let (paths, dropped) = burst.finish();
            return Some(Tick {
                events,
                coalesced_for: started.elapsed(),
                paths,
                dropped,
            });
        }
    }

    /// `None` means stop.
    fn accept(&mut self, message: Message) -> Option<Accepted> {
        let event = match message {
            Message::Stop => return None,
            Message::Event(event) => event,
        };

        // Reads are not changes. Most backends never report them, but inotify
        // can be configured to, and a monitor that redraws because something
        // read a file would be absurd.
        if matches!(event.kind, EventKind::Access(_)) {
            self.stats.filtered += 1;
            return Some(Accepted::default());
        }

        let accepted = accept_paths(&event.paths, |path| self.relative(path));
        if !accepted.relevant {
            self.stats.filtered += 1;
        }
        Some(accepted)
    }

    /// This event's path relative to the worktree, or `None` when nothing the
    /// display depends on is behind it.
    ///
    /// Returns the path rather than a bool so the caller can both decide
    /// relevance and learn *what* changed. Those were one question until
    /// follow mode needed the second half, and computing the answer twice
    /// would mean a second `symlink_metadata` per event.
    fn relative<'p>(&mut self, path: &'p Path) -> Option<&'p Path> {
        let rela = self
            .roots
            .iter()
            // Outside the worktree entirely. Nothing we display depends on it.
            .find_map(|root| path.strip_prefix(root).ok())?;

        if rela.components().next().map(|c| c.as_os_str()) == Some(OsStr::new(".git")) {
            // Inside the git directory only the index matters, because the
            // index is the left-hand side of every diff we draw. Everything
            // else there is object and log churn that changes no pixel.
            //
            // Matching on the file name rather than the full path also catches
            // `index.lock`'s rename into place, which is how git actually
            // publishes a new index.
            return (rela.file_name() == Some(OsStr::new("index"))).then_some(rela);
        }

        // The mode is not cosmetic. A rule like `target/` matches directories
        // only, and creating `target/debug/x.o` also emits an event for
        // `target` itself. Probing that as a file finds no match, and the
        // whole build tree leaks through.
        let mode = match std::fs::symlink_metadata(path) {
            Ok(meta) if meta.is_dir() => gix::index::entry::Mode::DIR,
            // Missing means deleted, and nothing is left to inspect. File is
            // the safe guess: it can cost a redundant sweep, where guessing
            // directory could hide a deleted file behind a rule like `build/`.
            _ => gix::index::entry::Mode::FILE,
        };

        (!self.is_ignored(rela, mode)).then_some(rela)
    }

    fn is_ignored(&mut self, rela: &Path, mode: gix::index::entry::Mode) -> bool {
        match self.excludes.at_path(rela, Some(mode)) {
            Ok(platform) => platform.is_excluded(),
            // If the rules cannot be consulted, do not filter. A wasted sweep
            // is cheaper than a change the monitor never showed.
            Err(_) => false,
        }
    }
}

/// Every spelling of the worktree root that an event path might carry.
///
/// Three, because the root is spelled by three parties that do not agree and
/// none of them is wrong.
///
/// * **As given.** `gix` hands back the path it was discovered with, so
///   `vigia .` leaves this as `"."`. Kept first because a backend that does not
///   resolve the watched path at all reports events under exactly this.
/// * **Absolute.** `notify` resolves the watch before asking the OS for it, and
///   builds every event path by joining the change onto that. This is the one
///   that was missing, and without it a relative root matched **nothing**: the
///   monitor drew its first frame and then silently ignored every event for the
///   rest of the session. `Components` normalises an interior `.` away, so an
///   event at `C:\w\.\SPEC.md` strips against `C:\w` even though the strings do
///   not share a prefix.
/// * **Canonical.** macOS reports FSEvents paths through `/private` for a
///   worktree opened as `/var`, which neither of the two above covers. On
///   Windows this is the `\\?\` verbatim form, which events never use, so it
///   earns its place on one platform and is inert on another.
///
/// Deduplicated, because in the ordinary case of an absolute argument the first
/// two are the same path and a repeated root would make every event cost an
/// extra comparison.
///
/// Pure, and separated from the watcher for the reason `SPEC.md` §7 gives: the
/// case it exists for cannot be reached from the rest of the suite without
/// changing the process working directory, which is global and races every other
/// test in the binary. `tests/relative.rs` is the one end-to-end gate, alone in
/// its own target so it can own that directory; this is the rule.
fn roots_of(workdir: &Path) -> Vec<PathBuf> {
    let mut roots = vec![workdir.to_path_buf()];
    for spelling in [std::path::absolute(workdir), workdir.canonicalize()]
        .into_iter()
        .flatten()
    {
        if !roots.contains(&spelling) {
            roots.push(spelling);
        }
    }
    roots
}

/// Fold the paths one event named into what that event meant.
///
/// `resolve` decides whether a path matters and where it sits in the worktree.
/// It needs the gitignore stack, so it cannot be pure; the **order** rule can
/// be, and separating them is not tidiness. Nearly every event names exactly
/// one path, where first and last are the same path, so no filesystem this
/// suite can drive distinguishes the two: a version reading the paths forwards
/// passed the entire integration suite, and only the unit tests below caught
/// it. `SPEC.md` §7 names this shape.
///
/// **Walked backwards**, because the last path an event names is the one that
/// landed last: a rename reports `[from, to]`, and the destination is where
/// the content is now. The first followable hit wins and settles relevance at
/// the same time, so the paths before it have nothing left to say. An
/// unfollowable one does not stop the walk, which is what lets an event ending
/// on `.git/index` still name the file beside it.
fn accept_paths<'p>(
    paths: &'p [PathBuf],
    mut resolve: impl FnMut(&'p Path) -> Option<&'p Path>,
) -> Accepted {
    let mut accepted = Accepted::default();
    for path in paths.iter().rev() {
        let Some(rela) = resolve(path) else {
            continue;
        };
        accepted.relevant = true;
        accepted.newest = followable(rela);
        if accepted.newest.is_some() {
            break;
        }
    }
    accepted
}

/// Where the view should move for a worktree-relative path an event named, or
/// `None` when there is nowhere to move.
///
/// Pure, and deliberately not inlined into [`Watcher::accept`], for the reason
/// `SPEC.md` §7 gives: neither of the two things it decides has a reachable
/// integration path on every platform. The separator rule is unobservable on
/// Unix and load bearing on Windows, and the `.git` rule needs a burst that
/// ends on an index write. A test that can only run on one platform, or only
/// under a race, is not the gate this needs.
///
/// **`.git` is excluded rather than filtered earlier.** An index write is a
/// real change, produces a real tick, and must keep doing so: the index is the
/// left-hand side of every diff drawn, so staging changes what is on screen.
/// It is simply not a file the viewport can sit on.
///
/// **Separators are normalised**, because this is compared against
/// [`crate::FileChange::path`], which is git's spelling and always `/`. A
/// Windows event arrives with `\`, so comparing unnormalised would match
/// nothing and leave follow mode silently inert on one tier-1 platform while
/// every test passed on the other two.
///
/// Joining components rather than replacing `\` with `/` is the reason this is
/// not a one-liner: on Unix a backslash is an ordinary filename character, and
/// replacing it would corrupt paths that are perfectly legal there.
fn followable(rela: &Path) -> Option<String> {
    let mut components = rela.components().peekable();
    if components.peek().map(|c| c.as_os_str()) == Some(OsStr::new(".git")) {
        return None;
    }

    let mut path = String::new();
    for component in components {
        if !path.is_empty() {
            path.push('/');
        }
        // Lossy for the same reason `FileChange::path` is, and it has to be
        // the same reason or the two would not compare equal. That both sides
        // lose the same information is what makes them match; that they lose
        // it at all is
        // [#17](https://github.com/breferrari/vigia/issues/17).
        path.push_str(&component.as_os_str().to_string_lossy());
    }

    // An event on the worktree root itself strips to nothing. There is no file
    // there to follow, and `Some("")` would match no change and read as an
    // answer.
    (!path.is_empty()).then_some(path)
}

#[cfg(test)]
mod tests {
    //! The follow-target rule, tested as the pure function it is.
    //!
    //! Both of its decisions are invisible from an integration test on at
    //! least one tier-1 platform, which is exactly the case `SPEC.md` §7 says
    //! to extract and test directly.

    use super::*;

    /// Build a path the way the host spells one, so the Windows run and the
    /// Unix run are testing their own separator rather than a literal.
    fn native(components: &[&str]) -> PathBuf {
        components.iter().collect()
    }

    /// `vigia .`, which is the default invocation and the one the name is built
    /// on, and which redrew exactly once before this test existed.
    ///
    /// `gix` hands back the path it was given, so a relative argument leaves
    /// `workdir()` as `"."`. `notify` resolves the watch to an absolute path and
    /// joins each change onto it, so an event never begins with `"."`, and the
    /// canonicalised root carries a `\\?\` prefix on Windows that events never
    /// use. With neither matching, every event is discarded as outside the
    /// worktree and `next_tick` blocks forever on a live channel: no tick, no
    /// error, and nothing on the footer to say so.
    ///
    /// Reproducing it needs the root to be `"."` exactly. Rust's `Components`
    /// normalises a `.` away everywhere except the start of a path, so even
    /// `<absolute>/.` strips correctly and looks fine.
    #[test]
    fn a_relative_worktree_root_matches_the_paths_events_carry() {
        let roots = roots_of(Path::new("."));
        // Where `notify` will have resolved the watch to, and therefore what
        // every event path begins with. Read rather than set, so this test
        // stays safe beside every other test in the binary.
        let resolved = std::path::absolute(".").expect("an absolute working directory");
        let event = resolved.join("SPEC.md");

        assert!(
            roots.iter().any(|root| event.strip_prefix(root).is_ok()),
            "an event at {event:?} matched none of {roots:?}, so every event a \
             relative worktree reports is dropped as outside itself"
        );
    }

    /// The macOS case the canonicalised root is there for, kept alongside so a
    /// later simplification cannot drop one while satisfying the other.
    #[test]
    fn the_root_set_keeps_the_spelling_it_was_given() {
        let roots = roots_of(Path::new("."));
        assert_eq!(
            roots.first().map(PathBuf::as_path),
            Some(Path::new(".")),
            "the workdir's own spelling has to stay in the set: it is what \
             matches on a platform whose events are not resolved at all"
        );
        assert!(
            roots.len() > 1,
            "a relative root collapsed to one spelling, so only one of the \
             three ways an event can be spelled is covered"
        );
    }

    #[test]
    fn a_worktree_file_is_followable() {
        assert_eq!(
            followable(&native(&["src", "watch.rs"])).as_deref(),
            Some("src/watch.rs")
        );
    }

    /// The separator rule, which is a no-op on Unix and the whole of the
    /// feature on Windows.
    #[test]
    fn a_platform_separator_becomes_a_git_separator() {
        let deep = followable(&native(&["crates", "vigia-core", "src", "watch.rs"]))
            .expect("a nested worktree file is followable");
        assert_eq!(deep, "crates/vigia-core/src/watch.rs");
        assert!(
            !deep.contains('\\'),
            "a native separator survived into a path that is compared against \
             FileChange::path, which git always spells with a forward slash"
        );
    }

    /// Staging is a real change and produces a real tick, because the index is
    /// the left-hand side of every diff. It is not somewhere to scroll to.
    #[test]
    fn an_index_write_names_no_file_to_follow() {
        assert_eq!(followable(&native(&[".git", "index"])), None);
        assert_eq!(followable(&native(&[".git", "index.lock"])), None);
    }

    /// A prefix match would take `.github/workflows/ci.yml` with it, and that
    /// is an ordinary tracked file whose edits should be followed.
    #[test]
    fn a_dot_prefixed_directory_that_is_not_dot_git_is_still_followable() {
        assert_eq!(
            followable(&native(&[".github", "workflows", "ci.yml"])).as_deref(),
            Some(".github/workflows/ci.yml")
        );
        assert_eq!(
            followable(&native(&[".gitignore"])).as_deref(),
            Some(".gitignore")
        );
    }

    /// An event on the worktree root strips to nothing, and an empty path
    /// matches no change while reading like an answer.
    #[test]
    fn the_worktree_root_itself_is_not_a_file_to_follow() {
        assert_eq!(followable(Path::new("")), None);
    }

    /// Everything is inside the worktree and nothing is ignored, so the tests
    /// below are about the order rule and only the order rule.
    fn take_all(path: &Path) -> Option<&Path> {
        Some(path)
    }

    /// The one case where the order of an event's paths is observable, and the
    /// reason it is unreachable from an integration test: a rename is the only
    /// event that names two files, and every other event makes first and last
    /// the same path. Reading them forwards passed every integration test in
    /// the suite.
    #[test]
    fn a_rename_follows_the_destination_rather_than_the_source() {
        let paths = [native(&["src", "before.rs"]), native(&["src", "after.rs"])];
        let accepted = accept_paths(&paths, take_all);

        assert!(
            accepted.relevant,
            "a rename inside the worktree was ignored"
        );
        assert_eq!(
            accepted.newest.as_deref(),
            Some("src/after.rs"),
            "the view would move to where the file used to be"
        );
    }

    /// An unfollowable path does not end the walk. Staging an edit can put the
    /// index after the file in one event, and stopping there would throw away
    /// the answer that was one step further back.
    #[test]
    fn an_event_ending_on_the_index_still_names_the_file_beside_it() {
        let paths = [native(&["src", "a.rs"]), native(&[".git", "index"])];
        let accepted = accept_paths(&paths, take_all);

        assert!(accepted.relevant);
        assert_eq!(accepted.newest.as_deref(), Some("src/a.rs"));
    }

    /// Relevant and unfollowable are different answers and both have to
    /// survive: a staging write must still redraw, and must still not move the
    /// viewport.
    #[test]
    fn an_index_write_alone_is_relevant_and_names_nothing() {
        let paths = [native(&[".git", "index"])];
        let accepted = accept_paths(&paths, take_all);

        assert!(
            accepted.relevant,
            "staging stopped producing a tick, so the left-hand side of every \
             diff can change with nothing redrawn"
        );
        assert_eq!(accepted.newest, None);
    }

    #[test]
    fn an_event_naming_nothing_the_display_depends_on_is_not_relevant() {
        let paths = [native(&["target", "debug", "build.o"])];
        let accepted = accept_paths(&paths, |_| None);

        assert!(!accepted.relevant);
        assert_eq!(accepted.newest, None);
    }

    /// The burst accumulator, tested directly for the reason `SPEC.md` §7
    /// gives about the racily-clean guard: the cases that matter need ten
    /// thousand writes inside one hundred-millisecond window, which no
    /// integration test in this suite can arrange on demand.
    mod burst {
        use super::*;

        fn collect(paths: &[&str]) -> (Vec<String>, u32) {
            let mut burst = Burst::default();
            for path in paths {
                burst.push((*path).to_owned());
            }
            burst.finish()
        }

        #[test]
        fn the_write_that_landed_last_is_last() {
            let (paths, dropped) = collect(&["a", "b", "c"]);

            assert_eq!(paths.last().map(String::as_str), Some("c"));
            assert_eq!(paths.len(), 3);
            assert_eq!(dropped, 0);
        }

        /// Four saves of one file inside one burst is one change to the reader,
        /// and I10 samples per tick rather than per event.
        #[test]
        fn a_file_saved_repeatedly_inside_one_burst_appears_once() {
            let (paths, _) = collect(&["a", "a", "a", "a"]);

            assert_eq!(paths, vec!["a".to_owned()]);
        }

        /// Repeating an earlier path still moves the follow target, because the
        /// last write is the last write however many times it has happened
        /// before.
        #[test]
        fn a_repeat_of_an_earlier_path_still_becomes_the_newest() {
            let (paths, _) = collect(&["a", "b", "a"]);

            assert_eq!(paths.last().map(String::as_str), Some("a"));
            assert_eq!(paths.len(), 2);
        }

        #[test]
        fn a_burst_that_named_nothing_carries_nothing() {
            let (paths, dropped) = collect(&[]);

            assert!(paths.is_empty());
            assert_eq!(dropped, 0);
        }

        /// I10's cap, enforced where the allocation would otherwise happen. A
        /// tick that carried ten thousand paths would hand the store a bound to
        /// apply after the cost had already been paid.
        #[test]
        fn a_bulk_burst_is_capped_and_says_how_much_it_refused() {
            let owned: Vec<String> = (0..10_000).map(|n| format!("f{n}")).collect();
            let mut burst = Burst::default();
            for path in &owned {
                burst.push(path.clone());
            }
            let (paths, dropped) = burst.finish();

            assert_eq!(paths.len(), HISTORY_PATHS);
            assert_eq!(dropped, 10_000 - HISTORY_PATHS as u32);
        }

        /// The one path the cap may never refuse. Losing it loses follow mode's
        /// answer, and it would be lost precisely during a bulk operation, which
        /// is when the viewport moving matters most.
        #[test]
        fn the_newest_path_survives_the_cap() {
            let owned: Vec<String> = (0..10_000).map(|n| format!("f{n}")).collect();
            let mut burst = Burst::default();
            for path in &owned {
                burst.push(path.clone());
            }
            let (paths, _) = burst.finish();

            assert_eq!(paths.last().map(String::as_str), Some("f9999"));
            assert_eq!(paths.len(), HISTORY_PATHS, "and it did not exceed the cap");
        }
    }
}
