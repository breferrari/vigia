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
//! [`Frame`] is that same call with the previous answer kept.
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
//! repeated quickly is precisely that shape. So a fingerprint counts as proof
//! only when its modification time is **strictly older than the moment the
//! content was read** — after which any write at all must move the time
//! forward, so an unchanged time means unchanged bytes. Everything else is
//! re-diffed. That costs a redundant diff of a file being actively written,
//! which is a file that changed anyway, and buys never showing a stale one.
//!
//! Content is never hashed to make this decision. Hashing is the read I2a
//! exists to avoid.

use std::collections::HashMap;
use std::path::Path;
use std::time::SystemTime;

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
    /// This is what a reuse costs: one `stat`, never a read.
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
    /// True when `print.mtime` was already strictly in the past when the read
    /// began. That is what makes an unchanged fingerprint proof of unchanged
    /// bytes instead of a guess; see the module comment on racily clean.
    settled: bool,
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
    /// Diffs nothing. A monitor draws the top of a long list without having
    /// looked at the bottom (I4), so content is fetched by [`Frame::diff`] when
    /// a caller asks for it.
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
    /// If `index` is out of range, the same way indexing a slice does.
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
                settled: print.mtime < read_started,
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
                bytes: 0,
            },
        }
    }

    /// A settled fingerprint that still matches: the only reusable case.
    fn settled(len: u64, mtime: SystemTime) -> Option<Observed> {
        Some(Observed {
            print: print(len, mtime),
            settled: true,
        })
    }

    #[test]
    fn an_unchanged_file_is_reusable() {
        let entry = cached(ChangeKind::Modified, blob(1), settled(40, epoch(10)));
        let now = change(ChangeKind::Modified, blob(1));
        assert!(reusable(&entry, &now, Some(print(40, epoch(10)))));
    }

    #[test]
    fn a_new_index_blob_invalidates_without_the_file_moving() {
        // Staging some other change rewrites the index, and the index is the
        // left-hand side of this diff. The bytes on disk are untouched.
        let entry = cached(ChangeKind::Modified, blob(1), settled(40, epoch(10)));
        let now = change(ChangeKind::Modified, blob(2));
        assert!(!reusable(&entry, &now, Some(print(40, epoch(10)))));
    }

    #[test]
    fn a_new_kind_invalidates() {
        let entry = cached(ChangeKind::Modified, blob(1), settled(40, epoch(10)));
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
        let entry = cached(ChangeKind::Modified, blob(1), settled(40, epoch(10)));
        let now = change(ChangeKind::Modified, blob(1));
        assert!(!reusable(&entry, &now, Some(print(41, epoch(10)))));
    }

    #[test]
    fn a_changed_mtime_invalidates() {
        let entry = cached(ChangeKind::Modified, blob(1), settled(40, epoch(10)));
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
        let entry = cached(ChangeKind::Modified, blob(1), settled(40, epoch(10)));
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
