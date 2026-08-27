//! Normalising the working-tree side the way git's clean filter would.

use std::path::Path;

use gix::filter::plumbing::pipeline::convert::ToGitOutcome;

use crate::error::{Error, Result};

/// A to-git conversion pipeline that owns everything it needs.
pub(crate) struct Filter {
    pipeline: gix::filter::plumbing::Pipeline,
    /// The attributes stack, which resolves `.gitattributes` per directory.
    stack: gix::worktree::Stack,
    /// Needed by the `text=auto` rule, which asks whether the path is already
    /// known to the index before deciding to convert it.
    index: gix::worktree::Index,
    objects: gix::OdbHandle,
}

impl Filter {
    /// Assemble a pipeline from the repository's configuration and attributes.
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
        // See the module header. Both of these are rulings, not oversights.
        options.drivers = Vec::new();
        options.crlf_roundtrip_check = gix::filter::plumbing::pipeline::CrlfRoundTripCheck::Skip;

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
    pub(crate) fn convert_to_git(&mut self, rela_path: &str, content: Vec<u8>) -> Result<Vec<u8>> {
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
            ToGitOutcome::Unchanged(_) => Ok(content),
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
