use std::cell::RefCell;
use std::path::{Path, PathBuf};

use gix::bstr::{BString, ByteSlice};
use gix::status::index_worktree::{Item, RewriteSource, iter::Summary};

use crate::change::{ChangeKind, FileChange, Origin, Side};
use crate::error::{Error, Result};
use crate::filter::Filter;
use crate::frame::Frame;
use crate::hunk::{self, FileDiff};
use crate::watch::{WatchOptions, Watcher};

/// Knobs that change what a change sweep costs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChangeOptions {
    /// Pair deletions with additions so a moved file reads as one change.
    pub track_renames: bool,
}

impl Default for ChangeOptions {
    fn default() -> Self {
        Self {
            track_renames: true,
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
    pub fn changes(&self) -> Result<Changes> {
        self.changes_with(ChangeOptions::default())
    }

    /// Stream the working-tree-vs-index changes.
    pub fn changes_with(&self, options: ChangeOptions) -> Result<Changes> {
        self.changes_of(Origin::Unstaged, options)
    }

    /// Stream one comparison's changes.
    pub fn changes_of(&self, origin: Origin, options: ChangeOptions) -> Result<Changes> {
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
                Ok(Changes::Unstaged(iter))
            }
            Origin::Staged => Ok(Changes::Staged(self.staged(options)?.into_iter())),
        }
    }

    /// How many changes one comparison holds, without keeping any of them.
    pub fn count_of(&self, origin: Origin) -> Result<usize> {
        // Rename tracking on, and the cheaper spelling is wrong here.
        self.changes_of(origin, ChangeOptions::default())?
            .try_fold(0, |n, change| change.map(|_| n + 1))
    }

    /// The index against `HEAD^{tree}`, collected.
    fn staged(&self, options: ChangeOptions) -> Result<Vec<FileChange>> {
        let tree = match self.repo.head_tree_id() {
            Ok(id) => id.detach(),
            // Unborn, detached at nothing, or an unreadable `HEAD`.
            Err(_) => self.repo.empty_tree().id().detach(),
        };
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

    /// Start watching this working tree for change.
    pub fn watch(&self, options: WatchOptions) -> Result<Watcher<'_>> {
        Watcher::new(&self.repo, &self.workdir, options)
    }

    /// Start a frame over this working tree.
    pub fn frame(&self) -> Frame<'_> {
        Frame::new(self)
    }

    /// Compute the line-level diff for one change.
    pub fn diff(&self, change: &FileChange) -> Result<FileDiff> {
        self.diff_counted(change, &mut 0)
    }

    /// [`Worktree::diff`], reporting the type probes it spent.
    pub(crate) fn diff_counted(&self, change: &FileChange, probes: &mut u64) -> Result<FileDiff> {
        if !change.is_diffable() {
            return Ok(FileDiff {
                path: change.path.clone(),
                binary: false,
                hunks: Vec::new(),
                added: 0,
                removed: 0,
                // A conflict and a type change are states rather than diffs, and this
                // method deliberately reads nothing for them.
                lines: 0,
                first_line: None,
                bytes: 0,
            });
        }

        let (before, after) = self.sides(change, probes)?;
        Ok(hunk::compute(change.path.clone(), &before, &after))
    }

    /// How tall one change's diff is, without building any of it.
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
            if std::fs::symlink_metadata(&full).is_ok_and(|meta| meta.file_type().is_symlink()) {
                return Self::link_target(&full, rela_path);
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

/// Iterator over one comparison's changes.
#[allow(
    clippy::large_enum_variant,
    reason = "the streaming arm is `gix`'s own iterator and is 1.5KB; boxing it \
              would put an allocation and a pointer chase on the walk I4 measures, \
              to shrink a value that exists once per frame"
)]
pub enum Changes {
    /// The working tree against the index, streamed off `gix`'s own iterator.
    Unstaged(gix::status::index_worktree::Iter),
    /// The index against `HEAD^{tree}`, already collected.
    Staged(std::vec::IntoIter<FileChange>),
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

impl Iterator for Changes {
    type Item = Result<FileChange>;

    fn next(&mut self) -> Option<Self::Item> {
        let inner = match self {
            Self::Unstaged(iter) => iter,
            Self::Staged(iter) => return iter.next().map(Ok),
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
