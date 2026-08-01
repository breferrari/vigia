/// What happened to a path between the index and the working tree.
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
///
/// Deliberately cheap. Enumerating changes has to be fast enough to paint a
/// file list within the first frame (I4), so content and hunks are fetched
/// separately, per file, by [`Worktree::diff`](crate::Worktree::diff).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChange {
    /// Repository-relative path, always with `/` separators.
    pub path: String,
    /// What happened to it.
    pub kind: ChangeKind,
    /// Blob the index holds for this path, when there is one.
    ///
    /// `None` for additions, which have no index-side content.
    pub(crate) index_blob: Option<gix::ObjectId>,
}

impl ChangeKind {
    /// Whether this kind of change can have hunks at all.
    ///
    /// Conflicts and type changes are real changes with no meaningful
    /// line-level diff, and a monitor should show them as a state rather than
    /// pretend to diff them.
    ///
    /// On the kind rather than on [`FileChange`] because callers that hold only
    /// a kind need it too, and the shell had grown a third copy of the `matches!`
    /// deciding how many rows such a file occupies. One rule, three readers.
    pub fn is_diffable(&self) -> bool {
        !matches!(self, ChangeKind::Conflict | ChangeKind::TypeChange)
    }
}

impl FileChange {
    /// Whether this change can have hunks at all.
    ///
    /// See [`ChangeKind::is_diffable`], which is where the rule lives.
    pub fn is_diffable(&self) -> bool {
        self.kind.is_diffable()
    }

    /// Whether computing this change's diff has to read the working tree.
    ///
    /// One definition, two callers: the diff itself, and the frame path
    /// deciding whether there is a working-tree side to fingerprint. Two copies
    /// of this rule would let them drift into disagreeing about whether a
    /// cached diff can go stale, and the failure would be a stale frame rather
    /// than a compile error.
    pub(crate) fn reads_worktree(&self) -> bool {
        self.is_diffable() && !matches!(self.kind, ChangeKind::Removed)
    }
}
