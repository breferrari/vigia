//! The `hide` pattern at the walk: `SPEC.md` §11.1 and §11.2 B6.

mod support;

use support::Scratch;
use vigia_core::{ChangeOptions, Frame, Hidden, Origin, Worktree};

const KEPT: &str = "src/lib.rs";
const GENERATED: &str = "target/debug/build.log";
const LOCK: &str = "deps/pinned.lock";

/// A repository with one commit, then three changed files: one the reader wants
/// and two they would rather never see.
fn fixture(name: &str) -> Scratch {
    let scratch = Scratch::new(name);
    scratch.write(KEPT, "one\ntwo\n");
    scratch.write(GENERATED, "noise\n");
    scratch.write(LOCK, "pins\n");
    scratch.git(&["add", "-A"]);
    scratch.git(&["commit", "-m", "init"]);
    scratch.write(KEPT, "one\nTWO\n");
    scratch.write(GENERATED, "more noise\n");
    scratch.write(LOCK, "more pins\n");
    scratch
}

/// The paths one frame holds, in the order it reports them.
fn paths(frame: &Frame) -> Vec<String> {
    frame
        .files()
        .iter()
        .map(|change| change.path.clone())
        .collect()
}

/// The issue's own acceptance: a matching path is absent from the walk.
#[test]
fn a_matching_path_never_reaches_the_file_list() {
    let scratch = fixture("hidden-absent");
    let worktree = Worktree::discover(scratch.root()).expect("discover");
    let mut frame = worktree.frame();
    frame.hide(Some(Hidden::new(r"^target/|\.lock$").expect("a pattern")));
    frame.advance().expect("advance");

    assert_eq!(
        paths(&frame),
        vec![KEPT.to_owned()],
        "a hidden path reached the file list, so it reaches the diff and the \
         counts under it too"
    );
}

/// Whatever the pattern keeps out, the walk knows how much of it there was.
#[test]
fn the_walk_counts_what_it_hid() {
    let scratch = fixture("hidden-counted");
    let worktree = Worktree::discover(scratch.root()).expect("discover");
    let mut frame = worktree.frame();
    frame.hide(Some(Hidden::new(r"^target/|\.lock$").expect("a pattern")));
    frame.advance().expect("advance");

    assert_eq!(
        frame.hidden(),
        2,
        "the walk hid two paths and told the header a different number, which is \
         a header that lies about what it is keeping back"
    );
}

/// The pane a reader with no pattern gets, which is what makes every assertion
/// above about the pattern rather than about an empty worktree.
#[test]
fn no_pattern_hides_nothing() {
    let scratch = fixture("hidden-none");
    let worktree = Worktree::discover(scratch.root()).expect("discover");
    let mut frame = worktree.frame();
    frame.advance().expect("advance");

    assert_eq!(paths(&frame).len(), 3, "the fixture changed three files");
    assert_eq!(
        frame.hidden(),
        0,
        "nothing was hidden and something was counted"
    );
}

/// A hidden file's lines are absent from the run's total, the way a binary
/// file's are, so the figure on the right describes what the rows below show.
#[test]
fn a_hidden_files_lines_are_absent_from_the_runs_total() {
    let scratch = fixture("hidden-churn");
    // Twenty more lines into the file the pattern covers, so the two totals
    // cannot coincide by accident.
    scratch.write(GENERATED, "noise\n".repeat(20));
    let worktree = Worktree::discover(scratch.root()).expect("discover");

    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let whole = frame.churn().expect("churn").expect("every file measured");

    let mut frame = worktree.frame();
    frame.hide(Some(Hidden::new(r"^target/|\.lock$").expect("a pattern")));
    frame.advance().expect("advance");
    let shown = frame.churn().expect("churn").expect("every file measured");

    assert!(
        shown.added < whole.added,
        "the total counts lines from a file the pane never draws: {shown:?} \
         against {whole:?}"
    );
    assert_eq!(
        shown.added, 1,
        "the total is not the kept file's alone: {shown:?}"
    );
}

