//! The watch and coalesce engine: I1.

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
    pub quiet: Duration,
    /// The longest a tick may be held back while events keep arriving.
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tick {
    /// Accepted events folded into this tick.
    pub events: u32,
    /// How long the tick was held back to coalesce.
    pub coalesced_for: Duration,
    /// The distinct files written in this tick, spelled the way
    /// [`crate::FileChange::path`] spells them.
    pub paths: Vec<String>,
    /// How many further paths this burst touched past [`HISTORY_PATHS`].
    pub dropped: u32,
}

impl Tick {
    /// The write that landed **last** in this tick.
    pub fn newest(&self) -> Option<&str> {
        self.paths.last().map(String::as_str)
    }
}

/// The paths accumulated while a burst is still being coalesced.
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
    pub fn delivered(&self) -> u64 {
        self.delivered.load(Ordering::Relaxed)
    }

    /// Block until the working tree changes, then return one coalesced tick.
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
                            // A stop ends the wait whether or not a burst is
                            // open. Returning the partial burst instead would
                            // make a stop mean "one more tick, then stop",
                            // which is neither what `Stop` documents nor what a
                            // timeout built on it can read.
                            None => return None,
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
    fn relative<'p>(&mut self, path: &'p Path) -> Option<&'p Path> {
        let rela = self
            .roots
            .iter()
            // Outside the worktree entirely. Nothing we display depends on it.
            .find_map(|root| path.strip_prefix(root).ok())?;

        if rela.components().next().map(|c| c.as_os_str()) == Some(OsStr::new(".git")) {
            return watched_in_git_dir(rela).then_some(rela);
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
fn watched_in_git_dir(rela: &Path) -> bool {
    if rela.file_name() == Some(OsStr::new("index")) {
        return true;
    }

    // Everything below is judged on the path *within* `.git`, so strip it once.
    let mut inside = rela.components();
    inside.next();
    let mut inside = inside.peekable();
    let Some(first) = inside.next() else {
        return false;
    };
    let first = first.as_os_str();

    if first == OsStr::new("HEAD") || first == OsStr::new("packed-refs") {
        // Only when it *is* that file: a directory called `HEAD` somewhere below
        // is not one, and `refs/heads/HEAD` reaches the arm underneath.
        return inside.peek().is_none();
    }

    // `refs/heads/**`, at any depth, because a branch name may carry slashes.
    // A bare `refs/heads` directory event names no ref and is dropped with it.
    first == OsStr::new("refs")
        && inside.next().map(|c| c.as_os_str()) == Some(OsStr::new("heads"))
        && inside.next().is_some()
}

/// Pure, and deliberately not inlined into [`Watcher::accept`], for the reason
/// `SPEC.md` §7 gives: neither of the two things it decides has a reachable
/// integration path on every platform. The separator rule is unobservable on
/// Unix and load bearing on Windows, and the `.git` rule needs a burst that
/// ends on an index write. A test that can only run on one platform, or only
/// under a race, is not the gate this needs.
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

    use super::*;

    /// Build a path the way the host spells one, so the Windows run and the
    /// Unix run are testing their own separator rather than a literal.
    fn native(components: &[&str]) -> PathBuf {
        components.iter().collect()
    }

    /// `vigia .`, which is the default invocation and the one the name is built
    /// on, and which redrew exactly once before this test existed.
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

    /// **A branch tip moving is a change the monitor must see**, and until
    /// [#313](https://github.com/breferrari/vigia/issues/313) it did not.
    #[test]
    fn a_branch_tip_moving_is_a_change_the_monitor_must_see() {
        let watched = |parts: &[&str]| watched_in_git_dir(&native(parts));

        assert!(watched(&[".git", "index"]), "staging, as it always was");
        assert!(watched(&[".git", "HEAD"]), "a checkout, or detaching");
        assert!(
            watched(&[".git", "refs", "heads", "main"]),
            "a branch tip moving is HEAD moving, because HEAD is a symref"
        );
        assert!(
            watched(&[".git", "refs", "heads", "feature", "nested"]),
            "and a branch name may carry slashes"
        );
        assert!(
            watched(&[".git", "packed-refs"]),
            "because gc moves a loose ref into it and the loose file then goes"
        );
    }

    /// **What stays excluded, and why each one would be a wake that draws exactly
    /// what was already on screen.**
    #[test]
    fn the_rest_of_the_git_directory_still_wakes_nothing() {
        let watched = |parts: &[&str]| watched_in_git_dir(&native(parts));

        assert!(!watched(&[".git", "refs", "remotes", "origin", "main"]));
        assert!(!watched(&[".git", "refs", "tags", "v1.0.0"]));
        assert!(!watched(&[".git", "logs", "HEAD"]));
        assert!(!watched(&[".git", "ORIG_HEAD"]));
        assert!(!watched(&[".git", "objects", "ab", "cdef01"]));
        assert!(!watched(&[".git", "COMMIT_EDITMSG"]));
        assert!(!watched(&[".git", "config"]));

        // **The lock file itself is not the write, and that is deliberate rather
        // than an oversight this widening should have swept up.** Git publishes a
        // new index by writing `index.lock` and renaming it over `index`, and it
        // is the rename's destination that names `index`. Waking on the lock too
        // would draw the *old* index a moment before the new one landed, then
        // draw it again — one wasted frame per stage, and the first of the two
        // showing a comparison that is about to stop being true.
        assert!(!watched(&[".git", "index.lock"]));

        // A directory event rather than a file one. `refs/heads` itself names no
        // ref, and a `HEAD` that is a directory somewhere below is not the file.
        assert!(!watched(&[".git", "refs", "heads"]));
        assert!(!watched(&[".git", "refs"]));
        assert!(
            !watched(&[".git", "worktrees", "other", "HEAD"]),
            "another worktree's HEAD is another worktree's business"
        );
    }

    /// Neither of the refs the staged run watches is somewhere to scroll to.
    #[test]
    fn a_ref_write_names_no_file_to_follow() {
        assert_eq!(followable(&native(&[".git", "HEAD"])), None);
        assert_eq!(
            followable(&native(&[".git", "refs", "heads", "main"])),
            None
        );
        assert_eq!(followable(&native(&[".git", "packed-refs"])), None);
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
