use std::fmt;

/// Result alias for every fallible operation in this crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Everything that can go wrong reading a working tree.
///
/// Boxed inner errors keep `Result<T>` small, which matters because the change
/// stream yields one `Result` per file and sits on the first-paint path (I4).
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
    /// A blob named by the index is absent from the object database.
    ///
    /// A monitor must survive this rather than exit: it happens legitimately
    /// mid-`git gc` and during a partial clone.
    MissingBlob {
        /// Repository-relative path whose blob is missing.
        path: String,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Discover(e) => write!(f, "not a git repository: {e}"),
            Error::Bare => f.write_str("repository is bare, so it has no working tree to watch"),
            Error::Status(e) => write!(f, "could not read working tree status: {e}"),
            Error::Read { path, source } => write!(f, "could not read {path}: {source}"),
            Error::MissingBlob { path } => {
                write!(f, "the index entry for {path} points at a missing blob")
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Discover(e) => Some(e),
            Error::Status(e) => Some(e.as_ref()),
            Error::Read { source, .. } => Some(source),
            Error::Bare | Error::MissingBlob { .. } => None,
        }
    }
}

impl From<gix::discover::Error> for Error {
    fn from(e: gix::discover::Error) -> Self {
        Error::Discover(Box::new(e))
    }
}
