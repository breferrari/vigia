use std::cell::{Cell, Ref, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use gix::bstr::{BString, ByteSlice};
use gix::status::index_worktree::{Item, RewriteSource, iter::Summary};

use crate::change::{ChangeKind, FileChange, Origin, Side};
use crate::error::{Error, Result};
use crate::filter::Filter;
use crate::frame::{Fingerprint, Frame, config_moved, followed_print, settled};
use crate::hidden::Hidden;
use crate::hunk::{self, FileDiff};
use crate::picture::{self, Format, ImageSide, Picture, Source, Sources};
use crate::rename;
use crate::standing::Standing;
use crate::watch::{WatchOptions, Watcher};

/// Knobs that change what a change sweep costs, and what it reports.
///
/// Borrowed rather than owned so this stays `Copy`: a compiled pattern is not,
/// and the walk needs one only for as long as it runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChangeOptions<'h> {
    /// Pair deletions with additions so a moved file reads as one change.
    pub track_renames: bool,
    /// Paths the reader asked to keep out of the pane. Applied here rather than
    /// at render, so a hidden path is absent from everything downstream of the
    /// walk instead of merely undrawn.
    pub hide: Option<&'h Hidden>,
}

impl Default for ChangeOptions<'_> {
    fn default() -> Self {
        Self {
            track_renames: true,
            hide: None,
        }
    }
}

/// A working tree under observation.
pub struct Worktree {
    /// The repository under its current configuration. The watcher takes a clone.
    /// A borrow must end inside the method that took it, or a reload waits a tick.
    live: RefCell<gix::Repository>,
    /// What the status walk reads: `live` with every `filter` section removed, so
    /// the walk compares a filtered file as its raw bytes and starts no driver.
    status: RefCell<gix::Repository>,
    workdir: PathBuf,
    /// The clean filter: built on the first working-tree read after each
    /// [`Frame::advance`], and not before.
    filter: RefCell<Option<Filter>>,
    /// The files under the git dir that shape the clean filter: the
    /// configuration and `info/attributes`.
    filter_sources: [PathBuf; 2],
    /// The config's fingerprint when `live` last loaded it. `None` at open, so the
    /// first tick reloads: a write between opening and a stat would be missed.
    loaded_config: Cell<Option<Fingerprint>>,
    /// How many reloads have landed, so a frame can drop what it cached before one.
    reloads: Cell<u64>,
    /// Whether the last tracked walk found a deletion, which lasts until it is
    /// committed or restored, so the next walk goes straight to tracking.
    deleted: Cell<bool>,
    /// LFS files compared with their local object.
    lfs_compares: Cell<u64>,
    /// Stored blobs read to learn whether they are LFS pointers.
    lfs_blob_reads: Cell<u64>,
    /// Whether the configuration names Git LFS, read with it.
    lfs: Cell<bool>,
    /// Whether each LFS file's bytes equal its object, by its fingerprint and its
    /// stored blob, kept only once the fingerprint has settled.
    lfs_verdicts: RefCell<HashMap<String, (LfsKey, bool)>>,
    /// What each stored blob the walk flagged is as an LFS pointer, if one.
    lfs_pointers: RefCell<HashMap<gix::ObjectId, Option<(String, u64)>>>,
    /// Bytes the current walk may still compare.
    lfs_budget: Cell<u64>,
    /// When the last walk's undecided LFS files can be decided: a print that
    /// settles, or at once for a file the walk could not afford.
    lfs_wait: Cell<Option<std::time::SystemTime>>,
    /// Git's score for each pair of versions a walk weighed as a rename.
    pair_scores: RefCell<HashMap<rename::PairKey, u64>>,
    /// Candidate bytes the current frame may still score.
    pair_budget: Cell<u64>,
    /// The pairs the last walks could not afford, for the picture worker.
    unscored: RefCell<Vec<rename::Candidate>>,
    /// Candidate bytes scored, for the tests that bound a frame.
    pair_bytes: Cell<u64>,
    /// Pairs scored, for the same.
    pairs_scored: Cell<u64>,
    /// Sides read to weigh or score them, for the same.
    side_reads: Cell<u64>,
    /// Whether each side version a walk weighed is binary, by its stamp.
    binaries: RefCell<HashMap<u64, bool>>,
    /// How many times `pair_scores` started again.
    pair_epoch: Cell<u64>,
    /// Each blob's size, as the rename pass found it.
    blob_sizes: RefCell<HashMap<gix::ObjectId, u64>>,
    /// Whether this frame's budget ran out before every side was weighed.
    sides_left: Cell<bool>,
    /// Whether this frame had more pairs for the worker than it listed.
    pairs_unlisted: Cell<bool>,
}

impl Worktree {
    /// Find the repository at or above `path` and open its working tree.
    ///
    /// # Errors
    ///
    /// `path` is not inside a git repository, the repository it finds is bare and has no
    /// worktree to compare against, or its configuration cannot be read without its
    /// filter drivers.
    pub fn discover(path: impl AsRef<Path>) -> Result<Self> {
        let repo = gix::discover(path).map_err(|e| Error::Discover(Box::new(e)))?;
        let workdir = repo.workdir().ok_or(Error::Bare)?.to_path_buf();
        let common = repo.common_dir();
        let filter_sources = [
            common.join("config"),
            common.join("info").join("attributes"),
        ];
        let lfs = lfs_configured(&repo);
        Ok(Self {
            status: RefCell::new(without_drivers(&repo)?),
            live: RefCell::new(repo),
            workdir,
            filter: RefCell::new(None),
            filter_sources,
            loaded_config: Cell::new(None),
            reloads: Cell::new(0),
            deleted: Cell::new(false),
            lfs_compares: Cell::new(0),
            lfs_blob_reads: Cell::new(0),
            lfs_verdicts: RefCell::new(HashMap::new()),
            lfs_pointers: RefCell::new(HashMap::new()),
            lfs_budget: Cell::new(LFS_COMPARE_MAX),
            lfs_wait: Cell::new(None),
            lfs: Cell::new(lfs),
            pair_scores: RefCell::new(HashMap::new()),
            pair_budget: Cell::new(rename::FRAME_BUDGET),
            unscored: RefCell::new(Vec::new()),
            pair_bytes: Cell::new(0),
            pairs_scored: Cell::new(0),
            side_reads: Cell::new(0),
            binaries: RefCell::new(HashMap::new()),
            pair_epoch: Cell::new(0),
            blob_sizes: RefCell::new(HashMap::new()),
            sides_left: Cell::new(false),
            pairs_unlisted: Cell::new(false),
        })
    }

    /// Absolute path of the working tree root.
    pub fn workdir(&self) -> &Path {
        &self.workdir
    }

