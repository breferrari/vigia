//! The staged run: `SPEC.md` §11.2 **B17**.
//!
//! Every test here is about the same claim, from a different side: **a change is
//! in exactly the run it belongs to, and the diff drawn for it is that run's own**.
//! Both halves matter, and the second is the one with teeth. A frame that put a
//! staged file in the unstaged run would be visibly wrong within a second; a frame
//! that put it in the right run and handed back the *other* run's diff for it
//! looks perfectly ordinary and is a lie about the reader's worktree.
//!
//! The fixtures are built by real `git` for the reason `tests/support` gives: the
//! question is whether `gix` reads an index the way git wrote it, and a fixture
//! written by the library under test cannot answer that.

mod support;

use support::Scratch;
use vigia_core::{ChangeKind, FileChange, Frame, Origin, Worktree};

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
///
/// Order is asserted rather than sorted away: the runs are drawn in the order
/// `advance` concatenates them, and a reader reads down from what the agent has
/// just written to what it has already put away.
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
///
/// [#313](https://github.com/breferrari/vigia/issues/313): *"a test that fails
/// when a staged file is drawn in the unstaged view or the reverse."*
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
///
/// Staged content and a further edit on top are **two different diffs of two
/// different pairs of bytes**, so the pane draws two rows and each says what it
/// actually shows. This is what a single `MM` row cannot do.
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
    let (_, staged_diff) = frame.diff(staged).expect("staged diff");
    assert!(
        staged_diff
            .hunks
            .iter()
            .flat_map(|hunk| hunk.lines.iter())
            .any(|line| line.text.contains("STAGED")),
        "the staged run draws what is in the index"
    );
}

/// **A staged diff reads no file at all**, which is what makes the second walk
/// affordable and is the premise the whole design rests on.
///
/// **Proved by deleting the file rather than by counting bytes**, and the reason is
/// worth stating because the obvious instrument does not work.
/// [`FrameStats::bytes`] counts what the diff *compared*, which for a staged change
/// is two blobs out of the object database — so it is non-zero here and says
/// nothing about the filesystem. [`FrameStats::probes`] is a `stat` counter and it
/// is only spent where [`FileChange::maybe_symlink`] is set, so a zero there is
/// consistent with an ordinary read too.
///
/// What *is* decisive is the content. With the file gone from disk, a diff that
/// consulted the working tree would compare `HEAD`'s three lines against nothing
/// and report a whole-file **deletion**; only a diff computed from the index blob
/// reports the one line the index rewrote. So the assertion is on the answer, which no
/// implementation can produce by accident, with the syscall count beside it as
/// corroboration rather than as the claim.
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
///
/// There is no tree to compare against, so the comparison is against the empty
/// one and every indexed path is a staged addition. That is what
/// `git diff --cached` reports there, and the alternative — refusing to draw
/// because a ref would not resolve — is a monitor that has stopped doing its job.
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
///
/// A staged artefact's freshness rests on object ids the *walk* supplies, and no
/// staged walk runs while the toggle is off — so a diff kept across an off stretch
/// would be vouched for by evidence nobody re-checked, under a `HEAD` that may
/// have moved. [`Frame::show_staged`] drops both caches, and this is what fails if
/// it stops.
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
///
/// One path in both runs holds **two** diffs, not one. Keyed by path alone the
/// second entry overwrites or reads back the first, and the pane draws one run's
/// content under the other run's row: a stale pane that no budget gate can see,
/// because it is exactly as fast as a correct one.
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
///
/// The regression this guards is the quiet one: a generalisation that makes the
/// new case work by changing what the old case computes. Same worktree, same
/// assertions the rest of the suite makes about it, with the toggle on.
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
///
/// Cheap on purpose — rename tracking off, since a count does not care whether a
/// deletion and an addition are one change or two — and asked only on a frame that
/// has no diff to compute.
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
