//! Does `gix` read a working tree the way git wrote it?
//!
//! This is the open question `SPEC.md` §10 puts on Phase 1, and it gates the
//! whole stack. Every test here builds its fixture with real `git` and then
//! reads it back through `vigia-core`.

mod support;

use support::Scratch;
use vigia_core::{ChangeKind, ChangeOptions, FileChange, LineKind, Worktree};

/// All changes, sorted by path so assertions do not depend on walk order.
fn changes_of(worktree: &Worktree) -> Vec<FileChange> {
    let mut all: Vec<FileChange> = worktree
        .changes()
        .expect("enumerate changes")
        .map(|c| c.expect("change"))
        .collect();
    all.sort_by(|a, b| a.path.cmp(&b.path));
    all
}

fn numbered_lines(count: usize) -> String {
    (1..=count)
        .map(|i| format!("line {i}\n"))
        .collect::<String>()
}

#[test]
fn clean_worktree_yields_no_changes() {
    let scratch = Scratch::new("clean");
    scratch.write("a.txt", "hello\n");
    scratch.commit_all("initial");

    assert_eq!(changes_of(&scratch.worktree()), Vec::new());
}

#[test]
fn untracked_file_is_added_and_diffs_as_all_additions() {
    let scratch = Scratch::new("added");
    scratch.write("tracked.txt", "kept\n");
    scratch.commit_all("initial");
    scratch.write("fresh.txt", "one\ntwo\n");

    let worktree = scratch.worktree();
    let changes = changes_of(&worktree);

    assert_eq!(changes.len(), 1, "only the new file should be reported");
    assert_eq!(changes[0].path, "fresh.txt");
    assert_eq!(changes[0].kind, ChangeKind::Added);

    let diff = worktree.diff(&changes[0]).expect("diff added file");
    assert_eq!((diff.added, diff.removed), (2, 0));
    assert!(
        diff.hunks[0]
            .lines
            .iter()
            .all(|l| l.kind == LineKind::Added),
        "a new file has no context and nothing removed"
    );
}

#[test]
fn modified_file_reports_only_the_changed_lines() {
    let scratch = Scratch::new("modified");
    scratch.write("a.txt", numbered_lines(20));
    scratch.commit_all("initial");
    scratch.write(
        "a.txt",
        numbered_lines(20).replace("line 10\n", "CHANGED\n"),
    );

    let worktree = scratch.worktree();
    let changes = changes_of(&worktree);
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].kind, ChangeKind::Modified);

    let diff = worktree.diff(&changes[0]).expect("diff modified file");
    assert_eq!(
        (diff.added, diff.removed),
        (1, 1),
        "one line changed, so exactly one added and one removed"
    );
    assert_eq!(diff.hunks.len(), 1);
}

#[test]
fn deleted_file_reports_every_line_removed() {
    let scratch = Scratch::new("removed");
    scratch.write("gone.txt", "one\ntwo\nthree\n");
    scratch.commit_all("initial");
    scratch.remove("gone.txt");

    let worktree = scratch.worktree();
    let changes = changes_of(&worktree);
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].kind, ChangeKind::Removed);

    let diff = worktree.diff(&changes[0]).expect("diff removed file");
    assert_eq!((diff.added, diff.removed), (0, 3));
}

#[test]
fn a_moved_file_is_reported_as_one_rename() {
    let scratch = Scratch::new("renamed");
    scratch.write("old/name.txt", numbered_lines(30));
    scratch.commit_all("initial");
    scratch.git(&["mv", "old/name.txt", "new-name.txt"]);
    // `git mv` stages the move; the monitor watches the worktree, so unstage
    // it and leave the rename sitting in the working tree where we can see it.
    scratch.git(&["reset", "-q"]);

    let worktree = scratch.worktree();
    let changes = changes_of(&worktree);

    let renamed = changes
        .iter()
        .find(|c| matches!(c.kind, ChangeKind::Renamed { .. }))
        .unwrap_or_else(|| panic!("expected a rename, got {changes:#?}"));
    assert_eq!(renamed.path, "new-name.txt");
    assert_eq!(
        renamed.kind,
        ChangeKind::Renamed {
            from: "old/name.txt".to_owned()
        }
    );
}