    fn repo(&self) -> Ref<'_, gix::Repository> {
        self.live.borrow()
    }

    /// The branch HEAD names, shortened, or `None` when HEAD is detached.
    pub fn branch(&self) -> Option<String> {
        let name = self.repo().head_name().ok()??;
        Some(name.shorten().to_string())
    }

    /// Stream the working-tree-vs-index changes with default options.
    ///
    /// # Errors
    ///
    /// `gix` cannot walk the worktree's status.
    pub fn changes(&self) -> Result<Changes<'static>> {
        self.changes_with(ChangeOptions::default())
    }

    /// Stream the working-tree-vs-index changes.
    ///
    /// # Errors
    ///
    /// `gix` cannot walk the worktree's status.
    pub fn changes_with<'h>(&self, options: ChangeOptions<'h>) -> Result<Changes<'h>> {
        self.changes_of(Origin::Unstaged, options)
    }

    /// Stream one comparison's changes.
    ///
    /// # Errors
    ///
    /// `gix` cannot walk the worktree's status.
    pub fn changes_of<'h>(
        &self,
        origin: Origin,
        options: ChangeOptions<'h>,
    ) -> Result<Changes<'h>> {
        let inner = match origin {
            Origin::Unstaged => self.unstaged(options.track_renames)?,
            Origin::Staged => Inner::Collected(self.staged(options)?.into_iter()),
        };
        Ok(Changes::over(inner, options.hide))
    }

    /// The working tree against the index, less the LFS files whose bytes are
    /// their object.
    fn unstaged(&self, track_renames: bool) -> Result<Inner> {
        let walked = self.unstaged_lfs(track_renames)?;
        if !track_renames {
            return Ok(walked);
        }
        let walked: Vec<FileChange> = walked.collect::<Result<_>>()?;
        Ok(Inner::Collected(self.paired(walked).into_iter()))
    }

    /// [`Worktree::unstaged`] before binaries are paired.
    fn unstaged_lfs(&self, track_renames: bool) -> Result<Inner> {
        let walked = self.unstaged_walk(track_renames)?;
        if !self.lfs.get() {
            return Ok(walked);
        }
        let walked: Vec<FileChange> = walked.collect::<Result<_>>()?;
        // A pane open for days keeps nothing for a file the walk stopped flagging.
        let flagged: std::collections::HashSet<&str> =
            walked.iter().map(|change| change.path.as_str()).collect();
        let blobs: std::collections::HashSet<gix::ObjectId> =
            walked.iter().filter_map(|change| change.before).collect();
        self.lfs_verdicts
            .borrow_mut()
            .retain(|path, _| flagged.contains(path.as_str()));
        self.lfs_pointers
            .borrow_mut()
            .retain(|id, _| blobs.contains(id));
        self.lfs_budget.set(LFS_COMPARE_MAX);
        self.lfs_wait.set(None);
        let kept: Vec<FileChange> = walked
            .into_iter()
            .filter(|change| !self.lfs_unchanged(change))
            .collect();
        Ok(Inner::Collected(kept.into_iter()))
    }

    /// Whether `change` is an LFS file whose worktree bytes are the object its
    /// stored pointer names, which the walk flagged only because their timestamp
    /// moved. The checks run cheapest first.
    fn lfs_unchanged(&self, change: &FileChange) -> bool {
        if change.kind != ChangeKind::Modified
            || change.after != Some(Side::Worktree)
            || change.maybe_symlink
            || change.mode_changed
        {
            return false;
        }
        let Some(id) = change.before else {
            return false;
        };
        let Some((oid, size)) = self.stored_pointer(id, &change.path) else {
            return false;
        };
        let path = self.workdir.join(&change.path);
        let started = std::time::SystemTime::now();
        let Some(print) = followed_print(&path) else {
            return false;
        };
        // The attribute is asked fresh, and before a compare, so a file it no
        // longer names spends nothing; the cache holds only what the bytes say.
        if !self
            .with_filter(|filter| filter.diff_attribute(&change.path))
            .is_ok_and(|(_, lfs)| lfs)
        {
            return false;
        }
        let key = (print, id);
        let cached = self
            .lfs_verdicts
            .borrow()
            .get(&change.path)
            .filter(|(seen, _)| *seen == key)
            .map(|&(_, same)| same);
        if let Some(same) = cached {
            return same;
        }
        let Some(same) = self.lfs_compare(&path, &oid, size, print) else {
            return false;
        };
        // A print still inside the settle margin can hide a same-length rewrite,
        // so its verdict holds for this walk, and the walk after it settles asks
        // again.
        if settled(print.mtime(), started) {
            self.lfs_verdicts
                .borrow_mut()
                .insert(change.path.clone(), (key, same));
        } else {
            self.lfs_wait_until(print.mtime() + crate::frame::SETTLE_MARGIN);
        }
        same
    }

    /// Whether the worktree file at `path`, of fingerprint `print`, holds the LFS
    /// object `oid` of `size` bytes, or `None` when that cannot be decided this
    /// walk: the object is not fetched, the walk's compare budget is spent, or a
    /// read failed.
    fn lfs_compare(&self, path: &Path, oid: &str, size: u64, print: Fingerprint) -> Option<bool> {
        if print.len() != size {
            return Some(false);
        }
        // A larger compare would cost the frame its budget, and belongs off the
        // paint.
        if size > self.lfs_budget.get() {
            self.lfs_wait_until(std::time::SystemTime::now());
            return None;
        }
        let object = self.lfs_path(oid)?;
        self.lfs_budget.set(self.lfs_budget.get() - size);
        self.lfs_compares.set(self.lfs_compares.get() + 1);
        same_bytes(path, &object, size)
    }

    /// The object and size the blob `id` names when it is an LFS pointer, read
    /// once per blob, with no read of a blob too large to be one.
    fn stored_pointer(&self, id: gix::ObjectId, path: &str) -> Option<(String, u64)> {
        if let Some(found) = self.lfs_pointers.borrow().get(&id) {
            return found.clone();
        }
        let small = self
            .repo()
            .find_header(id)
            .is_ok_and(|header| header.size() < picture::POINTER_MAX as u64);
        let found = small
            .then(|| {
                self.lfs_blob_reads.set(self.lfs_blob_reads.get() + 1);
                self.blob(id, path).ok()
            })
            .flatten()
            .and_then(|blob| picture::lfs_pointer(&blob).map(|(oid, size)| (oid.to_owned(), size)));
        self.lfs_pointers.borrow_mut().insert(id, found.clone());
        found
    }

    /// The working tree against the index.
    ///
    /// A rename pairs a deletion with an addition, so a walk that finds no
    /// deletion reads the same with tracking off. Tracking is what costs: `gix`
    /// reads and hashes every untracked file as a candidate whether or not
    /// anything was deleted, which is most of a walk over many. So the walk runs
    /// without it first, sorted as the tracking walk sorts, and again with it only
    /// when a deletion turned up.
    fn unstaged_walk(&self, track_renames: bool) -> Result<Inner> {
        if !track_renames {
            return Ok(Inner::Streamed(self.walk(Walk::Streamed)?));
        }
        if !self.deleted.get()
            && let Ok(plain) = Inner::Streamed(self.walk(Walk::Sorted)?).collect::<Result<Vec<_>>>()
            && !plain.iter().any(|c| c.kind == ChangeKind::Removed)
        {
            return Ok(Inner::Collected(plain.into_iter()));
        }
        // Collected rather than streamed costs nothing: tracking buffers the walk.
        let tracked: Vec<FileChange> =
            Inner::Streamed(self.walk(Walk::Renames)?).collect::<Result<_>>()?;
        // A rename's source is a deletion the plain walk would report as one.
        self.deleted.set(
            tracked
                .iter()
                .any(|c| matches!(c.kind, ChangeKind::Removed | ChangeKind::Renamed { .. })),
        );
        Ok(Inner::Collected(tracked.into_iter()))
    }

    fn walk(&self, walk: Walk) -> Result<gix::status::index_worktree::Iter> {
        use gix::status::plumbing::index_as_worktree_with_renames::Sorting;
        self.status
            .borrow()
            .status(gix::progress::Discard)
            .map_err(|e| Error::Status(Box::new(e)))?
            // Collapsed would report a changed directory as one entry. A
            // monitor has to name the file that changed.
            .untracked_files(gix::status::UntrackedFiles::Files)
            .index_worktree_options_mut(|options| {
                options.sorting = (walk == Walk::Sorted).then_some(Sorting::ByPathCaseSensitive);
            })
            // Sorts too, which is what makes `Sorted` the same order.
            .index_worktree_rewrites((walk == Walk::Renames).then(gix::diff::Rewrites::default))
            .into_index_worktree_iter(Vec::<BString>::new())
            .map_err(|e| Error::Status(Box::new(e)))
    }

    /// How many changes one comparison holds, without keeping any of them.
    ///
    /// Takes the pattern alone rather than the whole options struct, because this
    /// forces rename tracking on and a caller able to turn it off here would be
    /// changing what the number means rather than what it costs.
    ///
    /// # Errors
    ///
    /// `gix` cannot walk the worktree's status.
    pub fn count_of(&self, origin: Origin, hide: Option<&Hidden>) -> Result<Counted> {
        // Its own rename budget, and the frame's pairs left as they were.
        let budget = self.pair_budget.replace(rename::FRAME_BUDGET);
        let unscored = self.unscored.take();
        let left = (self.sides_left.get(), self.pairs_unlisted.get());
        let counted = self.count_walk(origin, hide);
        self.pair_budget.set(budget);
        self.unscored.replace(unscored);
        self.sides_left.set(left.0);
        self.pairs_unlisted.set(left.1);
        counted
    }

    /// [`Worktree::count_of`] on whatever rename budget is set.
    fn count_walk(&self, origin: Origin, hide: Option<&Hidden>) -> Result<Counted> {
        // Rename tracking on, and the cheaper spelling is wrong here.
        let mut walk = self.changes_of(
            origin,
            ChangeOptions {
                hide,
                ..ChangeOptions::default()
            },
        )?;
        let mut shown = 0;
        for change in &mut walk {
            change?;
            shown += 1;
        }
        Ok(Counted {
            shown,
            hidden: walk.hidden(),
        })
    }

    /// The index against `HEAD^{tree}`, collected.
    fn staged(&self, options: ChangeOptions<'_>) -> Result<Vec<FileChange>> {
        let repo = self.repo();
        let tree = match repo.head_tree_id() {
            Ok(id) => id.detach(),
            // Unborn, detached at nothing, or an unreadable `HEAD`.
            Err(_) => repo.empty_tree().id().detach(),
        };
        self.against_index(tree, options)
    }

    /// One tree against the index, which is the staged run's own walk with the
    /// tree left to the caller.
    fn against_index(
        &self,
        tree: gix::ObjectId,
        options: ChangeOptions<'_>,
    ) -> Result<Vec<FileChange>> {
        let repo = self.repo();
        let index = repo
            .index_or_empty()
            .map_err(|e| Error::Status(Box::new(e)))?;

        let renames = if options.track_renames {
            gix::status::tree_index::TrackRenames::Given(gix::diff::Rewrites::default())
        } else {
            gix::status::tree_index::TrackRenames::Disabled
        };

        // A sparse index yields no staged run rather than a dead pane. Asked before
        // the walk, because the walk reports it only as a message.
        if index.is_sparse() {
            return Ok(Vec::new());
        }

        let mut changes = Vec::new();
        repo.tree_index_status(&tree, &index, None, renames, |change, _, _| {
            if let Some(change) = staged_change(&change) {
                changes.push(change);
            }
            Ok(gix::diff::index::Action::Continue(()))
        })
        .map_err(|e| Error::Status(Box::new(e)))?;
        drop(repo);
        Ok(if options.track_renames {
            self.paired(changes)
        } else {
            changes
        })
    }

    /// `changes` with each removal and addition `gix` left unpaired, where a
    /// side is binary, paired by git's rule. A pair this frame cannot afford
    /// to score is left unpaired and listed for the picture worker.
    fn paired(&self, mut changes: Vec<FileChange>) -> Vec<FileChange> {
        /// A removal and an addition weighed as one rename.
        struct Weighed {
            from: usize,
            to: usize,
            key: rename::PairKey,
            sizes: (u64, u64),
            bytes: u64,
        }
        let of_kind = |kind: ChangeKind| -> Vec<usize> {
            (0..changes.len())
                .filter(|&at| changes[at].kind == kind)
                .collect()
        };
        let (removed, added) = (of_kind(ChangeKind::Removed), of_kind(ChangeKind::Added));
        // Within git's limit of removals times additions, and neither side
        // past it alone, so a frame never sizes more than 1,000 of either.
        if removed.is_empty()
            || added.is_empty()
            || removed.len() > rename::RENAME_LIMIT
            || added.len() > rename::RENAME_LIMIT
        {
            return changes;
        }
        // Takes `cost` from the frame's budget, or notes a side left unweighed.
        let spend = |cost: u64| -> bool {
            if cost > self.pair_budget.get() {
                self.sides_left.set(true);
                return false;
            }
            self.pair_budget.set(self.pair_budget.get() - cost);
            true
        };
        // Each side's size and stamp, from a header or a `stat`, before a byte.
        let sized = |at: usize| -> Option<(usize, u64, u64)> {
            let change = &changes[at];
            let side = match change.kind {
                ChangeKind::Removed => Side::Blob(change.before?),
                _ => change.after?,
            };
            match side {
                Side::Blob(id) => {
                    // A blob's size never changes, so its header is read once.
                    let known = self.blob_sizes.borrow().get(&id).copied();
                    let size = match known {
                        Some(size) => size,
                        None => {
                            if !spend(rename::READ_COST) {
                                return None;
                            }
                            self.side_reads.set(self.side_reads.get() + 1);
                            let size = self.repo().find_header(id).ok()?.size();
                            let mut sizes = self.blob_sizes.borrow_mut();
                            if sizes.len() >= PAIRS_KEPT {
                                sizes.clear();
                            }
                            sizes.insert(id, size);
                            size
                        }
                    };
                    Some((at, size, picture::blob_stamp(&id)))
                }
                Side::Worktree => {
                    let meta = std::fs::symlink_metadata(self.workdir.join(&change.path)).ok()?;
                    meta.is_file()
                        .then(|| (at, meta.len(), picture::path_stamp(&change.path, &meta)))
                }
            }
        };
        let removed: Vec<_> = removed.into_iter().filter_map(sized).collect();
        // Each side's bytes as git stores them, read once in this walk however
        // many pairs it is in, through the open repository.
        let read: RefCell<HashMap<usize, Option<std::rc::Rc<Vec<u8>>>>> = RefCell::default();
        let bytes_of = |at: usize| {
            read.borrow_mut()
                .entry(at)
                .or_insert_with(|| {
                    self.side_reads.set(self.side_reads.get() + 1);
                    let change = &changes[at];
                    let side = match change.kind {
                        ChangeKind::Removed => Side::Blob(change.before?),
                        _ => change.after?,
                    };
                    match side {
                        Side::Blob(id) => self.blob(id, &change.path).ok(),
                        Side::Worktree => std::fs::read(self.workdir.join(&change.path)).ok(),
                    }
                    .map(std::rc::Rc::new)
                })
                .clone()
        };
        // Each side cut into chunks once in this walk, however many pairs it is in.
        let cut: RefCell<HashMap<usize, Option<std::rc::Rc<rename::Spans>>>> = RefCell::default();
        let spans_of = |at: usize| {
            if let Some(spans) = cut.borrow().get(&at) {
                return spans.clone();
            }
            let spans = bytes_of(at).map(|bytes| std::rc::Rc::new(rename::Spans::of(&bytes)));
            cut.borrow_mut().insert(at, spans.clone());
            spans
        };
        // Each side the budget paid for in this walk, which is read at most once.
        let charged: RefCell<std::collections::HashSet<usize>> = RefCell::default();
        let charge = |at: usize, size: u64| -> bool {
            if charged.borrow().contains(&at) {
                return true;
            }
            if size > self.pair_budget.get() {
                return false;
            }
            self.pair_budget.set(self.pair_budget.get() - size);
            self.pair_bytes.set(self.pair_bytes.get() + size);
            charged.borrow_mut().insert(at);
            true
        };
        // Whether a side is binary, from its first bytes, once per version, or
        // `None` when the budget cannot pay to learn it this frame. A blob is
        // read whole to learn it; one past a whole budget counts as binary, and
        // its pair goes to the worker.
        let binary = |at: usize, stamp: u64, size: u64| -> Option<bool> {
            if let Some(&known) = self.binaries.borrow().get(&stamp) {
                return Some(known);
            }
            let change = &changes[at];
            let whole =
                change.kind == ChangeKind::Removed || matches!(change.after, Some(Side::Blob(_)));
            let found = if whole {
                if size > rename::FRAME_BUDGET {
                    return Some(true);
                }
                if !charge(at, size.max(rename::READ_COST)) {
                    self.sides_left.set(true);
                    return None;
                }
                bytes_of(at).is_some_and(|bytes| hunk::is_binary(&bytes))
            } else {
                if !spend(rename::READ_COST) {
                    return None;
                }
                use std::io::Read;
                self.side_reads.set(self.side_reads.get() + 1);
                let mut head = Vec::new();
                std::fs::File::open(self.workdir.join(&change.path))
                    .and_then(|file| {
                        file.take(hunk::BINARY_SNIFF_LEN as u64)
                            .read_to_end(&mut head)
                    })
                    .is_ok_and(|_| hunk::is_binary(&head))
            };
            let mut binaries = self.binaries.borrow_mut();
            if binaries.len() >= PAIRS_KEPT {
                binaries.clear();
            }
            binaries.insert(stamp, found);
            Some(found)
        };
        let added: Vec<_> = added.into_iter().filter_map(sized).collect();
        let mut weighed: Vec<Weighed> = removed
            .iter()
            .flat_map(|&(from, from_size, from_stamp)| {
                added.iter().filter_map(move |&(to, to_size, to_stamp)| {
                    rename::sizes_fit(from_size, to_size).then_some(Weighed {
                        from,
                        to,
                        key: (from_stamp, to_stamp),
                        sizes: (from_size, to_size),
                        bytes: from_size + to_size,
                    })
                })
            })
            .collect();
        // The cheapest first, so the budget goes furthest.
        weighed.sort_by_key(|pair| pair.bytes);
        let mut listed: std::collections::HashSet<rename::PairKey> = self
            .unscored
            .borrow()
            .iter()
            .map(|candidate| candidate.key)
            .collect();
        let mut scored = Vec::new();
        // Where a candidate's blobs are read, asked once a walk.
        let mut homes: Option<(PathBuf, PathBuf)> = None;
        // Every side is weighed before a pair is scored, so a burst's sides are
        // weighed within a few frames, and what they cannot score goes to the
        // worker.
        let weighed: Vec<(Weighed, Option<u64>)> = weighed
            .into_iter()
            .filter_map(|pair| {
                let known = self.pair_scores.borrow().get(&pair.key).copied();
                if known.is_none() {
                    let to = binary(pair.to, pair.key.1, pair.sizes.1)?;
                    let from = binary(pair.from, pair.key.0, pair.sizes.0)?;
                    // `gix` scored every pair with no binary side.
                    if !to && !from {
                        return None;
                    }
                }
                Some((pair, known))
            })
            .collect();
        for (pair, known) in weighed {
            // What is left to read of the two sides.
            let unread = |at: usize, size: u64| {
                if charged.borrow().contains(&at) {
                    0
                } else {
                    size.max(rename::READ_COST)
                }
            };
            let cost = unread(pair.from, pair.sizes.0) + unread(pair.to, pair.sizes.1);
            let score = match known {
                Some(score) => score,
                // Past what the scores hold with those listed, a new score
                // would evict another.
                None if self.pair_scores.borrow().len() + self.unscored.borrow().len()
                    >= PAIRS_KEPT =>
                {
                    continue;
                }
                None if cost + rename::PAIR_COST <= self.pair_budget.get() => {
                    charge(pair.from, pair.sizes.0.max(rename::READ_COST));
                    charge(pair.to, pair.sizes.1.max(rename::READ_COST));
                    self.pair_budget
                        .set(self.pair_budget.get() - rename::PAIR_COST);
                    self.pairs_scored.set(self.pairs_scored.get() + 1);
                    let score = match (spans_of(pair.from), spans_of(pair.to)) {
                        (Some(old), Some(new)) => old.score(&new),
                        _ => 0,
                    };
                    self.keep_score(pair.key, score);
                    score
                }
                None => {
                    if self.unscored.borrow().len() >= rename::LISTED_MAX {
                        self.pairs_unlisted.set(true);
                    } else if listed.insert(pair.key) {
                        let homes = homes.get_or_insert_with(|| {
                            (self.repo().git_dir().to_owned(), self.lfs_store())
                        });
                        self.unscored.borrow_mut().push(rename::Candidate {
                            key: pair.key,
                            sides: self.pair_sources(&changes[pair.from], &changes[pair.to], homes),
                        });
                    }
                    continue;
                }
            };
            if rename::pairs(score) {
                scored.push((score, pair.from, pair.to));
            }
        }
        // Best first; a tie goes to the same file name, as `gix` breaks one.
        let name = |at: usize| changes[at].path.rsplit('/').next().unwrap_or_default();
        scored.sort_by(|a, b| {
            let same = |&(_, from, to): &(u64, usize, usize)| name(from) == name(to);
            b.0.cmp(&a.0).then_with(|| same(b).cmp(&same(a)))
        });
        let mut used = std::collections::HashSet::new();
        for (_, from, to) in scored {
            if used.contains(&from) || used.contains(&to) {
                continue;
            }
            used.extend([from, to]);
            let source = changes[from].path.clone();
            let before = changes[from].before;
            let target = &mut changes[to];
            target.kind = ChangeKind::Renamed { from: source };
            target.before = before;
        }
        // A removal that paired is the rename now, listed under its new path.
        changes
            .into_iter()
            .enumerate()
            .filter(|(at, change)| !(change.kind == ChangeKind::Removed && used.contains(at)))
            .map(|(_, change)| change)
            .collect()
    }

    /// Where a candidate pair's two sides are read from, raw, as the frame
    /// reads them, with no file touched to say so. A blob side that is an LFS
    /// pointer is under 1 KiB and pairs only with a side as small, which the
    /// frame always scores itself.
    fn pair_sources(
        &self,
        from: &FileChange,
        to: &FileChange,
        (git_dir, store): &(PathBuf, PathBuf),
    ) -> Sources {
        let blob = |id| Source::Blob {
            git_dir: git_dir.clone(),
            id,
            store: store.clone(),
        };
        Sources {
            before: from.before.map(blob),
            after: match to.after {
                Some(Side::Blob(id)) => Some(blob(id)),
                Some(Side::Worktree) => Some(Source::File(self.workdir.join(&to.path))),
                None => None,
            },
        }
    }

    /// A fresh scoring budget, for the frame about to walk.
    pub(crate) fn renew_pair_budget(&self) {
        self.pair_budget.set(rename::FRAME_BUDGET);
        self.unscored.borrow_mut().clear();
        self.sides_left.set(false);
        self.pairs_unlisted.set(false);
    }

    /// Whether this frame's walks left a side unweighed for want of budget.
    pub(crate) fn sides_left(&self) -> bool {
        self.sides_left.get()
    }

    /// Whether this frame's walks had more pairs for the worker than they listed.
    pub(crate) fn pairs_unlisted(&self) -> bool {
        self.pairs_unlisted.get()
    }

    /// The pairs the last walks could not afford to score.
    pub(crate) fn unscored_pairs(&self) -> Vec<rename::Candidate> {
        self.unscored.borrow().clone()
    }

    /// File a score the picture worker found.
    pub(crate) fn learn_pair(&self, key: rename::PairKey, score: u64) {
        self.keep_score(key, score);
    }

    /// Keep a pair's score, bounded: past [`PAIRS_KEPT`] the record starts
    /// again, and so does the count of them, so a pair sent before is sent again.
    fn keep_score(&self, key: rename::PairKey, score: u64) {
        let mut scores = self.pair_scores.borrow_mut();
        if scores.len() >= PAIRS_KEPT {
            scores.clear();
            self.pair_epoch.set(self.pair_epoch.get() + 1);
        }
        scores.insert(key, score);
    }

    /// How many times the scores were started again.
    pub(crate) fn pair_epoch(&self) -> u64 {
        self.pair_epoch.get()
    }

    /// Candidate bytes scored so far.
    pub(crate) fn pair_bytes(&self) -> u64 {
        self.pair_bytes.get()
    }

    /// Pairs scored so far.
    pub(crate) fn pairs_scored(&self) -> u64 {
        self.pairs_scored.get()
    }

    /// Rename sides read so far.
    pub(crate) fn side_reads(&self) -> u64 {
        self.side_reads.get()
    }

    /// The commit this branch left the one it tracks, and what to call it.
    ///
    /// The name matters as much as the id: a reader is thinking `main`, not a
    /// hash, so the header draws the branch and the diff uses the merge-base
    /// behind it. Resolved through the branch's own upstream first, then the
    /// remote's default, then the two names a repository with neither is
    /// overwhelmingly likely to use.
    ///
    /// # Errors
    ///
    /// There is no other branch to measure from, or no commit in common with it.
    pub fn branch_point(&self) -> Result<(gix::ObjectId, String)> {
        let repo = self.repo();
        let head = repo
            .head_id()
            .map_err(|e| Error::Standing(Box::new(e)))?
            .detach();
        for (reference, named) in self.candidates() {
            let Ok(other) = repo.find_reference(reference.as_str()) else {
                continue;
            };
            let Ok(other) = other.into_fully_peeled_id() else {
                continue;
            };
            let other = other.detach();
            // A branch that is its own upstream has no point to measure from:
            // the merge-base is HEAD and the run would be empty for a reason
            // nothing on screen explains.
            if other == head {
                continue;
            }
            if let Ok(base) = repo.merge_base(head, other) {
                return Ok((base.detach(), named));
            }
        }
        Err(Error::NoBranchPoint)
    }

    /// The references `branch_point` tries, in order, with the name each would
    /// put in the header.
    fn candidates(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        if let Ok(Some(head)) = self.repo().head_ref()
            && let Some(Ok(upstream)) = head.remote_tracking_ref_name(gix::remote::Direction::Fetch)
        {
            // `refs/remotes/origin/main` reads as `origin/main`, which is what a
            // reader would type.
            let short = upstream.shorten().to_string();
            out.push((upstream.as_bstr().to_string(), short));
        }
        for name in ["main", "master"] {
            out.push((format!("refs/heads/{name}"), name.to_owned()));
            out.push((
                format!("refs/remotes/origin/{name}"),
                format!("origin/{name}"),
            ));
        }
        out
    }

    /// `want` commits, newest first, from HEAD or from the one after `after`.
    ///
    /// Resumed by id rather than by keeping the iterator. A `gix` walk borrows the
    /// repository, so one held between frames would have to live beside the
    /// `Worktree` it borrows; walking again from the last commit handed out costs a
    /// page, because the history behind a tip is the same history whenever it is
    /// asked for.
    ///
    /// # Errors
    ///
    /// HEAD is unborn, or the walk fails, which is [`Error::History`].
    pub fn commits_from(&self, after: Option<gix::ObjectId>, want: usize) -> Result<Page> {
        let repo = self.repo();
        let tip = match after {
            Some(id) => id,
            None => repo
                .head_id()
                .map_err(|e| Error::History(Box::new(e)))?
                .detach(),
        };
        let walk = repo
            .rev_walk([tip])
            .all()
            .map_err(|e| Error::History(Box::new(e)))?;
        // The tip a resumed page starts from was handed out by the page before it.
        let skip = usize::from(after.is_some());
        let mut page = Page::default();
        // Counted at the walk rather than in the loop, which is the whole of what makes
        // the number mean anything: a loop over a drained iterator still runs `want`
        // times, so a counter inside the body cannot tell a page from a drain.
        let mut visited = 0usize;
        for info in walk.skip(skip).inspect(|_| visited += 1).take(want) {
            let info = info.map_err(|e| Error::History(Box::new(e)))?;
            // Read off the last commit walked rather than by stepping one further:
            // a commit with a parent is a history with more in it, and the walk's
            // own traversal has already filled this in.
            page.more = info.parent_ids.iter().next().is_some();
            page.commits.push(Self::landmark(&info)?);
        }
        page.visited = visited;
        Ok(page)
    }

    /// What a row draws of one commit the walk handed back.
    fn landmark(info: &gix::revision::walk::Info<'_>) -> Result<Landmark> {
        let commit = info.object().map_err(|e| Error::History(Box::new(e)))?;
        let subject = commit
            .message()
            .map_err(|e| Error::History(e.into()))?
            .summary()
            .to_string();
        let when = commit
            .time()
            .map_err(|e| Error::History(Box::new(e)))?
            .seconds;
        Ok(Landmark {
            id: info.id,
            named: info.id.to_hex_with_len(SHORT_ID).to_string(),
            subject,
            when,
        })
    }

    /// The run this standing names, which for `Current` is the pane's own walk.
    ///
    /// # Errors
    ///
    /// The walk fails, which is a failure of the whole comparison.
    pub fn changes_at<'h>(
        &self,
        standing: &Standing,
        options: ChangeOptions<'h>,
    ) -> Result<Changes<'h>> {
        let collected = match standing {
            Standing::Current => return self.changes_with(options),
            Standing::Since { at, .. } => self.since(*at, options)?,
            Standing::Only { at, .. } => self.only(*at, options)?,
        };
        Ok(Changes::over(
            Inner::Collected(collected.into_iter()),
            options.hide,
        ))
    }

    /// One commit alone: its first parent's tree against its own.
    ///
    /// The first parent and not a merge of all of them, which is what `git show`
    /// does: a merge's other parents are the branch it took in, and drawing them
    /// would report work this commit did not do as work it did. A root commit has
    /// no parent and is measured against the empty tree, so the commit that
    /// created the repository reads as the additions it made.
    ///
    /// Nothing here touches the working tree, and that is the whole of what makes
    /// this reading a still picture: both ends are trees the object database
    /// already holds, so a write while the pane is parked changes nothing under
    /// it.
    ///
    /// # Errors
    ///
    /// The commit, either tree, or the diff between them cannot be read.
    fn only(&self, at: gix::ObjectId, options: ChangeOptions<'_>) -> Result<Vec<FileChange>> {
        // Where the line between the two errors falls, and it decides both whether a
        // failure moves the reader and what the footer tells them. [`Error::Standing`]
        // is what the shell answers by bringing the pane home, so it covers this
        // position's own object and nothing else: a commit that cannot be read is a
        // place nobody can stand. The parent and the diff between them are the
        // comparison, which leaves the reader where they are.
        let repo = self.repo();
        let commit = repo
            .find_object(at)
            .map_err(|e| Error::Standing(Box::new(e)))?
            .peel_to_commit()
            .map_err(|e| Error::Standing(Box::new(e)))?;
        let tree = commit.tree().map_err(|e| Error::Standing(Box::new(e)))?;

        let parent = commit.parent_ids().next().map(gix::Id::detach);
        let before = parent
            .map(|parent| {
                repo.find_object(parent)
                    .map_err(|e| Error::Comparison(Box::new(e)))?
                    .peel_to_tree()
                    .map_err(|e| Error::Comparison(Box::new(e)))
            })
            .transpose()?;

        let rewrites = options.track_renames.then(gix::diff::Rewrites::default);
        let changes = repo
            .diff_tree_to_tree(
                before.as_ref(),
                Some(&tree),
                gix::diff::Options::default().with_rewrites(rewrites),
            )
            .map_err(|e| Error::Comparison(Box::new(e)))?;

        Ok(changes.iter().filter_map(committed_change).collect())
    }

    /// Everything since `base`: that commit's tree against the working tree.
    ///
    /// Two walks, because `gix` offers tree-against-index and
    /// index-against-worktree and the working tree is not a tree. The path set is
    /// their union, and **each path's before side is the base tree's**, which is
    /// what makes this one diff against `base` rather than two diffs printed
    /// together: a file changed on both sides of the index would otherwise show
    /// its index blob as the thing it changed from, which is a state the reader
    /// never asked about.
    ///
    /// A path that cancels by *existence* is not a row at all, and [`compose`]
    /// says why. A path that cancels by *content* is one: detecting that it came
    /// back to the base bytes is a read per path, and `SPEC.md` §3's I4 says a
    /// walk does not read content, so the diff under it draws zero hunks and says
    /// so, which is the cheaper honesty.
    ///
    /// # Errors
    ///
    /// Either walk fails, which is a failure of the whole comparison.
    fn since(&self, base: gix::ObjectId, options: ChangeOptions<'_>) -> Result<Vec<FileChange>> {
        // A position measures from a commit and the walk below diffs a tree, so
        // the peel is the whole of the difference between the two.
        let tree = self
            .repo()
            .find_object(base)
            .map_err(|e| Error::Standing(Box::new(e)))?
            .peel_to_tree()
            .map_err(|e| Error::Standing(Box::new(e)))?
            .id;
        // Neither half filters: `changes_at` wraps the union in one [`Changes`],
        // and a filter inside a half drops its own hidden count on the floor,
        // which is a number the header owes the reader.
        let unfiltered = ChangeOptions {
            hide: None,
            ..options
        };
        let from_base = self.against_index(tree, unfiltered)?;
        let mut unstaged = Vec::new();
        for change in self.changes_of(Origin::Unstaged, unfiltered)? {
            unstaged.push(change?);
        }

        // Indexed by path rather than owned by a map: the union wants one pass
        // over each walk, and the pane draws the order the walk found them in,
        // which a `HashMap` gives back differently every process.
        let mut at: HashMap<String, usize> = from_base
            .iter()
            .enumerate()
            .map(|(i, change)| (change.path.clone(), i))
            .collect();
        let mut from_base: Vec<Option<FileChange>> = from_base.into_iter().map(Some).collect();

        let mut out = Vec::with_capacity(from_base.len() + unstaged.len());
        for mut change in unstaged {
            // Under its own name, then under the name a *rename* came from: a path
            // the branch renamed and the working tree renamed again is in the two
            // walks under two different names, and pairing only on the first leaves
            // the middle name as a row for a file that is not on disk. A copy is
            // not that case and must not take the fallback, because its source is
            // still there and still owns whatever the branch did to it.
            let vacated = match &change.kind {
                ChangeKind::Renamed { from } => Some(from.as_str()),
                _ => None,
            };
            let based = at
                .remove(change.path.as_str())
                .or_else(|| vacated.and_then(|from| at.remove(from)))
                .and_then(|i| from_base[i].take());
            if let Some(based) = based {
                // The path moved on both sides of the index. What it changed from
                // is the base tree's blob, and what it is now is on disk.
                let Some(kind) = compose(&based.kind, &change.kind) else {
                    continue;
                };
                change.before = based.before;
                change.kind = kind;
            }
            change.origin = Origin::Unstaged;
            out.push(change);
        }
        // Whatever the working tree did not touch is the base-to-index change
        // whole, and its index blob is what the file holds. In the order that walk
        // reported them, which is the order the pane draws.
        out.extend(from_base.into_iter().flatten().map(|mut change| {
            change.origin = Origin::Unstaged;
            change
        }));
        Ok(out)
    }

    /// Start watching this working tree for change.
    ///
    /// # Errors
    ///
    /// The filesystem watcher cannot be armed on this worktree.
    pub fn watch(&self, options: WatchOptions) -> Result<Watcher<'_>> {
        Watcher::new(
            &self.repo(),
            &self.filter_sources[0],
            self.loaded_config.get(),
            &self.workdir,
            options,
        )
    }

    /// Start a frame over this working tree.
    pub fn frame(&self) -> Frame<'_> {
        Frame::new(self)
    }

    /// Compute the line-level diff for one change.
    ///
    /// # Errors
    ///
    /// Either side cannot be read: the working-tree file is unreadable, or the index names an
    /// object the database does not hold.
    ///
    /// Asked about one file, so it reports that file's failure to the caller who
    /// asked. [`Frame::diff`](crate::Frame::diff) draws the same failure instead,
    /// because a frame serving a screen cannot lose the entries beside it.
    pub fn diff(&self, change: &FileChange) -> Result<FileDiff> {
        self.diff_counted(change, true, &mut 0)
    }

    /// [`Worktree::diff`], reporting the type probes it spent. `svg` says
    /// whether a changed SVG is drawn rather than diffed as text.
    pub(crate) fn diff_counted(
        &self,
        change: &FileChange,
        svg: bool,
        probes: &mut u64,
    ) -> Result<FileDiff> {
        if !change.is_diffable() {
            // A conflict and a type change are states rather than diffs, and this
            // method deliberately reads nothing for them.
            return Ok(FileDiff::without_hunks(change.path.clone(), None));
        }

        let diff = self.diff_attribute(change)?;
        if let Some((mut picture, cost)) = self.drawn_vector(change, svg, diff) {
            let mut drawn = hunk::compute(change.path.clone(), &[], &[], Some(false));
            drawn.bytes = cost;
            self.stamp(change, &mut picture);
            drawn.picture = Some(picture);
            return Ok(drawn);
        }
        if let Some(window) = self.early_binary(change, diff) {
            let mut binary = hunk::compute(change.path.clone(), &[], &[], Some(false));
            binary.bytes = window.len() as u64;
            // Nothing was sniffed when the attribute decided.
            let sniffed = (diff != Some(false)).then_some(window.as_slice());
            binary.picture = self.early_picture(change, sniffed, &mut binary.bytes);
            if let Some(picture) = binary.picture.as_mut() {
                self.stamp(change, picture);
            }
            return Ok(binary);
        }
        let (before, after) = self.sides(change, probes)?;
        let mut diff = hunk::compute(change.path.clone(), &before, &after, diff);
        if diff.binary {
            let mut cost = 0;
            let named = Format::named(&change.path);
            let before = change
                .before
                .map(|_| self.held_side(&before, named, &mut cost));
            let after = change
                .after
                .map(|_| self.held_side(&after, named, &mut cost));
            diff.bytes += cost;
            diff.picture = Picture::of(before, after);
            if let Some(picture) = diff.picture.as_mut() {
                self.stamp(change, picture);
            }
        }
        Ok(diff)
    }

    /// The picture of a change found binary before its sides were read. The new
    /// side goes first, and the old side is read only when the new one is an
    /// image, so a binary that is not one costs its sniff `window` alone.
    fn early_picture(
        &self,
        change: &FileChange,
        window: Option<&[u8]>,
        cost: &mut u64,
    ) -> Option<Picture> {
        let named = Format::named(&change.path);
        let after = match change.after {
            Some(Side::Worktree) if window.is_some_and(|head| !picture::may_be(head, named)) => {
                return None;
            }
            Some(Side::Worktree) => {
                let path = self.workdir.join(&change.path);
                // A link's bytes are its target, and a fifo blocks whoever opens it.
                if change.maybe_symlink
                    && !std::fs::symlink_metadata(&path).is_ok_and(|meta| meta.is_file())
                {
                    return None;
                }
                Some(self.file_side(&path, true, named, cost))
            }
            Some(Side::Blob(id)) => Some(self.blob_side(id, &change.path, named, cost)),
            None => None,
        };
        if after == Some(None) {
            return None;
        }
        let before = change
            .before
            .map(|id| self.blob_side(id, &change.path, named, cost));
        Picture::of(before, after)
    }

    /// The picture of a changed SVG or PDF, and the bytes read to learn it:
    /// none when an attribute says how to diff it, or a side gives no size.
    fn drawn_vector(
        &self,
        change: &FileChange,
        svg: bool,
        diff: Option<bool>,
    ) -> Option<(Picture, u64)> {
        if !svg || diff.is_some() {
            return None;
        }
        let mut cost = 0;
        let picture = if picture::svg_named(&change.path) {
            self.svg_picture(change, &mut cost)
        } else if picture::pdf_named(&change.path) {
            self.pdf_picture(change, &mut cost)
        } else {
            return None;
        };
        picture.map(|picture| (picture, cost))
    }

    /// The picture of a changed PDF when every side it has gives a size. Each
    /// side is read whole, up to [`picture::PDF_LIMIT`].
    fn pdf_picture(&self, change: &FileChange, cost: &mut u64) -> Option<Picture> {
        use std::io::Read;
        let blob = |id: gix::ObjectId, cost: &mut u64| {
            // Its size from the object's header, without inflating it.
            let length = self.repo().find_header(id).ok()?.size();
            if length > picture::PDF_LIMIT {
                return None;
            }
            if length > picture::PDF_SIZED_HERE {
                return Some(picture::pdf_unsized(length));
            }
            let held = self.blob(id, &change.path).ok()?;
            *cost += held.len() as u64;
            picture::pdf_side(held)
        };
        let after = match change.after {
            Some(Side::Worktree) => {
                let path = self.workdir.join(&change.path);
                let meta = std::fs::symlink_metadata(&path).ok()?;
                // A link's bytes are its target, and a fifo blocks whoever opens it.
                if !meta.is_file() || meta.len() > picture::PDF_LIMIT {
                    return None;
                }
                if meta.len() > picture::PDF_SIZED_HERE {
                    // Only its header here; the worker reads the rest.
                    let mut head = Vec::new();
                    std::fs::File::open(&path)
                        .ok()?
                        .take(picture::PDF_HEAD as u64)
                        .read_to_end(&mut head)
                        .ok()?;
                    *cost += head.len() as u64;
                    Some(picture::pdf_headed(&head).then(|| picture::pdf_unsized(meta.len())))
                } else {
                    let bytes = std::fs::read(&path).ok()?;
                    *cost += bytes.len() as u64;
                    Some(picture::pdf_side(bytes))
                }
            }
            Some(Side::Blob(id)) => Some(blob(id, cost)),
            None => None,
        };
        let before = change.before.map(|id| blob(id, cost));
        Picture::of(before, after)
    }

    /// The picture of a changed SVG when every side it has gives a size. Reads
    /// the start of a worktree side only, and a blob whole.
    fn svg_picture(&self, change: &FileChange, cost: &mut u64) -> Option<Picture> {
        use std::io::Read;
        let blob = |id, cost: &mut u64| {
            let held = self.blob(id, &change.path).ok()?;
            *cost += held.len() as u64;
            picture::svg_side(&held, held.len() as u64)
        };
        let after = match change.after {
            Some(Side::Worktree) => {
                let path = self.workdir.join(&change.path);
                let meta = std::fs::symlink_metadata(&path).ok()?;
                // A link's bytes are its target, and a fifo blocks whoever opens it.
                if !meta.is_file() {
                    return None;
                }
                let mut head = Vec::new();
                std::fs::File::open(&path)
                    .ok()?
                    .take(picture::SVG_HEAD)
                    .read_to_end(&mut head)
                    .ok()?;
                *cost += head.len() as u64;
                Some(picture::svg_side(&head, meta.len()))
            }
            Some(Side::Blob(id)) => Some(blob(id, cost)),
            None => None,
        };
        let before = change.before.map(|id| blob(id, cost));
        Picture::of(before, after)
    }

    /// A blob's side, read whole because the object database has no partial read.
    /// A blob that cannot be read only costs the size line, never the diff.
    fn blob_side(
        &self,
        id: gix::ObjectId,
        path: &str,
        named: Option<Format>,
        cost: &mut u64,
    ) -> Option<ImageSide> {
        let held = self.blob(id, path).ok()?;
        *cost += held.len() as u64;
        self.held_side(&held, named, cost)
    }

    /// One side already in memory, or the LFS object it points at.
    fn held_side(&self, bytes: &[u8], named: Option<Format>, cost: &mut u64) -> Option<ImageSide> {
        match picture::lfs_oid(bytes) {
            Some(oid) => self.lfs_side(oid, named, cost),
            None => {
                picture::side_of(std::io::Cursor::new(bytes), bytes.len() as u64, named).flatten()
            }
        }
    }

    /// The object an LFS pointer names, from this repository's local store.
    fn lfs_side(&self, oid: &str, named: Option<Format>, cost: &mut u64) -> Option<ImageSide> {
        let path = self.lfs_path(oid)?;
        self.file_side(&path, false, named, cost)
    }

    /// Where the LFS object `oid` is stored, when it is a file there. Followed, so
    /// a store reached through a link reads, and a fifo is never opened.
    fn lfs_path(&self, oid: &str) -> Option<PathBuf> {
        let path = picture::lfs_object(&self.lfs_store(), oid);
        std::fs::metadata(&path)
            .is_ok_and(|meta| meta.is_file())
            .then_some(path)
    }

    /// This repository's LFS store: `lfs.storage`, or `lfs` under the common
    /// git directory.
    fn lfs_store(&self) -> PathBuf {
        let repo = self.repo();
        let common = repo.common_dir();
        match repo.config_snapshot().trusted_path("lfs.storage") {
            Ok(Some(path)) => common.join(path),
            _ => common.join("lfs"),
        }
    }

    /// Where both sides of a changed image can be read from on another thread.
    /// Nothing is read here but a short worktree file that may be an LFS pointer.
    pub fn image_sources(&self, change: &FileChange) -> Sources {
        let blob = |id| Source::Blob {
            git_dir: self.repo().git_dir().to_owned(),
            id,
            store: self.lfs_store(),
        };
        let after = match change.after {
            Some(Side::Blob(id)) => Some(blob(id)),
            Some(Side::Worktree) => {
                let path = self.workdir.join(&change.path);
                let pointer = std::fs::metadata(&path)
                    .ok()
                    .filter(|meta| meta.len() < picture::POINTER_MAX as u64)
                    .and_then(|_| std::fs::read(&path).ok())
                    .and_then(|head| picture::lfs_oid(&head).and_then(|oid| self.lfs_path(oid)));
                Some(Source::File(pointer.unwrap_or(path)))
            }
            None => None,
        };
        Sources {
            before: change.before.map(blob),
            after,
        }
    }

    /// Fill each side's stamp: the blob's id, or the worktree file's path and
    /// `stat`.
    fn stamp(&self, change: &FileChange, picture: &mut Picture) {
        if let (Some(side), Some(id)) = (picture.before.as_mut(), change.before) {
            side.stamp = picture::blob_stamp(&id);
        }
        if let Some(side) = picture.after.as_mut() {
            side.stamp = match change.after {
                Some(Side::Blob(id)) => picture::blob_stamp(&id),
                _ => std::fs::metadata(self.workdir.join(&change.path)).map_or_else(
                    |_| picture::unread_stamp(&change.path),
                    |meta| picture::path_stamp(&change.path, &meta),
                ),
            };
        }
    }

    /// A file's side, read through its header in the steps of
    /// [`picture::HEAD_LIMITS`], so at most their sum is read. A pointer is followed to its LFS object when
    /// `follow` is set, and an object is never followed.
    fn file_side(
        &self,
        path: &Path,
        follow: bool,
        named: Option<Format>,
        cost: &mut u64,
    ) -> Option<ImageSide> {
        use std::io::{Read, Seek};
        let file = std::fs::File::open(path).ok()?;
        let meta = file.metadata().ok()?;
        if !meta.is_file() {
            return None;
        }
        let length = meta.len();
        if length < picture::POINTER_MAX as u64 {
            let mut head = Vec::new();
            let read = (&file)
                .take(picture::POINTER_MAX as u64)
                .read_to_end(&mut head);
            *cost += head.len() as u64;
            read.ok()?;
            return match picture::lfs_oid(&head) {
                Some(oid) if follow => self.lfs_side(oid, named, cost),
                Some(_) => None,
                None => picture::side_of(std::io::Cursor::new(&head), length, named).flatten(),
            };
        }
        // Small, because every seek drops the buffer and the refill is read again.
        let mut reader = std::io::BufReader::with_capacity(512, picture::Capped::new(file, 0));
        let mut side = None;
        for limit in picture::HEAD_LIMITS {
            *cost += reader.get_mut().restart(limit);
            if reader.rewind().is_err() {
                break;
            }
            let Some(read) = picture::side_of(&mut reader, length, named) else {
                break;
            };
            side = read;
            // A file within the limit was read whole, so a larger one cannot help.
            if side.is_some() || reader.get_ref().total < limit || length <= limit {
                break;
            }
        }
        *cost += reader.get_ref().total;
        side
    }

    /// How tall one change's diff is, without building any of it.
    ///
    /// # Errors
    ///
    /// Either side cannot be read, as [`Worktree::diff`].
    pub fn measure(&self, change: &FileChange) -> Result<hunk::FileSpan> {
        self.measure_counted(change, true, &mut 0)
    }

    /// [`Worktree::measure`], reporting the type probes it spent.
    pub(crate) fn measure_counted(
        &self,
        change: &FileChange,
        svg: bool,
        probes: &mut u64,
    ) -> Result<hunk::FileSpan> {
        if !change.is_diffable() {
            return Ok(hunk::FileSpan::default());
        }

        let diff = self.diff_attribute(change)?;
        if let Some((_, cost)) = self.drawn_vector(change, svg, diff) {
            let mut drawn = hunk::measure(&[], &[], Some(false));
            drawn.bytes = cost;
            return Ok(drawn);
        }
        if let Some(window) = self.early_binary(change, diff) {
            let mut binary = hunk::measure(&[], &[], Some(false));
            binary.bytes = window.len() as u64;
            return Ok(binary);
        }
        let (before, after) = self.sides(change, probes)?;
        Ok(hunk::measure(&before, &after, diff))
    }

    /// The bytes read to learn a change is binary before reading all of it, or
    /// `None` when that takes the whole change: the attribute unsets `diff`, and
    /// nothing is read, or the working-tree side's first sniff window holds a NUL.
    fn early_binary(&self, change: &FileChange, diff: Option<bool>) -> Option<Vec<u8>> {
        use std::io::Read;
        match diff {
            Some(false) => return Some(Vec::new()),
            Some(true) => return None,
            None => {}
        }
        // A symlink's bytes are its target, which `read_worktree` handles.
        if change.after != Some(Side::Worktree) || change.maybe_symlink {
            return None;
        }
        let file = std::fs::File::open(self.workdir.join(&change.path)).ok()?;
        let mut window = Vec::with_capacity(hunk::BINARY_SNIFF_LEN);
        file.take(hunk::BINARY_SNIFF_LEN as u64)
            .read_to_end(&mut window)
            .ok()?;
        hunk::is_binary(&window).then_some(window)
    }

    /// What `.gitattributes` says about diffing `change`: as text, never as
    /// text, or nothing. See [`Filter::diff_attribute`].
    fn diff_attribute(&self, change: &FileChange) -> Result<Option<bool>> {
        use crate::filter::Diff;
        let (diff, cleaned) = self.with_filter(|filter| filter.diff_attribute(&change.path))?;
        // A stored pointer against the content it stands for does not diff. Git
        // runs no clean filter on a symlink.
        let pointer = change.before.is_some() && change.after == Some(Side::Worktree);
        if cleaned && pointer && !change.maybe_symlink {
            return Ok(Some(false));
        }
        Ok(match diff {
            None => None,
            Some(Diff::Unset) => Some(false),
            Some(Diff::Set) => Some(true),
            // No textconv runs here, so a PDF's driver would diff its bytes.
            Some(Diff::Driver) if picture::pdf_path(&change.path) => None,
            Some(Diff::Driver) => Some(true),
        })
    }

    /// Run `f` on the filter, building it on first use.
    fn with_filter<T>(&self, f: impl FnOnce(&mut Filter) -> Result<T>) -> Result<T> {
        let mut filter = self.filter.borrow_mut();
        let filter = match filter.as_mut() {
            Some(filter) => filter,
            None => filter.insert(Filter::new(&self.repo())?),
        };
        f(filter)
    }

    /// Both sides of one change's diff, in the bytes git would compare.
    fn sides(&self, change: &FileChange, probes: &mut u64) -> Result<(Vec<u8>, Vec<u8>)> {
        let before = match change.before {
            Some(id) => self.blob(id, &change.path)?,
            None => Vec::new(),
        };
        let after = match change.after {
            Some(Side::Worktree) => self.read_worktree(change, probes)?,
            Some(Side::Blob(id)) => self.blob(id, &change.path)?,
            // A removal, on either side. Nothing is read, which is the same
            // early answer this had before there was a second comparison.
            None => Vec::new(),
        };
        Ok((before, after))
    }

    /// `try_into_blob`, never `into_blob`, and the difference is a panic.
    fn blob(&self, id: gix::ObjectId, path: &str) -> Result<Vec<u8>> {
        let missing = || Error::MissingBlob {
            path: path.to_owned(),
        };
        let repo = self.repo();
        let object = repo.find_object(id).map_err(|_| missing())?;
        Ok(object.try_into_blob().map_err(|_| missing())?.take_data())
    }

    /// What the files under the git dir that shape the clean filter look like now.
    pub(crate) fn filter_prints(&self) -> [Option<Fingerprint>; 2] {
        let [config, attributes] = &self.filter_sources;
        // Followed, as gix follows a linked config when it reads it.
        [followed_print(config), followed_print(attributes)]
    }

    /// Reload the repository's configuration when `now` differs from the config
    /// it last loaded. A reload that fails, as one mid-write does, is tried again
    /// on the next tick.
    pub(crate) fn follow_config(&self, now: Option<Fingerprint>) {
        if !config_moved(now, self.loaded_config.get()) {
            return;
        }
        // A borrow still held is a reader mid-call, and the next tick retries.
        let (Ok(mut repo), Ok(mut status)) =
            (self.live.try_borrow_mut(), self.status.try_borrow_mut())
        else {
            return;
        };
        // `reload` replaces the repository only once the open succeeds, and the
        // walk's copy only once its drivers are gone.
        if repo.reload().is_ok() {
            // A copy that cannot be stripped keeps the last one, which has no
            // drivers either.
            if let Ok(bare) = without_drivers(&repo) {
                *status = bare;
            }
            self.lfs.set(lfs_configured(&repo));
            self.loaded_config.set(now);
            self.reloads.set(self.reloads.get() + 1);
            self.invalidate_filter();
        }
    }

    /// When the last walk's undecided LFS files can be decided, if any are.
    pub(crate) fn lfs_wait(&self) -> Option<std::time::SystemTime> {
        self.lfs_wait.get()
    }

    /// Hold the next walk to `until` at the latest.
    fn lfs_wait_until(&self, until: std::time::SystemTime) {
        let wait = self.lfs_wait.get().map_or(until, |at| at.min(until));
        self.lfs_wait.set(Some(wait));
    }

    /// How many stored blobs this worktree has read to learn whether they are
    /// LFS pointers.
    pub fn lfs_blob_reads(&self) -> u64 {
        self.lfs_blob_reads.get()
    }

    /// How many LFS verdicts this worktree holds.
    pub fn lfs_cached(&self) -> usize {
        self.lfs_verdicts.borrow().len()
    }

    /// How many LFS files this worktree has compared with their local object.
    pub fn lfs_compares(&self) -> u64 {
        self.lfs_compares.get()
    }

    pub(crate) fn reloads(&self) -> u64 {
        self.reloads.get()
    }

    /// Drop the cached clean filter, so the next read rebuilds it.
    pub(crate) fn invalidate_filter(&self) {
        *self.filter.borrow_mut() = None;
    }

    /// Read a working-tree file as git would store it.
    fn read_worktree(&self, change: &FileChange, probes: &mut u64) -> Result<Vec<u8>> {
        let rela_path = change.path.as_str();
        let full = self.workdir.join(rela_path);

        // Counted, and that is what gives this branch a failing test.
        if change.maybe_symlink {
            *probes += 1;
            if let Ok(kind) = std::fs::symlink_metadata(&full).map(|meta| meta.file_type()) {
                if kind.is_symlink() {
                    return Self::link_target(&full, rela_path);
                }
                // A directory, a fifo, a socket or a device node: git tracks none
                // of them, so there are no bytes to compare. It has to stop the
                // read rather than tidy up after one, because opening a fifo with
                // no writer blocks until it has one, on the thread the pane draws
                // from. The error a directory returns could not be matched on
                // anyway, being `IsADirectory` on unix and `PermissionDenied` on
                // Windows, which is also what a file nobody may read returns.
                if !kind.is_file() {
                    return Ok(Vec::new());
                }
            }
        }

        let raw = match std::fs::read(&full) {
            Ok(data) => data,
            // The agent in the other pane can delete a file between the moment
            // status named it and the moment we read it. That is ordinary, not
            // a failure: report it as empty and let the next frame correct us.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(source) => return Err(Error::read(rela_path, source)),
        };

        self.with_filter(|filter| filter.convert_to_git(rela_path, raw))
    }

    /// The bytes git stores for a symlink: its target path, and nothing else.
    fn link_target(full: &Path, rela_path: &str) -> Result<Vec<u8>> {
        let target = match std::fs::read_link(full) {
            Ok(target) => target,
            // A link can go the same way a file can, in the same window.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(source) => return Err(Error::read(rela_path, source)),
        };

        Ok(git_separators(target.into_os_string().into_encoded_bytes()))
    }
}

