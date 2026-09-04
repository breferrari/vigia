//! The frame path: I2a.

use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, SystemTime};

use crate::change::{ChangeKind, FileChange, Origin, Side};
use crate::error::Result;
use crate::hunk::{FileDiff, FileSpan};
use crate::worktree::{ChangeOptions, Worktree};

/// What a [`Frame`] has done since it was created.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrameStats {
    /// Diffs computed from content.
    pub computed: u64,
    /// Diffs served unchanged from an earlier frame.
    pub reused: u64,
    /// Files counted by [`Frame::height`] without their text being built.
    pub measured: u64,
    /// Bytes compared by computed diffs.
    pub bytes: u64,
    /// `stat` calls made, either to record a fingerprint or to check one.
    pub probes: u64,
    /// Cached diffs dropped because their path stopped being changed.
    pub evicted: u64,
    /// Heights kept from an earlier read because their file moved inside the
    /// settle margin, each to be read again once it settles.
    pub deferred: u64,
}

/// A working-tree fingerprint that costs no read: size and modification time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Fingerprint {
    len: u64,
    mtime: SystemTime,
}

/// A fingerprint taken after a read, plus whether it may be trusted as one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Observed {
    print: Fingerprint,
    /// True when the granule `print.mtime` was stamped in had already closed before the
    /// read began. That, and not merely a modification time in the past, is what makes
    /// an unchanged fingerprint proof of unchanged bytes.
    settled: bool,
}

/// How far in the past a modification time has to sit before it identifies the
/// bytes that were read.
const SETTLE_MARGIN: Duration = Duration::from_secs(2);

/// Whether a modification time observed by a read starting at `read_started`
/// identifies the bytes that read returned.
fn settled(mtime: SystemTime, read_started: SystemTime) -> bool {
    // Subtraction rather than addition on purpose: adding the margin to `mtime`
    // needs an overflow arm no filesystem can reach, where a future modification
    // time is an `Err` and reachable.
    read_started
        .duration_since(mtime)
        .is_ok_and(|gap| gap >= SETTLE_MARGIN)
}

/// What a cached artefact was taken under, and therefore what proves it is still
/// true.
#[derive(Clone)]
struct Taken {
    kind: ChangeKind,
    before: Option<gix::ObjectId>,
    /// The right-hand side this artefact was computed from.
    after: Option<Side>,
    /// The working-tree side as it was when the content was read. `None` when
    /// this artefact has no working-tree side, or when it could not be
    /// fingerprinted.
    worktree: Option<Observed>,
}

impl Taken {
    /// The evidence a change carries, plus what a read observed of its
    /// working-tree side.
    fn of(change: &FileChange, worktree: Option<Observed>) -> Self {
        Self {
            kind: change.kind.clone(),
            before: change.before,
            after: change.after,
            worktree,
        }
    }
}

/// One cache, split by which run a change came from.
#[derive(Debug)]
struct Cache<T> {
    runs: [HashMap<String, T>; 2],
}

impl<T> Default for Cache<T> {
    fn default() -> Self {
        Self {
            runs: [HashMap::new(), HashMap::new()],
        }
    }
}

impl<T> Cache<T> {
    fn of(origin: Origin) -> usize {
        match origin {
            Origin::Unstaged => 0,
            Origin::Staged => 1,
        }
    }

    fn get(&self, change: &FileChange) -> Option<&T> {
        self.runs[Self::of(change.origin)].get(change.path.as_str())
    }

    fn get_mut(&mut self, change: &FileChange) -> Option<&mut T> {
        self.runs[Self::of(change.origin)].get_mut(change.path.as_str())
    }

    /// Store `value` for this change, keeping the key the map already owns.
    fn put(&mut self, change: &FileChange, value: T) {
        let run = &mut self.runs[Self::of(change.origin)];
        match run.get_mut(change.path.as_str()) {
            Some(slot) => *slot = value,
            None => {
                run.insert(change.path.clone(), value);
            }
        }
    }

    fn remove(&mut self, change: &FileChange) -> Option<T> {
        self.runs[Self::of(change.origin)].remove(change.path.as_str())
    }

    /// Move this change's entry out of `previous` and into this cache, carrying
    /// the `String` the old map owns rather than allocating a new one.
    fn migrate(&mut self, previous: &mut Self, change: &FileChange, mut then: impl FnMut(&mut T)) {
        let run = Self::of(change.origin);
        if let Some((path, mut value)) = previous.runs[run].remove_entry(change.path.as_str()) {
            then(&mut value);
            self.runs[run].insert(path, value);
        }
    }

    fn len(&self) -> usize {
        self.runs.iter().map(HashMap::len).sum()
    }

    fn clear(&mut self) {
        for run in &mut self.runs {
            run.clear();
        }
    }

