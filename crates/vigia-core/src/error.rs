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
    /// One file could not be normalised the way git's clean filter would.
    Filter {
        /// Repository-relative path that could not be normalised.
        path: String,
        /// The underlying failure.
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

impl Error {
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
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Discover(e) => Some(e),
            Error::Status(e) | Error::Watch(e) | Error::FilterSetup(e) => Some(e.as_ref()),
            Error::Read { source, .. } => Some(source),
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
