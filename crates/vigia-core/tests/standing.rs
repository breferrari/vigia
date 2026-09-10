//! Where the pane stands: `SPEC.md` §11.1.

mod support;

use support::Scratch;
use vigia_core::{ChangeKind, Frame, Hidden, Standing, Worktree};

const KEPT: &str = "src/lib.rs";
const OTHER: &str = "src/other.rs";

/// A repository with a commit on `main`, a branch off it, and one commit on the
/// branch. The branch point is `main`'s tip.
fn branched(name: &str) -> Scratch {
    let scratch = Scratch::new(name);
    scratch.write(KEPT, "one\ntwo\n");
    scratch.write(OTHER, "alpha\n");
    scratch.git(&["add", "-A"]);
    scratch.git(&["commit", "-m", "on main"]);
    scratch.git(&["branch", "-M", "main"]);
    scratch.git(&["checkout", "-b", "work"]);
    scratch
}

/// The paths one frame holds, sorted, because the union's order is not the
/// product and asserting it would gate the wrong thing.
fn paths(frame: &Frame) -> Vec<String> {
    let mut out: Vec<String> = frame
        .files()
        .iter()
        .map(|change| change.path.clone())
        .collect();
    out.sort();
    out
}

/// Non-vacuity for every other gate in this suite and for the whole suite beside
/// it: the default position is the pane a reader has today.
#[test]
fn current_is_the_pane_the_reader_has_today() {
    let scratch = branched("position-current");
    scratch.write(KEPT, "one\nTWO\n");
    let worktree = Worktree::discover(scratch.root()).expect("discover");

    let mut frame = worktree.frame();
    assert_eq!(*frame.standing(), Standing::Current);
    frame.advance().expect("advance");

    assert_eq!(paths(&frame), vec![KEPT.to_owned()]);
    assert_eq!(Standing::Current.label(), "current");
    assert_eq!(Standing::Current.at(), None);
}

/// The point is the merge-base, and what the header draws is the branch.
#[test]
fn the_branch_point_is_the_merge_base_named_by_the_branch() {
    let scratch = branched("position-base");
    scratch.write(KEPT, "one\ncommitted on the branch\n");
    scratch.git(&["add", "-A"]);
    scratch.git(&["commit", "-m", "on the branch"]);

    let worktree = Worktree::discover(scratch.root()).expect("discover");
    let (at, named) = worktree.branch_point().expect("a branch point");

    assert_eq!(named, "main", "the header would draw a hash, not a branch");
    let tip = scratch.git(&["rev-parse", "main"]).trim().to_owned();
    assert_eq!(
        at.to_string(),
        tip,
        "the point is not where this branch left main"
    );
    assert_eq!(
        Standing::Since {
            at,
            named: named.clone()
        }
        .label(),
        "since main"
    );
}

/// The whole reason the run is a union: a commit on the branch is invisible to
/// the pane today, and that is the defect the position answers.
#[test]
fn a_since_run_holds_what_was_committed_since_the_branch_point() {
    let scratch = branched("position-since");
    scratch.write(KEPT, "one\ncommitted\n");
    scratch.git(&["add", "-A"]);
    scratch.git(&["commit", "-m", "committed on the branch"]);
    scratch.write(OTHER, "alpha\nuncommitted\n");

    let worktree = Worktree::discover(scratch.root()).expect("discover");
    let (at, named) = worktree.branch_point().expect("a branch point");

    // The pane today: the committed file is gone from it, which is the report.
    let mut live = worktree.frame();
    live.advance().expect("advance");
    assert_eq!(paths(&live), vec![OTHER.to_owned()]);

    let mut frame = worktree.frame();
    frame.stand(Standing::Since { at, named });
    frame.advance().expect("advance");
    assert_eq!(
        paths(&frame),
        vec![KEPT.to_owned(), OTHER.to_owned()],
        "the run since the branch point does not hold both what was committed \
         and what was not"
    );
}

/// The union's own case, and the reason the before side is taken from the base.
#[test]
fn a_path_changed_on_both_sides_of_the_index_appears_once_from_the_base() {
    let scratch = branched("position-both");
    scratch.write(KEPT, "one\ncommitted\n");
    scratch.git(&["add", "-A"]);
    scratch.git(&["commit", "-m", "committed"]);
    // The same file again, this time left in the working tree.
    scratch.write(KEPT, "one\ncommitted\nand then edited\n");

    let worktree = Worktree::discover(scratch.root()).expect("discover");
    let (at, named) = worktree.branch_point().expect("a branch point");
    let mut frame = worktree.frame();
    frame.stand(Standing::Since { at, named });
    frame.advance().expect("advance");

    assert_eq!(
        paths(&frame),
        vec![KEPT.to_owned()],
        "a path that moved on both sides of the index is two rows, so the pane \
         says one file changed twice rather than once"
    );

    // And what it changed *from* is the base tree, not the index: the diff has to
    // hold both lines added since the branch point, not only the later one.
    let diff = frame.diff(0).expect("a diff").1;
    let added: Vec<&str> = diff
        .hunks
        .iter()
        .flat_map(|hunk| hunk.lines.iter())
        .filter(|line| line.kind == vigia_core::LineKind::Added)
        .map(|line| line.text.trim_end())
        .collect();
    assert_eq!(
        added,
        vec!["committed", "and then edited"],
        "the before side came from the index rather than from the base tree, so \
         the row describes a change the reader never asked about"
    );
}

