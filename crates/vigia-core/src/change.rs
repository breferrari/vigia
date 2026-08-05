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
    /// Whether the working-tree side may be a symlink, as the status walk saw
    /// it.
    ///
    /// **A hint that costs nothing, and it exists to keep a syscall off the read
    /// path.** Git stores a symlink as a mode `120000` blob holding the target
    /// path, so `Worktree::read_worktree` has to read the link rather than
    /// through it ([#15](https://github.com/breferrari/vigia/issues/15)) and
    /// therefore has to know which it is looking at. Asking the filesystem meant
    /// an `lstat` before **every** working-tree read, which measured **+1.18ms
    /// p50** over a hundred undrawn files inside the settle margin (13.45ms to
    /// 14.63ms) and would have put a second `stat` per file on the one path
    /// `crates/vigia/tests/reads.rs::a_tick_inside_the_settle_margin_stats_each_file_once`
    /// exists to hold at one.
    ///
    /// The walk already knows. `gix` reports an index entry's mode and a dirwalk
    /// entry's disk kind, and it has already paid for both, so this is the answer
    /// carried forward rather than bought again.
    ///
    /// **Conservative, and named `maybe_` for that reason.** `false` is claimed
    /// only where the walk positively reports a regular file; anything it does
    /// not resolve stays `true` and pays the `lstat`, because a wrong `false`
    /// reads a file where git reads a path and a wrong `true` costs one syscall.
    ///
    /// **What makes reading the index mode sound is that the two ways it can go
    /// stale are not symmetric, and only one of them reaches a read.** Both are
    /// measured rather than argued, by
    /// `crates/vigia-core/tests/fidelity.rs::swapping_a_symlink_and_a_regular_file_in_both_directions_agrees_with_git`:
    ///
    /// * A link replaced by a **regular file** keeps its `120000` index entry and
    ///   git calls it a *modification*. This says `true`, the `lstat` finds a
    ///   plain file, and the read is an ordinary one. The mode being stale costs
    ///   a syscall and changes no answer.
    /// * A regular file replaced by a **link** is what a stale `FILE` mode would
    ///   get wrong, and it never arrives: git and `gix` both report it as a
    ///   *type change*, which [`FileChange::is_diffable`] rejects before
    ///   `Worktree::diff` consults the working tree at all.
    ///
    /// So the soundness rests on `gix`'s type-change detection rather than on
    /// this field, and that test is what holds it: if a future `gix` reported
    /// that second case as a modification, it goes red rather than quietly
    /// reading a link's target through it.
    pub(crate) maybe_symlink: bool,
}

impl FileChange {
    /// Whether this change can have hunks at all.
    ///
    /// Conflicts and type changes are real changes with no meaningful
    /// line-level diff, and a monitor should show them as a state rather than
    /// pretend to diff them.
    pub fn is_diffable(&self) -> bool {
        !matches!(self.kind, ChangeKind::Conflict | ChangeKind::TypeChange)
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

    /// Whether this change can alter what git's clean filter does to **other**
    /// files.
    ///
    /// A `.gitattributes` decides `text`, `eol` and any filter driver for every
    /// path under its directory, so writing one changes the bytes a diff of an
    /// untouched file would compare, without moving that file's length,
    /// modification time or index blob. Every term the reuse rule behind
    /// [`Frame::diff`](crate::Frame::diff) consults therefore reports
    /// "unchanged" while the answer underneath has changed, which is why
    /// [`Frame::advance`](crate::Frame::advance) drops what it holds rather than
    /// trying to prove it.
    ///
    /// **Matched on the file name at any depth**, because attributes nest: a
    /// `src/.gitattributes` governs `src/` and nothing else, but the frame keeps
    /// one cache for the whole worktree and has no cheap way to ask which paths a
    /// given attributes file reaches. Over-invalidating costs one worktree of
    /// reads on a rare event; under-invalidating shows a diff that is wrong until
    /// something else touches the file.
    ///
    /// Case-insensitively, because git resolves the name that way on Windows and
    /// macOS and a `.GitAttributes` written there is the same file.
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
