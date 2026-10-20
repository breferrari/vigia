//! The watch and coalesce engine: I1.

use std::collections::VecDeque;
use std::ffi::OsStr;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::{Read, Seek, SeekFrom};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use notify::{EventKind, RecursiveMode, Watcher as _};

use crate::Hidden;
use crate::error::{Error, Result};
use crate::frame::{Fingerprint, config_moved, followed_print};
use crate::history::HISTORY_PATHS;
use crate::worktree::lfs_store_of;

/// Arms one OS watch. A parameter only so a test can make it fail.
type Arm = fn(&mut notify::RecommendedWatcher, &Path, RecursiveMode) -> notify::Result<()>;

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
    /// How many paths this burst evicted to stay within [`HISTORY_PATHS`].
    pub dropped: u32,
}

impl Tick {
    /// The write that landed last in this tick.
    pub fn newest(&self) -> Option<&str> {
        self.paths.last().map(String::as_str)
    }
}

/// The paths accumulated while a burst is still being coalesced.
#[derive(Debug, Default)]
struct Burst {
    order: VecDeque<String>,
    newest: Option<String>,
    dropped: u32,
}

impl Burst {
    /// Record a followable path, which by construction is the newest so far.
    fn push(&mut self, path: String) {
        if self.order.contains(&path) {
            self.newest = Some(path);
            return;
        }
        if self.order.len() >= HISTORY_PATHS {
            // Lose the oldest. Refusing the arrival would eventually refuse the
            // newest, which is follow mode's answer.
            self.order.pop_front();
            self.dropped += 1;
        }
        self.order.push_back(path.clone());
        self.newest = Some(path);
    }

    /// The paths, with the newest moved to the end.
    fn finish(self) -> (Vec<String>, u32) {
        let mut paths = Vec::from(self.order);
        if let Some(pos) = self
            .newest
            .and_then(|newest| paths.iter().position(|path| *path == newest))
        {
            let newest = paths.remove(pos);
            paths.push(newest);
        }
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
    /// A store watch the arming thread finished.
    Armed {
        path: PathBuf,
        mode: RecursiveMode,
        result: notify::Result<()>,
    },
}

/// What the arming thread is asked to do with the store's OS watch.
enum StoreRequest {
    Arm(PathBuf, RecursiveMode),
    Unwatch(PathBuf),
}

/// The thread that arms the store's own OS watch, apart from the tree's: on
/// macOS a watch arms by restarting its stream, which can take seconds under
/// load. It lists the paths it watches in `held`.
fn spawn_armer(
    tx: Sender<Message>,
    counter: Arc<AtomicU64>,
    arm: Arm,
    held: Arc<Mutex<Vec<PathBuf>>>,
) -> std::io::Result<Sender<StoreRequest>> {
    let (requests, received) = mpsc::channel();
    std::thread::Builder::new().spawn(move || {
        let mut backend = None;
        for request in received {
            match request {
                StoreRequest::Arm(path, mode) => {
                    let result = match &mut backend {
                        Some(backend) => Ok(backend),
                        None => event_backend(tx.clone(), Arc::clone(&counter))
                            .map(|made| backend.insert(made)),
                    }
                    .and_then(|backend| {
                        arm(backend, &path, mode).inspect_err(|_| {
                            // A recursive watch that stopped partway holds what
                            // it added.
                            let _ = backend.unwatch(&path);
                        })
                    });
                    if result.is_ok()
                        && let Ok(mut held) = held.lock()
                    {
                        held.push(path.clone());
                    }
                    let armed = Message::Armed { path, mode, result };
                    if tx.send(armed).is_err() {
                        return;
                    }
                }
                StoreRequest::Unwatch(path) => {
                    if let Some(backend) = &mut backend
                        && backend.unwatch(&path).is_ok()
                        && let Ok(mut held) = held.lock()
                    {
                        held.retain(|watched| *watched != path);
                    }
                }
            }
        }
    })?;
    Ok(requests)
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
    arm: Arm,
    /// Owned rather than borrowed, so a config write can reload it.
    repo: gix::Repository,
    excludes: gix::worktree::Stack,
    /// The common dir's config, which `repo` loads.
    config: PathBuf,
    /// The config's fingerprint when `repo` last loaded it.
    loaded_config: Option<Fingerprint>,
    /// Set by a config event, and at start so a write before the watch armed is
    /// not missed. The next path judged reloads.
    reload_due: bool,
    /// `info/exclude` and the file `core.excludesFile` names, which the stack
    /// read when it was built.
    rule_files: [Option<PathBuf>; 2],
    /// What [`Self::rule_files`] looked like when the stack was built.
    rule_prints: [Option<Fingerprint>; 2],
    /// Set by anything that changes the rules, and cleared once a stack is built
    /// from them.
    stack_stale: bool,
    /// When the rules were last refreshed. See [`RULES_EVERY`].
    refreshed_at: Option<Instant>,
    /// Unused. Keeps `Watcher<'repo>` source-compatible.
    _worktree: PhantomData<&'repo gix::Repository>,
    /// Prefixes an event path may carry for the same worktree. See [`roots_of`].
    roots: Vec<PathBuf>,
    /// The git dir, in every spelling as for `roots`. A linked worktree, a submodule
    /// or a separate `GIT_DIR` keeps it outside the worktree.
    git_dir: Vec<PathBuf>,
    /// The common dir, when it is not the git dir. Empty otherwise. It holds the
    /// branch refs, and also other worktrees' `HEAD` and index.
    common_dir: Vec<PathBuf>,
    /// The LFS store's `objects`, in every spelling as for `roots`. An object
    /// landing there can turn a `binary` side into a picture.
    store: Vec<PathBuf>,
    /// An out-of-tree store not watched because no attributes name LFS yet.
    store_deferred: bool,
    /// While `objects` outside the worktree does not exist yet, its deepest
    /// existing ancestor, watched until it appears.
    store_ancestor: Option<PathBuf>,
    /// The arming thread, started by the first store watch asked for.
    store_requests: Option<Sender<StoreRequest>>,
    /// Whether a store watch is being armed, so no second is asked for.
    store_arming: bool,
    /// The paths the arming thread watches.
    store_held: Arc<Mutex<Vec<PathBuf>>>,
    /// Why the store could not be watched, until [`Self::take_store_notice`].
    store_notice: Option<String>,
    index: IndexWatch,
    options: WatchOptions,
    stats: WatchStats,
    delivered: Arc<AtomicU64>,
    /// The reader's `hide` pattern, which keeps a path from waking the watch.
    hidden: Option<Hidden>,
}

impl<'repo> Watcher<'repo> {
    /// `loaded_config` is the config's fingerprint when `repo` loaded it, or
    /// `None` when unknown, which reloads on the first path judged.
    pub(crate) fn new(
        repo: &gix::Repository,
        config: &Path,
        loaded_config: Option<Fingerprint>,
        workdir: &Path,
        options: WatchOptions,
    ) -> Result<Self> {
        Self::armed_by(
            repo,
            config,
            loaded_config,
            workdir,
            options,
            |backend, path, mode| backend.watch(path, mode),
        )
    }