/// The same pattern, over the run the staged toggle adds.
#[test]
fn the_staged_run_hides_by_the_same_pattern() {
    let scratch = fixture("hidden-staged");
    scratch.git(&["add", "-A"]);
    // One unstaged change on top, so both runs are populated and the sum below is
    // over two walks rather than one.
    scratch.write(KEPT, "one\nTHREE\n");

    let worktree = Worktree::discover(scratch.root()).expect("discover");
    let mut frame = worktree.frame();
    frame.show_staged(true);
    frame.hide(Some(Hidden::new(r"^target/").expect("a pattern")));
    frame.advance().expect("advance");

    assert!(
        !paths(&frame).iter().any(|path| path == GENERATED),
        "the staged run drew a path the unstaged run hides, so a reader who \
         presses `a` gets their pattern back: {:?}",
        paths(&frame)
    );
    assert!(
        frame
            .files()
            .iter()
            .any(|change| change.origin == Origin::Staged),
        "the fixture staged nothing, so this gate walked one run and asserted \
         over an empty set"
    );
    assert_eq!(
        frame.hidden(),
        1,
        "the count is over one run rather than both"
    );
}

/// The count the empty state draws for the run the pane is not showing.
#[test]
fn a_hidden_path_is_absent_from_the_other_runs_count() {
    let scratch = fixture("hidden-count");
    scratch.git(&["add", "-A"]);

    let worktree = Worktree::discover(scratch.root()).expect("discover");
    assert_eq!(
        worktree.count_of(Origin::Staged, None).expect("count"),
        3,
        "the fixture staged three files"
    );

    let hide = Hidden::new(r"^target/|\.lock$").expect("a pattern");
    assert_eq!(
        worktree
            .count_of(Origin::Staged, Some(&hide))
            .expect("count"),
        1,
        "the count of the run the pane is not drawing ignores the pattern, so a \
         reader is told about work they asked never to see"
    );
}

/// The `hide` pattern reaches the public walk as well as the frame's, which is
/// what makes it an option on the sweep rather than a filter on one caller.
#[test]
fn the_option_reaches_the_walk_itself() {
    let scratch = fixture("hidden-option");
    let worktree = Worktree::discover(scratch.root()).expect("discover");
    let hide = Hidden::new(r"^target/").expect("a pattern");

    let mut walk = worktree
        .changes_with(ChangeOptions {
            hide: Some(&hide),
            ..ChangeOptions::default()
        })
        .expect("walk");
    let mut seen = Vec::new();
    for change in &mut walk {
        seen.push(change.expect("a change").path);
    }
    seen.sort();

    assert_eq!(seen, vec![LOCK.to_owned(), KEPT.to_owned()]);
    assert_eq!(walk.hidden(), 1);
}

/// Searched, not anchored, which is what makes the issue's own example read the
/// way it is written.
#[test]
fn a_pattern_searches_rather_than_anchors() {
    let hide = Hidden::new(r"\.lock$").expect("a pattern");
    assert!(hide.is_hidden("deps/pinned.lock"), "a suffix match failed");
    assert!(
        !hide.is_hidden("deps/pinned.lock.txt"),
        "the `$` was ignored, so the pattern is being matched as a substring \
         rather than as a regular expression"
    );

    // And with no anchor at all, the whole path is the subject.
    let anywhere = Hidden::new("target").expect("a pattern");
    assert!(anywhere.is_hidden("crates/target/x"));
    assert!(!anywhere.is_hidden("crates/src/x"));
}

/// Refused rather than dropped, so a typo is reported instead of silently
/// hiding nothing.
///
/// The refusal carries the engine's words and no sentence around them. Every
/// caller has a sentence of its own, and a wrapper here is what made the config
/// file say *is not a pattern* twice in one line.
#[test]
fn a_pattern_that_does_not_compile_is_refused() {
    let why = Hidden::new("^target/(").expect_err("an unclosed group is not a pattern");
    let said = why.to_string();
    assert!(!said.trim().is_empty(), "the refusal says nothing at all");
    assert!(
        !said.contains("is not a pattern"),
        "the refusal frames the engine's words in a sentence its caller will          say again: {said}"
    );
}

/// A matcher that cannot finish answers *no*, because the other answer hides a
/// file the reader never asked to hide.
#[test]
fn a_matcher_that_gives_up_hides_nothing() {
    // Catastrophic backtracking, which only the fancy engine can even reach: a
    // pattern with no backreference and no look-around wraps `regex-automata` and
    // runs in linear time, so this needs the backreference to be the thing under
    // test at all.
    let hide = Hidden::new(r"^(a+)+\1$").expect("a pattern");
    let subject = "a".repeat(64) + "b";
    assert!(
        !hide.is_hidden(&subject),
        "a match that ran out of budget was read as a match, so a pane hides a \
         file on a failure rather than on a decision"
    );

    // Non-vacuity: the same pattern still answers where it can.
    assert!(hide.is_hidden("aaaa"));
}

