/// Which comparison a change was found by. `SPEC.md` §11.2 B17.
///
/// Carried on the change rather than derived from it, because one path can be in
/// both runs at once — staged, then edited again on disk — and the two entries
/// are genuinely two different diffs. Only [`crate::Frame::advance`] is in a
/// position to know which walk produced one.
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
    ///
    /// Here rather than in the shell because it is the run's own name rather than
    /// one drawer's wording, and because the shell already spells the word in more
    /// than one place: the separator draws this, and `count_of`'s header fact and
    /// `empty_state_with`'s line each spell their own sentence around it.
    pub fn label(self) -> &'static str {
        match self {
            Self::Unstaged => "unstaged",
            Self::Staged => "staged",
        }
    }
}

/// Where the right-hand side of a diff comes from.
///
/// Data rather than a rule, because a staged change compares two blobs and reads
/// no file: [`ChangeKind`] alone cannot say which side to take.
///
/// `None` on [`FileChange::after`] is a removal — a third case neither variant
/// here should have to represent.
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
    /// Which comparison found this change.
    ///
    /// See [`Origin`]. Two entries for one path are ordinary since
    /// [#313](https://github.com/breferrari/vigia/issues/313), and this is what
    /// tells them apart everywhere downstream: the ink on the row's kind letter,
    /// the cache key, and which run the separator counts it into.
    pub origin: Origin,
    /// Blob the left-hand side holds for this path, when there is one.
    ///
    /// The index's blob for an unstaged change and `HEAD`'s for a staged one.
    /// `None` for an addition, which has no left-hand side on either.
    pub(crate) before: Option<gix::ObjectId>,
    /// Where the right-hand side comes from, or `None` for a removal.
    ///
    /// See [`Side`]. An unstaged change reads the working tree; a staged change
    /// takes the index's blob and touches no file.
    pub(crate) after: Option<Side>,
    /// Whether the working-tree side may be a symlink, as the status walk saw it.
    ///
    /// Git stores a symlink as a mode `120000` blob holding the target path, so
    /// `Worktree::read_worktree` must read the link rather than through it. The
    /// walk already knows — `gix` reports an index entry's mode and a dirwalk
    /// entry's disk kind — so this carries that answer forward instead of
    /// buying an `lstat` before every working-tree read, which measured
    /// **+1.18ms p50** over a hundred undrawn files inside the settle margin.
    ///
    /// **Conservative, and named `maybe_` for that reason.** `false` is claimed
    /// only where the walk positively reports a regular file. A wrong `false`
    /// reads a file where git reads a path; a wrong `true` costs one syscall.
    ///
    /// **Only `Mode::FILE` counts as that positive report, never
    /// `Mode::FILE_EXECUTABLE`.** `gix`'s `change_to_match_fs_with_values` has
    /// an arm for the first and none for the second, so a `100755` entry
    /// replaced by a link arrives as a *modification* with the index still
    /// reading `100755`. Every other direction is safe by asymmetry rather than
    /// by this field: a link replaced by a regular file yields `true` and an
    /// ordinary read, and a `100644` file replaced by a link is reported as a
    /// type change, which [`FileChange::is_diffable`] rejects before the
    /// working tree is consulted at all. **Which label either direction gets is
    /// not portable**, so the gates assert the property both labels must satisfy
    /// — neither may end in a read that follows the link — rather than the label:
    /// `fidelity.rs::swapping_a_symlink_and_a_regular_file_in_both_directions_agrees_with_git`
    /// and `fidelity.rs::an_executable_replaced_by_a_symlink_diffs_as_its_target_path`.
    ///
    /// **A walk-time answer read at read-time**, so a path that was regular when
    /// the walk classified it and is a link by the time the diff reads it gets
    /// `false` and is followed. That window is one tick wide and the next frame
    /// reclassifies.
    ///
    /// **Meaningful only where [`Self::after`] is [`Side::Worktree`].** A staged
    /// change compares two blobs and never opens a file; the staged walk sets
    /// this `true`, which costs nothing where no read happens.
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
    /// **Resolved from [`Self::after`] rather than from [`Self::kind`] since
    /// [#313](https://github.com/breferrari/vigia/issues/313).** A staged
    /// modification is a `Modified` that reads no file, so the kind stopped being
    /// evidence the moment a second comparison existed. The rule is now the datum
    /// the walk recorded, which cannot disagree with the diff that reads it.
    pub(crate) fn reads_worktree(&self) -> bool {
        self.is_diffable() && matches!(self.after, Some(Side::Worktree))
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