    fn armed_by(
        repo: &gix::Repository,
        config: &Path,
        loaded_config: Option<Fingerprint>,
        workdir: &Path,
        options: WatchOptions,
        arm: Arm,
    ) -> Result<Self> {
        // Everything that reads the repository happens before the watch is armed, so
        // the watcher never observes its own construction.
        // Stated before the stack reads them, so a write in between is seen.
        let rule_files = rule_files_of(repo);
        let rule_prints = prints_of(&rule_files);
        let excludes = excludes_of(repo)?;

        let roots = roots_of(workdir);
        // A linked worktree's common dir comes back as `<git dir>/../..`, which would
        // match as a path under the git dir.
        let tidy = |dir: &Path| {
            let dir = std::path::absolute(dir).unwrap_or_else(|_| dir.to_path_buf());
            gix::path::normalize(dir.as_path().into(), &dir)
                .map_or_else(|| dir.clone(), std::borrow::Cow::into_owned)
        };
        let (git, common) = (tidy(repo.git_dir()), tidy(repo.common_dir()));
        let git_dir = spellings_of(&git, &roots);
        let common_dir = if common == git {
            Vec::new()
        } else {
            spellings_of(&common, &roots)
        };
        let objects = tidy(&lfs_store_of(repo)).join("objects");
        let mut store = spellings_of(&objects, &roots);
        if let Some(spelling) = ancestor_spelling(&objects)
            && !store.contains(&spelling)
        {
            store.push(spelling);
        }
        let index = IndexWatch::new(repo);

        let (tx, rx) = mpsc::channel();

        let delivered = Arc::new(AtomicU64::new(0));
        let mut backend = event_backend(tx.clone(), Arc::clone(&delivered))
            .map_err(|e| Error::Watch(Box::new(e)))?;

        backend
            .watch(workdir, RecursiveMode::Recursive)
            .map_err(|e| Error::Watch(Box::new(e)))?;
        // Not recursive: `objects/` churns on every write.
        let inside = |dir: &Path| roots.iter().any(|root| dir.starts_with(root));
        for dir in [git_dir.first(), common_dir.first()].into_iter().flatten() {
            if inside(dir) {
                continue;
            }
            backend
                .watch(dir, RecursiveMode::NonRecursive)
                .map_err(|e| Error::Watch(Box::new(e)))?;
            // A linked worktree's git dir has no refs of its own.
            let heads = dir.join("refs").join("heads");
            if heads.is_dir() {
                backend
                    .watch(&heads, RecursiveMode::Recursive)
                    .map_err(|e| Error::Watch(Box::new(e)))?;
            }
            // For `info/attributes`, which a non-recursive watch above cannot see.
            let info = dir.join("info");
            if info.is_dir() {
                backend
                    .watch(&info, RecursiveMode::NonRecursive)
                    .map_err(|e| Error::Watch(Box::new(e)))?;
            }
        }

        let outside = !inside(&objects);
        let mut watcher = Self {
            rx,
            tx,
            _backend: backend,
            arm,
            repo: repo.clone(),
            excludes,
            config: config.to_path_buf(),
            loaded_config,
            reload_due: true,
            rule_files,
            rule_prints,
            stack_stale: false,
            refreshed_at: None,
            _worktree: PhantomData,
            roots,
            git_dir,
            common_dir,
            store,
            store_deferred: false,
            store_ancestor: None,
            store_requests: None,
            store_arming: false,
            store_held: Arc::default(),
            store_notice: None,
            index,
            options,
            stats: WatchStats::default(),
            delivered,
            hidden: None,
        };
        // Inside the worktree, the recursive watch above already covers it.
        if outside {
            if lfs_attributes(repo, workdir, None) {
                watcher.arm_store();
            } else {
                watcher.store_deferred = true;
            }
        }
        Ok(watcher)
    }

