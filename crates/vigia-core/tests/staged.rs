//! The staged run: `SPEC.md` §11.2 **B17**.

mod support;

use support::Scratch;
use vigia_core::{ChangeKind, FileChange, Frame, LineKind, Origin, Worktree};

const FILE: &str = "src/lib.rs";
const OTHER: &str = "src/other.rs";

/// A repository with one commit and two tracked files, nothing changed yet.
fn fixture(name: &str) -> Scratch {
    let scratch = Scratch::new(name);
    scratch.write(FILE, "one\ntwo\nthree\n");
    scratch.write(OTHER, "alpha\nbeta\n");
    scratch.git(&["add", "-A"]);
    scratch.git(&["commit", "-m", "init"]);
    scratch
}

/// Every change the frame holds, as `(origin, path)`, in the order it reports.
fn runs(frame: &Frame) -> Vec<(Origin, String)> {
    frame
        .files()
        .iter()
        .map(|change| (change.origin, change.path.clone()))
        .collect()
}

/// The one change at `path` in `origin`'s run.
fn only<'f>(frame: &'f Frame, origin: Origin, path: &str) -> &'f FileChange {
    let mut found = frame
        .files()
        .iter()
        .filter(|change| change.origin == origin && change.path == path);
    let change = found
        .next()
        .unwrap_or_else(|| panic!("no {origin:?} change for {path}"));
    assert!(
        found.next().is_none(),
        "{path} appears twice in the {origin:?} run, which is one row too many"
    );
    change
}

/// The issue's own acceptance, both directions on one worktree.
#[test]
fn a_staged_change_is_absent_from_the_unstaged_walk_and_present_in_the_staged_one() {
    let scratch = fixture("staged-absent");
    scratch.write(FILE, "one\nSTAGED\nthree\n");
    scratch.git(&["add", FILE]);
    scratch.write(OTHER, "alpha\nUNSTAGED\n");

    let worktree = Worktree::discover(scratch.root()).expect("discover");

    // With the toggle off the pane is what it has always been: the staged file is
    // not there at all, which is the defect the issue was opened on.
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    assert_eq!(
        runs(&frame),
        vec![(Origin::Unstaged, OTHER.to_owned())],
        "with the staged run hidden the pane shows the unstaged comparison alone"
    );

    // With it on, both are there and each is in its own run.
    frame.show_staged(true);
    frame.advance().expect("advance");
    assert_eq!(
        runs(&frame),
        vec![
            (Origin::Unstaged, OTHER.to_owned()),
            (Origin::Staged, FILE.to_owned()),
        ],
        "unstaged first, then staged, and neither file is in the other's run"
    );
}