    /// Reserved per run from the real split, not halved.
    fn reserve(&mut self, unstaged: usize, staged: usize) {
        self.runs[Self::of(Origin::Unstaged)].reserve(unstaged);
        self.runs[Self::of(Origin::Staged)].reserve(staged);
    }
}

/// One path's diff, with everything needed to know it is still true.
struct Cached {
    taken: Taken,
    diff: FileDiff,
}

/// One path's height, with everything needed to know it is still true.
struct Measured {
    /// What this span was taken under, or `None` when the read that would have
    /// produced it failed.
    taken: Option<Taken>,
    span: FileSpan,
    /// Whether this tick has asked about this span: proved it, or deliberately
    /// kept it while its file is inside the settle margin.
    answered: bool,
}

/// Fingerprint a working-tree file, or `None` when it cannot be.
fn fingerprint(path: &Path) -> Option<Fingerprint> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    Some(Fingerprint {
        len: meta.len(),
        mtime: meta.modified().ok()?,
    })
}

/// What a change's own evidence says about an artefact taken under it, before a
/// fingerprint is spent.
enum Provenance {
    /// The kind or a side moved: the artefact describes another comparison.
    Moved,
    /// Computed from the index side alone, so nothing on disk could have gone stale.
    NoWorktree,
    /// The working-tree side has to be asked, and this is what the read observed of
    /// it, or `None` when it could not be fingerprinted then.
    Worktree(Option<Observed>),
}

fn provenance(taken: &Taken, current: &FileChange) -> Provenance {
    // A new blob on either side is a new diff even when the file on disk never
    // moved, and a new kind is a different diff outright.
    if taken.kind != current.kind || taken.before != current.before || taken.after != current.after
    {
        return Provenance::Moved;
    }
    // A removal, a conflict and a type change are computed from the left side
    // alone, so they have no working-tree side that could have gone stale.
    if !current.reads_worktree() {
        return Provenance::NoWorktree;
    }
    Provenance::Worktree(taken.worktree)
}

/// Whether a diff taken from a file still describes the working tree.
fn reusable(
    taken: &Taken,
    current: &FileChange,
    fresh: impl FnOnce() -> Option<Fingerprint>,
) -> bool {
    match provenance(taken, current) {
        Provenance::Moved => false,
        Provenance::NoWorktree => true,
        // Unfingerprintable then is not a failure, and it forbids reuse all the same:
        // the alternative is drawing a diff we cannot vouch for.
        Provenance::Worktree(None) => false,
        // Unfingerprintable now forbids it the same way. An unsettled observation is
        // refused before the stat, so that corner costs one syscall rather than two.
        Provenance::Worktree(Some(observed)) => {
            observed.settled && fresh().is_some_and(|fresh| observed.print == fresh)
        }
    }
}

/// What a height taken earlier is worth now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    /// The file still says what it said, so the height stands.
    Keep,
    /// The file moved inside the settle margin, so the height stands until the
    /// moment it settles, which is carried.
    Wait(SystemTime),
    /// The file moved and has settled, or nothing vouches for it: read it again.
    Remeasure,
}

/// A diff is refused the moment its file moves, because drawing bytes nobody can
/// vouch for is wrong; a height is a proportion, and one two seconds stale costs
/// less than reading every file inside a burst.
fn verdict(
    taken: &Taken,
    current: &FileChange,
    now: SystemTime,
    fresh: impl FnOnce() -> Option<Fingerprint>,
) -> Verdict {
    let observed = match provenance(taken, current) {
        Provenance::Moved | Provenance::Worktree(None) => return Verdict::Remeasure,
        Provenance::NoWorktree => return Verdict::Keep,
        Provenance::Worktree(Some(observed)) => observed,
    };
    let Some(fresh) = fresh() else {
        return Verdict::Remeasure;
    };
    // An unchanged print proves the bytes only if the read that took it began after
    // the file's granule closed; taken earlier, it is read again once it settles.
    if observed.print == fresh && observed.settled {
        return Verdict::Keep;
    }
    // A modification time ahead of the clock never settles, so waiting on it would
    // freeze the height for as long as the skew lasts; it is read, as a diff is.
    if now.duration_since(fresh.mtime).is_err() {
        return Verdict::Remeasure;
    }
    if settled(fresh.mtime, now) {
        Verdict::Remeasure
    } else {
        Verdict::Wait(fresh.mtime.checked_add(SETTLE_MARGIN).unwrap_or(now))
    }
}