    /// Ask for a watch on `objects`, or on its deepest existing ancestor until it
    /// appears.
    fn arm_store(&mut self) {
        if self.store_arming {
            return;
        }
        let objects = self.store[0].clone();
        let (path, mode) = if objects.is_dir() {
            (objects, RecursiveMode::Recursive)
        } else {
            let Some(ancestor) = objects.ancestors().skip(1).find(|dir| dir.is_dir()) else {
                return;
            };
            // Already watched, and its child is still missing.
            if self.store_ancestor.as_deref() == Some(ancestor) {
                return;
            }
            (ancestor.to_path_buf(), RecursiveMode::NonRecursive)
        };
        if self.store_requests.is_none() {
            match spawn_armer(
                self.tx.clone(),
                Arc::clone(&self.delivered),
                self.arm,
                Arc::clone(&self.store_held),
            ) {
                Ok(requests) => self.store_requests = Some(requests),
                Err(error) => {
                    self.store_notice = Some(store_notice(&notify::Error::io(error)));
                    return;
                }
            }
        }
        self.store_arming = self
            .store_requests
            .as_ref()
            .is_some_and(|requests| requests.send(StoreRequest::Arm(path, mode)).is_ok());
    }

    /// Take a finished store watch, and say whether it ticks. A failure keeps
    /// every other watch, stops waiting for the store, and ticks so the notice
    /// is read. A watch on `objects` ticks, since an object may have landed
    /// while it armed.
    fn armed(&mut self, path: PathBuf, mode: RecursiveMode, result: notify::Result<()>) -> bool {
        self.store_arming = false;
        self.drop_ancestor();
        if let Err(error) = result {
            self.store_notice = Some(store_notice(&error));
            return true;
        }
        if mode == RecursiveMode::Recursive {
            return true;
        }
        // The child may have appeared while its parent armed.
        self.store_ancestor = Some(path);
        self.arm_store();
        false
    }

    /// Stop watching the store's ancestor.
    fn drop_ancestor(&mut self) {
        if let (Some(ancestor), Some(requests)) = (self.store_ancestor.take(), &self.store_requests)
        {
            let _ = requests.send(StoreRequest::Unwatch(ancestor));
        }
    }

    /// Whether a store watch has been asked for.
    #[doc(hidden)]
    pub fn store_wanted(&self) -> bool {
        self.store_requests.is_some()
    }

    /// The paths the store's own watch covers now, for a test to wait on.
    #[doc(hidden)]
    pub fn store_watched(&self) -> Vec<PathBuf> {
        self.store_held
            .lock()
            .map(|held| held.clone())
            .unwrap_or_default()
    }

    /// Once, why an LFS object landing will not redraw its file until the next
    /// change.
    pub fn take_store_notice(&mut self) -> Option<String> {
        self.store_notice.take()
    }

    /// Keep every path `hide` covers from waking the watch.
    pub fn hide(&mut self, hide: Option<Hidden>) {
        self.hidden = hide;
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
                            // A stop ends the wait whether or not a burst is open.
                            None => return None,
                            Some(accepted) if accepted.relevant => {
                                events += 1;
                                // Only an event that named a file adds one.
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
            Message::Armed { path, mode, result } => {
                let relevant = self.armed(path, mode, result);
                if !relevant {
                    self.stats.filtered += 1;
                }
                return Some(Accepted {
                    relevant,
                    ..Accepted::default()
                });
            }
        };
        // Any `.gitignore` the event names, judged or not: a rename away from one
        // names the new path first, and a hidden one still shapes the rules.
        if event
            .paths
            .iter()
            .any(|path| path.file_name() == Some(OsStr::new(".gitignore")))
        {
            self.stack_stale = true;
        }

        // Reads are not changes. Most backends never report them, but inotify
        // can be configured to, and a monitor that redraws because something
        // read a file would be absurd.
        if matches!(event.kind, EventKind::Access(_)) {
            self.stats.filtered += 1;
            return Some(Accepted::default());
        }

        // An ignored or hidden `.gitattributes` is not one git reads here.
        if self.store_deferred
            && let Some(named) = event.paths.iter().find(|path| {
                let info = within(&self.git_dir, path).or_else(|| within(&self.common_dir, path));
                info == Some(Path::new("info/attributes"))
                    || path.file_name() == Some(OsStr::new(".gitattributes"))
                        && self.relative(path).is_some()
            })
            && lfs_attributes(&self.repo, &self.roots[0], Some(named))
        {
            self.store_deferred = false;
            self.arm_store();
        }

        let accepted = accept_paths(&event.paths, |path| self.relative(path));
        if !accepted.relevant {
            self.stats.filtered += 1;
        }
        Some(accepted)
    }

