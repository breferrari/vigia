//! Where the pane stands: `SPEC.md` §11.1's position.

mod support;

use support::Scratch;
use vigia_core::{ChangeKind, Frame, Hidden, Position, Worktree};

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
    assert_eq!(*frame.position(), Position::Current);
    frame.advance().expect("advance");

    assert_eq!(paths(&frame), vec![KEPT.to_owned()]);
    assert_eq!(Position::Current.label(), "current");
    assert_eq!(Position::Current.at(), None);
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
        Position::Since {
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
    frame.stand(Position::Since { at, named });
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
    frame.stand(Position::Since { at, named });
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
fn a_path_added_and_then_deleted_reads_as_removed_rather_than_added() {
    let scratch = branched("position-cancel");
    scratch.write("src/gone.rs", "temporary\n");
    scratch.git(&["add", "-A"]);
    scratch.git(&["commit", "-m", "add a file"]);
    std::fs::remove_file(scratch.root().join("src/gone.rs")).expect("remove");

    let worktree = Worktree::discover(scratch.root()).expect("discover");
    let (at, named) = worktree.branch_point().expect("a branch point");
    let mut frame = worktree.frame();
    frame.stand(Position::Since { at, named });
    frame.advance().expect("advance");

    let gone = frame
        .files()
        .iter()
        .find(|change| change.path == "src/gone.rs")
        .expect("the path is in the run");
    assert_eq!(
        gone.kind,
        ChangeKind::Removed,
        "a file added on the branch and then deleted reads as added, so the pane \
         says a file exists that does not"
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
    frame.stand(Position::Since {
        at,
        named: named.clone(),
    });
    frame.advance().expect("advance");
    assert_eq!(paths(&frame).len(), 2, "the fixture committed two files");

    let mut frame = worktree.frame();
    frame.stand(Position::Since { at, named });
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

    frame.stand(Position::Since { at, named });
    assert!(
        frame.stats().evicted > held,
        "moving kept the diffs the old position produced"
    );

    frame.advance().expect("advance");
    assert_eq!(paths(&frame), vec![KEPT.to_owned(), OTHER.to_owned()]);

    // And standing where it already stands is not a move.
    let settled = frame.stats().evicted;
    frame.stand(frame.position().clone());
    assert_eq!(
        frame.stats().evicted,
        settled,
        "standing still counted as moving, so every frame would clear its caches"
    );
}