/// The working-tree diff, advanced one frame at a time.
///
/// Holds the previous frame's diffs and revalidates them, which is the whole of
/// I2a. Created by [`Worktree::frame`](crate::Worktree::frame).
///
/// ```no_run
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let worktree = vigia_core::Worktree::discover(".")?;
/// let mut frame = worktree.frame();
/// frame.advance()?;
/// for i in 0..frame.files().len() {
///     let (change, diff) = frame.diff(i)?;
///     println!("{:?} {} +{} -{}", change.kind, diff.path, diff.added, diff.removed);
/// }
/// println!("{:?}", frame.stats());
/// # Ok(())
/// # }
/// ```
pub struct Frame<'w> {
    worktree: &'w Worktree,
    files: Vec<FileChange>,
    cached: Cache<Cached>,
    /// How tall every changed file is, once something has asked.
    spans: Cache<Measured>,
    /// The attributes files in the changed set, and what they looked like, as of
    /// the last tick.
    attributes: HashMap<String, Option<Fingerprint>>,
    /// The failure [`Frame::diff`] last contained, held only so it can be handed
    /// back by reference.
    ///
    /// Deliberately not a cache: everything reachable from the caches is read as
    /// evidence, and a failed read is evidence about nothing.
    failure: Option<FileDiff>,
    /// Whether the staged run is drawn beside the unstaged one.
    staged: bool,
    /// Where [`Self::files`]'s staged run begins, recorded by the walk that built
    /// it rather than recovered by scanning. See [`Frame::staged_at`].
    staged_at: usize,
    stats: FrameStats,
    /// The moment the last height kept waiting inside the settle margin settles,
    /// for a wake at that moment rather than at the next event.
    settles_at: Option<SystemTime>,
}

impl<'w> Frame<'w> {
    pub(crate) fn new(worktree: &'w Worktree) -> Self {
        Self {
            worktree,
            files: Vec::new(),
            cached: Cache::default(),
            spans: Cache::default(),
            attributes: HashMap::new(),
            failure: None,
            staged: false,
            staged_at: 0,
            stats: FrameStats::default(),
            settles_at: None,
        }
    }

    /// Re-read which files changed, keeping every diff still known to be valid.
    ///
    /// # Errors
    ///
    /// The status walk fails.
    pub fn advance(&mut self) -> Result<()> {
        // Every wait is decided again by the walk that follows it, and disarmed
        // before the walk rather than after: the loop folds a spent deadline to a
        // zero wait and advances on it, so a walk that failed and left the deadline
        // armed would be walked again on the next zero wait, flat out, for as long
        // as it kept failing. A clock that cannot do its work stops, and the next
        // event brings the walk back.
        self.settles_at = None;

        let mut files = Vec::with_capacity(self.files.len());
        for change in self.worktree.changes()? {
            files.push(change?);
        }
        // Unstaged first, then staged, and the order is the product.
        let staged_at = files.len();
        if self.staged {
            for change in self
                .worktree
                .changes_of(Origin::Staged, ChangeOptions::default())?
            {
                files.push(change?);
            }
        }

        // Nothing above this line mutated the frame's picture of the worktree, which
        // is what makes a failed walk leave the previous frame intact.

        // The clean filter is rebuilt by the next read rather than kept for the life of
        // the process.
        self.worktree.invalidate_filter();

        // And what was computed under the old rules goes with it.
        let taken_at = SystemTime::now();
        let attributes: HashMap<String, Option<Fingerprint>> = files
            .iter()
            .filter(|change| change.rewrites_attributes())
            .map(|change| {
                let path = self.worktree.workdir().join(&change.path);
                self.stats.probes += 1;
                (change.path.clone(), fingerprint(&path))
            })
            .collect();
        // Only the files that have a fingerprint have to prove it.
        let provable = attributes
            .values()
            .copied()
            .flatten()
            .all(|print| settled(print.mtime, taken_at));
        if !provable || attributes != self.attributes {
            // Credited before the clear, for the reason [`Frame::show_staged`] credits
            // its own.
            self.stats.evicted += self.cached.len() as u64;
            self.cached.clear();
            self.spans.clear();
        }
        self.attributes = attributes;

        // Both caches are migrated, and neither is dropped. Clearing a span here rests
        // on its being derived from content with no freshness check of its own.
        let mut previous = std::mem::take(&mut self.cached);
        self.cached.reserve(staged_at, files.len() - staged_at);
        let mut previous_spans = std::mem::take(&mut self.spans);
        self.spans.reserve(staged_at, files.len() - staged_at);
        for change in &files {
            // `Cache::migrate` moves the key the old map owns rather than cloning
            // it. With two maps that is up to 2N needless `String` allocations a
            // tick, which on the hundred-file gate is two hundred.
            self.cached.migrate(&mut previous, change, |_| {});
            self.spans.migrate(&mut previous_spans, change, |measured| {
                // Carried, and not yet asked. What it described was true of the
                // previous tick, and `fill_span` is where it is asked again.
                measured.answered = false;
            });
        }
        // Whatever is left is a path that stopped being changed.
        self.stats.evicted += previous.len() as u64;

        self.staged_at = staged_at;
        self.files = files;
        Ok(())
    }

