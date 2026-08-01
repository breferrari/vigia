//! Normalising the working-tree side the way git's clean filter would.
//!
//! A diff compares the working tree against the index blob, and those two sides
//! are not stored the same way. Git runs the working-tree side through its
//! **clean** filter before comparing: on Windows, where `core.autocrlf=true` is
//! what the installer writes, that filter turns CRLF back into the LF the blob
//! holds. Comparing the raw bytes instead makes every line of every CRLF file
//! differ from its stored form, so a one-line edit draws as a whole-file
//! rewrite and a file that changed nothing at all draws as one too.
//!
//! That was [#65](https://github.com/breferrari/vigia/issues/65), and it is the
//! worst failure shape this tool has: a monitor whose entire job is to show what
//! changed, burying a real 21-line edit inside a wall of nine hundred red and
//! green rows, on the default configuration of a tier-1 target.
//!
//! `gix-filter` is already in the dependency graph — `status` implies
//! `blob-diff`, which implies `attributes`, which brings it — so this is a call
//! that was not being made rather than a dependency that was missing.
//!
//! # External filter drivers are deliberately not run
//!
//! `gitattributes` can name an external program to clean a file, and Git LFS is
//! the common one: a repository using it sets `filter.lfs.clean` to a command.
//! Running those would mean **spawning a process per file per frame**, which is
//! precisely the shape `SPEC.md` §6 rules out when it chooses an in-process diff
//! over a `git diff` subprocess per tick. So [`Filter::new`] empties the driver
//! list, and what remains — `eol`, `working-tree-encoding` and `ident` — is pure
//! Rust and costs no process.
//!
//! The consequence is recorded rather than hidden: under Git LFS the worktree
//! holds real content while the blob holds a pointer, so such a file diffs as a
//! rewrite. That is what happens today as well, so it is not a regression, and
//! it is [#66](https://github.com/breferrari/vigia/issues/66).

use std::path::Path;

use crate::error::{Error, Result};

/// A to-git conversion pipeline that owns everything it needs.
///
/// Owned rather than borrowed on purpose. `gix::filter::Pipeline` carries a
/// `&Repository`, and [`Worktree`](crate::Worktree) owns the repository it would
/// borrow, so holding one there would make that struct self-referential. The
/// plumbing types underneath take no such borrow, and assembling them here costs
/// about twenty lines and removes the lifetime entirely.
pub(crate) struct Filter {
    pipeline: gix::filter::plumbing::Pipeline,
    /// The attributes stack, which resolves `.gitattributes` per directory.
    ///
    /// Stateful: it caches the directory it last looked at, so walking paths in
    /// the order status reports them costs about one `stat` per *directory*
    /// rather than one per file.
    stack: gix::worktree::Stack,
    /// Needed by the `text=auto` rule, which asks whether the path is already
    /// known to the index before deciding to convert it.
    index: gix::worktree::Index,
    objects: gix::OdbHandle,
}

impl Filter {
    /// Assemble a pipeline from the repository's configuration and attributes.
    ///
    /// Costs one index load and one attribute-globals read, which is why
    /// [`Worktree`](crate::Worktree) builds this lazily: a monitor pointed at a
    /// clean tree draws the empty state, diffs nothing, and should not pay for
    /// an attributes stack it will never consult. That keeps the cost off the
    /// path I7 measures and on the first frame that actually reads a file.
    pub(crate) fn new(repo: &gix::Repository) -> Result<Self> {
        let index = repo.index_or_empty().map_err(Error::filter_setup)?;
        let stack = repo
            .attributes_only(
                &index,
                gix::worktree::stack::state::attributes::Source::WorktreeThenIdMapping,
            )
            .map_err(Error::filter_setup)?
            .detach();

        let mut options = gix::filter::Pipeline::options(repo).map_err(Error::filter_setup)?;
        // See the module header. This is a ruling, not an oversight.
        options.drivers = Vec::new();

        let pipeline = gix::filter::plumbing::Pipeline::new(
            repo.command_context().map_err(Error::filter_setup)?,
            options,
        );

        Ok(Filter {
            pipeline,
            stack,
            index,
            objects: repo.objects.clone(),
        })
    }

    /// `content`, as git would store it for `rela_path`.
    ///
    /// Returns the input untouched when no filter applies, which is every file
    /// on a platform that does not convert line endings and every file marked
    /// `-text`. The common case therefore costs one attributes lookup and no
    /// copy at all.
    ///
    /// Never falls back to the raw bytes on failure. A silent fallback would
    /// reproduce exactly the defect this module exists to fix, and reproduce it
    /// invisibly: the pane would go back to drawing whole-file rewrites with
    /// nothing anywhere saying why. A failure here reaches the footer as a
    /// notice, the way every other frame failure does (`SPEC.md` §11.1).
    pub(crate) fn to_git(&mut self, rela_path: &str, content: Vec<u8>) -> Result<Vec<u8>> {
        let path = Path::new(rela_path);

        // Destructured so the three fields borrow separately: the stack is
        // mutated by the lookup, the pipeline by the conversion, and the closure
        // below reads the other two.
        let Filter {
            pipeline,
            stack,
            index,
            objects,
        } = self;

        let entry = stack
            .at_path(path, None, &*objects)
            .map_err(|source| Error::filter(rela_path, source))?;

        let outcome = pipeline
            .convert_to_git(
                content.as_slice(),
                path,
                &mut |_, attrs| {
                    entry.matching_attributes(attrs);
                },
                &mut |buf| {
                    // `text=auto` converts a file that git already tracks and
                    // leaves an untracked one alone, so the rule needs to know
                    // whether the index names this path.
                    let unix = gix::path::to_unix_separators_on_windows(gix::path::into_bstr(path));
                    let Some(entry) = index.entry_by_path(unix.as_ref()) else {
                        return Ok(None);
                    };
                    use gix::prelude::Find;
                    let object = objects.try_find(&entry.id, buf)?;
                    Ok(object
                        .filter(|object| object.kind == gix::object::Kind::Blob)
                        .map(|_| ()))
                },
            )
            .map_err(|source| Error::filter(rela_path, source))?;

        match outcome {
            // Nothing applied, so the bytes already read are the answer.
            gix::filter::plumbing::pipeline::convert::ToGitOutcome::Unchanged(_) => Ok(content),
            other => {
                use std::io::Read;
                let mut converted = Vec::with_capacity(content.len());
                let mut reader = other;
                reader
                    .read_to_end(&mut converted)
                    .map_err(|source| Error::filter(rela_path, source))?;
                Ok(converted)
            }
        }
    }
}