/// A link target spelled the way git stores it, whatever this platform hands
/// back.
fn git_separators(mut bytes: Vec<u8>) -> Vec<u8> {
    if cfg!(windows) {
        for byte in &mut bytes {
            if *byte == b'\\' {
                *byte = b'/';
            }
        }
    }
    bytes
}

/// What a comparison holds: what a reader would see, and what the pattern took.
///
/// Both, from one walk, because a caller with only the first cannot tell an empty
/// comparison from one the reader hid every path in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counted {
    /// Changes this comparison would draw.
    pub shown: usize,
    /// Changes the pattern kept out of it.
    pub hidden: usize,
}

/// How many hex digits a commit row spells of an id.
///
/// Fixed rather than `gix`'s own `short_id`, which is however many digits
/// disambiguate the commit in *this* object database: the same commit would draw a
/// different width on a clone with more objects in it, and a column whose width
/// moves between machines is worse than one that is occasionally ambiguous.
pub const SHORT_ID: usize = 6;

/// One commit a row of the position list names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Landmark {
    /// The commit itself, which is what a position measures from.
    pub id: gix::ObjectId,
    /// The abbreviation a row draws, [`SHORT_ID`] digits of the id.
    pub named: String,
    /// The commit message's first line.
    pub subject: String,
    /// Seconds since the epoch, spelled as an age by whoever draws it: how long
    /// ago this was depends on when the question is asked, and the engine has no
    /// business answering that twice.
    pub when: i64,
}

