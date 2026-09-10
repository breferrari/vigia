//! Keeping a path out of the walk, by a pattern the reader wrote once.

use std::fmt;

use crate::error::{Error, Result};

/// A pattern that decides which changed paths the pane never sees.
///
/// Searched rather than anchored, which is what makes `^target/|\.lock$` read the
/// way it is written: a bare `target` hides every path with that word anywhere in
/// it, and a reader who wants otherwise says so with `^` or `$`.
#[derive(Clone)]
pub struct Hidden {
    matcher: fancy_regex::Regex,
}

impl Hidden {
    /// Compile `pattern`.
    ///
    /// # Errors
    ///
    /// `pattern` is not a regular expression this engine can build. The caller
    /// reports it before taking the terminal, because a full-screen program that
    /// paints an error and then hands the terminal back has painted nothing.
    pub fn new(pattern: &str) -> Result<Self> {
        match fancy_regex::Regex::new(pattern) {
            Ok(matcher) => Ok(Self { matcher }),
            Err(why) => Err(Error::Pattern {
                pattern: pattern.to_owned(),
                why: why.to_string(),
            }),
        }
    }

    /// Whether `path` is one the reader asked to keep out of the pane.
    ///
    /// A match that fails to *run* answers no. `fancy_regex` returns an error
    /// rather than hanging when a pattern with a backreference or a look-around
    /// exhausts its backtracking budget, and the two ways to spend that answer are
    /// not equal: showing a file nobody wanted costs a row, and hiding one the
    /// reader never asked to hide makes the pane lie about the tree.
    #[must_use]
    pub fn is_hidden(&self, path: &str) -> bool {
        self.matcher.is_match(path).unwrap_or(false)
    }

    /// The pattern as it was written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.matcher.as_str()
    }
}

impl fmt::Debug for Hidden {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Hidden").field(&self.as_str()).finish()
    }
}

/// Two patterns are the same setting when they are the same text. A compiled
/// program has no equality of its own, and the shell needs one so a config read
/// from a file can be compared with the config a reader would have had without it.
impl PartialEq for Hidden {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for Hidden {}
