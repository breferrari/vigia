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
pub enum Standing {
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
    /// One commit alone: its first parent's tree against its own.
    ///
    /// The working tree is at neither end, which is what makes this the reading
    /// that costs something. Everything the pane draws from the watch describes
    /// now, and the body is showing then.
    Only {
        /// The commit, which is the diff's right-hand side rather than its left.
        at: ObjectId,
        /// What the reader calls it, which here is always an abbreviation: a
        /// branch name would claim the whole branch rather than one commit on it.
        named: String,
    },
}

/// Which of the two readings of one position a standing is.
///
/// Derived from a [`Standing`] and never stored beside one. Two copies of this
/// answer is how a token and the list behind it come to disagree about what a
/// click will do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Reading {
    /// Everything from a point up to the working tree, which stays live.
    #[default]
    Since,
    /// That commit alone, which is a still picture.
    Only,
}

impl Reading {
    /// Whether the working tree is one of the ends, and so whether anything the
    /// watch feeds still describes what the pane is drawing.
    #[must_use]
    pub const fn is_live(self) -> bool {
        matches!(self, Self::Since)
    }

    /// The word the token spells and the list titles itself with.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::Since => Standing::SINCE,
            Self::Only => Standing::ONLY,
        }
    }
}

impl Standing {
    /// What [`Standing::Current`] draws, named so the header's drop ladder can
    /// recognise it without a second copy of the word.
    pub const CURRENT: &'static str = "current";

    /// The reading, which the token spells before the commit and the position
    /// list spells on its own title bar.
    pub const SINCE: &'static str = "since";

    /// The other one.
    pub const ONLY: &'static str = "only";

    /// Which reading this is.
    ///
    /// `Current` is `Since`, because it is the special case of `since` whose point
    /// is the index: there is no "only the working tree" to be the other half.
    #[must_use]
    pub const fn reading(&self) -> Reading {
        match self {
            Self::Current | Self::Since { .. } => Reading::Since,
            Self::Only { .. } => Reading::Only,
        }
    }

    /// The word the header draws after the branch.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::Current => Self::CURRENT.to_owned(),
            Self::Since { named, .. } | Self::Only { named, .. } => {
                format!("{} {named}", self.reading().word())
            }
        }
    }

    /// The commit this position names, or `None` for the live pane.
    ///
    /// A commit is the left end under `since` and the right end under `only`, so
    /// this says *which* commit and never which end. A caller that needs the end
    /// matches on the variant.
    #[must_use]
    pub fn at(&self) -> Option<ObjectId> {
        match self {
            Self::Current => None,
            Self::Since { at, .. } | Self::Only { at, .. } => Some(*at),
        }
    }

    /// The same position under the other reading, or `None` where no commit is
    /// named and there is nothing for a reading to be about.
    ///
    /// The name crosses over, so a caller holding a position named by a *branch*
    /// must refuse before reaching here: `only main` would claim a whole branch
    /// where the reading draws one commit on it.
    #[must_use]
    pub fn flipped(&self) -> Option<Self> {
        match self {
            Self::Current => None,
            Self::Since { at, named } => Some(Self::Only {
                at: *at,
                named: named.clone(),
            }),
            Self::Only { at, named } => Some(Self::Since {
                at: *at,
                named: named.clone(),
            }),
        }
    }
}
