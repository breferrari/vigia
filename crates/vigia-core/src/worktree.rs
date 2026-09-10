use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use gix::bstr::{BString, ByteSlice};
use gix::status::index_worktree::{Item, RewriteSource, iter::Summary};

use crate::change::{ChangeKind, FileChange, Origin, Side};
use crate::error::{Error, Result};
use crate::filter::Filter;
use crate::frame::Frame;
use crate::hidden::Hidden;
use crate::hunk::{self, FileDiff};
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
    repo: gix::Repository,
    workdir: PathBuf,
    /// The clean filter: built on the first working-tree read after each
    /// [`Frame::advance`], and not before.
    filter: RefCell<Option<Filter>>,
}

impl Worktree {
    /// Find the repository at or above `path` and open its working tree.
    ///
    /// # Errors
    ///
    /// `path` is not inside a git repository, or the repository it finds is bare and has no
    /// worktree to compare against.
    pub fn discover(path: impl AsRef<Path>) -> Result<Self> {
        let repo = gix::discover(path)?;
        let workdir = repo.workdir().ok_or(Error::Bare)?.to_path_buf();
        Ok(Self {
            repo,
            workdir,
            filter: RefCell::new(None),
        })
    }

    /// Absolute path of the working tree root.
    pub fn workdir(&self) -> &Path {
        &self.workdir
    }

    /// The branch HEAD names, shortened, or `None` when HEAD is detached.
    pub fn branch(&self) -> Option<String> {
        let name = self.repo.head_name().ok()??;
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
        match origin {
            Origin::Unstaged => {
                let iter = self
                    .repo
                    .status(gix::progress::Discard)
                    .map_err(|e| Error::Status(Box::new(e)))?
                    // Collapsed would report a changed directory as one entry. A
                    // monitor has to name the file that changed.
                    .untracked_files(gix::status::UntrackedFiles::Files)
                    .index_worktree_rewrites(
                        options.track_renames.then(gix::diff::Rewrites::default),
                    )
                    .into_index_worktree_iter(Vec::<BString>::new())
                    .map_err(|e| Error::Status(Box::new(e)))?;
                Ok(Changes::over(Inner::Streamed(iter), options.hide))
            }
            Origin::Staged => Ok(Changes::over(
                Inner::Collected(self.staged(options)?.into_iter()),
                options.hide,
            )),
        }
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
        let tree = match self.repo.head_tree_id() {
            Ok(id) => id.detach(),
            // Unborn, detached at nothing, or an unreadable `HEAD`.
            Err(_) => self.repo.empty_tree().id().detach(),
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
        let index = self
            .repo
            .index_or_empty()
            .map_err(|e| Error::Status(Box::new(e)))?;

        let renames = if options.track_renames {
            gix::status::tree_index::TrackRenames::Given(gix::diff::Rewrites::default())
        } else {
            gix::status::tree_index::TrackRenames::Disabled
        };

        let mut changes = Vec::new();
        let walked = self
            .repo
            .tree_index_status(&tree, &index, None, renames, |change, _, _| {
                if let Some(change) = staged_change(&change) {
                    changes.push(change);
                }
                Ok::<_, std::convert::Infallible>(gix::diff::index::Action::Continue(()))
            });

        // A sparse index yields no staged run rather than a dead pane.
        if let Err(e) = walked {
            if matches!(
                e,
                gix::status::tree_index::Error::TreeIndexDiff(gix::diff::index::Error::IsSparse)
            ) {
                return Ok(Vec::new());
            }
            return Err(Error::Status(Box::new(e)));
        }
        Ok(changes)
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
        let head = self
            .repo
            .head_id()
            .map_err(|e| Error::Standing(Box::new(e)))?
            .detach();
        for (reference, named) in self.candidates() {
            let Ok(other) = self.repo.find_reference(reference.as_str()) else {
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
            if let Ok(base) = self.repo.merge_base(head, other) {
                return Ok((base.detach(), named));
            }
        }
        Err(Error::NoBranchPoint)
    }

    /// The references `branch_point` tries, in order, with the name each would
    /// put in the header.
    fn candidates(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        if let Ok(Some(head)) = self.repo.head_ref()
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
        match standing.at() {
            None => self.changes_with(options),
            Some(base) => Ok(Changes::over(
                Inner::Collected(self.since(base, options)?.into_iter()),
                options.hide,
            )),
        }
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
            .repo
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
        Watcher::new(&self.repo, &self.workdir, options)
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
        self.diff_counted(change, &mut 0)
    }

    /// [`Worktree::diff`], reporting the type probes it spent.
    pub(crate) fn diff_counted(&self, change: &FileChange, probes: &mut u64) -> Result<FileDiff> {
        if !change.is_diffable() {
            // A conflict and a type change are states rather than diffs, and this
            // method deliberately reads nothing for them.
            return Ok(FileDiff::without_hunks(change.path.clone(), None));
        }

        let (before, after) = self.sides(change, probes)?;
        Ok(hunk::compute(change.path.clone(), &before, &after))
    }

    /// How tall one change's diff is, without building any of it.
    ///
    /// # Errors
    ///
    /// Either side cannot be read, as [`Worktree::diff`].
    pub fn measure(&self, change: &FileChange) -> Result<hunk::FileSpan> {
        self.measure_counted(change, &mut 0)
    }

    /// [`Worktree::measure`], reporting the type probes it spent.
    pub(crate) fn measure_counted(
        &self,
        change: &FileChange,
        probes: &mut u64,
    ) -> Result<hunk::FileSpan> {
        if !change.is_diffable() {
            return Ok(hunk::FileSpan::default());
        }

        let (before, after) = self.sides(change, probes)?;
        Ok(hunk::measure(&before, &after))
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
        let object = self.repo.find_object(id).map_err(|_| missing())?;
        Ok(object.try_into_blob().map_err(|_| missing())?.take_data())
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

        let mut filter = self.filter.borrow_mut();
        let filter = match filter.as_mut() {
            Some(filter) => filter,
            None => filter.insert(Filter::new(&self.repo)?),
        };
        filter.convert_to_git(rela_path, raw)
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

/// Iterator over one comparison's changes, less the ones the reader hid.
pub struct Changes<'h> {
    inner: Inner,
    hide: Option<&'h Hidden>,
    hidden: usize,
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
