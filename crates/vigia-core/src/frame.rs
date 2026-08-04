//! The frame path: I2a.
//!
//! > Re-diffing is incremental. The frame path never re-diffs a file that did
//! > not change.
//!
//! [`Worktree::diff`](crate::Worktree::diff) is a pure function of one file's
//! two sides, which is exactly what makes calling it once per changed file per
//! frame the wrong shape. Measured on a 100k-line diff, re-diffing every changed
//! file costs 18.58ms p99 against a 16ms I9 budget, so the naive frame path
//! breaks the frame budget on its own rather than merely wasting work. A
//! [`Frame`] is that same call with the previous answer kept, and it brings a
//! real frame under continuous edits to 6.97ms p99.
//!
//! Everything here reduces to one question: when may a diff be reused? Three
//! things can invalidate one, and deciding costs no file read.
//!
//! * The **index blob** the change names, because the index is the left-hand
//!   side of every diff drawn. Staging changes it without touching the disk.
//! * The **kind** of change, because a rename carries the path it came from and
//!   a removal has no working-tree side at all.
//! * A **`stat`** of the working-tree file: size and modification time.
//!
//! The third is the one that needs care, and the trap has a name in git:
//! *racily clean*. Two writes of the same length inside one modification-time
//! granule are indistinguishable by `stat`, and an in-place one-character edit
//! repeated quickly is precisely that shape.
//!
//! The subtlety, and it is the whole of [`settled`]: a filesystem **floors** the
//! time it stamps to its own granularity, so a write that happens *after* a read
//! can record a time *before* it. "Modification time earlier than the read"
//! therefore proves nothing, because the granule may still be open. Measured on
//! NTFS, that mistake serves a stale diff on roughly one in four hundred
//! same-length rewrites, and on a 1s-granule volume it would be most of them. So
//! a fingerprint counts as proof only once a **full granule** has passed between
//! the stamp and the read, and everything else is re-diffed. That costs
//! redundant diffs of files written in the last two seconds, which are files
//! that just changed, and buys never showing a stale one.
//!
//! What this still cannot see is a writer that restores a modification time it
//! did not advance, which is `cp -p`, `rsync -t`, `unzip` and `touch -r`. Git
//! carries the inode change time for exactly that reason, and `std` does not
//! expose one on Windows, so it is recorded rather than guessed at: see the
//! deferral shelf in `ROADMAP.md`.
//!
//! Content is never hashed to make this decision. Hashing is the read I2a
//! exists to avoid.
//!
//! **The same three things decide a second artefact, and that is why the rule
//! reads as [`reusable`] over a [`Taken`] rather than as a rule about diffs.**
//! A [`FileSpan`] is a projection of the same two sides of the same file, so
//! nothing can invalidate one without invalidating the other. Since
//! [#101](https://github.com/breferrari/vigia/issues/101) the frame carries a
//! span per **changed** file where it carries a diff per **drawn** one, and both
//! are proved by these lines: without that, the height walk re-read every
//! undrawn file on every tick, which is 94 files and 3.7 MiB over a hundred-file
//! worktree and 18.36ms p99 against I9's 16ms. `SPEC.md` §3's I4 notes carry the
//! numbers.

use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, SystemTime};

use crate::change::{ChangeKind, FileChange};
use crate::error::Result;
use crate::hunk::{FileDiff, FileSpan};
use crate::worktree::Worktree;

/// What a [`Frame`] has done since it was created.
///
/// Cumulative rather than per-frame: a caller can total them over a run, and a
/// test describes one frame by subtracting two readings. I2a is a claim about
/// that subtraction.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrameStats {
    /// Diffs computed from content.
    pub computed: u64,
    /// Diffs served unchanged from an earlier frame.
    pub reused: u64,
    /// Files counted by [`Frame::height`] without their text being built.
    ///
    /// Its own counter rather than folded into [`Self::computed`], because the
    /// two bound different claims: `computed` is what I2a and I4 hold the
    /// *window* to, and this is the one thing in the frame path that walks the
    /// whole changed set. A gate that could not tell them apart could not say
    /// which of the two had regressed.
    pub measured: u64,
    /// Bytes compared by computed diffs.
    ///
    /// A reuse adds nothing here, which is the number I2a is written against:
    /// what a frame costs has to follow what changed, not how large the
    /// worktree is.
    ///
    /// **Compared, not read**, and the two stopped being the same number when
    /// the working-tree side began going through git's clean filter: a CRLF file
    /// is read a carriage return per line larger than it is compared. Compared
    /// is the right one to count, because it is what the diff algorithm walks,
    /// and it is what makes a CRLF worktree and its LF twin report identical
    /// costs. See `filter.rs`.
    pub bytes: u64,
    /// `stat` calls made, either to record a fingerprint or to check one.
    ///
    /// The shape is the point: a reuse costs a `stat`, never a read.
    ///
    /// **It totals two different populations, and a gate has to know which it is
    /// bounding.** [`Frame::diff`] probes once per file a caller asked to draw,
    /// plus once more for each diff it recomputed. Since
    /// [#101](https://github.com/breferrari/vigia/issues/101),
    /// [`Frame::height`] probes once per changed file whose span was carried and
    /// is worth proving, which follows the **whole changed set** and not the
    /// window. So `probes` is the sum of a window-sized term and a
    /// worktree-sized one, exactly as [`Self::bytes`] is: a fixture that mixes
    /// the two cannot express a bound on either, and the gates that assert on
    /// this number pick a fixture where one of the two is zero
    /// (`a_height_taken_from_a_diff_in_hand_costs_no_stat` is the clearest,
    /// asserting **no** probe at all over a fully-diffed worktree).
    /// Splitting it is the same open question
    /// [#85](https://github.com/breferrari/vigia/issues/85) already holds for
    /// `bytes`, one counter over.
    pub probes: u64,
    /// Cached diffs dropped because their path stopped being changed.
    ///
    /// I3 forbids unbounded growth over days, and this is what says the map is
    /// bounded by the current diff rather than by everything ever edited.
    pub evicted: u64,
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
    /// True when the granule `print.mtime` was stamped in had already closed
    /// before the read began. That, and not merely a modification time in the
    /// past, is what makes an unchanged fingerprint proof of unchanged bytes.
    /// See [`settled`].
    settled: bool,
}