/// A page of history, and what the walk spent taking it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Page {
    /// The commits, newest first.
    pub commits: Vec<Landmark>,
    /// Whether there is history behind the last of them.
    pub more: bool,
    /// Commits the walk stepped over, counted at the walk and not in the loop over it.
    ///
    /// It is here so a gate can watch the walk stop. It equals `commits.len()` while the
    /// walk is lazy and becomes the whole history the moment anything drains the
    /// iterator first, which is the one way this starts costing what I4 forbids.
    pub visited: usize,
}

/// Iterator over one comparison's changes, less the ones the reader hid.
pub struct Changes<'h> {
    inner: Inner,
    hide: Option<&'h Hidden>,
    hidden: usize,
}

/// Which status walk to take over the working tree.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Walk {
    /// As the threads finish, so the first change arrives first.
    Streamed,
    /// Buffered and in path order, without rename tracking.
    Sorted,
    /// With rename tracking, which sorts as well.
    Renames,
}

/// Which comparison is being walked.
#[allow(
    clippy::large_enum_variant,
    reason = "the streaming arm is `gix`'s own iterator and is 1.5KB; boxing it \
              would put an allocation and a pointer chase on the walk I4 measures, \
              to shrink a value that exists once per frame"
)]
enum Inner {
    /// The working tree against the index, streamed off `gix`'s own iterator.
    Streamed(gix::status::index_worktree::Iter),
    /// A walk that had to be drained before it could be handed on, which is any
    /// comparison against a tree: `Origin` says which one, and a `since` run is
    /// collected the same way while being unstaged.
    Collected(std::vec::IntoIter<FileChange>),
}

