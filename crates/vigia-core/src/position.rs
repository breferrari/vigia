//! Where in the history the pane is standing.

use gix::ObjectId;

/// The pair of endpoints the pane's diff is between.
///
/// `Origin` says which of two comparisons a change was found by, and both of
/// those are relative to the index. This says what the comparison is *against*,
/// which is the thing the model could not express before: the pane has always
/// stood in the working tree because there was no term for standing anywhere
/// else.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Position {
    /// The working tree against the index, which is the pane a reader has today.
    ///
    /// Not spelled `HEAD`: the live view is ahead of it, so that label would name
    /// a state git would not agree with, the way `working tree clean` would.
    #[default]
    Current,
    /// Everything since a commit: that commit's tree against the working tree.
    Since {
        /// The commit the diff is measured from.
        at: ObjectId,
        /// What the reader calls it. A branch name where one resolved it, because
        /// the reader is thinking `main` rather than a hash.
        named: String,
    },
}

impl Position {
    /// The word the header draws after the branch.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::Current => "current".to_owned(),
            Self::Since { named, .. } => format!("since {named}"),
        }
    }

    /// The commit this position measures from, or `None` for the live pane.
    #[must_use]
    pub fn at(&self) -> Option<ObjectId> {
        match self {
            Self::Current => None,
            Self::Since { at, .. } => Some(*at),
        }
    }
}