    /// Where the staged run begins in [`Frame::files`].
    pub fn staged_at(&self) -> usize {
        self.staged_at
    }

    /// Draw the staged run, or stop drawing it.
    pub fn show_staged(&mut self, staged: bool) {
        if self.staged == staged {
            return;
        }
        self.staged = staged;
        // Credited before the clear, so I3's bound stays observable across a toggle.
        self.stats.evicted += self.cached.len() as u64;
        self.cached.clear();
        self.spans.clear();
        self.settles_at = None;
    }

    /// The changed files, in the order status reported them.
    pub fn files(&self) -> &[FileChange] {
        &self.files
    }

    /// Counters for what this frame path has done.
    pub fn stats(&self) -> FrameStats {
        self.stats
    }

    /// Diffs currently held between frames.
    pub fn tracked(&self) -> usize {
        self.cached.len()
    }

    /// Heights currently held between frames.
    pub fn tracked_spans(&self) -> usize {
        self.spans.len()
    }

    /// How long until the last height kept waiting inside the settle margin
    /// settles, or `None` when none is waiting. No filesystem event marks a write
    /// ending, so a wake at that moment has nothing to be armed from but this.
    /// Decided by each tick's walk, so a diff taken between two ticks can leave it
    /// one wake early, and a wake that finds nothing waiting costs a walk and no read.
    pub fn settles_in(&self, now: SystemTime) -> Option<Duration> {
        self.settles_at
            .map(|at| at.duration_since(now).unwrap_or(Duration::ZERO))
    }

    /// Re-read the changed set once the last height kept waiting has settled, so
    /// the walk asks it again and reads it once; nothing while the wait still runs
    /// or none is armed. The loop asks on every timeout without knowing which of
    /// its clocks fired, so the decision lives here rather than at the call.
    ///
    /// # Errors
    ///
    /// The status walk fails, as [`Frame::advance`]'s does.
    pub fn advance_if_settled(&mut self, now: SystemTime) -> Result<()> {
        if self.settles_at.is_some_and(|at| at <= now) {
            self.advance()?;
        }
        Ok(())
    }

    /// How many rows the whole diff is, counting every changed file.
    ///
    /// # Errors
    ///
    /// A file's span cannot be measured, which is a read of either side.
    pub fn height(&mut self, rows_of: impl Fn(&FileChange, &FileSpan) -> usize) -> Result<usize> {
        // One clock for the walk, so two files written in the same instant fall on
        // the same side of the margin.
        let now = SystemTime::now();
        let mut total = 0usize;
        for index in 0..self.files.len() {
            self.fill_span(index, now)?;
            let change = &self.files[index];
            total += rows_of(change, &self.span_of(change).span);
        }
        Ok(total)
    }

    /// How many rows one file occupies, from its span.
    ///
    /// # Errors
    ///
    /// The file's span cannot be measured, which is a read of either side.
    pub fn rows_of(
        &mut self,
        index: usize,
        rows_of: impl Fn(&FileChange, &FileSpan) -> usize,
    ) -> Result<usize> {
        self.fill_span(index, SystemTime::now())?;
        let change = &self.files[index];
        Ok(rows_of(change, &self.span_of(change).span))
    }

    /// The span this change's row is drawn from, which [`Self::fill_span`] has
    /// just guaranteed exists.
    fn span_of(&self, change: &FileChange) -> &Measured {
        self.spans
            .get(change)
            .expect("fill_span guarantees this, and both callers fill first")
    }