impl<'h> Changes<'h> {
    fn over(inner: Inner, hide: Option<&'h Hidden>) -> Self {
        Self {
            inner,
            hide,
            hidden: 0,
        }
    }

    /// How many changes this walk kept from its caller, once it has been drained.
    ///
    /// Counted here because nothing downstream can recover it: a hidden path is
    /// not a file the caller has and declines to draw, it is one the caller never
    /// receives, and the header owes the reader that number.
    #[must_use]
    pub fn hidden(&self) -> usize {
        self.hidden
    }
}

fn path_of(raw: &gix::bstr::BStr) -> String {
    raw.to_str_lossy().into_owned()
}

/// Whether this item's working-tree side may be a symlink.
fn maybe_symlink(item: &Item, summary: &Summary) -> bool {
    // An intent-to-add entry's mode describes nothing, and trusting it was a live
    // instance of exactly the defect this whole field guards against.
    if matches!(summary, Summary::IntentToAdd) {
        return true;
    }

    // A regular file, executable or not, is the only positive answer taken from
    // an index entry. `SYMLINK` is obviously true.
    let not_a_plain_file =
        |mode: gix::index::entry::Mode| !matches!(mode, gix::index::entry::Mode::FILE);
    let disk_is_not_a_file =
        |kind: Option<gix::dir::entry::Kind>| !matches!(kind, Some(gix::dir::entry::Kind::File));

    match item {
        Item::Modification { entry, .. } => not_a_plain_file(entry.mode),
        Item::DirectoryContents { entry, .. } => disk_is_not_a_file(entry.disk_kind),
        // The *destination* of a rewrite is the working-tree side, and it is the
        // dirwalk entry rather than the index one the source names.
        Item::Rewrite { dirwalk_entry, .. } => disk_is_not_a_file(dirwalk_entry.disk_kind),
    }
}