#[test]
fn disabling_rename_tracking_splits_the_move_into_two_changes() {
    let scratch = Scratch::new("renames-off");
    scratch.write("old/name.txt", numbered_lines(30));
    scratch.commit_all("initial");
    scratch.git(&["mv", "old/name.txt", "new-name.txt"]);
    scratch.git(&["reset", "-q"]);

    let worktree = scratch.worktree();
    let mut kinds: Vec<ChangeKind> = worktree
        .changes_with(ChangeOptions {
            track_renames: false,
        })
        .expect("enumerate without rename tracking")
        .map(|c| c.expect("change").kind)
        .collect();
    kinds.sort_by_key(|k| format!("{k:?}"));

    assert_eq!(
        kinds,
        vec![ChangeKind::Added, ChangeKind::Removed],
        "without rename tracking a move is a deletion plus an addition"
    );
}

#[test]
fn binary_content_is_flagged_and_not_diffed() {
    let scratch = Scratch::new("binary");
    scratch.write("blob.bin", [0x00, 0x01, 0x02, 0xff, 0x00]);
    scratch.commit_all("initial");
    scratch.write("blob.bin", [0x00, 0x09, 0x02, 0xfe, 0x00, 0x07]);

    let worktree = scratch.worktree();
    let changes = changes_of(&worktree);
    assert_eq!(changes.len(), 1);

    let diff = worktree.diff(&changes[0]).expect("diff binary file");
    assert!(diff.binary, "content with NUL bytes must sniff as binary");
    assert!(diff.hunks.is_empty(), "binary files get no hunks");
    assert_eq!((diff.added, diff.removed), (0, 0));
}

#[test]
fn ignored_files_stay_invisible() {
    let scratch = Scratch::new("ignored");
    scratch.write(".gitignore", "secret.txt\n");
    scratch.commit_all("initial");
    scratch.write("secret.txt", "do not show me\n");
    scratch.write("visible.txt", "show me\n");

    let paths: Vec<String> = changes_of(&scratch.worktree())
        .into_iter()
        .map(|c| c.path)
        .collect();

    assert_eq!(paths, vec!["visible.txt".to_owned()]);
}

#[test]
fn nested_paths_use_forward_slashes_on_every_platform() {
    let scratch = Scratch::new("separators");
    scratch.write("keep.txt", "x\n");
    scratch.commit_all("initial");
    scratch.write("src/deep/nested.txt", "y\n");

    let changes = changes_of(&scratch.worktree());
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].path, "src/deep/nested.txt");
}

#[test]
fn hunk_headers_match_what_git_reports() {
    let scratch = Scratch::new("headers");
    scratch.write("a.txt", numbered_lines(40));
    scratch.commit_all("initial");

    // Lines 5 and 8 are close enough that their context windows overlap, so
    // they belong to one hunk. Line 30 is far away and must get its own.
    let edited = numbered_lines(40)
        .replace("line 5\n", "FIVE\n")
        .replace("line 8\n", "EIGHT\n")
        .replace("line 30\n", "THIRTY\n");
    scratch.write("a.txt", &edited);

    let worktree = scratch.worktree();
    let changes = changes_of(&worktree);
    let diff = worktree.diff(&changes[0]).expect("diff");

    let ours: Vec<(u32, u32, u32, u32)> = diff
        .hunks
        .iter()
        .map(|h| (h.old_start, h.old_lines, h.new_start, h.new_lines))
        .collect();

    assert_eq!(
        ours,
        scratch.git_hunk_headers("a.txt"),
        "hunk boundaries must agree with git exactly"
    );
    assert_eq!(diff.hunks.len(), 2, "two edits far apart, two hunks");
}

