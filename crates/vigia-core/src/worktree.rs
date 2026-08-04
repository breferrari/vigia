use std::cell::RefCell;
use std::path::{Path, PathBuf};

use gix::bstr::{BString, ByteSlice};
use gix::status::index_worktree::{Item, RewriteSource, iter::Summary};

use crate::change::{ChangeKind, FileChange};
use crate::error::{Error, Result};
use crate::filter::Filter;
use crate::frame::Frame;
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
    /// The clean filter: built on the first working-tree read after each
    /// [`Frame::advance`], and not before.
    ///
    /// Lazy because a monitor pointed at a clean tree draws the empty state and
    /// diffs nothing, and assembling this costs an index load and an
    /// attribute-globals read. Paying for those at [`Worktree::discover`] would
    /// put them on the path I7 measures, to build something that session might
    /// never consult.
    ///
    /// Dropped once per frame rather than held for the process, because the
    /// rules it caches live in files the agent in the other pane can rewrite.
    /// See [`Worktree::invalidate_filter`] for why that is a correctness rule
    /// and not a refresh policy.
    ///
    /// `RefCell` costs nothing that was not already given up: `gix::Repository`
    /// is `Send` and not `Sync`, so each thread already opens its own
    /// `Worktree` rather than sharing one.
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
    ///
    /// This is orientation for the empty state and nothing else: `SPEC.md` §11.1
    /// rules B3, and it is explicit that the branch does **not** describe the
    /// comparison. The diff here is the working tree against the index, so HEAD
    /// does not enter into it, and a reader who took the branch for the left-hand
    /// side of the diff would be reading it wrong.
    ///
    /// Shortened, because `refs/heads/` is a prefix every branch carries and
    /// therefore says nothing. A slash inside the name survives.
    ///
    /// `None` is ordinary rather than a failure. A detached HEAD is where a
    /// rebase or a bisect leaves a tree, and the empty state drops the branch
    /// instead of inventing one. An unreadable HEAD reaches the same answer for
    /// the same reason a frame failure reaches the footer rather than the exit
    /// code: a monitor that refuses to draw because it could not name a branch
    /// has stopped doing its job over a decoration.
    ///
    /// An unborn branch still names itself, which is not a quirk to work around:
    /// a repository with no commits is what an agent's first minute looks like,
    /// and `main` is the honest answer there rather than nothing.
    ///
    /// Costs one `.git/HEAD` read, so the caller decides when to pay it. The
    /// shell asks only on a frame that draws the empty state, which is a frame
    /// with no diff to compute and nothing else to read, and that is what keeps
    /// I4 true: the thing read is the thing drawn.
    pub fn branch(&self) -> Option<String> {
        let name = self.repo.head_name().ok()??;
        Some(name.shorten().to_string())
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

    /// Start a frame over this working tree.
    ///
    /// The frame is what holds diffs between redraws, so it is what I2a is
    /// about. It starts empty; [`Frame::advance`] gives it its first contents.
    pub fn frame(&self) -> Frame<'_> {
        Frame::new(self)
    }

    /// Compute the line-level diff for one change.
    ///
    /// Reads both sides every time. A monitor calls this through a
    /// [`Frame`], which is what stops a redraw paying for files that did not
    /// change (I2a).
    pub fn diff(&self, change: &FileChange) -> Result<FileDiff> {
        if !change.is_diffable() {
            return Ok(FileDiff {
                path: change.path.clone(),
                binary: false,
                hunks: Vec::new(),
                added: 0,
                removed: 0,
                // A conflict and a type change are states rather than diffs, and
                // this method deliberately reads nothing for them. Reporting a
                // length would mean opening the file to find one, which is the
                // read the early return exists to avoid.
                lines: 0,
                bytes: 0,
            });
        }

        let before = match change.index_blob {
            Some(id) => self.blob(id, &change.path)?,
            None => Vec::new(),
        };
        let after = if change.reads_worktree() {
            self.read_worktree(change)?
        } else {
            Vec::new()
        };

        Ok(hunk::compute(change.path.clone(), &before, &after))
    }

    /// How tall one change's diff is, without building any of it.
    ///
    /// Reads both sides exactly as [`Worktree::diff`] does, and then counts
    /// instead of materialising. That is the whole saving: the reads are the same
    /// bytes, and what it skips is a `String` per drawn line.
    pub fn measure(&self, change: &FileChange) -> Result<hunk::FileSpan> {
        if !change.is_diffable() {
            return Ok(hunk::FileSpan::default());
        }

        let before = match change.index_blob {
            Some(id) => self.blob(id, &change.path)?,
            None => Vec::new(),
        };
        let after = if change.reads_worktree() {
            self.read_worktree(change)?
        } else {
            Vec::new()
        };

        Ok(hunk::measure(&before, &after))
    }

    fn blob(&self, id: gix::ObjectId, path: &str) -> Result<Vec<u8>> {
        let object = self.repo.find_object(id).map_err(|_| Error::MissingBlob {
            path: path.to_owned(),
        })?;
        Ok(object.into_blob().take_data())
    }

    /// Drop the cached clean filter, so the next read rebuilds it.
    ///
    /// Called once per [`Frame::advance`], which is what bounds how stale the
    /// filter can be to a single frame. It has to be bounded by something: the
    /// rules live in `.gitattributes` and `core.autocrlf`, and **the agent in
    /// the other pane can write a `.gitattributes` at any moment.** Built once
    /// per process, the pane then goes on drawing the old answer indefinitely
    /// while a restart would draw a different one, which is I5 (correct with
    /// zero interaction) failing silently in a process I3 expects to run for
    /// days.
    ///
    /// Dropping rather than rebuilding keeps the laziness that made this cheap.
    /// A frame whose diffs are all reused reads no file, so it rebuilds nothing;
    /// only a frame that actually recomputes a diff pays, and that frame was
    /// already reading from disk. `gix` rebuilds its own attributes stack on
    /// every status walk for the same reason, so this matches the freshness of
    /// the walk it is paired with rather than inventing a policy.
    pub(crate) fn invalidate_filter(&self) {
        *self.filter.borrow_mut() = None;
    }

    /// Read a working-tree file as git would store it.
    ///
    /// The normalisation is not a nicety. Git diffs the working-tree side
    /// *through* its clean filter, so on a checkout with `core.autocrlf=true`
    /// the bytes on disk are CRLF while the blob they are compared against is
    /// LF. Skipping the filter makes every line of every such file differ from
    /// its stored form: see `filter.rs` and
    /// [#65](https://github.com/breferrari/vigia/issues/65).
    ///
    /// **A symlink is stored by content too, and its content is the target
    /// path.** Git keeps one as a mode `120000` blob holding the target verbatim,
    /// so `fs::read` is the wrong primitive for it twice over: it follows the
    /// link, and it therefore compares the *target file's* bytes against a blob
    /// holding a path. Reading the link itself is the same rule this function
    /// already applies to a text file, not an exception to it: compare the bytes
    /// git would store. See [`Worktree::link_target`] and
    /// [#15](https://github.com/breferrari/vigia/issues/15).
    ///
    /// **Deciding between the two costs no syscall on the ordinary path**, which
    /// is the whole reason [`FileChange::maybe_symlink`] exists: the status walk
    /// has already seen the entry's mode, and asking the filesystem again put a
    /// second `stat` per file on the path
    /// `crates/vigia/tests/reads.rs::a_tick_inside_the_settle_margin_stats_each_file_once`
    /// holds at one, for a measured **+1.18ms p50** over a hundred undrawn files
    /// inside the settle margin. Only a change the walk could not resolve as a
    /// plain file reaches the `lstat` below.
    fn read_worktree(&self, change: &FileChange) -> Result<Vec<u8>> {
        let rela_path = change.path.as_str();
        let full = self.workdir.join(rela_path);

        // The agent in the other pane can delete a file between the moment
        // status named it and the moment we read it. That is ordinary, not a
        // failure: report it as empty and let the next frame correct us.
        //
        // Returned before the filter rather than through it, because there is
        // nothing to normalise and priming the attributes stack for a file that
        // no longer exists would be a read for no reader.
        if change.maybe_symlink {
            match std::fs::symlink_metadata(&full) {
                Ok(meta) if meta.file_type().is_symlink() => {
                    return Self::link_target(&full, rela_path);
                }
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
                Err(source) => {
                    return Err(Error::Read {
                        path: rela_path.to_owned(),
                        source,
                    });
                }
            }
        }

        let raw = match std::fs::read(&full) {
            Ok(data) => data,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(source) => {
                return Err(Error::Read {
                    path: rela_path.to_owned(),
                    source,
                });
            }
        };

        let mut filter = self.filter.borrow_mut();
        let filter = match filter.as_mut() {
            Some(filter) => filter,
            None => filter.insert(Filter::new(&self.repo)?),
        };
        filter.convert_to_git(rela_path, raw)
    }

    /// The bytes git stores for a symlink: its target path, and nothing else.
    ///
    /// **No terminator and no clean filter**, which is what the blob on the
    /// other side of the diff holds. Git appends no newline to a link target and
    /// runs no conversion over one, so a `\ No newline at end of file` on both
    /// sides is the correct answer rather than a defect: `git diff` prints
    /// exactly that for a repointed link. Running the filter here would be the
    /// mistake [#65](https://github.com/breferrari/vigia/issues/65) made in
    /// reverse, normalising something git never normalised.
    ///
    /// **The filter half of that is correct by construction and no gate can hold
    /// it, which is said here so nobody concludes it is merely untested.**
    /// Mutation-tested: wrapping this in `convert_to_git` leaves all 40 tests in
    /// `fidelity.rs` and `frame.rs` green, and it has to. That filter is a
    /// line-ending conversion, and a link target contains no line ending, so both
    /// paths emit identical bytes and no fixture can separate them. What the
    /// bypass actually buys is the *cost*: priming the attributes stack for a
    /// path whose answer could not depend on it.
    ///
    /// Reads through `OsString::into_encoded_bytes` rather than through `str`,
    /// because a target is a path and a path is not required to be UTF-8. That
    /// is the one half of [#17](https://github.com/breferrari/vigia/issues/17)
    /// this touches: the *target* is byte-exact here, while `FileChange::path`
    /// remains lossy and remains #17's.
    fn link_target(full: &Path, rela_path: &str) -> Result<Vec<u8>> {
        let target = match std::fs::read_link(full) {
            Ok(target) => target,
            // A link can go the same way a file can, in the same window.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(source) => {
                return Err(Error::Read {
                    path: rela_path.to_owned(),
                    source,
                });
            }
        };

        let bytes = target.into_os_string().into_encoded_bytes();

        // A reparse point stores `dir\target.txt` where git stores
        // `dir/target.txt`, so without this a nested link reads as changed on
        // Windows and unchanged everywhere else. Git's own `readlink` does the
        // same substitution on this platform and only on this platform.
        //
        // Byte-level is safe: `\` is 0x5C, which never appears inside a
        // multi-byte UTF-8 or WTF-8 sequence. `cfg`'d rather than unconditional
        // because a backslash is a legal character in a Unix filename, so
        // converting there would corrupt a legitimate target.
        #[cfg(windows)]
        let bytes = bytes
            .into_iter()
            .map(|byte| if byte == b'\\' { b'/' } else { byte })
            .collect();

        Ok(bytes)
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

/// Whether this item's working-tree side may be a symlink.
///
/// See [`FileChange::maybe_symlink`] for why this is read off the walk rather
/// than asked of the filesystem, and for the one case it does not cover.
///
/// **Every arm defaults to `true`**, so a `gix` version that grows an item shape
/// or a disk kind this does not know about pays a syscall rather than reading a
/// link as a file. The cost of being wrong is not symmetric, and this is where
/// that asymmetry is spent.
fn maybe_symlink(item: &Item) -> bool {
    // A regular file, executable or not, is the only positive answer taken from
    // an index entry. `SYMLINK` is obviously true; `DIR` and `COMMIT` are
    // submodules and are not read through this path at all, so `true` costs
    // nothing real and keeps the rule one line.
    let not_a_plain_file = |mode: gix::index::entry::Mode| {
        !matches!(
            mode,
            gix::index::entry::Mode::FILE | gix::index::entry::Mode::FILE_EXECUTABLE
        )
    };
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
                maybe_symlink: maybe_symlink(&item),
            }));
        }
    }
}