/// The right-hand side an index-worktree change of this kind has.
pub(crate) fn reads_side(kind: &ChangeKind) -> Option<Side> {
    match kind {
        ChangeKind::Conflict | ChangeKind::TypeChange | ChangeKind::Removed => None,
        _ => Some(Side::Worktree),
    }
}

/// What a path did between the base tree and the working tree, given what it did
/// on each side of the index, or `None` where it did nothing.
///
/// The working tree's own kind decides first where it is one of the two that read
/// no bytes from it, and only then do the endpoints: composing a conflict or a
/// type change into anything diffable leaves a row with no right-hand side.
///
/// Otherwise the endpoints decide it and the middle does not: a file the branch added and
/// the working tree then deleted was never in the base and is not on disk, so
/// nothing happened to it and it is not a row. `git diff <base>` says the same,
/// and a row reading `removed` would claim the reader deleted something the
/// branch point never had.
///
/// A rename survives where the other side did not delete the path. The name it
/// moved from is a fact about the base, so it outranks a plain modification the
/// worktree made on top of it.
fn compose(from_base: &ChangeKind, in_worktree: &ChangeKind) -> Option<ChangeKind> {
    // A conflict and a type change are why a row reads no working-tree bytes
    // ([`reads_side`]), and they describe the end the reader is looking at. Composing
    // one into `Modified` leaves a diffable row with nothing on its right, which
    // draws the whole of the base content as deleted.
    if matches!(in_worktree, ChangeKind::Conflict | ChangeKind::TypeChange) {
        return Some(in_worktree.clone());
    }
    let absent_from_base = matches!(from_base, ChangeKind::Added | ChangeKind::IntentToAdd);
    let gone_from_worktree = matches!(in_worktree, ChangeKind::Removed);
    match (absent_from_base, gone_from_worktree) {
        (true, true) => None,
        (true, false) => Some(ChangeKind::Added),
        (false, true) => Some(ChangeKind::Removed),
        (false, false) => Some(match from_base {
            moved @ (ChangeKind::Renamed { .. } | ChangeKind::Copied { .. }) => moved.clone(),
            _ => ChangeKind::Modified,
        }),
    }
}