/// A file the branch added and then deleted was never in the base and is not on
/// disk, so it belongs to neither end.
#[test]
fn a_path_added_and_then_deleted_is_not_a_row_at_all() {
    let scratch = branched("position-cancel");
    scratch.write("src/gone.rs", "temporary\n");
    scratch.write(KEPT, "one\nkept\n");
    scratch.git(&["add", "-A"]);
    scratch.git(&["commit", "-m", "add two files"]);
    std::fs::remove_file(scratch.root().join("src/gone.rs")).expect("remove");

    let worktree = Worktree::discover(scratch.root()).expect("discover");
    let (at, named) = worktree.branch_point().expect("a branch point");
    let mut frame = worktree.frame();
    frame.stand(Standing::Since { at, named });
    frame.advance().expect("advance");

    // Non-vacuity: the walk reached this tree and kept the file beside it.
    assert_eq!(
        paths(&frame),
        vec![KEPT.to_owned()],
        "the run holds something other than the one path that actually moved"
    );
    assert!(
        !frame
            .files()
            .iter()
            .any(|change| change.path == "src/gone.rs"),
        "a file the branch added and the working tree then deleted is a row. \
         The branch point never had it and disk does not have it, so nothing \
         happened to it, and `git diff <base>` says the same"
    );
}

/// A rename the branch made survives the working tree touching the file after.
///
/// The composed kind reads off the endpoints, and *renamed from* is a fact about
/// the base end. Collapsing it to `Modified` loses the one label naming where the
/// file came from, and only on the paths busy enough to have moved on both sides
/// of the index.
#[test]
fn a_rename_on_the_branch_survives_a_later_edit_to_the_same_path() {
    let scratch = branched("position-rename");
    scratch.git(&["mv", KEPT, "src/moved.rs"]);
    scratch.git(&["commit", "-m", "move it"]);
    scratch.write("src/moved.rs", "one\ntwo\nand more\n");

    let worktree = Worktree::discover(scratch.root()).expect("discover");
    let (at, named) = worktree.branch_point().expect("a branch point");
    let mut frame = worktree.frame();
    frame.stand(Standing::Since { at, named });
    frame.advance().expect("advance");

    let moved = frame
        .files()
        .iter()
        .find(|change| change.path == "src/moved.rs")
        .expect("the moved path is in the run");
    assert_eq!(
        moved.kind,
        ChangeKind::Renamed {
            from: KEPT.to_owned()
        },
        "the run says the file was {:?}, so a reader sees an edit to a path that \
         did not exist at the branch point and no sign of where it came from",
        moved.kind
    );
}

/// A path the branch renamed and the working tree renamed again is one row.
///
/// The two walks hold it under two different names, so pairing on the drawn path
/// alone leaves the middle name standing as a row for a file that is not on disk,
/// beside a second row claiming the file came from it.
#[test]
fn a_path_renamed_twice_arrives_once_under_its_last_name() {
    let scratch = branched("position-rename-twice");
    scratch.git(&["mv", KEPT, "src/middle.rs"]);
    scratch.git(&["commit", "-m", "move it once"]);
    // On disk and not through git, so the second move is the working tree's:
    // `git mv` stages it, which puts both halves in the same walk and asks the
    // union to pair nothing.
    std::fs::rename(
        scratch.root().join("src/middle.rs"),
        scratch.root().join("src/last.rs"),
    )
    .expect("move it again");

    let worktree = Worktree::discover(scratch.root()).expect("discover");
    let (at, named) = worktree.branch_point().expect("a branch point");
    let mut frame = worktree.frame();
    frame.stand(Standing::Since { at, named });
    frame.advance().expect("advance");

    assert_eq!(
        paths(&frame),
        vec!["src/last.rs".to_owned()],
        "the run holds a name the file passed through rather than the one it has"
    );
    let moved = &frame.files()[0];
    assert_eq!(
        moved.kind,
        ChangeKind::Renamed {
            from: KEPT.to_owned()
        },
        "the row says {:?}, so it names the middle of the journey rather than \
         where the file was at the branch point",
        moved.kind
    );
}

