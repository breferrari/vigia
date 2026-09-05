/// Which comparison a change was found by. `SPEC.md` §11.2 B17.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Origin {
    /// The working tree against the index. The pane's default, and the thesis:
    /// it is what the agent in the other pane just wrote.
    #[default]
    Unstaged,
    /// The index against `HEAD^{tree}`. What has been staged and not committed.
    Staged,
}

impl Origin {
    /// The word the pane draws for this run.
    pub fn label(self) -> &'static str {
        match self {
            Self::Unstaged => "unstaged",
            Self::Staged => "staged",
        }
    }
}

/// Where the right-hand side of a diff comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Side {
    /// Read the file on disk, through git's clean filter.
    Worktree,
    /// Take the object database's bytes for this id.
    Blob(gix::ObjectId),
}

/// What happened to a path between the two sides of a comparison.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeKind {
    /// Untracked in the index, present on disk.
    Added,
    /// Tracked, and the worktree content differs from the index.
    Modified,
    /// Tracked, and absent from the worktree.
    Removed,
    /// Rename-tracking matched a removal to an addition.
    Renamed {
        /// Repository-relative path the content moved from.
        from: String,
    },
    /// Rename-tracking matched an addition to content that still exists elsewhere.
    Copied {
        /// Repository-relative path the content was copied from.
        from: String,
    },
    /// File became a symlink, a symlink became a file, or similar.
    TypeChange,
    /// An unresolved merge conflict.
    Conflict,
    /// `git add -N`: tracked, but with no content staged yet.
    IntentToAdd,
}

/// One changed path, without its content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChange {
    /// Repository-relative path, always with `/` separators.
    pub path: String,
    /// What happened to it.
    pub kind: ChangeKind,
    /// Which comparison found this change.
    pub origin: Origin,
    /// Blob the left-hand side holds for this path, when there is one.
    pub(crate) before: Option<gix::ObjectId>,
    /// Where the right-hand side comes from, or `None` for a removal.
    pub(crate) after: Option<Side>,
    /// Whether the working-tree side may be a symlink, as the status walk saw it.
    pub(crate) maybe_symlink: bool,
}

impl ChangeKind {
    /// The path the content moved or was copied from, for the kinds that have
    /// one, which is where a note pinned under the old path is looked for.
    pub fn source(&self) -> Option<&str> {
        match self {
            Self::Renamed { from } | Self::Copied { from } => Some(from),
            _ => None,
        }
    }
}

impl FileChange {
    /// Every path this change answers to: its own, then the one a rename or a
    /// copy left behind, so a note pinned under the old path finds it.
    pub fn paths(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.path.as_str()).chain(self.kind.source())
    }

    /// Whether this change can have hunks at all.
    pub fn is_diffable(&self) -> bool {
        !matches!(self.kind, ChangeKind::Conflict | ChangeKind::TypeChange)
    }

    /// Whether computing this change's diff has to read the working tree.
    pub(crate) fn reads_worktree(&self) -> bool {
        self.is_diffable() && matches!(self.after, Some(Side::Worktree))
    }

    /// Whether this change can alter what git's clean filter does to other
    /// files.
    pub(crate) fn rewrites_attributes(&self) -> bool {
        let name = |path: &str| {
            path.rsplit(['/', '\\'])
                .next()
                .unwrap_or(path)
                .to_ascii_lowercase()
        };
        if name(&self.path) == ".gitattributes" {
            return true;
        }
        // A rename moves the rules from where they were, which is the same event
        // seen from the other end.
        match &self.kind {
            ChangeKind::Renamed { from } | ChangeKind::Copied { from } => {
                name(from) == ".gitattributes"
            }
            _ => false,
        }
    }
}
