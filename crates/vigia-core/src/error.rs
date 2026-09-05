use std::fmt;

/// Result alias for every fallible operation in this crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Everything that can go wrong reading a working tree.
#[derive(Debug)]
pub enum Error {
    /// No git repository was found at or above the given path.
    Discover(Box<gix::discover::Error>),
    /// The repository has no working tree, so there is nothing to watch.
    Bare,
    /// Enumerating working-tree-vs-index changes failed.
    Status(Box<dyn std::error::Error + Send + Sync>),
    /// A worktree file could not be read.
    Read {
        /// Repository-relative path of the file.
        path: String,
        /// The underlying I/O failure.
        source: std::io::Error,
    },
    /// The filesystem watch could not be established.
    Watch(Box<dyn std::error::Error + Send + Sync>),
    /// A blob named by the index is absent from the object database.
    MissingBlob {
        /// Repository-relative path whose blob is missing.
        path: String,
    },
    /// The filter configuration git would apply could not be assembled.
    FilterSetup(Box<dyn std::error::Error + Send + Sync>),
    /// The notes store could not be created, written, read or pruned.
    Store {
        /// The path the operation was on.
        path: std::path::PathBuf,
        /// The underlying I/O failure.
        source: std::io::Error,
    },
    /// A worktree path has no canonical form, which means it does not exist.
    Canonicalise {
        /// The path as given.
        path: std::path::PathBuf,
        /// The underlying I/O failure.
        source: std::io::Error,
    },
    /// One file could not be normalised the way git's clean filter would.
    Filter {
        /// Repository-relative path that could not be normalised.
        path: String,
        /// The underlying failure.
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

impl Error {
    /// What one file's own failure was, or `None` when the failure is not one
    /// file's.
    ///
    /// The frame path contains the first, so an entry it cannot read costs that
    /// entry and not the frame: a permanent failure would otherwise hold the
    /// previous picture forever, and a stale pane is indistinguishable from a
    /// quiet tree. The second is propagated, because a failure the whole
    /// comparison shares leaves nothing to vouch for the rest of it.
    ///
    /// The reason carries no path. Its caller has one, and at the widths I6 is
    /// named for a repeated path is what pushes the reason off the edge.
    pub fn of_one_file(&self) -> Option<String> {
        match self {
            Error::Read { source, .. } => Some(source.to_string()),
            Error::Filter { source, .. } => Some(format!("could not be normalised: {source}")),
            Error::MissingBlob { .. } => Some("the index names a missing blob".to_owned()),
            Error::Discover(_)
            | Error::Bare
            | Error::Status(_)
            | Error::Watch(_)
            | Error::FilterSetup(_)
            | Error::Store { .. }
            | Error::Canonicalise { .. } => None,
        }
    }

    /// A working-tree path could not be read.
    pub(crate) fn read(path: &str, source: std::io::Error) -> Self {
        Error::Read {
            path: path.to_owned(),
            source,
        }
    }

    /// One path's conversion failed.
    pub(crate) fn filter(
        path: &str,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Error::Filter {
            path: path.to_owned(),
            source: Box::new(source),
        }
    }

    /// The store failed at `path`.
    pub(crate) fn store(path: &std::path::Path, source: std::io::Error) -> Self {
        Error::Store {
            path: path.to_owned(),
            source,
        }
    }

    /// The pipeline could not be built at all.
    pub(crate) fn filter_setup(source: impl std::error::Error + Send + Sync + 'static) -> Self {
        Error::FilterSetup(Box::new(source))
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Discover(e) => write!(f, "not a git repository: {e}"),
            Error::Bare => f.write_str("repository is bare, so it has no working tree to watch"),
            Error::Status(e) => write!(f, "could not read working tree status: {e}"),
            Error::Watch(e) => write!(f, "could not watch the working tree: {e}"),
            Error::Read { path, source } => write!(f, "could not read {path}: {source}"),
            Error::MissingBlob { path } => {
                write!(f, "the index entry for {path} points at a missing blob")
            }
            Error::FilterSetup(e) => write!(
                f,
                "could not read the line-ending rules git would apply: {e}"
            ),
            Error::Filter { path, source } => {
                write!(f, "could not normalise {path} the way git would: {source}")
            }
            Error::Store { path, source } => {
                write!(f, "the notes store failed at {}: {source}", path.display())
            }
            Error::Canonicalise { path, source } => {
                write!(f, "{} has no canonical path: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Discover(e) => Some(e),
            Error::Status(e) | Error::Watch(e) | Error::FilterSetup(e) => Some(e.as_ref()),
            Error::Read { source, .. }
            | Error::Store { source, .. }
            | Error::Canonicalise { source, .. } => Some(source),
            Error::Filter { source, .. } => Some(source.as_ref()),
            Error::Bare | Error::MissingBlob { .. } => None,
        }
    }
}

impl From<gix::discover::Error> for Error {
    fn from(e: gix::discover::Error) -> Self {
        Error::Discover(Box::new(e))
    }
}