    /// This event's path relative to the worktree, and whether it is a
    /// directory, or `None` when nothing the display depends on is behind it.
    fn relative<'p>(&mut self, path: &'p Path) -> Option<(&'p Path, bool)> {
        // A directory on the way to a store that does not exist yet.
        if self.store_ancestor.is_some()
            && self.store.iter().any(|spelling| spelling.starts_with(path))
        {
            self.arm_store();
        }
        if within(&self.store, path).is_some() {
            return Some((Path::new(".git"), false));
        }
        // A git-dir event comes back as `.git`, which `followable` never follows.
        if let Some(inside) = within(&self.git_dir, path) {
            let watched = if inside == Path::new("index") {
                self.index.moved()
            } else {
                self.reload_due |= inside == Path::new("config");
                watched_in_git_dir(inside)
            };
            return watched.then_some((Path::new(".git"), false));
        }
        if let Some(inside) = within(&self.common_dir, path) {
            // Other worktrees' index and `HEAD` live here too, so only the refs and
            // the files that shape the filter are taken.
            let shared = inside.starts_with("refs")
                || inside.starts_with("info")
                || inside == Path::new("packed-refs")
                || inside == Path::new("config");
            self.reload_due |= inside == Path::new("config");
            return (shared && watched_in_git_dir(inside)).then_some((Path::new(".git"), false));
        }

        // Outside the worktree entirely. Nothing we display depends on it.
        let rela = within(&self.roots, path)?;
        // A linked worktree's `.git` file, or a `.git` that is not this repository's.
        if rela.starts_with(".git") {
            return None;
        }
        // Before the syscall below, so a hidden file costs one match.
        let spelled = self.hidden.as_ref().and(followable(rela));
        if let (Some(hidden), Some(spelled)) = (&self.hidden, &spelled)
            && hidden.is_hidden(spelled)
        {
            return None;
        }

        // The mode is not cosmetic. A rule like `target/` matches directories only, and
        // creating `target/debug/x.o` also emits an event for `target` itself.
        let mode = match std::fs::symlink_metadata(path) {
            Ok(meta) if meta.is_dir() => gix::index::entry::Mode::DIR,
            // Missing means deleted, and nothing is left to inspect. File is
            // the safe guess: it can cost a redundant sweep, where guessing
            // directory could hide a deleted file behind a rule like `build/`.
            _ => gix::index::entry::Mode::FILE,
        };
        // A directory's own event, hidden by the spelling its contents start with.
        // Only an existing one: a deleted file named like it must still wake.
        if mode == gix::index::entry::Mode::DIR
            && let (Some(hidden), Some(spelled)) = (&self.hidden, &spelled)
            && hidden.is_hidden(&format!("{spelled}/"))
        {
            return None;
        }

        self.refresh_rules();
        (!self.is_ignored(rela, mode)).then_some((rela, mode == gix::index::entry::Mode::DIR))
    }

    /// Reload the config as [`Worktree`](crate::Worktree) reloads its own, and
    /// mark the stack stale. One that does not load keeps the config before it,
    /// and the next refresh tries again.
    fn reload_config(&mut self) {
        let now = followed_print(&self.config);
        if now == self.loaded_config {
            self.reload_due = false;
            return;
        }
        if !config_moved(now, self.loaded_config) {
            return;
        }
        if self.repo.reload().is_ok() {
            self.rule_files = rule_files_of(&self.repo);
            self.stack_stale = true;
            self.loaded_config = now;
            self.reload_due = false;
        }
    }

    /// Reload the config if one is due, stat the rule files, and rebuild the stack
    /// if anything marked it stale. At most once per [`RULES_EVERY`]; what fails
    /// stays due and is tried at the next refresh.
    fn refresh_rules(&mut self) {
        let now = Instant::now();
        if self
            .refreshed_at
            .is_some_and(|at| now.duration_since(at) < RULES_EVERY)
        {
            return;
        }
        self.refreshed_at = Some(now);
        if self.reload_due {
            self.reload_config();
        }
        let prints = prints_of(&self.rule_files);
        if prints != self.rule_prints {
            self.rule_prints = prints;
            self.stack_stale = true;
        }
        if self.stack_stale
            && let Ok(excludes) = excludes_of(&self.repo)
        {
            self.excludes = excludes;
            self.stack_stale = false;
        }
    }

    fn is_ignored(&mut self, rela: &Path, mode: gix::index::entry::Mode) -> bool {
        match self.excludes.at_path(rela, Some(mode), &self.repo.objects) {
            Ok(platform) => platform.is_excluded(),
            // If the rules cannot be consulted, do not filter. A wasted sweep
            // is cheaper than a change the monitor never showed.
            Err(_) => false,
        }
    }
}

/// The longest a rule edit waits to reach the paths judged after it. It bounds
/// what a flood of ignored events costs in stats, reloads and rebuilds, and only
/// decides whether an idle pane wakes: a burst that holds the edit ticks anyway.
const RULES_EVERY: Duration = Duration::from_millis(100);

/// `info/exclude` in the common dir, and the file `core.excludesFile` names or,
/// unset, the default gix falls back to.
fn rule_files_of(repo: &gix::Repository) -> [Option<PathBuf>; 2] {
    let excludes_file = repo
        .config_snapshot()
        .trusted_path("core.excludesFile")
        .ok()
        .flatten()
        .or_else(|| {
            std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .or_else(|| gix::path::env::home_dir().map(|home| home.join(".config")))
                .map(|config| config.join("git").join("ignore"))
        });
    [
        Some(repo.common_dir().join("info").join("exclude")),
        excludes_file,
    ]
}

fn prints_of(files: &[Option<PathBuf>; 2]) -> [Option<Fingerprint>; 2] {
    // Followed, as gix follows it when it reads the file.
    files
        .each_ref()
        .map(|file| file.as_deref().and_then(followed_print))
}

/// The ignore rules the repository's config and index name, detached from it.
fn excludes_of(repo: &gix::Repository) -> Result<gix::worktree::Stack> {
    let index = repo
        .index_or_empty()
        .map_err(|e| Error::Watch(Box::new(e)))?;
    Ok(repo
        .excludes(
            &index,
            None,
            gix::worktree::stack::state::ignore::Source::WorktreeThenIdMappingIfNotSkipped,
        )
        .map_err(|e| Error::Watch(Box::new(e)))?
        .detach())
}

