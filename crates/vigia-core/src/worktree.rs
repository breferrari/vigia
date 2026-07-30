use std::path::{Path, PathBuf};

use gix::bstr::{BString, ByteSlice};
use gix::status::index_worktree::{Item, RewriteSource, iter::Summary};

use crate::change::{ChangeKind, FileChange};
use crate::error::{Error, Result};
use crate::hunk::{self, FileDiff};
use crate::watch::{WatchOptions, Watcher};

/// Knobs that change what a change sweep costs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChangeOptions {
    /// Pair deletions with additions so a moved file reads as one change.
    ///
    /// Costs streaming. Rename detection cannot emit its first result until
    /// the walk has finished, because any later addition might still pair with
    /// an earlier deletion. With this on, time-to-first-change equals
    /// time-to-last-change, which is the shape I4 exists to forbid.
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
///
/// Holds an open `gix::Repository` for the process lifetime. Reopening per
/// frame would re-read config and re-mmap the object database, which is
/// exactly the per-tick cost the engine exists to avoid.
pub struct Worktree {
    repo: gix::Repository,
    workdir: PathBuf,
}

impl Worktree {
    /// Find the repository at or above `path` and open its working tree.
    pub fn discover(path: impl AsRef<Path>) -> Result<Self> {
        let repo = gix::discover(path)?;
        let workdir = repo.workdir().ok_or(Error::Bare)?.to_path_buf();
        Ok(Self { repo, workdir })
    }

    /// Absolute path of the working tree root.
    pub fn workdir(&self) -> &Path {
        &self.workdir
    }

    /// Stream the working-tree-vs-index changes with default options.
    ///
    /// An iterator rather than a `Vec` on purpose: I4 makes first paint a
    /// budget, so the caller must be able to render file one before file one
    /// thousand has been looked at. Returning a collection here would make
    /// that impossible to honour further up.
    pub fn changes(&self) -> Result<Changes> {
        self.changes_with(ChangeOptions::default())
    }

    /// Stream the working-tree-vs-index changes.
    pub fn changes_with(&self, options: ChangeOptions) -> Result<Changes> {
        let iter = self
            .repo
            .status(gix::progress::Discard)
            .map_err(|e| Error::Status(Box::new(e)))?
            // Collapsed would report a changed directory as one entry. A
            // monitor has to name the file that changed.
            .untracked_files(gix::status::UntrackedFiles::Files)
            .index_worktree_rewrites(options.track_renames.then(gix::diff::Rewrites::default))
            .into_index_worktree_iter(Vec::<BString>::new())
            .map_err(|e| Error::Status(Box::new(e)))?;
        Ok(Changes { inner: iter })
    }

    /// Start watching this working tree for change.
    ///
    /// The watcher borrows the repository, because the gitignore rules it
    /// filters against are resolved through it.
    pub fn watch(&self, options: WatchOptions) -> Result<Watcher<'_>> {
        Watcher::new(&self.repo, &self.workdir, options)
    }

    /// Compute the line-level diff for one change.
    pub fn diff(&self, change: &FileChange) -> Result<FileDiff> {
        if !change.is_diffable() {
            return Ok(FileDiff {
                path: change.path.clone(),
                binary: false,
                hunks: Vec::new(),
                added: 0,
                removed: 0,
                bytes: 0,
            });
        }

        let before = match change.index_blob {
            Some(id) => self.blob(id, &change.path)?,
            None => Vec::new(),
        };
        let after = match change.kind {
            ChangeKind::Removed => Vec::new(),
            _ => self.read_worktree(&change.path)?,
        };

        Ok(hunk::compute(change.path.clone(), &before, &after))
    }

    fn blob(&self, id: gix::ObjectId, path: &str) -> Result<Vec<u8>> {
        let object = self.repo.find_object(id).map_err(|_| Error::MissingBlob {
            path: path.to_owned(),
        })?;
        Ok(object.into_blob().take_data())
    }

    fn read_worktree(&self, rela_path: &str) -> Result<Vec<u8>> {
        match std::fs::read(self.workdir.join(rela_path)) {
            Ok(data) => Ok(data),
            // The agent in the other pane can delete a file between the moment
            // status named it and the moment we read it. That is ordinary, not
            // a failure: report it as empty and let the next frame correct us.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(source) => Err(Error::Read {
                path: rela_path.to_owned(),
                source,
            }),
        }
    }
}

/// Iterator over working-tree-vs-index changes.
///
/// Yields one `Result` per path, so a single unreadable file does not end the
/// stream. A monitor keeps going.
pub struct Changes {
    inner: gix::status::index_worktree::Iter,
}

fn path_of(raw: &gix::bstr::BStr) -> String {
    raw.to_str_lossy().into_owned()
}

impl Iterator for Changes {
    type Item = Result<FileChange>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let item = match self.inner.next()? {
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

            return Some(Ok(FileChange {
                path,
                kind,
                index_blob,
            }));
        }
    }
}