/// The case the union exists for: one path, changed on both sides.
#[test]
fn a_path_staged_and_then_edited_again_appears_once_in_each_run_with_different_content() {
    let scratch = fixture("staged-both-sides");
    scratch.write(FILE, "one\nSTAGED\nthree\n");
    scratch.git(&["add", FILE]);
    scratch.write(FILE, "one\nSTAGED\nthree\nAND UNSTAGED\n");

    let worktree = Worktree::discover(scratch.root()).expect("discover");
    let mut frame = worktree.frame();
    frame.show_staged(true);
    frame.advance().expect("advance");

    assert_eq!(
        runs(&frame),
        vec![
            (Origin::Unstaged, FILE.to_owned()),
            (Origin::Staged, FILE.to_owned()),
        ],
        "one path, two runs, two rows"
    );

    let index_of = |origin: Origin| {
        frame
            .files()
            .iter()
            .position(|change| change.origin == origin)
            .expect("both runs are present")
    };
    let (unstaged, staged) = (index_of(Origin::Unstaged), index_of(Origin::Staged));

    let added_unstaged = frame.diff(unstaged).expect("unstaged diff").1.added;
    let added_staged = frame.diff(staged).expect("staged diff").1.added;

    // The unstaged run holds the one line added since the file was staged; the
    // staged run holds the line staged before that. A cache keyed by path alone
    // hands the same number back twice, which is exactly the failure this pair
    // of assertions exists to catch.
    assert_eq!(
        (added_unstaged, added_staged),
        (1, 1),
        "each run diffs its own pair of bytes"
    );
    let (_, unstaged_diff) = frame.diff(unstaged).expect("unstaged diff");
    assert!(
        unstaged_diff
            .hunks
            .iter()
            .flat_map(|hunk| hunk.lines.iter())
            .any(|line| line.text.contains("AND UNSTAGED")),
        "the unstaged run draws the edit that is not staged yet"
    );
    // **Which side each line is on, not merely that it appears.** Found by
    // mutation: swapping `before` and `after` on a staged modification left the
    // whole suite green, because a fixture that rewrites one line is +1/-1 either
    // way and the word is in the hunk whichever side it lands on. A diff drawn
    // backwards shows every staged addition as a removal, in green and red, on
    // every row of the run.
    let (_, staged_diff) = frame.diff(staged).expect("staged diff");
    let side = |want: &str| {
        staged_diff
            .hunks
            .iter()
            .flat_map(|hunk| hunk.lines.iter())
            .find(|line| line.text.contains(want))
            .map(|line| line.kind)
    };
    assert_eq!(
        side("STAGED"),
        Some(LineKind::Added),
        "the index's content is not on the added side, so the staged run is \
         diffing HEAD against the index backwards"
    );
    assert_eq!(
        side("two"),
        Some(LineKind::Removed),
        "HEAD's content is not on the removed side"
    );
}

/// **A staged diff reads no file at all**, which is what makes the second walk
/// affordable and is the premise the whole design rests on.
#[test]
fn a_staged_diff_reads_no_file_from_the_working_tree() {
    let scratch = fixture("staged-no-read");
    scratch.write(FILE, "one\nSTAGED\nthree\n");
    scratch.git(&["add", FILE]);
    // Gone from disk, still in the index. `git diff --cached` still reports it,
    // and so must this.
    scratch.remove(FILE);

    let worktree = Worktree::discover(scratch.root()).expect("discover");
    let mut frame = worktree.frame();
    frame.show_staged(true);
    frame.advance().expect("advance");

    let staged = frame
        .files()
        .iter()
        .position(|change| change.origin == Origin::Staged && change.path == FILE)
        .expect("the staged run still holds a file that is gone from disk");

    let before = frame.stats();
    let (_, diff) = frame.diff(staged).expect("staged diff");
    let (added, removed) = (diff.added, diff.removed);
    let after = frame.stats();

    assert_eq!(
        (added, removed),
        (1, 1),
        "the staged diff is the one line the index rewrote over HEAD's. A diff \
         that had consulted the working tree would find nothing there and report \
         all three lines removed and none added"
    );
    assert_eq!(
        after.probes - before.probes,
        0,
        "and no type probe was spent, because there is no working-tree side to \
         classify"
    );
}

/// A staged rename is one change, for the same reason an unstaged one is: showing
/// a move as an unrelated delete plus add misdescribes what the agent did.
#[test]
fn a_staged_rename_reads_as_one_change_rather_than_a_delete_and_an_add() {
    let scratch = fixture("staged-rename");
    scratch.git(&["mv", FILE, "src/renamed.rs"]);

    let worktree = Worktree::discover(scratch.root()).expect("discover");
    let mut frame = worktree.frame();
    frame.show_staged(true);
    frame.advance().expect("advance");

    assert_eq!(
        runs(&frame),
        vec![(Origin::Staged, "src/renamed.rs".to_owned())],
        "one row, at the path the content is at now"
    );
    let change = only(&frame, Origin::Staged, "src/renamed.rs");
    assert_eq!(
        change.kind,
        ChangeKind::Renamed {
            from: FILE.to_owned()
        },
        "and it says where the content came from"
    );
}