/// Two patterns are the same setting when they are the same text, which the
/// shell needs so a config can be compared with the one a reader would have had.
#[test]
fn a_pattern_is_its_text() {
    let one = Hidden::new(r"^target/").expect("a pattern");
    assert_eq!(one, Hidden::new(r"^target/").expect("a pattern"));
    assert_ne!(one, Hidden::new(r"^target").expect("a pattern"));
    assert_eq!(one.as_str(), r"^target/");
    assert!(format!("{one:?}").contains("^target/"));
}

/// The pattern holds across ticks, which is the shape the pane actually runs in.
///
/// Every other gate here advances a fresh frame once. A live pane arms the
/// pattern before the first walk and then advances the same frame for the life
/// of the session, migrating caches between ticks and re-counting both runs
/// whenever the staged toggle moves. The count is rewritten by each walk, so
/// what it says has to follow the tree rather than accumulate.
#[test]
fn a_pattern_survives_the_ticks_a_session_is_made_of() {
    let scratch = fixture("hidden-ticks");
    let worktree = Worktree::discover(scratch.root()).expect("discover");
    let mut frame = worktree.frame();
    frame.hide(Some(Hidden::new(r"^target/").expect("a pattern")));

    frame.advance().expect("advance");
    assert_eq!(frame.hidden(), 1);
    assert_eq!(paths(&frame).len(), 2);

    // A second tick over an unchanged tree says the same thing, rather than
    // adding this walk's count to the last one's.
    frame.advance().expect("advance");
    assert_eq!(
        frame.hidden(),
        1,
        "the count accumulates across ticks instead of describing this one"
    );

    // The hidden file stops being changed, and the count follows the tree down.
    scratch.git(&["checkout", "--", GENERATED]);
    frame.advance().expect("advance");
    assert_eq!(
        frame.hidden(),
        0,
        "the count kept a path the tree no longer changes"
    );
    assert_eq!(paths(&frame).len(), 2);

    // And the staged run joins mid-session, which clears both caches and walks
    // a second comparison the pattern has to reach as well.
    scratch.write(GENERATED, "staged noise\n");
    scratch.git(&["add", "-A"]);
    frame.show_staged(true);
    frame.advance().expect("advance");
    assert!(
        !paths(&frame).iter().any(|path| path == GENERATED),
        "a run added mid-session drew a path the pattern covers: {:?}",
        paths(&frame)
    );
    assert!(
        frame.hidden() >= 1,
        "a run added mid-session hid nothing at all"
    );
}

/// What a pattern costs the walk it filters, both fixtures interleaved.
///
/// Not a gate. The shipped default is no pattern, so every budget gate measures
/// the unfiltered walk, and a reader who sets one is on a path nothing bounds.
/// This is what says how far off the budget that path is: run it with
/// `--ignored --nocapture` when the matcher or its call site moves.
///
/// Interleaved rather than run in two blocks, so a machine that gets busy moves
/// both numbers rather than whichever happened to go second.
#[test]
#[ignore = "diagnostic, not a gate"]
fn what_a_pattern_costs_the_walk() {
    const ROUNDS: usize = 40;
    let scratch = Scratch::large_diff("hidden-cost", 100, 1_000);
    let worktree = Worktree::discover(scratch.root()).expect("discover");
    // Matches nothing in the fixture, so both walks return all hundred files and
    // the difference is the matching itself rather than the work it saves.
    let hide = Hidden::new(r"^target/|\.lock$").expect("a pattern");

    let walk = |hide: Option<&Hidden>| {
        let started = std::time::Instant::now();
        let mut walk = worktree
            .changes_with(ChangeOptions {
                hide,
                ..ChangeOptions::default()
            })
            .expect("walk");
        let mut n = 0;
        for change in &mut walk {
            change.expect("a change");
            n += 1;
        }
        assert_eq!(n, 100, "the fixture stopped being a hundred files");
        started.elapsed()
    };

    let (mut off, mut on) = (Vec::new(), Vec::new());
    for _ in 0..ROUNDS {
        off.push(walk(None));
        on.push(walk(Some(&hide)));
    }
    off.sort_unstable();
    on.sort_unstable();
    println!(
        "100 changed paths, median of {ROUNDS}: no pattern {:?}, pattern {:?}",
        off[ROUNDS / 2],
        on[ROUNDS / 2]
    );
}