/// Where exactly two edits stop sharing a hunk.
///
/// Sweeping the gap rather than picking one value is deliberate: a single
/// fixture with a comfortable gap passes no matter what the merge threshold
/// is, so it tests nothing. Git is the oracle at every step.
#[test]
fn hunk_grouping_boundary_matches_git_at_every_gap() {
    const TOTAL_LINES: usize = 60;
    const FIRST_EDIT: u32 = 20;

    for gap in 0..=10u32 {
        let scratch = Scratch::new(&format!("gap-{gap}"));
        scratch.write("a.txt", numbered_lines(TOTAL_LINES));
        scratch.commit_all("initial");

        // `gap` unchanged lines sit between the two edits.
        let second_edit = FIRST_EDIT + gap + 1;
        let edited = numbered_lines(TOTAL_LINES)
            .replace(&format!("line {FIRST_EDIT}\n"), "FIRST\n")
            .replace(&format!("line {second_edit}\n"), "SECOND\n");
        scratch.write("a.txt", &edited);

        let worktree = scratch.worktree();
        let changes = changes_of(&worktree);
        let diff = worktree.diff(&changes[0]).expect("diff");

        let ours: Vec<(u32, u32, u32, u32)> = diff
            .hunks
            .iter()
            .map(|h| (h.old_start, h.old_lines, h.new_start, h.new_lines))
            .collect();

        assert_eq!(
            ours,
            scratch.git_hunk_headers("a.txt"),
            "disagreed with git when {gap} unchanged lines separate the edits"
        );
    }
}

#[test]
fn rendered_lines_carry_no_line_terminators() {
    let scratch = Scratch::new("terminators");
    scratch.write("crlf.txt", "alpha\r\nbeta\r\ngamma\r\n");
    scratch.commit_all("initial");
    scratch.write("crlf.txt", "alpha\r\nBETA\r\ngamma\r\n");

    let worktree = scratch.worktree();
    let changes = changes_of(&worktree);
    let diff = worktree.diff(&changes[0]).expect("diff");

    for line in diff.hunks.iter().flat_map(|h| &h.lines) {
        assert!(
            !line.text.contains('\r') && !line.text.contains('\n'),
            "line text {:?} still carries a terminator",
            line.text
        );
    }
    assert!(
        diff.hunks
            .iter()
            .flat_map(|h| &h.lines)
            .any(|l| l.kind == LineKind::Added && l.text == "BETA")
    );
}

#[test]
fn a_file_without_a_trailing_newline_still_diffs() {
    let scratch = Scratch::new("no-eof-newline");
    scratch.write("a.txt", "one\ntwo");
    scratch.commit_all("initial");
    scratch.write("a.txt", "one\nTWO");

    let worktree = scratch.worktree();
    let changes = changes_of(&worktree);
    let diff = worktree.diff(&changes[0]).expect("diff");

    assert_eq!((diff.added, diff.removed), (1, 1));
    assert!(
        diff.hunks
            .iter()
            .flat_map(|h| &h.lines)
            .any(|l| l.kind == LineKind::Added && l.text == "TWO")
    );
}

#[test]
fn one_unreadable_path_does_not_end_the_stream() {
    let scratch = Scratch::new("resilience");
    scratch.write("a.txt", "one\n");
    scratch.write("b.txt", "two\n");
    scratch.commit_all("initial");
    scratch.write("a.txt", "one changed\n");
    scratch.write("b.txt", "two changed\n");

    let worktree = scratch.worktree();
    let changes = changes_of(&worktree);
    assert_eq!(changes.len(), 2);

    // Delete one file after enumeration, which is exactly what the agent in
    // the other pane does. Diffing it must degrade, not fail.
    scratch.remove("a.txt");
    for change in &changes {
        worktree
            .diff(change)
            .unwrap_or_else(|e| panic!("diffing {} after deletion failed: {e}", change.path));
    }
}