/// Whether either side of a tree-index change is a gitlink.
fn touches_gitlink(change: &gix::diff::index::ChangeRef<'_, '_>) -> bool {
    use gix::diff::index::ChangeRef;
    let commit = |mode: &gix::index::entry::Mode| *mode == gix::index::entry::Mode::COMMIT;
    match change {
        ChangeRef::Addition { entry_mode, .. } | ChangeRef::Deletion { entry_mode, .. } => {
            commit(entry_mode)
        }
        ChangeRef::Modification {
            previous_entry_mode,
            entry_mode,
            ..
        } => commit(previous_entry_mode) || commit(entry_mode),
        ChangeRef::Rewrite {
            source_entry_mode,
            entry_mode,
            ..
        } => commit(source_entry_mode) || commit(entry_mode),
    }
}

/// Whether either side of a tree-to-tree change is something this pane can draw.
///
/// A tree diff reports the **directories** whose contents moved as changes in
/// their own right, and `git show --name-status` prints none of them. A gitlink
/// is a commit rather than content, and the index walk drops those already. One
/// predicate answers both, on every side each kind of change has.
fn drawable(change: &gix::diff::tree_with_rewrites::Change) -> bool {
    use gix::diff::tree_with_rewrites::Change;
    let content = gix::object::tree::EntryMode::is_blob_or_symlink;
    match change {
        Change::Addition { entry_mode, .. } | Change::Deletion { entry_mode, .. } => {
            content(entry_mode)
        }
        Change::Modification {
            previous_entry_mode,
            entry_mode,
            ..
        } => content(previous_entry_mode) && content(entry_mode),
        Change::Rewrite {
            source_entry_mode,
            entry_mode,
            ..
        } => content(source_entry_mode) && content(entry_mode),
    }
}

/// One tree-to-tree change, as this crate spells changes.
///
/// [`staged_change`]'s shape over a different walk. `Origin::Unstaged` for the
/// reason `since` gives it: the gutter column names which of the two live
/// comparisons found a change, neither found this one, and a run drawn as the
/// pane's own is what a reader standing at a commit asked for.
fn committed_change(change: &gix::diff::tree_with_rewrites::Change) -> Option<FileChange> {
    use gix::diff::tree_with_rewrites::Change;

    if !drawable(change) {
        return None;
    }

    let (path, kind, before, after) = match change {
        Change::Addition { location, id, .. } => (
            path_of(location.as_ref()),
            ChangeKind::Added,
            None,
            Some(Side::Blob(*id)),
        ),
        Change::Deletion { location, id, .. } => (
            path_of(location.as_ref()),
            ChangeKind::Removed,
            Some(*id),
            None,
        ),
        Change::Modification {
            location,
            previous_id,
            id,
            ..
        } => (
            path_of(location.as_ref()),
            ChangeKind::Modified,
            Some(*previous_id),
            Some(Side::Blob(*id)),
        ),
        // The *destination* names the change, as it does for both other walks: the
        // row a reader sees is the path the content is at now, and `from` is what
        // it says about where it came from.
        Change::Rewrite {
            source_location,
            source_id,
            location,
            id,
            copy,
            ..
        } => {
            let from = path_of(source_location.as_ref());
            let kind = if *copy {
                ChangeKind::Copied { from }
            } else {
                ChangeKind::Renamed { from }
            };
            (
                path_of(location.as_ref()),
                kind,
                Some(*source_id),
                Some(Side::Blob(*id)),
            )
        }
    };

    Some(FileChange {
        path,
        kind,
        origin: Origin::Unstaged,
        before,
        after,
        // Both sides are blobs the object database holds, so nothing here reads a
        // link off disk and the answer cannot reach anything.
        maybe_symlink: false,
        mode_changed: false,
    })
}

/// One tree-index change, as this crate spells changes.
fn staged_change(change: &gix::diff::index::ChangeRef<'_, '_>) -> Option<FileChange> {
    use gix::diff::index::ChangeRef;

    // A gitlink is dropped, on either side.
    if touches_gitlink(change) {
        return None;
    }

    let (path, kind, before, after) = match change {
        ChangeRef::Addition { location, id, .. } => (
            path_of(location.as_ref()),
            ChangeKind::Added,
            None,
            Some(Side::Blob(id.as_ref().to_owned())),
        ),
        ChangeRef::Deletion { location, id, .. } => (
            path_of(location.as_ref()),
            ChangeKind::Removed,
            Some(id.as_ref().to_owned()),
            None,
        ),
        ChangeRef::Modification {
            location,
            previous_id,
            id,
            ..
        } => (
            path_of(location.as_ref()),
            ChangeKind::Modified,
            Some(previous_id.as_ref().to_owned()),
            Some(Side::Blob(id.as_ref().to_owned())),
        ),
        // The *destination* names the change, exactly as it does for an
        // index-worktree rewrite: the row a reader sees is the path the content
        // is at now, and `from` is what it says about where it came from.
        ChangeRef::Rewrite {
            source_location,
            source_id,
            location,
            id,
            copy,
            ..
        } => {
            let from = path_of(source_location.as_ref());
            let kind = if *copy {
                ChangeKind::Copied { from }
            } else {
                ChangeKind::Renamed { from }
            };
            (
                path_of(location.as_ref()),
                kind,
                Some(source_id.as_ref().to_owned()),
                Some(Side::Blob(id.as_ref().to_owned())),
            )
        }
    };

    Some(FileChange {
        path,
        kind,
        origin: Origin::Staged,
        before,
        after,
        // Conservative, and it costs nothing here.
        maybe_symlink: true,
        mode_changed: false,
    })
}

impl Iterator for Changes<'_> {
    type Item = Result<FileChange>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let change = self.inner.next()?;
            // A failure describes the whole comparison rather than one path, so
            // there is no path to hold a pattern against and nothing to hide.
            let Ok(change) = change else {
                return Some(change);
            };
            match self.hide {
                Some(hide) if hide.is_hidden(&change.path) => self.hidden += 1,
                _ => return Some(Ok(change)),
            }
        }
    }
}

impl Iterator for Inner {
    type Item = Result<FileChange>;

    fn next(&mut self) -> Option<Self::Item> {
        let inner = match self {
            Self::Streamed(iter) => iter,
            Self::Collected(iter) => return iter.next().map(Ok),
        };
        loop {
            let item = match inner.next()? {
                Ok(item) => item,
                Err(e) => return Some(Err(Error::Status(Box::new(e)))),
            };

            // `None` means the entry carries no user-visible change: either an
            // index stat refresh, or a dirwalk entry that is tracked and clean.
            let Some(summary) = item.summary() else {
                continue;
            };

            let path = path_of(item.rela_path());

            let (kind, index_blob) = match (&summary, &item) {
                (Summary::Added, _) => (ChangeKind::Added, None),
                (Summary::Removed, Item::Modification { entry, .. }) => {
                    (ChangeKind::Removed, Some(entry.id))
                }
                (Summary::Modified, Item::Modification { entry, .. }) => {
                    (ChangeKind::Modified, Some(entry.id))
                }
                (Summary::TypeChange, Item::Modification { entry, .. }) => {
                    (ChangeKind::TypeChange, Some(entry.id))
                }
                (Summary::IntentToAdd, _) => (ChangeKind::IntentToAdd, None),
                (Summary::Conflict, Item::Modification { entry, .. }) => {
                    (ChangeKind::Conflict, Some(entry.id))
                }
                (Summary::Renamed | Summary::Copied, Item::Rewrite { source, copy, .. }) => {
                    let (from, blob) = match source {
                        RewriteSource::RewriteFromIndex {
                            source_rela_path,
                            source_entry,
                            ..
                        } => (path_of(source_rela_path.as_ref()), Some(source_entry.id)),
                        RewriteSource::CopyFromDirectoryEntry {
                            source_dirwalk_entry,
                            source_dirwalk_entry_id,
                            ..
                        } => (
                            path_of(source_dirwalk_entry.rela_path.as_ref()),
                            Some(*source_dirwalk_entry_id),
                        ),
                    };
                    let kind = if *copy {
                        ChangeKind::Copied { from }
                    } else {
                        ChangeKind::Renamed { from }
                    };
                    (kind, blob)
                }
                // gix pairs each summary with a specific item shape; anything
                // else is a version skew we would rather drop than mislabel.
                _ => continue,
            };

            let after = reads_side(&kind);
            return Some(Ok(FileChange {
                path,
                kind,
                origin: Origin::Unstaged,
                before: index_blob,
                // The working tree, unless there is nothing there to read.
                after,
                maybe_symlink: maybe_symlink(&item, &summary),
                mode_changed: matches!(
                    &item,
                    Item::Modification {
                        status: gix::status::plumbing::index_as_worktree::EntryStatus::Change(
                            gix::status::plumbing::index_as_worktree::Change::Modification {
                                executable_bit_changed: true,
                                ..
                            }
                        ),
                        ..
                    }
                ),
            }));
        }
    }
}

