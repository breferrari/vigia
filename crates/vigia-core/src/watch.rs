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

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use notify::{EventKind, RecursiveMode, Watcher as _};

use crate::error::{Error, Result};

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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tick {
    /// Accepted events folded into this tick.
    pub events: u32,
    /// How long the tick was held back to coalesce.
    pub coalesced_for: Duration,
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
    /// Prefixes an event path may carry for the same worktree.
    ///
    /// Two of them because macOS reports FSEvents paths through `/private`
    /// while the worktree was opened as `/var`, and Windows canonicalises to a
    /// `\\?\` prefix that events never use.
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

        let mut roots = vec![workdir.to_path_buf()];
        if let Ok(canonical) = workdir.canonicalize()
            && canonical != workdir
        {
            roots.push(canonical);
        }

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

    /// Raw events the OS has delivered since the watcher started.
    ///
    /// Readable without blocking and without `&mut`, which is what makes I1
    /// checkable from outside: if this is still zero after an idle window,
    /// nothing reached the process at all.
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
            if !self.accept(message)? {
                continue;
            }

            let started = Instant::now();
            let hard_deadline = started + self.options.max_delay;
            let mut quiet_until = started + self.options.quiet;
            let mut events = 1;

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
                            Some(true) => {
                                events += 1;
                                quiet_until = Instant::now() + self.options.quiet;
                            }
                            Some(false) => {}
                        }
                    }
                    Err(RecvTimeoutError::Timeout) => break,
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            }

            self.stats.ticks += 1;
            return Some(Tick {
                events,
                coalesced_for: started.elapsed(),
            });
        }
    }

    /// `None` means stop; `Some(false)` means the event was filtered out.
    fn accept(&mut self, message: Message) -> Option<bool> {
        let event = match message {
            Message::Stop => return None,
            Message::Event(event) => event,
        };

        // Reads are not changes. Most backends never report them, but inotify
        // can be configured to, and a monitor that redraws because something
        // read a file would be absurd.
        if matches!(event.kind, EventKind::Access(_)) {
            self.stats.filtered += 1;
            return Some(false);
        }

        let relevant = event.paths.iter().any(|path| self.is_relevant(path));
        if !relevant {
            self.stats.filtered += 1;
        }
        Some(relevant)
    }

    fn is_relevant(&mut self, path: &Path) -> bool {
        let Some(rela) = self
            .roots
            .iter()
            .find_map(|root| path.strip_prefix(root).ok())
        else {
            // Outside the worktree entirely. Nothing we display depends on it.
            return false;
        };

        if rela.components().next().map(|c| c.as_os_str()) == Some(OsStr::new(".git")) {
            // Inside the git directory only the index matters, because the
            // index is the left-hand side of every diff we draw. Everything
            // else there is object and log churn that changes no pixel.
            //
            // Matching on the file name rather than the full path also catches
            // `index.lock`'s rename into place, which is how git actually
            // publishes a new index.
            return rela.file_name() == Some(OsStr::new("index"));
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

        !self.is_ignored(rela, mode)
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