/// The worktree's own index, and what it held when an event was last judged.
///
/// Git rewrites the index whenever a refresh updates stat data, which an IDE, a
/// git GUI or a prompt running `git status` does on a timer, and a `touch`
/// rewrites nothing at all. Neither changes an entry, so neither changes what
/// the pane shows, and a walk on each of them is an idle pane doing work.
struct IndexWatch {
    path: PathBuf,
    hash: gix::hash::Kind,
    /// `None` when it could not be read, so the next event is taken.
    print: Option<IndexPrint>,
}

struct IndexPrint {
    /// `None` when git wrote no checksum, as `index.skipHash` does, since a
    /// zero there matches every index and so proves nothing.
    checksum: Option<Vec<u8>>,
    digest: u64,
}

impl IndexWatch {
    /// Starts with no print, so the first index event always walks. Seeding from
    /// the index read here would hide a change written between the pane's first
    /// walk and this watch arming: no event reports it, and every later refresh
    /// would match the seed.
    fn new(repo: &gix::Repository) -> Self {
        Self {
            path: repo.index_path(),
            hash: repo.object_hash(),
            print: None,
        }
    }

    /// Whether the index has changed what it says since it was last asked.
    /// Anything unreadable counts as a change: a wasted walk is cheaper than a
    /// staged file the pane never showed.
    fn moved(&mut self) -> bool {
        if let Some(print) = &self.print
            && print.checksum.is_some()
            && print.checksum == self.trailer()
        {
            return false;
        }
        // Unverified: a check would hash the whole file on every refresh.
        let fresh = gix::index::File::at(&self.path, self.hash, true, Default::default())
            .ok()
            .map(|file| IndexPrint::of(&file));
        let moved = fresh.is_none()
            || self.print.as_ref().map(|p| p.digest) != fresh.as_ref().map(|p| p.digest);
        self.print = fresh;
        moved
    }

    /// The checksum git wrote last, from the file's final bytes alone.
    fn trailer(&self) -> Option<Vec<u8>> {
        let len = self.hash.len_in_bytes();
        let mut file = std::fs::File::open(&self.path).ok()?;
        file.seek(SeekFrom::End(-i64::try_from(len).ok()?)).ok()?;
        let mut trailer = vec![0; len];
        file.read_exact(&mut trailer).ok()?;
        Some(trailer)
    }
}

impl IndexPrint {
    fn of(file: &gix::index::File) -> Self {
        // The flags that change what status reports. The rest are bookkeeping a
        // refresh may toggle, `FSMONITOR_VALID` among them.
        let meaning = gix::index::entry::Flags::STAGE_MASK
            | gix::index::entry::Flags::ASSUME_VALID
            | gix::index::entry::Flags::INTENT_TO_ADD
            | gix::index::entry::Flags::SKIP_WORKTREE;
        let mut digest = DefaultHasher::new();
        for entry in file.entries() {
            entry.path(file).hash(&mut digest);
            entry.id.hash(&mut digest);
            entry.mode.bits().hash(&mut digest);
            (entry.flags & meaning).bits().hash(&mut digest);
        }
        Self {
            checksum: file
                .checksum()
                .filter(|id| !id.is_null())
                .map(|id| id.as_bytes().to_vec()),
            digest: digest.finish(),
        }
    }
}

/// An OS watch whose events, and their count, go to `tx`.
fn event_backend(
    tx: Sender<Message>,
    counter: Arc<AtomicU64>,
) -> notify::Result<notify::RecommendedWatcher> {
    notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        counter.fetch_add(1, Ordering::Relaxed);
        // A watch error is not worth killing a monitor over. Dropping
        // it costs at most one missed frame, and the next real event
        // resynchronises us.
        if let Ok(event) = res {
            let _ = tx.send(Message::Event(Box::new(event)));
        }
    })
}

/// Whether an attributes file git reads names the LFS filter: every tracked
/// `.gitattributes`, `info/attributes`, the user's, system and installation
/// files, and `named` when an event names one.
fn lfs_attributes(repo: &gix::Repository, workdir: &Path, named: Option<&Path>) -> bool {
    let names_lfs = |path: &Path| {
        std::fs::read_to_string(path).is_ok_and(|text| {
            text.lines()
                .filter(|line| !line.trim_start().starts_with('#'))
                .any(|line| line.split_whitespace().any(|word| word == "filter=lfs"))
        })
    };
    let mut env = |name: &str| std::env::var_os(name);
    let user = repo
        .config_snapshot()
        .trusted_path("core.attributesFile")
        .ok()
        .flatten()
        .or_else(|| gix::attrs::Source::Git.storage_location(&mut env));
    let globals = [
        gix::attrs::Source::GitInstallation,
        gix::attrs::Source::System,
    ]
    .into_iter()
    .filter_map(|source| source.storage_location(&mut env))
    .chain(user);
    if named.is_some_and(names_lfs)
        || names_lfs(&repo.common_dir().join("info").join("attributes"))
        || globals.into_iter().any(|path| names_lfs(&path))
    {
        return true;
    }
    let Ok(index) = repo.index_or_empty() else {
        return false;
    };
    index.entries().iter().any(|entry| {
        let path = entry.path(&index);
        (path == ".gitattributes" || path.ends_with(b"/.gitattributes"))
            && names_lfs(&workdir.join(gix::path::from_bstr(path)))
    })
}