    /// Answer for the height of the file at `index` this tick: kept, kept while its
    /// file settles, or read again.
    ///
    /// # Errors
    ///
    /// The measure fails in a way that is not one file's, which [`Frame::diff`]
    /// propagates for the same reason.
    fn fill_span(&mut self, index: usize, now: SystemTime) -> Result<()> {
        let change = &self.files[index];
        if self
            .spans
            .get(change)
            .is_some_and(|measured| measured.answered)
        {
            return Ok(());
        }
        let path = self.worktree.workdir().join(&change.path);
        let mut probed = false;
        let mut fresh = || {
            probed = true;
            fingerprint(&path)
        };

        // (1) A span carried from an earlier tick, or put beside the diff this tick
        // took. It was true of the file when it was taken, and `advance` migrated it
        // without asking whether the file moved, so this is where it is asked: one
        // stat, and no read while the file is still being written. A diff in hand
        // is never asked here: every fresh diff puts its span beside it, so a span
        // is at least as fresh as any diff for the same path.
        let what = if let Some(measured) = self.spans.get_mut(change)
            && let Some(taken) = measured.taken.as_ref()
        {
            let what = verdict(taken, change, now, &mut fresh);
            if what != Verdict::Remeasure {
                measured.answered = true;
            }
            what
        } else {
            Verdict::Remeasure
        };
        // Counted from whether the closure ran, so `probes` stays a count of
        // syscalls taken rather than of call sites reached.
        self.stats.probes += u64::from(probed);
        match what {
            Verdict::Remeasure => {}
            Verdict::Keep => return Ok(()),
            Verdict::Wait(until) => {
                self.stats.deferred += 1;
                // The last print to settle, so one wake serves the whole burst: on
                // it every waiting file has settled and is read once. The first
                // would fire once per settle instant, and a sweep whose prints
                // spread over a hundred milliseconds would be a cascade of walks.
                self.settles_at = Some(self.settles_at.map_or(until, |at| at.max(until)));
                return Ok(());
            }
        }

        // (2) A read.
        let read_started = SystemTime::now();
        // The read reports what it spent deciding *how* to read, and it is
        // folded into the same counter as the fingerprints. Added before the
        // `match`, so a failed read still reports the probe it took.
        let mut probes = 0;
        let measured = self.worktree.measure_counted(change, &mut probes);
        self.stats.probes += probes;
        let (span, taken) = match measured {
            Ok(span) => {
                self.stats.measured += 1;
                self.stats.bytes += span.bytes;
                let worktree = if change.reads_worktree() {
                    self.stats.probes += 1;
                    fingerprint(&path).map(|print| Observed {
                        print,
                        settled: settled(print.mtime, read_started),
                    })
                } else {
                    None
                };
                (span, Some(Taken::of(change, worktree)))
            }
            // A failed read describes nothing, so it is recorded with no evidence at
            // all and [`Measured::taken`] carries why that matters. The flag keeps
            // this walk's height and the screen agreeing, because a row that draws
            // a note is two rows rather than one.
            //
            // The same split [`Frame::diff`] makes, because this walk reaches files
            // it does not: a file above the viewport is measured and never diffed,
            // so swallowing a whole-comparison failure here would put rows in the
            // total that nothing can draw when the reader scrolls to them.
            Err(e) if e.of_one_file().is_none() => return Err(e),
            Err(_) => (
                FileSpan {
                    unreadable: true,
                    ..FileSpan::default()
                },
                None,
            ),
        };
        let measured = Measured {
            taken,
            span,
            answered: true,
        };
        self.spans.put(change, measured);
        Ok(())
    }

    /// The change at `index` and its diff, computed now or reused from an
    /// earlier frame.
    /// # Panics
    ///
    /// If `index` is out of range, the same way indexing a slice does. Holding an
    /// index across [`Frame::advance`] is how: the list shrinks under it.
    ///
    /// # Errors
    ///
    /// The comparison fails in a way that is not one file's: [`Error::of_one_file`]
    /// decides which, and a failure that names a single path becomes that file's
    /// note instead.
    ///
    /// [`Error::of_one_file`]: crate::Error::of_one_file
    pub fn diff(&mut self, index: usize) -> Result<(&FileChange, &FileDiff)> {
        let change = &self.files[index];
        let path = self.worktree.workdir().join(&change.path);

        // Lazily, for the reason [`reusable`] gives: a `stat` this answer will
        // not read is a syscall bought for nothing, and `probes` should count
        // the ones actually taken.
        let mut probed = false;
        let reuse = match self.cached.get(change) {
            None => false,
            Some(cached) => reusable(&cached.taken, change, || {
                probed = true;
                fingerprint(&path)
            }),
        };
        self.stats.probes += u64::from(probed);

        if reuse {
            self.stats.reused += 1;
            // Proved unchanged, and the height taken from this diff is proved with it.
            if let Some(measured) = self.spans.get_mut(change) {
                measured.answered = true;
            }
            let diff = &self
                .cached
                .get(change)
                .expect("`reuse` is only true where the lookup above found one")
                .diff;
            return Ok((change, diff));
        }

        // Timed from before the read starts, so the window a write would have
        // to land in to be missed is over-stated rather than under-stated.
        let read_started = SystemTime::now();
        // Counted before the `?`, so a failed read still reports the probe it
        // took. See `Worktree::diff_counted`.
        let mut probes = 0;
        let computed = self.worktree.diff_counted(change, &mut probes);
        self.stats.probes += probes;
        let diff = match computed {
            Ok(diff) => diff,
            // One path's own failure costs that path a note and nothing else: held
            // as an error it would end the whole comparison, and a frame the shell
            // keeps because the next one failed is a pane that looks quiet and is
            // lying. A failure that is not one path's still ends it, because
            // nothing is left to vouch for the rest of the comparison.
            Err(e) => {
                let reason = e.of_one_file().ok_or(e)?;
                // Both caches drop this path. What they hold described a read that
                // is no longer the answer, and a height nothing can vouch for is a
                // height the screen does not draw.
                self.cached.remove(change);
                self.spans.remove(change);
                let failure = self
                    .failure
                    .insert(FileDiff::without_hunks(change.path.clone(), Some(reason)));
                return Ok((change, failure));
            }
        };
        let worktree = if change.reads_worktree() {
            self.stats.probes += 1;
            fingerprint(&path).map(|print| Observed {
                print,
                settled: settled(print.mtime, read_started),
            })
        } else {
            None
        };

        self.stats.computed += 1;
        self.stats.bytes += diff.bytes;
        // The height goes with the diff it was taken from: this read proved both, and
        // a walk that asked the file again would spend a second stat on every drawn
        // file every tick.
        let taken = Taken::of(change, worktree);
        self.spans.put(
            change,
            Measured {
                taken: Some(taken.clone()),
                span: FileSpan::from(&diff),
                answered: true,
            },
        );
        self.cached.put(change, Cached { taken, diff });
        let diff = &self.cached.get(change).expect("just inserted").diff;
        Ok((change, diff))
    }
}

