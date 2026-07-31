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

use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, SystemTime};

use crate::change::{ChangeKind, FileChange};
use crate::error::Result;
use crate::hunk::FileDiff;
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
    /// Bytes compared by computed diffs.
    ///
    /// A reuse adds nothing here, which is the number I2a is written against:
    /// bytes read has to follow what changed, not how large the worktree is.
    pub bytes: u64,
    /// `stat` calls made, either to record a fingerprint or to check one.
    ///
    /// One per file visited, and one more for each diff recomputed, which has to
    /// be re-fingerprinted afterwards. The shape is the point: a reuse costs a
    /// `stat`, never a read.
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

/// One path's diff, with everything needed to know it is still true.
struct Cached {
    kind: ChangeKind,
    index_blob: Option<gix::ObjectId>,
    /// The working-tree side as it was when the content was read. `None` when
    /// this diff has no working-tree side, or when it could not be
    /// fingerprinted.
    worktree: Option<Observed>,
    diff: FileDiff,
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

/// Whether a cached diff still describes the working tree.
///
/// Pure on purpose. Every way a diff can go stale is one branch here, which is
/// what keeps the rule reviewable and what lets the racily-clean case be tested
/// without racing anything.
fn reusable(cached: &Cached, current: &FileChange, fresh: Option<Fingerprint>) -> bool {
    // A new blob for this path is a new diff even when the file on disk never
    // moved, and a new kind is a different diff outright.
    if cached.kind != current.kind || cached.index_blob != current.index_blob {
        return false;
    }

    // A removal, a conflict and a type change are computed from the index side
    // alone, so they have no working-tree side that could have gone stale.
    if !current.reads_worktree() {
        return true;
    }

    match (cached.worktree, fresh) {
        (Some(observed), Some(fresh)) => observed.settled && observed.print == fresh,
        // Unfingerprintable then, or unfingerprintable now. Neither is a
        // failure, and both forbid reuse: the alternative is drawing a diff we
        // cannot vouch for.
        _ => false,
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
    stats: FrameStats,
}

impl<'w> Frame<'w> {
    pub(crate) fn new(worktree: &'w Worktree) -> Self {
        Self {
            worktree,
            files: Vec::new(),
            cached: HashMap::new(),
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

        // Nothing above this line touched `self`, which is what makes a failed
        // walk leave the previous frame intact.
        let mut previous = std::mem::take(&mut self.cached);
        self.cached.reserve(files.len());
        for change in &files {
            if let Some(cached) = previous.remove(&change.path) {
                self.cached.insert(change.path.clone(), cached);
            }
        }
        // Whatever is left is a path that stopped being changed.
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

        let reuse = match self.cached.get(&change.path) {
            None => false,
            Some(cached) => {
                let fresh = if change.reads_worktree() {
                    self.stats.probes += 1;
                    fingerprint(&path)
                } else {
                    None
                };
                reusable(cached, change, fresh)
            }
        };

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
        self.cached.insert(
            change.path.clone(),
            Cached {
                kind: change.kind.clone(),
                index_blob: change.index_blob,
                worktree,
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

    fn cached(
        kind: ChangeKind,
        index_blob: Option<gix::ObjectId>,
        worktree: Option<Observed>,
    ) -> Cached {
        Cached {
            kind,
            index_blob,
            worktree,
            diff: FileDiff {
                path: "src/lib.rs".to_owned(),
                binary: false,
                hunks: Vec::new(),
                added: 0,
                removed: 0,
                lines: 0,
                bytes: 0,
            },
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
        let entry = cached(ChangeKind::Modified, blob(1), trusted(40, epoch(10)));
        let now = change(ChangeKind::Modified, blob(1));
        assert!(reusable(&entry, &now, Some(print(40, epoch(10)))));
    }

    #[test]
    fn a_new_index_blob_invalidates_without_the_file_moving() {
        // Staging some other change rewrites the index, and the index is the
        // left-hand side of this diff. The bytes on disk are untouched.
        let entry = cached(ChangeKind::Modified, blob(1), trusted(40, epoch(10)));
        let now = change(ChangeKind::Modified, blob(2));
        assert!(!reusable(&entry, &now, Some(print(40, epoch(10)))));
    }

    #[test]
    fn a_new_kind_invalidates() {
        let entry = cached(ChangeKind::Modified, blob(1), trusted(40, epoch(10)));
        let now = change(
            ChangeKind::Renamed {
                from: "src/old.rs".to_owned(),
            },
            blob(1),
        );
        assert!(!reusable(&entry, &now, Some(print(40, epoch(10)))));
    }

    #[test]
    fn a_changed_length_invalidates() {
        let entry = cached(ChangeKind::Modified, blob(1), trusted(40, epoch(10)));
        let now = change(ChangeKind::Modified, blob(1));
        assert!(!reusable(&entry, &now, Some(print(41, epoch(10)))));
    }

    #[test]
    fn a_changed_mtime_invalidates() {
        let entry = cached(ChangeKind::Modified, blob(1), trusted(40, epoch(10)));
        let now = change(ChangeKind::Modified, blob(1));
        assert!(!reusable(&entry, &now, Some(print(40, epoch(11)))));
    }

    /// The racily-clean case, which is why `settled` exists.
    ///
    /// Same length, same modification time, and that time was *not* already in
    /// the past when the content was read. A second write inside the same
    /// granule would look exactly like this, so the rule has to refuse.
    #[test]
    fn an_unsettled_fingerprint_is_never_reusable() {
        let entry = cached(
            ChangeKind::Modified,
            blob(1),
            Some(Observed {
                print: print(40, epoch(10)),
                settled: false,
            }),
        );
        let now = change(ChangeKind::Modified, blob(1));
        assert!(!reusable(&entry, &now, Some(print(40, epoch(10)))));
    }

    #[test]
    fn a_file_that_cannot_be_fingerprinted_now_is_not_reusable() {
        let entry = cached(ChangeKind::Modified, blob(1), trusted(40, epoch(10)));
        let now = change(ChangeKind::Modified, blob(1));
        assert!(!reusable(&entry, &now, None));
    }

    #[test]
    fn a_file_that_could_not_be_fingerprinted_then_is_not_reusable() {
        let entry = cached(ChangeKind::Modified, blob(1), None);
        let now = change(ChangeKind::Modified, blob(1));
        assert!(!reusable(&entry, &now, Some(print(40, epoch(10)))));
    }

    /// A removal is computed from the index side alone, so there is no
    /// working-tree fingerprint to check and its absence is not suspicious.
    #[test]
    fn a_removal_is_reusable_with_no_fingerprint_at_all() {
        let entry = cached(ChangeKind::Removed, blob(1), None);
        let now = change(ChangeKind::Removed, blob(1));
        assert!(reusable(&entry, &now, None));
    }

    #[test]
    fn a_conflict_is_reusable_with_no_fingerprint_at_all() {
        let entry = cached(ChangeKind::Conflict, blob(1), None);
        let now = change(ChangeKind::Conflict, blob(1));
        assert!(reusable(&entry, &now, None));
    }
}