/// An unborn `HEAD` is where an agent's first minute actually is.
#[test]
fn a_repository_with_no_commits_reads_its_whole_index_as_staged_additions() {
    let scratch = Scratch::new("staged-unborn");
    scratch.write(FILE, "one\ntwo\n");
    scratch.write(OTHER, "alpha\n");
    scratch.git(&["add", "-A"]);
    // Deliberately no commit.

    let worktree = Worktree::discover(scratch.root()).expect("discover");
    let mut frame = worktree.frame();
    frame.show_staged(true);
    frame.advance().expect("advance");

    let mut staged: Vec<_> = frame
        .files()
        .iter()
        .filter(|change| change.origin == Origin::Staged)
        .map(|change| (change.path.clone(), format!("{:?}", change.kind)))
        .collect();
    staged.sort();
    assert_eq!(
        staged,
        vec![
            (FILE.to_owned(), format!("{:?}", ChangeKind::Added)),
            (OTHER.to_owned(), format!("{:?}", ChangeKind::Added)),
        ],
        "with no commit behind it, everything in the index is staged and new"
    );
}

/// Turning the run off must not leave its diffs behind to be handed back later.
#[test]
fn hiding_the_staged_run_discards_the_diffs_taken_for_it() {
    let scratch = fixture("staged-hide");
    scratch.write(FILE, "one\nSTAGED\nthree\n");
    scratch.git(&["add", FILE]);

    let worktree = Worktree::discover(scratch.root()).expect("discover");
    let mut frame = worktree.frame();
    frame.show_staged(true);
    frame.advance().expect("advance");
    frame.diff(0).expect("staged diff");
    assert_eq!(frame.tracked(), 1, "one staged diff is held");

    frame.show_staged(false);
    assert_eq!(
        frame.tracked(),
        0,
        "hiding the run drops what was taken for it, rather than keeping an \
         answer no later walk will re-prove"
    );

    // And the file list follows on the next advance, not before it: a failed or
    // absent walk must never blank the pane on its own.
    frame.advance().expect("advance");
    assert!(
        frame.files().is_empty(),
        "with the run hidden and nothing unstaged, the pane draws the empty state"
    );
}

/// The cache key, asserted through behaviour rather than by reading it.
#[test]
fn two_entries_for_one_path_do_not_share_a_cached_diff() {
    let scratch = fixture("staged-two-keys");
    scratch.write(FILE, "one\nSTAGED\nthree\n");
    scratch.git(&["add", FILE]);
    scratch.write(FILE, "one\nSTAGED\nthree\nfour\nfive\n");

    let worktree = Worktree::discover(scratch.root()).expect("discover");
    let mut frame = worktree.frame();
    frame.show_staged(true);
    frame.advance().expect("advance");
    assert_eq!(
        frame.files().len(),
        2,
        "the fixture puts one path in both runs"
    );

    frame.diff(0).expect("first diff");
    frame.diff(1).expect("second diff");
    assert_eq!(
        frame.tracked(),
        2,
        "two rows for one path are two cached diffs; one means the key is still \
         the bare path and the two runs are reading each other's answers"
    );

    // The heights follow the same key, and `height` walks the whole changed set,
    // so a shared span is visible here even for rows nobody has drawn.
    let mut fresh = worktree.frame();
    fresh.show_staged(true);
    fresh.advance().expect("advance");
    fresh.height(|_, span| span.lines as usize).expect("height");
    assert_eq!(
        fresh.tracked_spans(),
        2,
        "and two measured spans, for the same reason"
    );
}