/// The reader's words for a store watch that could not be armed.
fn store_notice(error: &notify::Error) -> String {
    let reason = match &error.kind {
        notify::ErrorKind::MaxFilesWatch => "watch limit reached".to_owned(),
        notify::ErrorKind::Io(io) => io.kind().to_string(),
        _ => error.to_string(),
    };
    format!("LFS images redraw on the next change, not when their objects arrive: {reason}")
}

/// `path` spelled through its nearest existing ancestor's canonical form, which a
/// path not made yet has none of its own.
fn ancestor_spelling(path: &Path) -> Option<PathBuf> {
    path.ancestors()
        .find_map(|dir| Some(dir.canonicalize().ok()?.join(path.strip_prefix(dir).ok()?)))
}

/// `path` relative to the first of `roots` it is under.
fn within<'p>(roots: &[PathBuf], path: &'p Path) -> Option<&'p Path> {
    roots.iter().find_map(|root| path.strip_prefix(root).ok())
}

/// Every spelling of `dir`, with each worktree root's spelling when it is under one.
fn spellings_of(dir: &Path, roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut all = roots_of(dir);
    if let Some(rela) = within(roots, dir) {
        for root in roots {
            let spelling = root.join(rela);
            if !all.contains(&spelling) {
                all.push(spelling);
            }
        }
    }
    all
}

/// Every spelling of a watched root that an event path might carry.
pub(crate) fn roots_of(workdir: &Path) -> Vec<PathBuf> {
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
    mut resolve: impl FnMut(&'p Path) -> Option<(&'p Path, bool)>,
) -> Accepted {
    let mut accepted = Accepted::default();
    for path in paths.iter().rev() {
        let Some((rela, dir)) = resolve(path) else {
            continue;
        };
        accepted.relevant = true;
        // Relevant, because a directory made after the watch was armed is seen
        // only through its own event. Never the newest: Windows reports the
        // parent after a save inside it, and no row is a directory.
        if dir {
            continue;
        }
        accepted.newest = followable(rela);
        if accepted.newest.is_some() {
            break;
        }
    }
    accepted
}

/// Whether a write at `inside`, a path within the git dir, can change the pane.
fn watched_in_git_dir(inside: &Path) -> bool {
    let mut inside = inside.components().peekable();
    let Some(first) = inside.next() else {
        return false;
    };
    let first = first.as_os_str();

    // Only when it *is* that file: an `index`, `HEAD` or directory by that name
    // somewhere below belongs to another worktree or a submodule. `config`
    // shapes the clean filter, which the frame checks for itself.
    if first == OsStr::new("index")
        || first == OsStr::new("HEAD")
        || first == OsStr::new("packed-refs")
        || first == OsStr::new("config")
    {
        return inside.peek().is_none();
    }
    // `attributes` shapes the filter, and `exclude` which untracked files show.
    if first == OsStr::new("info") {
        return inside
            .next()
            .is_some_and(|c| c.as_os_str() == "attributes" || c.as_os_str() == "exclude")
            && inside.peek().is_none();
    }

    // `refs/heads/`, at any depth, because a branch name may carry slashes.
    // A bare `refs/heads` directory event names no ref and is dropped with it.
    first == OsStr::new("refs")
        && inside.next().map(|c| c.as_os_str()) == Some(OsStr::new("heads"))
        && inside.next().is_some()
}