/// How far in the past a modification time has to sit before it identifies the
/// bytes that were read.
///
/// A filesystem stamps a modification time by **flooring** the current time to
/// its own granularity, so a write landing *after* a read can still record a
/// time *before* that read. Comparing an mtime against a precise clock therefore
/// proves nothing by itself: the granule the write fell in may still be open,
/// and a second write of the same length inside it is invisible. A full granule
/// of margin is what closes it.
///
/// Two seconds, because that is the coarsest granularity a git worktree can
/// plausibly sit on. FAT and exFAT quantise to 2s, HFS+ and ext3 to 1s, NTFS to
/// somewhere between 1ms and 16ms, and ext4, APFS, xfs and btrfs to far less.
/// The constant has to be an **upper bound** on the real granularity, so being
/// generous is the safe direction.
///
/// It is not free: a file written once is re-diffed for the whole margin, and a
/// bulk rewrite puts every file in that state at the same time. The obvious
/// answer is to stop guessing and measure the granularity per worktree, from the
/// smallest positive difference between the modification times status already
/// reports. **That does not work, and the reason is worth keeping.**
///
/// Granularity is not uniform within one volume, so the smallest gap observed
/// bounds the *smallest* granule while soundness needs the *largest*. Measured on
/// NTFS: 10,324 same-length rewrites of one file over three seconds produced
/// 1,959 distinct stamps whose positive gaps spanned 502µs to 17,522µs, a 34.8x
/// spread, and a hundred-file bulk write left a smallest cross-path gap of 998µs.
/// A margin of 998µs would leave a real 17.5ms granule uncovered, which is
/// precisely the stale diff this constant exists to prevent. Nothing passive does
/// better, because a monitor never writes and so only ever sees the gaps its
/// user's tools happened to leave. Measuring it properly would mean writing into
/// the worktree, which is not something a monitor gets to do.
///
/// So the number stays a bound taken from the table above rather than a sample,
/// and what it costs is bounded elsewhere: a caller reads only what it draws, so
/// the shell recomputes about one file a frame through a bulk rewrite rather than
/// a hundred. Both tiers of `crates/vigia/tests` gate that, measuring *inside*
/// this margin rather than after it. `SPEC.md` §10 holds the numbers.
///
/// **One thing this margin bounds is not window-sized, and it arrived with
/// [#101](https://github.com/breferrari/vigia/issues/101).** [`Frame::height`]
/// walks every changed file, so a bulk rewrite of files nothing has drawn leaves
/// every carried span unsettled at once and the walk re-measures the whole
/// changed set for the length of this margin. Measured over a hundred of them,
/// across eight runs: **13.05ms p50 and a p99 between 13.83ms and 15.88ms**
/// against I9's 16ms. The spread rather than the best run, because the gate was
/// first written from a single lucky one and then failed three times in eight.
/// The lazy fingerprint in [`Frame::fill_span`] is what holds it to one
/// `stat` per file rather than two, since an unsettled observation is refused
/// before a fresh print is asked for. Two gates, because they catch different
/// halves: `budgets.rs::what_a_bulk_rewrite_of_undrawn_files_costs`
/// holds the wall clock at this intersection and is the tightest budget in the
/// repo, and `reads.rs::a_tick_inside_the_settle_margin_stats_each_file_once`
/// holds the syscall count, which is the half a percentile with headroom cannot
/// see.
const SETTLE_MARGIN: Duration = Duration::from_secs(2);