/// A conflicted path is a conflict since the branch point too.
///
/// The tree walk reports no unmerged path, so this one never reaches `compose`
/// and does not gate it: the composition's own table is a unit test beside it,
/// named for the reason. What this asserts is the half a reader can see, that a
/// path in conflict mid-merge still reads as one from the branch point and is
/// still not diffable, since `reads_side` is what stops anything reading a side
/// it has not got.
#[test]
fn a_conflicted_path_in_a_since_run_still_reads_as_a_conflict() {
    let scratch = branched("position-conflict");
    scratch.write(KEPT, "one\nbranch\n");
    scratch.git(&["commit", "-am", "on the branch"]);
    scratch.git(&["checkout", "-q", "main"]);
    scratch.write(KEPT, "one\nmain\n");
    scratch.git(&["commit", "-am", "on main"]);
    scratch.git(&["checkout", "-q", "work"]);
    scratch.git_may_fail(&["merge", "main"]);

    let worktree = Worktree::discover(scratch.root()).expect("discover");
    let (at, named) = worktree.branch_point().expect("a branch point");
    let mut frame = worktree.frame();
    frame.stand(Standing::Since { at, named });
    frame.advance().expect("advance");

    let conflicted = frame
        .files()
        .iter()
        .find(|change| change.path == KEPT)
        .expect("the conflicted path is in the run");
    assert_eq!(
        conflicted.kind,
        ChangeKind::Conflict,
        "a conflicted path reads {:?} since the branch point, and a diffable kind \
         over a row that reads no working tree draws the base content as deleted",
        conflicted.kind
    );
    assert!(
        !conflicted.is_diffable(),
        "the row is diffable, so something will read a side it has not got"
    );
}

/// The filter lives in `Changes`, and a second walk must not go round it.
#[test]
fn the_hide_pattern_reaches_a_since_run() {
    let scratch = branched("position-hide");
    scratch.write("target/build.log", "noise\n");
    scratch.write(KEPT, "one\ncommitted\n");
    scratch.git(&["add", "-A"]);
    scratch.git(&["commit", "-m", "committed"]);

    let worktree = Worktree::discover(scratch.root()).expect("discover");
    let (at, named) = worktree.branch_point().expect("a branch point");

    let mut frame = worktree.frame();
    frame.stand(Standing::Since {
        at,
        named: named.clone(),
    });
    frame.advance().expect("advance");
    assert_eq!(paths(&frame).len(), 2, "the fixture committed two files");

    let mut frame = worktree.frame();
    frame.stand(Standing::Since { at, named });
    frame.hide(Some(Hidden::new("^target/").expect("a pattern")));
    frame.advance().expect("advance");
    assert_eq!(
        paths(&frame),
        vec![KEPT.to_owned()],
        "the second walk went round the filter, so a reader's pattern stops \
         working the moment they move the pane"
    );
    assert_eq!(frame.hidden(), 1);
}

/// A repository with one branch and no other has no point to measure from, and
/// says so rather than measuring from itself and drawing an empty pane.
#[test]
fn a_branch_with_nothing_to_measure_from_refuses_rather_than_empties() {
    let scratch = Scratch::new("position-alone");
    scratch.write(KEPT, "one\n");
    scratch.git(&["add", "-A"]);
    scratch.git(&["commit", "-m", "only commit"]);
    scratch.git(&["branch", "-M", "solo"]);

    let worktree = Worktree::discover(scratch.root()).expect("discover");
    assert!(
        worktree.branch_point().is_err(),
        "a branch with no other branch resolved a point anyway"
    );
}

/// Moving drops what the old position measured, because every diff and every
/// height under it describes a comparison the frame is no longer making.
#[test]
fn standing_somewhere_else_drops_what_the_old_position_measured() {
    let scratch = branched("position-move");
    scratch.write(KEPT, "one\ncommitted\n");
    scratch.git(&["add", "-A"]);
    scratch.git(&["commit", "-m", "committed"]);
    scratch.write(OTHER, "alpha\nedited\n");

    let worktree = Worktree::discover(scratch.root()).expect("discover");
    let (at, named) = worktree.branch_point().expect("a branch point");

    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    support::materialise(&mut frame);
    let held = frame.stats().evicted;

    frame.stand(Standing::Since { at, named });
    assert!(
        frame.stats().evicted > held,
        "moving kept the diffs the old position produced"
    );

    frame.advance().expect("advance");
    assert_eq!(paths(&frame), vec![KEPT.to_owned(), OTHER.to_owned()]);

    // And standing where it already stands is not a move.
    let settled = frame.stats().evicted;
    frame.stand(frame.standing().clone());
    assert_eq!(
        frame.stats().evicted,
        settled,
        "standing still counted as moving, so every frame would clear its caches"
    );
}