/// Pure, and deliberately not inlined into [`Watcher::accept`], for the reason
/// `SPEC.md` §7 gives: neither of the two things it decides has a reachable integration
/// path on every platform.
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
        // Lossy for the same reason `FileChange::path` is, and it has to be the same
        // reason or the two would not compare equal.
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

    /// A directory removed when the test ends, pass or fail.
    struct Removed(PathBuf);

    impl Drop for Removed {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// A repository using LFS whose store, with its `objects`, sits outside it:
    /// the directory holding both, the worktree and the store.
    fn outside_store(name: &str) -> (Removed, PathBuf, PathBuf) {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let base = Removed(
            std::env::temp_dir().join(format!("vigia-{name}-{}-{stamp}", std::process::id())),
        );
        let (tree, store) = (base.0.join("tree"), base.0.join("store"));
        std::fs::create_dir_all(store.join("objects")).expect("store");
        std::fs::create_dir_all(&tree).expect("tree");
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .arg("-C")
                .arg(&tree)
                .args(args)
                .output()
                .expect("run git");
            assert!(out.status.success(), "git {args:?}");
        };
        git(&["init", "-q"]);
        git(&["config", "lfs.storage", &store.to_string_lossy()]);
        std::fs::create_dir_all(tree.join(".git").join("info")).expect("info");
        std::fs::write(tree.join(".git/info/attributes"), "*.png filter=lfs\n")
            .expect("attributes");
        (base, tree, store)
    }

    fn watch_with(tree: &Path, repo: &gix::Repository, arm: Arm) -> Watcher<'static> {
        let config = repo.common_dir().join("config");
        Watcher::armed_by(repo, &config, None, tree, WatchOptions::default(), arm)
            .expect("the main watch arms")
    }

    /// Whether a tick arrives within `timeout`.
    fn ticks_within(watcher: &mut Watcher<'_>, timeout: Duration) -> bool {
        let (stop, (ticked, waiting)) = (watcher.stopper(), mpsc::channel::<()>());
        let guard = std::thread::spawn(move || {
            if waiting.recv_timeout(timeout).is_err() {
                stop.stop();
            }
        });
        let tick = watcher.next_tick();
        let _ = ticked.send(());
        guard.join().expect("the guard");
        tick.is_some()
    }

    /// Only a guard against a hang: a passing wait returns when the arm does.
    /// `VIGIA_TEST_CEILING` (seconds) shortens it for a mutation run.
    fn arming() -> Duration {
        let guard = Duration::from_secs(60);
        std::env::var("VIGIA_TEST_CEILING")
            .ok()
            .and_then(|secs| secs.parse().ok())
            .map_or(guard, |secs| guard.min(Duration::from_secs(secs)))
    }

    /// A store arm that fails as a full inotify table does.
    #[test]
    fn store_arm_fails_soft() {
        let (_base, tree, store) = outside_store("store-arm");
        let repo = gix::open(&tree).expect("open");
        // As a full table does: partway through, holding what it added.
        let mut watcher = watch_with(&tree, &repo, |backend, path, mode| {
            let _ = backend.watch(path, mode);
            Err(notify::Error::new(notify::ErrorKind::MaxFilesWatch))
        });
        assert!(
            ticks_within(&mut watcher, arming()),
            "the failure did not tick"
        );
        let notice = watcher.take_store_notice().expect("no notice");
        assert!(
            notice.starts_with("LFS images redraw on the next change")
                && notice.ends_with("watch limit reached"),
            "{notice}"
        );
        assert_eq!(watcher.take_store_notice(), None, "the notice came twice");

        std::fs::write(store.join("objects").join("object"), "x\n").expect("write");
        assert!(
            !ticks_within(&mut watcher, Duration::from_millis(600)),
            "the failed store watch was left armed"
        );
        std::fs::write(tree.join("a.txt"), "x\n").expect("write");
        assert!(
            ticks_within(&mut watcher, Duration::from_secs(10)),
            "a tree write did not tick"
        );
        assert_eq!(watcher.take_store_notice(), None, "the notice came again");
    }

    /// The tree keeps ticking while the store's watch is slow to arm.
    #[test]
    fn tree_ticks_during_store_arm() {
        const SLOW: Duration = Duration::from_secs(3);
        let (_base, tree, _store) = outside_store("store-slow");
        let repo = gix::open(&tree).expect("open");
        let mut watcher = watch_with(&tree, &repo, |backend, path, mode| {
            std::thread::sleep(SLOW);
            backend.watch(path, mode)
        });
        let started = std::time::Instant::now();
        std::fs::write(tree.join("a.txt"), "x\n").expect("write");
        assert!(
            ticks_within(&mut watcher, SLOW),
            "the tree write waited on the store"
        );
        assert!(started.elapsed() < SLOW, "the tick came after the arm");
        assert!(
            watcher.store_watched().is_empty(),
            "the store armed before the tree ticked"
        );
    }

    /// An object that lands while the store's watch arms still ticks.
    #[test]
    fn object_during_arm_ticks() {
        const SLOW: Duration = Duration::from_secs(1);
        let (_base, tree, store) = outside_store("store-landing");
        let repo = gix::open(&tree).expect("open");
        let mut watcher = watch_with(&tree, &repo, |backend, path, mode| {
            std::thread::sleep(SLOW);
            backend.watch(path, mode)
        });
        std::fs::write(store.join("objects").join("object"), "x\n").expect("write");
        assert!(
            ticks_within(&mut watcher, arming()),
            "an object landing during the arm produced no tick"
        );
    }

    /// A store watch that ticks is not counted as filtered; one that does not is.
    #[test]
    fn armed_filter_counts() {
        let (base, tree, store) = outside_store("store-filtered");
        let repo = gix::open(&tree).expect("open");
        let mut watcher = watch_with(&tree, &repo, |_, _, _| Ok(()));
        let before = watcher.stats.filtered;
        let ancestor = Message::Armed {
            path: base.0.clone(),
            mode: RecursiveMode::NonRecursive,
            result: Ok(()),
        };
        let accepted = watcher.accept(ancestor).expect("not a stop");
        assert!(!accepted.relevant, "an ancestor's arm ticked");
        assert_eq!(
            watcher.stats.filtered,
            before + 1,
            "an ancestor's arm was not filtered"
        );
        let objects = Message::Armed {
            path: store.join("objects"),
            mode: RecursiveMode::Recursive,
            result: Ok(()),
        };
        let accepted = watcher.accept(objects).expect("not a stop");
        assert!(accepted.relevant, "the store's arm did not tick");
        assert_eq!(
            watcher.stats.filtered,
            before + 1,
            "a ticking arm was filtered"
        );
    }

    /// An I/O failure says its kind, not its message.
    #[test]
    fn store_notice_reason() {
        let error = notify::Error::io(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "EACCES on the store",
        ));
        let notice = store_notice(&error);
        assert!(notice.ends_with(": permission denied"), "{notice}");
    }

    /// A path not made yet is spelled through its nearest real ancestor.
    #[cfg(unix)]
    #[test]
    fn ancestor_spelled_canonically() {
        let (base, _tree, _store) = outside_store("store-spelling");
        let real = base.0.join("real");
        std::fs::create_dir_all(&real).expect("real");
        let link = base.0.join("link");
        std::os::unix::fs::symlink(&real, &link).expect("link");
        let spelled = ancestor_spelling(&link.join("lfs").join("objects"));
        let expected = real
            .canonicalize()
            .expect("canonical")
            .join("lfs")
            .join("objects");
        assert_eq!(spelled, Some(expected));
    }

    /// A store named through a symlink is matched by its real spelling too, which
    /// is the one an event reports.
    #[cfg(unix)]
    #[test]
    fn store_canonical_spelling() {
        let (base, tree, _store) = outside_store("store-linked");
        let real = base.0.join("real");
        std::fs::create_dir_all(&real).expect("real");
        let link = base.0.join("link");
        std::os::unix::fs::symlink(&real, &link).expect("link");
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(&tree)
            .args(["config", "lfs.storage"])
            .arg(link.join("store"))
            .output()
            .expect("git config");
        assert!(out.status.success(), "git config");
        let repo = gix::open(&tree).expect("open");
        let watcher = watch_with(&tree, &repo, |_, _, _| Ok(()));
        let canonical = real
            .canonicalize()
            .expect("canonical")
            .join("store")
            .join("objects");
        assert!(
            watcher.store.contains(&canonical),
            "{canonical:?} is not among {:?}",
            watcher.store
        );
    }

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

    /// A branch tip moving is a change the monitor must see, and until it did not.
    #[test]
    fn a_branch_tip_moving_is_a_change_the_monitor_must_see() {
        let watched = |parts: &[&str]| {
            watched_in_git_dir(native(parts).strip_prefix(".git").expect("under .git"))
        };

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

    /// What stays excluded, and why each one would be a wake that draws exactly
    /// what was already on screen.
    #[test]
    fn the_rest_of_the_git_directory_still_wakes_nothing() {
        let watched = |parts: &[&str]| {
            watched_in_git_dir(native(parts).strip_prefix(".git").expect("under .git"))
        };

        assert!(!watched(&[".git", "refs", "remotes", "origin", "main"]));
        assert!(!watched(&[".git", "refs", "tags", "v1.0.0"]));
        assert!(!watched(&[".git", "logs", "HEAD"]));
        assert!(!watched(&[".git", "ORIG_HEAD"]));
        assert!(!watched(&[".git", "objects", "ab", "cdef01"]));
        assert!(!watched(&[".git", "COMMIT_EDITMSG"]));
        assert!(!watched(&[".git", "config.lock"]));

        // The lock file itself is not the write, and that is deliberate rather than an
        // oversight this widening should have swept up.
        assert!(!watched(&[".git", "index.lock"]));

        // A directory event rather than a file one. `refs/heads` itself names no
        // ref, and a `HEAD` that is a directory somewhere below is not the file.
        assert!(!watched(&[".git", "refs", "heads"]));
        assert!(!watched(&[".git", "refs"]));
        assert!(
            !watched(&[".git", "worktrees", "other", "HEAD"]),
            "another worktree's HEAD is another worktree's business"
        );
        assert!(!watched(&[".git", "worktrees", "other", "index"]));
        assert!(!watched(&[".git", "modules", "sub", "index"]));
    }

    /// What shapes the clean filter wakes the watch, so a change to it reaches
    /// the pane with nothing else written.
    #[test]
    fn filter_sources_wake() {
        let watched = |parts: &[&str]| {
            watched_in_git_dir(native(parts).strip_prefix(".git").expect("under .git"))
        };
        assert!(watched(&[".git", "config"]), "a config write woke nothing");
        assert!(
            watched(&[".git", "info", "attributes"]),
            "an info/attributes write woke nothing"
        );
        assert!(
            watched(&[".git", "info", "exclude"]),
            "an info/exclude write woke nothing, so the untracked files it hides stayed listed"
        );
        assert!(
            !watched(&[".git", "info"]),
            "the info directory itself is not the file"
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
    fn take_all(path: &Path) -> Option<(&Path, bool)> {
        Some((path, false))
    }

    /// The one case where the order of an event's paths is observable, and the reason
    /// it is unreachable from an integration test: a rename is the only event that
    /// names two files, and every other event makes first and last the same path.
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

    /// A directory event still wakes the pane, and names nothing to follow.
    #[test]
    fn directory_is_not_newest() {
        let paths = [native(&["src", "a.rs"]), native(&["src"])];
        let accepted = accept_paths(&paths, |path| Some((path, path == Path::new("src"))));

        assert!(accepted.relevant);
        assert_eq!(accepted.newest.as_deref(), Some("src/a.rs"));

        let accepted = accept_paths(&paths[1..], |path| Some((path, true)));
        assert!(
            accepted.relevant,
            "a new directory stopped producing a tick"
        );
        assert_eq!(accepted.newest, None);
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

        /// The victim is fixed by arrival order, so the same burst drops the same
        /// path on every run.
        #[test]
        fn oldest_evicted_first() {
            let mut burst = Burst::default();
            for n in 0..HISTORY_PATHS + 1 {
                burst.push(format!("f{n}"));
            }
            let (paths, dropped) = burst.finish();
            let expected: Vec<String> = (1..=HISTORY_PATHS).map(|n| format!("f{n}")).collect();
            assert_eq!(dropped, 1);
            assert_eq!(paths, expected);
        }

        /// Oldest by arrival, not least recently touched: the history store
        /// orders by recency.
        #[test]
        fn repeat_keeps_its_place() {
            let mut burst = Burst::default();
            for n in 0..HISTORY_PATHS {
                burst.push(format!("f{n}"));
            }
            burst.push("f0".to_owned());
            burst.push(format!("f{HISTORY_PATHS}"));
            let (paths, dropped) = burst.finish();

            let expected: Vec<String> = (1..=HISTORY_PATHS).map(|n| format!("f{n}")).collect();
            assert_eq!(dropped, 1);
            assert_eq!(
                paths, expected,
                "the repeat moved f0 to the back, so f1 was evicted instead"
            );
        }
    }
}