#[cfg(test)]
mod tests {
    //! The reuse rule, tested as the pure function it is.

    use std::time::Duration;

    use super::*;

    fn blob(byte: u8) -> Option<gix::ObjectId> {
        Some(gix::ObjectId::from_bytes_or_panic(&[byte; 20]))
    }

    fn print(len: u64, mtime: SystemTime) -> Fingerprint {
        Fingerprint { len, mtime }
    }

    fn epoch(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
    }

    fn change(kind: ChangeKind, index_blob: Option<gix::ObjectId>) -> FileChange {
        let after = crate::worktree::reads_side(&kind);
        FileChange {
            path: "src/lib.rs".to_owned(),
            kind,
            origin: Origin::Unstaged,
            before: index_blob,
            after,
            // These unit tests are over [`reusable`], which does not consult it:
            // the field decides how a file is *read*, and nothing here reads one.
            maybe_symlink: false,
        }
    }

    /// The same path as a staged change: two object ids and no file at all.
    fn staged_change(before: Option<gix::ObjectId>, after: Option<gix::ObjectId>) -> FileChange {
        FileChange {
            path: "src/lib.rs".to_owned(),
            kind: ChangeKind::Modified,
            origin: Origin::Staged,
            before,
            after: after.map(Side::Blob),
            maybe_symlink: true,
        }
    }

    /// The evidence an artefact was taken under.
    fn taken(
        kind: ChangeKind,
        index_blob: Option<gix::ObjectId>,
        worktree: Option<Observed>,
    ) -> Taken {
        let after = crate::worktree::reads_side(&kind);
        Taken {
            kind,
            before: index_blob,
            after,
            worktree,
        }
    }

    /// [`taken`] for a staged artefact, whose right-hand side is a blob.
    fn taken_staged(before: Option<gix::ObjectId>, after: Option<gix::ObjectId>) -> Taken {
        Taken {
            kind: ChangeKind::Modified,
            before,
            after: after.map(Side::Blob),
            worktree: None,
        }
    }

    /// A fingerprint already proved to identify its bytes: the reusable case.
    fn trusted(len: u64, mtime: SystemTime) -> Option<Observed> {
        Some(Observed {
            print: print(len, mtime),
            settled: true,
        })
    }

    /// A staged artefact is proved by two object ids and nothing else, which is the arm
    /// added.
    #[test]
    fn a_staged_artefact_with_both_ids_unchanged_is_reusable_without_a_fingerprint() {
        assert!(reusable(
            &taken_staged(blob(1), blob(2)),
            &staged_change(blob(1), blob(2)),
            || panic!("a staged artefact must not reach for a fingerprint"),
        ));
    }

    /// And either id moving is a different diff.
    #[test]
    fn a_staged_artefact_is_refused_when_either_side_moved() {
        assert!(
            !reusable(
                &taken_staged(blob(1), blob(2)),
                &staged_change(blob(9), blob(2)),
                || None,
            ),
            "HEAD moved under the index"
        );
        assert!(
            !reusable(
                &taken_staged(blob(1), blob(2)),
                &staged_change(blob(1), blob(9)),
                || None,
            ),
            "something else was staged"
        );
    }