/// Whether a modification time observed by a read starting at `read_started`
/// identifies the bytes that read returned.
///
/// Pure, and deliberately not inlined into the read. This is the one rule in the
/// engine that cannot be tested by racing a filesystem: the window is smaller
/// than the machinery needed to hit it, so an inline comparison would be a rule
/// no test could reach. Written as a function, every mutation of the arithmetic
/// is caught by the unit tests at the bottom of this file.
fn settled(mtime: SystemTime, read_started: SystemTime) -> bool {
    // Subtraction rather than addition on purpose. Adding the margin to `mtime`
    // needs an overflow arm for a time within two seconds of the largest the
    // platform represents, and that arm is unreachable: no filesystem reports
    // such a time and no portable test can build one, so it would be a branch
    // nothing could ever check. Going the other way, a modification time in the
    // future is an `Err`, which is the same "cannot prove" answer and is
    // reachable.
    read_started
        .duration_since(mtime)
        .is_ok_and(|gap| gap >= SETTLE_MARGIN)
}

/// What a cached artefact was taken under, and therefore what proves it is still
/// true.
///
/// **One definition, two artefacts.** A [`FileDiff`] and a [`FileSpan`] are
/// derived from the same two sides of the same file, so they go stale for
/// exactly the same reasons and there is one rule ([`reusable`]) rather than a
/// rule each. Two copies of it would be free to drift into disagreeing about
/// whether a file changed, and the failure would be a stale pane rather than a
/// compile error: the same argument `reads_worktree` was extracted on.
#[derive(Clone)]
struct Taken {
    kind: ChangeKind,
    index_blob: Option<gix::ObjectId>,
    /// The working-tree side as it was when the content was read. `None` when
    /// this artefact has no working-tree side, or when it could not be
    /// fingerprinted.
    worktree: Option<Observed>,
}

impl Taken {
    /// The evidence a change carries, plus what a read observed of its
    /// working-tree side.
    ///
    /// A constructor rather than three field literals, because
    /// [#16](https://github.com/breferrari/vigia/issues/16) adds a field here
    /// and the literals differ only in their `worktree` argument: nothing would
    /// tell you which of them you had missed.
    fn of(change: &FileChange, worktree: Option<Observed>) -> Self {
        Self {
            kind: change.kind.clone(),
            index_blob: change.index_blob,
            worktree,
        }
    }
}

/// Store `value` under `path`, keeping the key the map already owns.
///
/// `HashMap::insert` keeps the existing key and drops the one handed to it, so
/// the obvious `insert(path.clone(), …)` allocates a `String` per call and frees
/// it again on the common path, where [`Frame::advance`] has already migrated an
/// entry for every changed file. Only a genuine miss needs a new key.
///
/// Generic over the value because both caches want it: the span map pays it once
/// per changed file, the diff map once per recompute.
fn put<T>(map: &mut HashMap<String, T>, path: &str, value: T) {
    match map.get_mut(path) {
        Some(slot) => *slot = value,
        None => {
            map.insert(path.to_owned(), value);
        }
    }
}

/// One path's diff, with everything needed to know it is still true.
struct Cached {
    taken: Taken,
    diff: FileDiff,
}

/// One path's height, with everything needed to know it is still true.
///
/// **Why this exists at all**, since a span could be re-derived from a diff: the
/// files this holds are the ones that have **no** diff, because nothing has drawn
/// them. Re-deriving their height means reading them, and re-reading every
/// undrawn file on every tick is
/// [#101](https://github.com/breferrari/vigia/issues/101): 94 files and 3.7 MiB
/// a tick over a hundred-file worktree, 18.36ms p99 against I9's 16ms.
///
/// A `FileSpan` is three numbers where a `FileDiff` owns a `String` per drawn
/// line, so keeping one per changed file is a different order of cost from
/// keeping a diff per changed file. I3 bounds it the same way regardless: the
/// map is migrated on [`Frame::advance`] and a path that stops being changed is
/// dropped.
struct Measured {
    /// What this span was taken under, or `None` when the read that would have
    /// produced it **failed**.
    ///
    /// **`None` is what stops a failure being cached forever, and the case that
    /// needs it is narrower than it looks.** A failed measure contributes a zero
    /// height for this tick, which is right: a file that vanished between status
    /// naming it and the read has no height to report. What is not right is
    /// carrying that zero. For a change with a working-tree side the evidence
    /// alone would refuse it, since [`reusable`] treats a missing fingerprint as
    /// unprovable. For a `Removed` one it would **not**: that diff is computed
    /// from the index side alone, so `reusable` returns `true` on the kind and
    /// the blob without asking anything else, and a `MissingBlob` on a pruned or
    /// partially-cloned object would then pin a zero row-count for the life of
    /// the frame with no retry.
    ///
    /// [`Frame::diff`] has no equivalent hazard because it propagates the error
    /// with `?` and caches nothing. This is the infallible path's version of the
    /// same rule: report zero now, and ask again next tick.
    taken: Option<Taken>,
    span: FileSpan,
    /// Whether this span has been shown to describe the file **on this tick**.
    ///
    /// Set by whatever established it and cleared for every entry by
    /// [`Frame::advance`], so the proof is worth exactly one tick. Without it a
    /// carried span is re-examined by every caller that asks: `height` walks all
    /// of them and `rows_of` then asks again for each file above the viewport.
    /// The contract callers are written against is *one* count per changed file
    /// per tick, and this is what keeps it one.
    ///
    /// **What that second examination costs is a read, not a `stat`, which is
    /// worth stating because the cheaper number invites deleting this field.**
    /// Inside the settle margin — the ordinary state while an agent is writing —
    /// a span taken by a read is recorded unsettled, so [`reusable`] refuses it
    /// and the second `fill_span` in the same tick falls all the way through to
    /// [`Worktree::measure`]. This flag is load-bearing well past "it saves a
    /// syscall".
    ///
    /// The failure modes are not symmetric: forgetting to **set** it costs work,
    /// and forgetting to **clear** it on a future carry path would serve a stale
    /// height silently and forever. `advance`'s migration is the only carry path
    /// today and the clear sits inside it; a tick counter recorded per entry
    /// would make that structural rather than local, and is the shape to reach
    /// for if a second one is ever added.
    proven: bool,
}