/// The unstaged run is untouched by any of this.
#[test]
fn drawing_the_staged_run_changes_nothing_about_the_unstaged_one() {
    let scratch = fixture("staged-no-side-effect");
    scratch.write(FILE, "one\nTWO\nthree\n");
    scratch.write(OTHER, "alpha\nBETA\n");
    scratch.git(&["add", OTHER]);

    let worktree = Worktree::discover(scratch.root()).expect("discover");

    let mut hidden = worktree.frame();
    hidden.advance().expect("advance");
    let alone: Vec<_> = hidden
        .files()
        .iter()
        .map(|change| (change.path.clone(), change.kind.clone()))
        .collect();
    let alone_diff = hidden.diff(0).expect("diff").1.clone();

    let mut shown = worktree.frame();
    shown.show_staged(true);
    shown.advance().expect("advance");
    let beside: Vec<_> = shown
        .files()
        .iter()
        .filter(|change| change.origin == Origin::Unstaged)
        .map(|change| (change.path.clone(), change.kind.clone()))
        .collect();
    let beside_diff = shown.diff(0).expect("diff").1.clone();

    assert_eq!(alone, beside, "the unstaged run is the same set either way");
    assert_eq!(
        alone_diff, beside_diff,
        "and the same diff, computed the same way"
    );
}

/// The count the empty state's second fact is drawn from.
#[test]
fn a_run_can_be_counted_without_being_walked_for_content() {
    let scratch = fixture("staged-count");
    scratch.write(FILE, "one\nSTAGED\nthree\n");
    scratch.git(&["add", FILE]);
    scratch.write(OTHER, "alpha\nUNSTAGED\n");

    let worktree = Worktree::discover(scratch.root()).expect("discover");
    assert_eq!(worktree.count_of(Origin::Staged).expect("count"), 1);
    assert_eq!(worktree.count_of(Origin::Unstaged).expect("count"), 1);

    scratch.git(&["add", OTHER]);
    assert_eq!(worktree.count_of(Origin::Staged).expect("count"), 2);
    assert_eq!(
        worktree.count_of(Origin::Unstaged).expect("count"),
        0,
        "a fully staged worktree has nothing unstaged, which is the pane going \
         blank that #313 was opened on"
    );
}

/// **The whole staged run survives the working tree being deleted**, which is the
/// same claim as the test above made over one file, made over every file and
/// without naming a counter.
#[test]
fn the_staged_run_is_unchanged_by_the_working_tree_disappearing() {
    let scratch = fixture("staged-tree-gone");
    scratch.write(FILE, "one\nSTAGED\nthree\n");
    scratch.write(OTHER, "alpha\nSTAGED TOO\n");
    scratch.write("src/added.rs", "brand new\n");
    scratch.git(&["add", "-A"]);

    let worktree = Worktree::discover(scratch.root()).expect("discover");
    let mut frame = worktree.frame();
    frame.show_staged(true);
    frame.advance().expect("advance");

    let staged: Vec<usize> = frame
        .files()
        .iter()
        .enumerate()
        .filter(|(_, change)| change.origin == Origin::Staged)
        .map(|(at, _)| at)
        .collect();
    assert_eq!(staged.len(), 3, "the fixture stages three files");

    let before: Vec<_> = staged
        .iter()
        .map(|at| {
            let (change, diff) = frame.diff(*at).expect("staged diff");
            (change.path.clone(), diff.clone())
        })
        .collect();

    // The whole working tree goes. The index is untouched, so `git diff --cached`
    // still reports every one of these and so must this.
    for change in [FILE, OTHER, "src/added.rs"] {
        scratch.remove(change);
    }

    let mut fresh = worktree.frame();
    fresh.show_staged(true);
    fresh.advance().expect("advance");
    let after: Vec<_> = fresh
        .files()
        .iter()
        .enumerate()
        .filter(|(_, change)| change.origin == Origin::Staged)
        .map(|(at, change)| {
            let path = change.path.clone();
            (at, path)
        })
        .collect();
    assert_eq!(
        after.len(),
        3,
        "the staged run lost a file when the working tree did, so it is reading \
         from disk rather than from the index"
    );

    for (at, path) in after {
        let (_, diff) = fresh.diff(at).expect("staged diff");
        let (_, want) = before
            .iter()
            .find(|(had, _)| *had == path)
            .expect("the same path is in both runs");
        assert_eq!(
            &diff.clone(),
            want,
            "{path}'s staged diff changed when the working tree was deleted, so \
             it was computed from a file rather than from the index"
        );
    }
}