/// Distinct extensions [`indexed_extensions`] will track, at most.
pub const INDEXED_EXTENSIONS: usize = 1024;

/// Bytes of a path [`indexed_extensions`] will retain, at most.
pub const INDEXED_PATH: usize = 4096;

/// Bytes of an extension [`indexed_extensions`] will consider, at most.
pub const INDEXED_EXTENSION: usize = 32;

/// One extension the index carries, with how many entries have it and a bounded
/// sample of their paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Indexed {
    /// Lowercased, because a repository holding both `.MD` and `.md` is one
    /// language rather than two.
    pub extension: String,
    /// Index entries carrying it, counted in full.
    pub files: usize,
    /// Working-tree paths that have it, at most `per_extension` of them.
    pub paths: Vec<String>,
}

/// Every extension the index carries, commonest first.
pub fn indexed_extensions(root: &Path, per_extension: usize) -> Vec<Indexed> {
    if per_extension == 0 {
        return Vec::new();
    }
    let Ok(repo) = gix::discover(root) else {
        return Vec::new();
    };
    let Ok(index) = repo.index_or_empty() else {
        return Vec::new();
    };

    let mut counts: std::collections::HashMap<String, (usize, Vec<String>)> =
        std::collections::HashMap::new();
    for entry in index.entries() {
        let path = entry.path(&index);
        let Ok(path) = path.to_str() else {
            continue;
        };
        let Some(extension) = Path::new(path).extension().and_then(|e| e.to_str()) else {
            continue;
        };
        // Length first, because it is the cheaper of the two rejections and the
        // one that bounds a single entry. No grammar in the dump registers an
        // extension anywhere near this long.
        if extension.len() > INDEXED_EXTENSION {
            continue;
        }
        let extension = extension.to_ascii_lowercase();
        // A known extension is always counted, however full the tally is: the
        // cap bounds how many distinct ones are *tracked*, and dropping later
        // entries of one already being tracked would make its count wrong, which
        // is the one thing the caller cannot recover from.
        if counts.len() >= INDEXED_EXTENSIONS && !counts.contains_key(&extension) {
            continue;
        }
        let slot = counts
            .entry(extension)
            .or_insert_with(|| (0, Vec::with_capacity(per_extension)));
        // Counted whatever its length, because the count is what the merge ranks on and
        // a path too long to open is still a file of that language.
        slot.0 += 1;
        if slot.1.len() < per_extension && path.len() <= INDEXED_PATH {
            slot.1.push(path.to_owned());
        }
    }

    let mut ranked: Vec<Indexed> = counts
        .into_iter()
        .map(|(extension, (files, paths))| Indexed {
            extension,
            files,
            paths,
        })
        .collect();
    ranked.sort_by(|left, right| {
        right
            .files
            .cmp(&left.files)
            .then_with(|| left.extension.cmp(&right.extension))
    });
    ranked
}

#[cfg(test)]
mod tests {
    use super::git_separators;

    /// The separator rule, on every platform rather than on one.
    #[test]
    fn a_link_target_is_spelled_the_way_git_stores_it() {
        let converted = git_separators(br"dir\other.txt".to_vec());
        if cfg!(windows) {
            assert_eq!(
                converted, b"dir/other.txt",
                "a reparse point's separators reached the diff unconverted, so a \
                 nested link reads as changed here and unchanged everywhere else"
            );
        } else {
            assert_eq!(
                converted, br"dir\other.txt",
                "a backslash is a legal character in a Unix filename, and \
                 converting it corrupts a target that is perfectly valid"
            );
        }
    }

    /// Nothing to convert is left exactly alone, on both platforms.
    #[test]
    fn a_target_with_no_separator_to_fix_is_unchanged() {
        assert_eq!(git_separators(b"dir/other.txt".to_vec()), b"dir/other.txt");
        assert_eq!(git_separators(Vec::new()), b"");
    }

    /// A target that is not UTF-8 loses its separator and nothing else.
    #[test]
    fn a_target_that_is_not_utf8_keeps_every_byte_but_the_separator() {
        let raw = vec![0xff, b'd', 0x5C, 0x80, b'x'];
        let converted = git_separators(raw.clone());
        if cfg!(windows) {
            assert_eq!(
                converted,
                vec![0xff, b'd', b'/', 0x80, b'x'],
                "either the separator did not move, or something moved with it"
            );
        } else {
            assert_eq!(converted, raw);
        }
    }
}

#[cfg(test)]
mod reading {
    /// Nothing `only` reaches past the position's own object reads as a working
    /// tree failing.
    ///
    /// `Error::Standing` takes the reader home and `Error::Status` says *working
    /// tree* on the footer, and a commit against a commit has neither a position
    /// that is gone nor a working tree in it. The source rather than a fixture,
    /// because two of these mappings need an object database broken in a way no
    /// test can arrange: a tree whose entries are there and whose diff still will
    /// not walk.
    #[test]
    fn only_maps_every_failure_past_the_commit_to_the_comparison() {
        let source = include_str!("worktree.rs");
        let body = source
            .split("fn only(&self, at: gix::ObjectId")
            .nth(1)
            .expect("`Worktree::only` is gone");
        let body = &body[..body
            .find(
                "
    }
",
            )
            .expect("`only` never closes")];
        let (position, comparison) = body
            .split_once("let parent =")
            .expect("`only` no longer takes the parent after the commit it names");

        // Which kinds each half names, never how many times. A count would fail the
        // day the three identical `map_err` calls in either half became one helper,
        // which changes nothing about where a failure sends the reader.
        assert!(
            position.contains("Error::Standing(Box::new"),
            "nothing before the parent brings a reader home, so a commit that              cannot be read leaves them standing at it:{position}"
        );
        assert!(
            comparison.contains("Error::Comparison(Box::new"),
            "nothing past the commit reads as the comparison, so this gate has              lost the half it is about:{comparison}"
        );
        assert!(
            !comparison.contains("Error::Standing(Box::new"),
            "a failure past the commit brings the reader home, so one object the              object database could not read empties a pane that is fine:{comparison}"
        );
        assert!(
            !comparison.contains("Error::Status(Box::new"),
            "a failure past the commit says the working tree could not be read,              and neither end of this comparison is one:{comparison}"
        );
    }
}

#[cfg(test)]
mod composing {
    use super::{ChangeKind, compose};

    /// The kinds a walk can hand the composition, and what it must make of them.
    ///
    /// A unit test because one arm of this table is reachable from the outside on
    /// one platform only: a type change needs a stored symlink, and the tree walk
    /// does not report an unmerged path at all, so `crates/vigia-core/tests/
    /// standing.rs` can drive every other arm and not these two.
    #[test]
    fn the_working_trees_own_kind_outranks_the_composition() {
        for undiffable in [ChangeKind::Conflict, ChangeKind::TypeChange] {
            for from_base in [
                ChangeKind::Modified,
                ChangeKind::Added,
                ChangeKind::Renamed {
                    from: "old".to_owned(),
                },
            ] {
                assert_eq!(
                    compose(&from_base, &undiffable),
                    Some(undiffable.clone()),
                    "{from_base:?} then {undiffable:?} composed to something diffable, \
                     and a row with no right-hand side to read draws the whole \
                     of the base content as deleted"
                );
            }
        }
    }

    /// The rest of the table, which the integration gates drive as well.
    #[test]
    fn the_endpoints_decide_and_the_middle_does_not() {
        let renamed = ChangeKind::Renamed {
            from: "old".to_owned(),
        };
        for (from_base, in_worktree, want) in [
            (ChangeKind::Added, ChangeKind::Removed, None),
            (ChangeKind::IntentToAdd, ChangeKind::Removed, None),
            (
                ChangeKind::Added,
                ChangeKind::Modified,
                Some(ChangeKind::Added),
            ),
            (
                ChangeKind::Modified,
                ChangeKind::Removed,
                Some(ChangeKind::Removed),
            ),
            (
                ChangeKind::Modified,
                ChangeKind::Modified,
                Some(ChangeKind::Modified),
            ),
            (renamed.clone(), ChangeKind::Modified, Some(renamed.clone())),
            (
                renamed.clone(),
                ChangeKind::Removed,
                Some(ChangeKind::Removed),
            ),
        ] {
            assert_eq!(
                compose(&from_base, &in_worktree),
                want,
                "{from_base:?} then {in_worktree:?}"
            );
        }
    }
}

/// What an LFS verdict holds for: the worktree file's fingerprint and its
/// stored blob.
type LfsKey = (Fingerprint, gix::ObjectId);

/// Bytes one status walk may compare between LFS files and their objects.
const LFS_COMPARE_MAX: u64 = 16 * 1024 * 1024;

/// The most rename scores kept before the cache starts again.
const PAIRS_KEPT: usize = 4096;

/// `repo` with every `filter` section removed from its configuration.
fn without_drivers(repo: &gix::Repository) -> Result<gix::Repository> {
    let mut bare = repo.clone();
    let mut config = bare.config_snapshot_mut();
    let names: Vec<BString> = config
        .sections_by_name("filter")
        .into_iter()
        .flatten()
        .filter_map(|section| section.header().subsection_name().map(ToOwned::to_owned))
        .collect();
    for name in names {
        while config
            .remove_section("filter", Some(name.as_ref()))
            .is_some()
        {}
    }
    config.commit().map_err(Error::filter_setup)?;
    Ok(bare)
}

/// Whether the configuration names Git LFS as a filter.
fn lfs_configured(repo: &gix::Repository) -> bool {
    let config = repo.config_snapshot();
    config.string("filter.lfs.process").is_some() || config.string("filter.lfs.clean").is_some()
}

/// Whether two regular files of `len` bytes each hold the same bytes, read side
/// by side and stopped at the first chunk that differs, or `None` when a read
/// failed and nothing is known.
fn same_bytes(a: &Path, b: &Path, len: u64) -> Option<bool> {
    use std::io::Read;
    // Checked by path first, so a fifo is never opened.
    let regular = |path: &Path| std::fs::metadata(path).is_ok_and(|meta| meta.is_file());
    if !regular(a) || !regular(b) {
        return Some(false);
    }
    let open = |path: &Path| -> Option<Option<std::fs::File>> {
        let file = std::fs::File::open(path).ok()?;
        let meta = file.metadata().ok()?;
        Some((meta.is_file() && meta.len() == len).then_some(file))
    };
    let (Some(mut a), Some(mut b)) = (open(a)?, open(b)?) else {
        return Some(false);
    };
    let (mut left, mut right) = (vec![0u8; 64 * 1024], vec![0u8; 64 * 1024]);
    let mut remaining = len;
    while remaining > 0 {
        let chunk = left
            .len()
            .min(usize::try_from(remaining).unwrap_or(usize::MAX));
        a.read_exact(&mut left[..chunk]).ok()?;
        b.read_exact(&mut right[..chunk]).ok()?;
        if left[..chunk] != right[..chunk] {
            return Some(false);
        }
        remaining -= chunk as u64;
    }
    Some(true)
}