    /// The rule that cannot be raced, so it is checked arithmetically instead.
    #[test]
    fn a_read_inside_the_granule_it_was_stamped_in_is_never_settled() {
        // NTFS observed at 1ms, Windows' default timer at 15.625ms, ext4 at a
        // jiffy, HFS+ and ext3 at 1s, FAT and exFAT at 2s.
        for granule in [
            Duration::from_nanos(100),
            Duration::from_micros(1),
            Duration::from_millis(1),
            Duration::from_micros(15_625),
            Duration::from_millis(4),
            Duration::from_secs(1),
            Duration::from_secs(2),
        ] {
            let stamped = epoch(1_000_000);
            // Anywhere in `[stamped, stamped + granule)` is a time the very next
            // write could still be stamped with.
            for offset in [Duration::ZERO, granule / 2, granule - granule / 100] {
                let read_started = stamped + offset;
                assert!(
                    !settled(stamped, read_started),
                    "a read {offset:?} into a {granule:?} granule was trusted, so a \
                     same-length write later in that granule would be reused"
                );
            }
        }
    }

    #[test]
    fn a_read_a_full_margin_after_the_stamp_is_settled() {
        let stamped = epoch(1_000_000);
        assert!(
            settled(stamped, stamped + SETTLE_MARGIN),
            "the margin itself has to be enough, or nothing is ever reusable"
        );
        assert!(settled(
            stamped,
            stamped + SETTLE_MARGIN + Duration::from_secs(60)
        ));
    }

    /// Strictness matters in both directions: a hair short is not settled.
    #[test]
    fn a_read_just_short_of_the_margin_is_not_settled() {
        let stamped = epoch(1_000_000);
        let just_short = stamped + SETTLE_MARGIN - Duration::from_micros(1);
        assert!(
            just_short < stamped + SETTLE_MARGIN,
            "the clock cannot represent the gap this test needs"
        );
        assert!(!settled(stamped, just_short));
    }

    /// A network share with a fast clock, or an archive unpacked with bad
    /// timestamps, leaves a modification time ahead of ours. It cannot be proved
    /// settled, and the safe answer is to re-diff for as long as that is true.
    #[test]
    fn a_modification_time_in_the_future_is_never_settled() {
        let read_started = epoch(1_000_000);
        assert!(!settled(
            read_started + Duration::from_secs(3600),
            read_started
        ));

        // A year ahead, which is what a wrong clock or a bad archive actually
        // produces, rather than a synthetic edge.
        assert!(!settled(
            read_started + Duration::from_secs(365 * 24 * 3600),
            read_started
        ));
    }

    #[test]
    fn an_unchanged_file_is_reusable() {
        let entry = taken(ChangeKind::Modified, blob(1), trusted(40, epoch(10)));
        let now = change(ChangeKind::Modified, blob(1));
        assert!(reusable(&entry, &now, || Some(print(40, epoch(10)))));
    }

    #[test]
    fn a_new_index_blob_invalidates_without_the_file_moving() {
        // Staging some other change rewrites the index, and the index is the
        // left-hand side of this diff. The bytes on disk are untouched.
        let entry = taken(ChangeKind::Modified, blob(1), trusted(40, epoch(10)));
        let now = change(ChangeKind::Modified, blob(2));
        assert!(!reusable(&entry, &now, || Some(print(40, epoch(10)))));
    }

    #[test]
    fn a_new_kind_invalidates() {
        let entry = taken(ChangeKind::Modified, blob(1), trusted(40, epoch(10)));
        let now = change(
            ChangeKind::Renamed {
                from: "src/old.rs".to_owned(),
            },
            blob(1),
        );
        assert!(!reusable(&entry, &now, || Some(print(40, epoch(10)))));
    }

    #[test]
    fn a_changed_length_invalidates() {
        let entry = taken(ChangeKind::Modified, blob(1), trusted(40, epoch(10)));
        let now = change(ChangeKind::Modified, blob(1));
        assert!(!reusable(&entry, &now, || Some(print(41, epoch(10)))));
    }

    #[test]
    fn a_changed_mtime_invalidates() {
        let entry = taken(ChangeKind::Modified, blob(1), trusted(40, epoch(10)));
        let now = change(ChangeKind::Modified, blob(1));
        assert!(!reusable(&entry, &now, || Some(print(40, epoch(11)))));
    }

    /// The racily-clean case, which is why `settled` exists.
    #[test]
    fn an_unsettled_fingerprint_is_never_reusable() {
        let entry = taken(
            ChangeKind::Modified,
            blob(1),
            Some(Observed {
                print: print(40, epoch(10)),
                settled: false,
            }),
        );
        let now = change(ChangeKind::Modified, blob(1));
        assert!(!reusable(&entry, &now, || Some(print(40, epoch(10)))));
    }

    #[test]
    fn a_file_that_cannot_be_fingerprinted_now_is_not_reusable() {
        let entry = taken(ChangeKind::Modified, blob(1), trusted(40, epoch(10)));
        let now = change(ChangeKind::Modified, blob(1));
        assert!(!reusable(&entry, &now, || None));
    }