/// **A sparse index yields no staged run rather than a dead pane.**
#[test]
fn a_sparse_index_leaves_the_unstaged_run_drawn() {
    let scratch = Scratch::new("staged-sparse");
    scratch.write("kept/a.txt", "one\n");
    scratch.write("dropped/b.txt", "two\n");
    scratch.git(&["add", "-A"]);
    scratch.git(&["commit", "-m", "init"]);

    // A cone-mode sparse index. Older gits do not have `--sparse-index`; if this
    // one cannot make the index sparse there is nothing to assert, so say so
    // rather than passing vacuously.
    let made = std::process::Command::new("git")
        .args(["sparse-checkout", "init", "--cone", "--sparse-index"])
        .current_dir(scratch.root())
        .output()
        .expect("run git");
    if !made.status.success() {
        eprintln!("skipped: this git cannot build a sparse index");
        return;
    }
    scratch.git(&["sparse-checkout", "set", "kept"]);

    let listed = scratch.git(&["ls-files", "--sparse"]);
    assert!(
        listed.lines().any(|line| line.ends_with('/')),
        "the fixture's index is not actually sparse, so this asserts nothing: \
         {listed:?}"
    );

    // Something staged and something not, so both runs have work in them.
    scratch.write("kept/a.txt", "one\nSTAGED\n");
    scratch.git(&["add", "kept/a.txt"]);
    scratch.write("kept/c.txt", "untracked\n");

    let worktree = Worktree::discover(scratch.root()).expect("discover");
    let mut frame = worktree.frame();
    frame.show_staged(true);
    frame.advance().expect(
        "a sparse index must not fail the walk: the pane would keep its \
                 pre-toggle contents and stop updating",
    );

    assert!(
        frame
            .files()
            .iter()
            .any(|change| change.origin == Origin::Unstaged),
        "the unstaged run went with the staged one, so a sparse checkout has no \
         pane at all"
    );
    assert!(
        !frame
            .files()
            .iter()
            .any(|change| change.origin == Origin::Staged),
        "the staged run reported changes from an index this walk cannot read"
    );
}