/// Fingerprint a working-tree file, or `None` when it cannot be.
///
/// Follows symlinks, because the content side of a diff is read with
/// `fs::read`, which follows them too. Fingerprinting anything other than the
/// bytes actually compared would be measuring the wrong file.
///
/// `None` is not an error. A file can vanish between status naming it and this
/// call, and a platform can decline to report a modification time. Both mean
/// the same thing here: no fingerprint, so no reuse.
fn fingerprint(path: &Path) -> Option<Fingerprint> {
    let meta = std::fs::metadata(path).ok()?;
    Some(Fingerprint {
        len: meta.len(),
        mtime: meta.modified().ok()?,
    })
}

/// Whether something taken from a file still describes the working tree.
///
/// Pure on purpose. Every way an artefact can go stale is one branch here, which
/// is what keeps the rule reviewable and what lets the racily-clean case be
/// tested without racing anything.
///
/// Takes a [`Taken`] rather than a [`Cached`] so the diff cache and the span
/// cache are governed by the same lines. Written against a `Cached` it read as a
/// rule about diffs, and #101 needed the identical rule about spans.
///
/// **`fresh` is a closure, so the `stat` is the rule's decision rather than the
/// caller's.** Three of the four ways to answer `false` here need no fingerprint
/// at all, and the caller cannot know which applies without restating the rule.
/// Taking it eagerly bought a syscall on every one of them, which
/// [`Frame::fill_span`] pays over the whole changed set rather than over the
/// drawn part of it. A second predicate beside this one would be the other way
/// to fix that, and it would be free to drift into disagreeing with this
/// function about whether a file changed.
fn reusable(
    taken: &Taken,
    current: &FileChange,
    fresh: impl FnOnce() -> Option<Fingerprint>,
) -> bool {
    // A new blob for this path is a new diff even when the file on disk never
    // moved, and a new kind is a different diff outright.
    if taken.kind != current.kind || taken.index_blob != current.index_blob {
        return false;
    }

    // A removal, a conflict and a type change are computed from the index side
    // alone, so they have no working-tree side that could have gone stale.
    if !current.reads_worktree() {
        return true;
    }

    // Unfingerprintable then, or unfingerprintable now. Neither is a failure,
    // and both forbid reuse: the alternative is drawing a diff we cannot vouch
    // for. `settled` is asked **first**, and that ordering is what makes the
    // lazy `fresh` pay: an observation that was never proof cannot be rescued by
    // a fresh fingerprint, so there is no reason to take one.
    match taken.worktree {
        Some(observed) => observed.settled && fresh().is_some_and(|fresh| observed.print == fresh),
        None => false,
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
    cached: HashMap<String, Cached>,
    /// How tall every changed file is, once something has asked.
    ///
    /// Separate from [`Self::cached`] because it answers a different question
    /// and costs a different amount: a `FileSpan` is three numbers, where a
    /// `FileDiff` owns a `String` per drawn line. Filled lazily, carried across
    /// [`Frame::advance`] under the same proof the diffs beside it are carried
    /// under, and migrated so it is bounded by the changed set rather than by
    /// the session.
    spans: HashMap<String, Measured>,
    /// The attributes files in the changed set, and what they looked like, as of
    /// the last tick.
    ///
    /// **Because the guard below has to fire on a *change* and not on a
    /// presence.** A `.gitattributes` that is written and not committed stays in
    /// the changed set, so "clear the caches when one is present" clears them on
    /// every tick for as long as it sits there, which is the ordinary state of a
    /// repository being set up. Measured: with one uncommitted `.gitattributes`
    /// in a 100-file worktree, every tick re-measured 95 files and read 3.7 MiB,
    /// at **19.91ms p50 against 9.16ms** without it. That is #101's own defect,
    /// reintroduced by #101's own fix for a different defect.
    ///
    /// Comparing this against the next tick's costs one `stat` per attributes
    /// file, and a worktree has none of those in it almost always.
    attributes: HashMap<String, Option<Fingerprint>>,
    stats: FrameStats,
}

impl<'w> Frame<'w> {
    pub(crate) fn new(worktree: &'w Worktree) -> Self {
        Self {
            worktree,
            files: Vec::new(),
            cached: HashMap::new(),
            spans: HashMap::new(),
            attributes: HashMap::new(),
            stats: FrameStats::default(),
        }
    }

    /// Re-read which files changed, keeping every diff still known to be valid.
    ///
    /// Diffs nothing, so a monitor draws the top of a long diff without having
    /// read the bottom (I4). Content is fetched by [`Frame::diff`] when a caller
    /// asks for it.
    ///
    /// The *file list* is a different matter: this walks status to completion,
    /// because a monitor cannot draw a scrollbar without knowing how many files
    /// there are. That costs nothing today, since rename tracking is on by
    /// default and cannot stream either. See `SPEC.md` section 10.
    ///
    /// A frame starts empty, so this has to be called once before
    /// [`Frame::files`] reports anything. That makes the first frame the same
    /// shape as every later one instead of a special case.
    ///
    /// On failure the frame is left exactly as it was. A monitor that discarded
    /// its state because one status walk failed would blank the pane for a
    /// reason its reader cannot see.
    pub fn advance(&mut self) -> Result<()> {
        let mut files = Vec::with_capacity(self.files.len());
        for change in self.worktree.changes()? {
            files.push(change?);
        }

        // Nothing above this line mutated anything, which is what makes a failed
        // walk leave the previous frame intact. That covers the line below as
        // well: a walk that returned early leaves the filter alone too, so a
        // failed frame does not throw away work the next one would have reused.

        // The clean filter is rebuilt by the next read rather than kept for the
        // life of the process. `.gitattributes` and `core.autocrlf` decide what
        // a diff normalises, and the agent in the other pane is free to write
        // either at any moment; a filter built once would go on answering from
        // rules that no longer exist. See `Worktree::invalidate_filter`.
        self.worktree.invalidate_filter();

        // **And what was computed under the old rules goes with it.**
        // `invalidate_filter` makes the next *read* correct and does nothing for
        // an answer already in hand, which is a fourth way a cached artefact can
        // go stale and the one [`reusable`] cannot see: an attributes change
        // moves neither the file, nor its length, nor its modification time, nor
        // its index blob, so every term in the rule says "unchanged" while the
        // bytes the diff would compare have changed underneath it.
        //
        // Measured over a file committed LF and held CRLF, on a `*.txt text
        // eol=lf` written mid-session: the carried span reported **80 rows where
        // a cold frame computes 8**, and never recovered, because nothing was
        // going to touch that file again. That is [#65](https://github.com/breferrari/vigia/issues/65)'s
        // own population, every text file on a default `core.autocrlf` checkout.
        //
        // **Fired on a change rather than on a presence**, which is the whole of
        // `Frame::attributes`: an uncommitted `.gitattributes` stays in the
        // changed set, so testing for one clears both caches on *every* tick
        // rather than on the tick it arrived, at 19.91ms p50 against 9.16ms.
        //
        // Dropping both maps rather than adding a term to `reusable` is the
        // deliberate choice: an attributes change is rare, a whole-worktree
        // re-read is what the first frame already costs, and a *fifth* term in
        // the reuse rule that no fingerprint can carry would be a rule about
        // repository configuration living in a function about one file. What it
        // does **not** cover is a change to `core.autocrlf` or to
        // `.git/info/attributes`, neither of which the status walk reports;
        // `SPEC.md` §10 records that residue rather than implying it away.
        let attributes: HashMap<String, Option<Fingerprint>> = files
            .iter()
            .filter(|change| change.rewrites_attributes())
            .map(|change| {
                let path = self.worktree.workdir().join(&change.path);
                self.stats.probes += 1;
                (change.path.clone(), fingerprint(&path))
            })
            .collect();
        if attributes != self.attributes {
            self.cached.clear();
            self.spans.clear();
        }
        self.attributes = attributes;

        // **Both caches are migrated, and neither is dropped.** A span used to be
        // cleared here, on the reasoning that it is derived from content and has
        // no freshness check of its own. Giving it one ([`Measured`]) is
        // [#101](https://github.com/breferrari/vigia/issues/101): clearing meant
        // every changed file the reader had not scrolled to was read from disk
        // again on **every tick**, forever, which is 94 files and 3.7 MiB a tick
        // over a hundred-file worktree and 18.36ms p99 against I9's 16ms.
        //
        // Migrating rather than keeping is what bounds them. A path that stopped
        // being changed is dropped from both, so each map is the size of the
        // current diff and not of the session, which is I3.
        let mut previous = std::mem::take(&mut self.cached);
        self.cached.reserve(files.len());
        let mut previous_spans = std::mem::take(&mut self.spans);
        self.spans.reserve(files.len());
        for change in &files {
            // `remove_entry` rather than `remove`, so the key the old map owns
            // is moved rather than cloned. With two maps that is up to 2N
            // needless `String` allocations a tick, which on the hundred-file
            // gate is two hundred.
            if let Some((path, cached)) = previous.remove_entry(&change.path) {
                self.cached.insert(path, cached);
            }
            if let Some((path, mut measured)) = previous_spans.remove_entry(&change.path) {
                // Carried, and no longer proved. What it described was true of
                // the previous tick, and `fill_span` is where it is asked again.
                measured.proven = false;
                self.spans.insert(path, measured);
            }
        }
        // Whatever is left is a path that stopped being changed. Counted from
        // the diffs alone: `evicted` is what a caller reads to check I3's bound
        // on the diff cache, and adding a second, differently-sized population
        // into it would make that number mean neither one.
        self.stats.evicted += previous.len() as u64;

        self.files = files;
        Ok(())
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
    ///
    /// At most one per changed file, never one per file ever changed. I3 is a
    /// claim about a process that runs for days, so this is the number that
    /// says the map is bounded by the diff rather than by the session.
    pub fn tracked(&self) -> usize {
        self.cached.len()
    }

    /// Heights currently held between frames.
    ///
    /// The diff cache's sibling and I3's claim about it: at most one per changed
    /// file, never one per file ever changed. It exists as a separate reading
    /// because the two populations differ — a span is kept for every changed file
    /// once something has totalled the diff, where a diff is kept only for what
    /// has been drawn — so a soak bounding one of them says nothing about the
    /// other.
    ///
    /// A `FileSpan` plus its evidence is a few dozen bytes against a `FileDiff`'s
    /// `String` per drawn line, which is why the larger population is the cheaper
    /// map and not the other way round.
    pub fn tracked_spans(&self) -> usize {
        self.spans.len()
    }

    /// How many rows the whole diff is, counting every changed file.
    ///
    /// **This is the one thing in the frame path that is not bounded by the
    /// window**, and it is here rather than refused because a scrollbar that
    /// cannot say where the end is says nothing. `SPEC.md` §3's I4 carries two
    /// notes: the rewording that admits the walk, and the one that made it
    /// incremental. Both carry the measurements they were decided on.
    ///
    /// Four things keep it affordable. It counts through
    /// [`Worktree::measure`], which reads the same bytes a diff would and skips
    /// the `String` per line that made the obvious version ten times slower than
    /// `git diff --numstat`. It reuses a [`FileDiff`] the frame already holds
    /// rather than re-reading, so a file on screen is free. And a span is kept
    /// once taken: within a tick because [`Measured::proven`] says it was proved
    /// this one, and **across** ticks because [`Frame::advance`] migrates it and
    /// [`reusable`] re-proves it for one `stat`. So scrolling never pays, and a
    /// tick pays only for the files that moved
    /// ([#101](https://github.com/breferrari/vigia/issues/101)).
    ///
    /// `rows_of` maps a file's span to the rows a caller draws for it, because
    /// what a conflict or a binary file occupies is the shell's ruling rather
    /// than the engine's.
    pub fn height(&mut self, rows_of: impl Fn(&FileChange, &FileSpan) -> usize) -> Result<usize> {
        let mut total = 0usize;
        for index in 0..self.files.len() {
            self.fill_span(index);
            let change = &self.files[index];
            total += rows_of(change, &self.spans[&change.path].span);
        }
        Ok(total)
    }

    /// How many rows one file occupies, from its span.
    ///
    /// Uses the cache [`Frame::height`] fills, so a caller that has totalled the
    /// diff can walk part of it again for free.
    pub fn rows_of(
        &mut self,
        index: usize,
        rows_of: impl Fn(&FileChange, &FileSpan) -> usize,
    ) -> Result<usize> {
        self.fill_span(index);
        let change = &self.files[index];
        Ok(rows_of(change, &self.spans[&change.path].span))
    }

    /// Put a span for the file at `index` in the cache, if one is not there.
    ///
    /// **Shared, because the two callers answered the same question
    /// differently.** [`Frame::height`] consulted the diff cache first and
    /// [`Frame::rows_of`] went straight to disk, so for one file in one frame
    /// the answer depended on which had been called first, and `rows_of`'s own
    /// doc claiming it uses the cache `height` fills was true only because both
    /// live call sites happen to call `height` first. Nothing enforced that, and
    /// the cold path was an unbudgeted whole-file read on the scroll arithmetic.
    ///
    /// Infallible on purpose; the read-error rule is on [`Measured::taken`].
    ///
    /// **Three sources, cheapest first, and the order is the design.**
    ///
    /// 1. A [`FileDiff`] the frame already holds, by **presence**. Free, no
    ///    syscall at all, and taken without proof, which is
    ///    [#84](https://github.com/breferrari/vigia/issues/84): a file changed off
    ///    screen contributes its old height until the viewport reaches it. Left
    ///    exactly as it was. #84 records that proving this branch re-measures the
    ///    whole worktree on a bulk rewrite and breached I9 at 20.71ms, so it is a
    ///    separate question with a cost of its own and not a line to change in
    ///    passing.
    /// 2. A span carried from an earlier tick and *proved* still true, which
    ///    costs one `stat`. This is [#101](https://github.com/breferrari/vigia/issues/101):
    ///    at a hundred changed files a stat each is **1.29ms** against **12.90ms**
    ///    to read them all, and the ratio runs 6.8x at 2000 files to 10.0x at
    ///    100.
    /// 3. A read, through [`Worktree::measure`], which skips the `String` per line
    ///    a full diff allocates.
    ///
    /// **(1) before (2), and the order cost a regression to learn.** Putting the
    /// carried span first reads as "cheapest first" and is not: a file with a
    /// diff in hand needs no evidence, because deriving its height from that diff
    /// is free and is what this did before #101 existed. Statting it first buys
    /// nothing and pays a syscall, and when the print has moved it pays that
    /// syscall **again on every subsequent frame**, since the span is then
    /// rebuilt from the same stale diff and mismatches identically next time.
    /// Measured on `the_frame_budget_holds_through_a_bulk_rewrite`, where every
    /// file is rewritten at once and all hundred are in hand: **8.27ms p50 before,
    /// 11.12ms p50 with the stat first**, and the gate went from passing 4 of 4
    /// runs to 2 of 4. `a_height_taken_from_a_diff_in_hand_costs_no_stat`, in
    /// this crate's own `tests/frame.rs`, holds that order structurally so it
    /// cannot be reordered back by reading.
    ///
    /// A span from (2) is exactly as trustworthy as a diff from [`Frame::diff`]'s
    /// reuse branch, because it is the same [`reusable`] rule over the same
    /// [`Taken`] evidence, and it inherits the same single limit: a write that
    /// restores both length and modification time
    /// ([#16](https://github.com/breferrari/vigia/issues/16)).
    fn fill_span(&mut self, index: usize) {
        let change = &self.files[index];
        if self
            .spans
            .get(&change.path)
            .is_some_and(|measured| measured.proven)
        {
            return;
        }

        // (1) A diff in hand. Free, and no syscall.
        if let Some(cached) = self.cached.get(&change.path) {
            let measured = Measured {
                taken: Some(cached.taken.clone()),
                span: FileSpan::from(&cached.diff),
                proven: true,
            };
            put(&mut self.spans, &change.path, measured);
            return;
        }

        // (2) A span carried from an earlier tick, and the only thing standing
        // between this file and a whole-file read. `advance` migrated it without
        // asking whether the file moved, so this is where it is asked.
        //
        // The fingerprint is taken **lazily**, because `reusable` refuses on
        // several grounds that no `stat` can change: a new kind, a new index
        // blob, evidence that was never settled, or no working-tree evidence at
        // all. Passing it eagerly bought a syscall for every one of those, and
        // this walk runs over every changed file rather than over the drawn
        // ones. A closure keeps the decision inside the rule rather than
        // restating half of it here, which is what a second predicate would do.
        let path = self.worktree.workdir().join(&change.path);
        let mut probed = false;
        let mut proved = false;
        if let Some(measured) = self.spans.get_mut(&change.path)
            && let Some(taken) = measured.taken.as_ref()
            && reusable(taken, change, || {
                probed = true;
                fingerprint(&path)
            })
        {
            measured.proven = true;
            proved = true;
        }
        // Counted from whether the closure ran, so `probes` stays a count of
        // syscalls taken rather than of call sites reached.
        self.stats.probes += u64::from(probed);
        if proved {
            return;
        }

        // (3) A read. Both arms yield the same shape, so `Taken`'s field list
        // appears once: #16 adds a field to it, and two literals here would be
        // two places to forget.
        //
        // Timed from before the read starts, for the reason [`Frame::diff`]
        // gives: the window a write would have to land in to be missed is
        // over-stated rather than under-stated.
        let read_started = SystemTime::now();
        let (span, taken) = match self.worktree.measure(change) {
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
            // **A failed read describes nothing, so it is recorded with no
            // evidence at all** and [`Measured::taken`] carries why that matters.
            // A file can vanish between status naming it and this call; the
            // height it contributes this tick is zero, and the next tick asks
            // again rather than inheriting the answer.
            Err(_) => (FileSpan::default(), None),
        };
        let measured = Measured {
            taken,
            span,
            proven: true,
        };
        put(&mut self.spans, &change.path, measured);
    }

    /// The change at `index` and its diff, computed now or reused from an
    /// earlier frame.
    ///
    /// Both, rather than the diff alone, because a renderer needs both and
    /// cannot have them separately: the returned reference is derived from
    /// `&mut self`, so while it is alive [`Frame::files`] cannot be called. A
    /// caller wanting the kind as well would have to clone the change first,
    /// once per visible file per frame, on the one path this whole type exists
    /// to keep cheap.
    ///
    /// # Panics
    ///
    /// If `index` is out of range, the same way indexing a slice does. The way
    /// to hit that is to hold an index across [`Frame::advance`], which is what
    /// a scroll position is: the agent in the other pane commits, the list
    /// shrinks, and the index now points past the end.
    ///
    /// Panicking is the deliberate choice there rather than returning `None`. A
    /// caller has to clamp such an index against [`Frame::files`] for its own
    /// correctness anyway, or it renders a selection that no longer exists, so a
    /// lenient accessor would turn a caller bug into a silently wrong row
    /// instead of preventing it.
    pub fn diff(&mut self, index: usize) -> Result<(&FileChange, &FileDiff)> {
        let change = &self.files[index];
        let path = self.worktree.workdir().join(&change.path);

        // Lazily, for the reason [`reusable`] gives: a `stat` this answer will
        // not read is a syscall bought for nothing, and `probes` should count
        // the ones actually taken.
        let mut probed = false;
        let reuse = match self.cached.get(&change.path) {
            None => false,
            Some(cached) => reusable(&cached.taken, change, || {
                probed = true;
                fingerprint(&path)
            }),
        };
        self.stats.probes += u64::from(probed);

        if reuse {
            self.stats.reused += 1;
            return Ok((change, &self.cached[&change.path].diff));
        }

        // Timed from before the read starts, so the window a write would have
        // to land in to be missed is over-stated rather than under-stated.
        let read_started = SystemTime::now();
        let diff = self.worktree.diff(change)?;
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
        // **The height goes with the diff it was taken from.** A span filled
        // earlier in this same tick describes the file as it was before this
        // recompute, and dropping it costs nothing: `height`'s cached branch
        // rebuilds it from the fresh diff without reading a byte. This is the
        // half of #84 that is free. The half that is not is a file that changed
        // and has *not* been re-diffed, which needs a read to notice.
        self.spans.remove(&change.path);
        put(
            &mut self.cached,
            &change.path,
            Cached {
                taken: Taken::of(change, worktree),
                diff,
            },
        );
        Ok((change, &self.cached[&change.path].diff))
    }
}

#[cfg(test)]
mod tests {
    //! The reuse rule, tested as the pure function it is.
    //!
    //! The racily-clean guard cannot be tested by racing a filesystem: the
    //! whole point is that it covers a window too small to hit on demand. It
    //! can be tested exactly, here, by handing the rule the observation such a
    //! race would produce.

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
        FileChange {
            path: "src/lib.rs".to_owned(),
            kind,
            index_blob,
        }
    }

    /// The evidence an artefact was taken under.
    ///
    /// Named for what [`reusable`] now takes rather than for the diff it used to
    /// be reached through: the rule governs the span cache too, and a helper
    /// called `cached` would read as though it did not.
    fn taken(
        kind: ChangeKind,
        index_blob: Option<gix::ObjectId>,
        worktree: Option<Observed>,
    ) -> Taken {
        Taken {
            kind,
            index_blob,
            worktree,
        }
    }

    /// A fingerprint already proved to identify its bytes: the reusable case.
    fn trusted(len: u64, mtime: SystemTime) -> Option<Observed> {
        Some(Observed {
            print: print(len, mtime),
            settled: true,
        })
    }

    /// The rule that cannot be raced, so it is checked arithmetically instead.
    ///
    /// Every case below is a real filesystem's granularity. The mtime is floored
    /// to the granule, exactly as a filesystem stamps it, and the read happens
    /// somewhere inside that same granule: the shape of a write that lands after
    /// the read and is invisible to it.
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
    ///
    /// A hair is a microsecond rather than a nanosecond because `SystemTime` is
    /// not infinitely precise. Windows resolves it to 100ns, so subtracting 1ns
    /// from a timestamp is a no-op and the test would be asserting nothing.
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
    ///
    /// Same length, same modification time, and that time was *not* already in
    /// the past when the content was read. A second write inside the same
    /// granule would look exactly like this, so the rule has to refuse.
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
}