    #[test]
    fn a_file_that_could_not_be_fingerprinted_then_is_not_reusable() {
        let entry = taken(ChangeKind::Modified, blob(1), None);
        let now = change(ChangeKind::Modified, blob(1));
        assert!(!reusable(&entry, &now, || Some(print(40, epoch(10)))));
    }

    /// A removal is computed from the index side alone, so there is no
    /// working-tree fingerprint to check and its absence is not suspicious.
    #[test]
    fn a_removal_is_reusable_with_no_fingerprint_at_all() {
        let entry = taken(ChangeKind::Removed, blob(1), None);
        let now = change(ChangeKind::Removed, blob(1));
        assert!(reusable(&entry, &now, || None));
    }

    #[test]
    fn a_conflict_is_reusable_with_no_fingerprint_at_all() {
        let entry = taken(ChangeKind::Conflict, blob(1), None);
        let now = change(ChangeKind::Conflict, blob(1));
        assert!(reusable(&entry, &now, || None));
    }

    /// A fingerprint taken inside the granule it was stamped in: bytes it cannot
    /// vouch for.
    fn untrusted(len: u64, mtime: SystemTime) -> Option<Observed> {
        Some(Observed {
            print: print(len, mtime),
            settled: false,
        })
    }

    #[test]
    fn a_height_whose_file_did_not_move_is_kept() {
        let entry = taken(ChangeKind::Modified, blob(1), trusted(40, epoch(10)));
        let now = change(ChangeKind::Modified, blob(1));
        assert_eq!(
            verdict(&entry, &now, epoch(100), || Some(print(40, epoch(10)))),
            Verdict::Keep
        );
    }

    #[test]
    fn a_height_whose_file_moved_inside_the_margin_waits_until_the_margin_ends() {
        let entry = taken(ChangeKind::Modified, blob(1), trusted(40, epoch(10)));
        let now = change(ChangeKind::Modified, blob(1));
        // Written one second ago, so the margin closes one second from now.
        assert_eq!(
            verdict(&entry, &now, epoch(100), || Some(print(41, epoch(99)))),
            Verdict::Wait(epoch(101))
        );
    }

    #[test]
    fn a_modification_time_in_the_future_is_read_again_rather_than_waited_on() {
        let entry = taken(ChangeKind::Modified, blob(1), trusted(40, epoch(10)));
        let now = change(ChangeKind::Modified, blob(1));
        assert_eq!(
            verdict(&entry, &now, epoch(100), || Some(print(41, epoch(200)))),
            Verdict::Remeasure
        );
    }

    #[test]
    fn a_height_whose_file_moved_and_settled_is_read_again() {
        let entry = taken(ChangeKind::Modified, blob(1), trusted(40, epoch(10)));
        let now = change(ChangeKind::Modified, blob(1));
        assert_eq!(
            verdict(&entry, &now, epoch(100), || Some(print(41, epoch(90)))),
            Verdict::Remeasure
        );
    }

    #[test]
    fn a_height_taken_inside_the_granule_is_read_again_once_the_file_settles() {
        let entry = taken(ChangeKind::Modified, blob(1), untrusted(40, epoch(10)));
        let now = change(ChangeKind::Modified, blob(1));
        assert_eq!(
            verdict(&entry, &now, epoch(11), || Some(print(40, epoch(10)))),
            Verdict::Wait(epoch(12))
        );
        assert_eq!(
            verdict(&entry, &now, epoch(100), || Some(print(40, epoch(10)))),
            Verdict::Remeasure
        );
    }

    #[test]
    fn a_height_is_read_again_when_an_index_side_moved_and_no_stat_is_spent() {
        let entry = taken(ChangeKind::Modified, blob(1), trusted(40, epoch(10)));
        let now = change(ChangeKind::Modified, blob(2));
        assert_eq!(
            verdict(&entry, &now, epoch(100), || panic!(
                "a stat the ids already answered"
            )),
            Verdict::Remeasure
        );
    }

    #[test]
    fn a_removals_height_is_kept_with_no_fingerprint_at_all() {
        let entry = taken(ChangeKind::Removed, blob(1), None);
        let now = change(ChangeKind::Removed, blob(1));
        assert_eq!(verdict(&entry, &now, epoch(100), || None), Verdict::Keep);
    }

    #[test]
    fn a_height_nothing_can_fingerprint_is_read_again() {
        let now = change(ChangeKind::Modified, blob(1));
        let entry = taken(ChangeKind::Modified, blob(1), trusted(40, epoch(10)));
        assert_eq!(
            verdict(&entry, &now, epoch(100), || None),
            Verdict::Remeasure
        );
        let entry = taken(ChangeKind::Modified, blob(1), None);
        assert_eq!(
            verdict(&entry, &now, epoch(100), || Some(print(40, epoch(10)))),
            Verdict::Remeasure
        );
    }
}