/// **A staged submodule bump does not take the process with it.**
#[test]
fn a_staged_gitlink_reports_a_state_rather_than_panicking() {
    let inner = Scratch::new("staged-submodule-inner");
    inner.write("f.txt", "one\n");
    inner.git(&["add", "-A"]);
    inner.git(&["commit", "-m", "init"]);

    let scratch = fixture("staged-submodule");
    // `-c protocol.file.allow=always` because git refuses local submodule adds by
    // default since CVE-2022-39253.
    let added = std::process::Command::new("git")
        .args([
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            inner.root().to_str().expect("utf-8 fixture path"),
            "sub",
        ])
        .current_dir(scratch.root())
        .output()
        .expect("run git");
    if !added.status.success() {
        eprintln!("skipped: this git will not add a local submodule");
        return;
    }

    let worktree = Worktree::discover(scratch.root()).expect("discover");
    let mut frame = worktree.frame();
    frame.show_staged(true);
    frame.advance().expect("advance");

    // **Every staged row must diff without an error, not merely without a panic.**
    // That is the claim, and asserting only "no panic" is what let a mutation
    // removing the gitlink drop survive: `try_into_blob` refuses safely either
    // way, so both arms are panic-free and only one is usable. An `Err` here is
    // not benign — `View::collect` propagates it with `?`, so the whole collect
    // fails, the shell keeps the previous screen, and the pane freezes exactly the
    // way a sparse index made it freeze.
    for at in 0..frame.files().len() {
        let path = frame.files()[at].path.clone();
        let origin = frame.files()[at].origin;
        assert!(
            frame.diff(at).is_ok(),
            "{origin:?} {path} cannot be diffed, so a collect over this worktree \
             fails and the pane stops updating"
        );
    }

    // **And the same when the submodule is replaced by a real file**, which is the
    // case a one-sided guard lets through: the destination is an ordinary blob and
    // the *source* is the commit, so a guard that asks only about the destination
    // passes it to a read that must then refuse it. Same freeze, by the door the
    // first fix left open.
    // **Committed first**, so `HEAD` really holds the gitlink and the staged change
    // is a modification *from* it. Without that the tree has no `sub` at all and
    // the change is an ordinary addition, which is the case a one-sided guard
    // already handles: the fixture would pass while proving nothing.
    scratch.git(&["commit", "-m", "the submodule"]);
    scratch.git(&["rm", "-f", "--cached", "sub"]);
    std::fs::remove_dir_all(scratch.path_of("sub")).ok();
    scratch.write("sub", "a real file where the submodule was\n");
    scratch.git(&["add", "sub"]);

    let mut swapped = worktree.frame();
    swapped.show_staged(true);
    swapped.advance().expect("advance");
    for at in 0..swapped.files().len() {
        let path = swapped.files()[at].path.clone();
        let origin = swapped.files()[at].origin;
        assert!(
            swapped.diff(at).is_ok(),
            "{origin:?} {path} cannot be diffed after the submodule became a \
             file, so a collect over this worktree fails and the pane freezes"
        );
    }

    // And the submodule is not in the run at all, which is the shape of the fix:
    // a gitlink has no content to compare, so it is dropped at the walk rather
    // than carried to a read that must then refuse it.
    assert!(
        !frame
            .files()
            .iter()
            .any(|change| change.origin == Origin::Staged && change.path == "sub"),
        "the staged run reports a submodule, which `SPEC.md` §11.2 B5 keeps out \
         of v1 and which has no content to diff"
    );
}

/// **The run boundary is the walk's own answer, on every path out of it.**
#[test]
fn the_run_boundary_agrees_with_the_files_it_partitions() {
    let scratch = fixture("staged-boundary");
    scratch.write(FILE, "one\nSTAGED\nthree\n");
    scratch.git(&["add", FILE]);
    scratch.write(OTHER, "alpha\nUNSTAGED\n");

    let worktree = Worktree::discover(scratch.root()).expect("discover");
    let mut frame = worktree.frame();

    // A frame that has never advanced: no files, and the staged run begins at the
    // end of them, which is zero.
    assert_eq!(frame.staged_at(), 0);
    assert_eq!(frame.files().len(), 0);

    // The run hidden: everything is unstaged, so the staged run begins past the
    // end. That is the honest answer rather than a sentinel.
    frame.advance().expect("advance");
    assert_eq!(
        frame.staged_at(),
        frame.files().len(),
        "with the staged run hidden the boundary is not the end of the list"
    );

    frame.show_staged(true);
    frame.advance().expect("advance");
    boundary_agrees(&frame);

    // **Flipped but not yet walked**: the pair still describes the frame that is
    // actually in hand, which is what everything reading it between a keypress
    // and its walk depends on.
    frame.show_staged(false);
    boundary_agrees(&frame);
    frame.advance().expect("advance");
    boundary_agrees(&frame);
}

/// Everything before the boundary is unstaged and everything from it is staged.
fn boundary_agrees(frame: &Frame) {
    let at = frame.staged_at();
    assert!(
        at <= frame.files().len(),
        "the boundary is past the end of the list, so the staged count underflows"
    );
    for (index, change) in frame.files().iter().enumerate() {
        let want = if index < at {
            Origin::Unstaged
        } else {
            Origin::Staged
        };
        assert_eq!(
            change.origin, want,
            "{} sits at {index} against a boundary of {at} and is {:?}",
            change.path, change.origin
        );
    }
}
