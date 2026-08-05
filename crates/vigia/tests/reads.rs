//! I4, held against the shell rather than against the core.
//!
//! > Streams, never buffers. First paint is independent of total diff size.
//!
//! `crates/vigia-core/tests/budgets.rs` gates the engine's half: content is
//! fetched per file, so a caller *can* paint the top of a large diff without
//! reading the bottom. Nothing there stops a caller from asking for every file
//! anyway, and asking is the natural way to write a renderer. So the number that
//! matters here is what one screenful costs, and the claim is that it follows the
//! screen instead of the worktree.
//!
//! Structural, not wall-clock: exact byte counts compared across two fixtures
//! that differ only in how many files changed. That is hardware-independent and
//! takes no slack. The two-fixture shape is not decoration either. Comparing one
//! screen against the whole diff would pass against exactly the regression this
//! file exists to catch, because a screen that read every file would inflate both
//! sides and leave the ratio alone.

#[path = "../../vigia-core/tests/support/mod.rs"]
mod support;

use std::time::Instant;

use ratatui::layout::Rect;
use vigia::{
    App, Body, FileEntry, HEAT_BUCKETS, HeatBucket, LIST_ROWS, Position, Row, body_layout,
};
use vigia_core::{FrameStats, HighlightStats, Highlighter, History, Recency};

use support::{Scratch, delta, materialise, settle, settle_spans};

/// The wide fixture: enough files that reading all of them is unmistakable.
const FILES: usize = 100;
/// The narrow one. Same content per file, so the per-file cost is comparable.
///
/// **At least `LIST_ROWS`, and derived rather than written.** `SPEC.md` §11.1's
/// pinned list is one row per changed file up to a cap, so a fixture below the
/// cap draws a *shorter* list than the wide one and the two stop being
/// comparable: the equality gates here would then be reading a difference in
/// region height and calling it a difference in worktree size. Two above the cap
/// keeps it unmistakably "few" against a hundred while both screens draw the
/// same six rows.
const FEW_FILES: usize = LIST_ROWS + 2;
/// Lines per file, chosen so one file is far taller than any screen.
const LINES: usize = 500;

/// An ordinary terminal, which is where the row count comes from.
///
/// Eighty columns, so the footer is one line whatever the state and the two
/// fixtures below are compared over the same number of rows. At forty this would
/// still be honest but the row count would be one lower, which is I6's business
/// rather than I4's.
fn body() -> usize {
    layout().diff
}

/// Rows the pinned list takes on this screen.
///
/// Every read count in this file is the diff's plus this, because §11.1 makes a
/// screen two regions and each list row is one `Frame::diff`. Bounded by the
/// region and never by the changed set, which is the claim rather than an
/// accounting detail.
fn listed() -> usize {
    layout().list
}

/// The shipped split, list included.
///
/// **Every gate in this file is about what a screen costs**, and `SPEC.md` §11.1
/// makes a screen two regions. Asking for a list-free layout here would leave the
/// pinned list outside every read bound in the repo, which is §7's rule about a
/// stage a gate never calls, one region over.
fn layout() -> Body {
    body_layout(
        Rect::new(0, 0, 80, 24),
        &App::new().chrome("fixture", None),
        FILES,
    )
}

/// What one screenful cost, and what it produced.
struct Screen {
    cost: FrameStats,
    /// What highlighting that same screenful cost.
    ///
    /// I2b's caller-side half. The core will parse exactly the lines it is asked
    /// for and nothing in it stops a renderer from asking for every hunk of
    /// every file, which is the shape `SPEC.md` §7 says needs a second gate over
    /// the caller.
    highlight: HighlightStats,
    /// Files the view asked the frame for.
    read: usize,
    rows: usize,
    files: usize,
}

/// Draw one screen over a fresh fixture and report what it cost.
///
/// The frame is cold, so every file it touches is computed rather than reused.
/// That is the measurement wanted here: a warm frame reads nothing at all, which
/// would make the byte comparison pass without saying anything.
///
/// **The shell is not cold, and the two are independent.** `App::past_first_paint`
/// stands in for a monitor past its first frame, because `App::new`'s first
/// [`App::view`] draws plain (`Viewport::highlight`, I7) and the highlighting
/// half of this helper would then measure the one frame that deliberately parses
/// nothing. Priming with an extra view instead would warm the *frame* too and
/// take the reads with it, which is the comparison the other half is about.
fn one_screen(name: &str, files: usize) -> Screen {
    let scratch = Scratch::large_diff(name, files, LINES);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    assert_eq!(
        frame.files().len(),
        files,
        "fixture {name} is not {files} files"
    );

    let mut app = App::past_first_paint();
    let mut highlighter = Highlighter::new();
    let history = History::new();
    let before = frame.stats();
    let view = app
        .view(&mut frame, &mut highlighter, &history, layout())
        .expect("view");

    Screen {
        cost: delta(before, frame.stats()),
        highlight: highlighter.stats(),
        read: view.read,
        rows: view.rows.len(),
        files: view.files,
    }
}

#[test]
fn one_screenful_costs_the_same_however_much_else_changed() {
    let few = one_screen("shell-reads-few", FEW_FILES);
    let many = one_screen("shell-reads-many", FILES);

    // Non-vacuity first. Both fixtures have to have really been the size they
    // claim, and the screen has to have really been full, or the equality below
    // is two zeroes agreeing.
    assert_eq!(few.files, FEW_FILES);
    assert_eq!(many.files, FILES);
    assert_eq!(
        few.rows,
        body(),
        "the narrow fixture did not fill the screen, so nothing was measured"
    );
    assert_eq!(many.rows, body());
    assert!(
        many.cost.bytes > 0,
        "a cold screen read nothing, so the fixture has no content"
    );

    // **What a frame BUILDS still follows the window**, which is the half of I4
    // that was doing the real work and the one this file was written for. A diff
    // owns a `String` per drawn line; this is the number that bounds them.
    assert_eq!(
        few.cost.computed, many.cost.computed,
        "one screen computed {} diffs among {FEW_FILES} changed files and {} \
         among {FILES}, so what a frame builds follows the worktree rather than \
         the screen",
        few.cost.computed, many.cost.computed
    );

    // **And what it counts does not.** I4 was narrowed on 2026-08-01: the diff's
    // height is totalled over every changed file, once per tick, without
    // materialising any of it. So the byte counts genuinely differ, and asserting
    // them equal would be asserting the feature away. What is asserted instead is
    // that the difference is exactly the counting pass and nothing else: every
    // file measured once, and no file measured that the screen also diffed.
    // **Every changed file is either built or counted, exactly once.** A file the
    // screen drew is already in hand, so the counting pass takes its height from
    // the diff rather than reading again; every other file is counted and never
    // built. That partition is the narrowing stated as arithmetic, and it fails
    // in both directions: a counting pass that re-read what was drawn pushes the
    // sum over, and one that skipped a file leaves it under.
    assert_eq!(
        few.cost.computed + few.cost.measured,
        FEW_FILES as u64,
        "{} built and {} counted over {FEW_FILES} changed files",
        few.cost.computed,
        few.cost.measured
    );
    assert_eq!(
        many.cost.computed + many.cost.measured,
        FILES as u64,
        "{} built and {} counted over {FILES} changed files",
        many.cost.computed,
        many.cost.measured
    );
    assert!(
        many.cost.bytes > few.cost.bytes,
        "the wide fixture read no more than the narrow one, so the counting pass \
         is not running and none of this proves anything"
    );
}

#[test]
fn one_screenful_highlights_the_same_however_much_else_changed() {
    // I2b held against the shell rather than against the core, which is the same
    // shape `one_screenful_costs_the_same_however_much_else_changed` holds I4 in
    // and is needed for the same reason. `vigia_core::Highlighter` parses
    // exactly the lines it is asked for; asking it for every hunk of every file
    // is the natural way to write a renderer, and would satisfy every gate in
    // `vigia-core/tests/budgets.rs` while making a frame cost the worktree.
    let few = one_screen("shell-highlight-few", FEW_FILES);
    let many = one_screen("shell-highlight-many", FILES);

    // Non-vacuity: a renderer that highlighted nothing would satisfy every
    // equality below, and it would look exactly like today's screen because a
    // line with no spans is drawn plain.
    assert!(
        many.highlight.lines > 0 && many.highlight.bytes > 0,
        "one screen parsed {} lines and {} bytes, so nothing was highlighted at \
         all",
        many.highlight.lines,
        many.highlight.bytes
    );
    assert!(
        many.highlight.lines <= many.rows as u64,
        "one screen of {} rows parsed {} lines, so it is highlighting rows it \
         does not draw",
        many.rows,
        many.highlight.lines
    );

    assert_eq!(
        few.highlight.bytes, many.highlight.bytes,
        "one screen highlighted {} bytes among {FEW_FILES} changed files and {} \
         among {FILES}, so what a frame parses follows the worktree rather than \
         the screen",
        few.highlight.bytes, many.highlight.bytes
    );
    assert_eq!(
        few.highlight.lines, many.highlight.lines,
        "one screen parsed {} lines against {}",
        few.highlight.lines, many.highlight.lines
    );
    assert_eq!(
        few.highlight.parsed, many.highlight.parsed,
        "one screen parsed {} hunks against {}",
        few.highlight.parsed, many.highlight.parsed
    );
}

#[test]
fn scrolling_for_a_long_time_does_not_grow_the_highlight_cache() {
    // I3's shape, held against the shell, and it exists because a mutation
    // survived without it. `vigia_core::Highlighter` bounds itself by whatever
    // the last frame asked for, but only `View::collect` says where a frame
    // begins and ends: delete its `sweep` and the cache grows by everything ever
    // scrolled past, for as long as the process runs, with every gate in
    // `vigia-core/tests/budgets.rs` still green because they drive the
    // highlighter directly.
    const SCREENS: usize = 40;
    /// Lines apart, so each change gets a hunk of its own. Matches the constant
    /// `Scratch::sparse_edits` is documented against.
    const SPACING: usize = 20;
    /// Long enough that forty screens do not reach the end of it.
    const LONG_FILE: usize = 5_000;

    let scratch = Scratch::sparse_edits("shell-highlight-bound", 1, LONG_FILE, SPACING);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);

    let mut app = App::new();
    let mut highlighter = Highlighter::new();
    let history = History::new();
    let height = body();

    let mut most = 0usize;
    for screen in 0..SCREENS {
        app.apply(vigia::Action::Page(1), &mut frame, height)
            .expect("page down");
        let view = app
            .view(&mut frame, &mut highlighter, &history, layout())
            .expect("view");

        // Non-vacuity per screen: a scroll that ran off the end would draw a
        // short screen, cache almost nothing, and satisfy the bound by having
        // stopped rather than by being bounded.
        assert_eq!(
            view.rows.len(),
            height,
            "screen {screen} drew {} of {height} rows, so the scroll left the diff",
            view.rows.len()
        );

        most = most.max(highlighter.tracked());
        assert!(
            highlighter.tracked() <= height,
            "after {screen} screens the highlighter holds {} hunks for a \
             {height}-row body, so the cache follows what has been read rather \
             than what is on screen",
            highlighter.tracked()
        );
    }

    assert!(
        most > 0,
        "nothing was ever cached, so the bound proves nothing"
    );
    assert!(
        highlighter.stats().evicted > 0,
        "forty screens evicted nothing, so hunks are being kept after they leave"
    );
}

#[test]
fn a_screen_a_single_file_fills_reads_that_single_file() {
    // The sharper version of the claim above, and the one a reader can check
    // against the fixture by hand: each file here is five hundred rewritten
    // lines, so one file is forty times taller than the screen and the other
    // ninety-nine have no business being opened.
    let many = one_screen("shell-reads-one", FILES);

    // One for the diff, which this fixture fills with a single file, plus one
    // per pinned list row. The list's share is the region's height and not the
    // worktree's: ninety-nine files changed and six are asked for.
    assert_eq!(
        many.read,
        1 + listed(),
        "the view asked the frame for {} files to fill {} diff rows and {} list          rows",
        many.read,
        body(),
        listed()
    );
    // **And one fewer diff than asks, because the two regions overlap by a
    // file.** This is the gate for a defect that shipped and was caught by
    // measurement: `take_list` used to call `Frame::diff` for every row,
    // including rows the walk had already built. `Frame::diff` re-reads any file
    // written in the last two seconds, and the file an agent just wrote is
    // always the current file and always in the window, so the hottest file in
    // the worktree was read and diffed **twice on every frame a monitor exists
    // for**. The walk now hands its entries to the list.
    //
    // Written as a relation rather than a constant: it says "the overlap costs
    // nothing", which stays true if the region's height changes.
    assert_eq!(
        many.cost.computed,
        many.read as u64 - 1,
        "{} diffs for {} asks, so the file both regions draw was computed twice",
        many.cost.computed,
        many.read
    );

    // Bounded against the files on disk. Since I4's narrowing the bound is the
    // **changed set** rather than the window, because the height is counted for
    // every file. What it still says, and what would fail if the counting pass
    // started building rather than counting, is that each file's two sides are
    // read once and no more: a path that re-read to materialise what it had just
    // counted would double this.
    let scratch = Scratch::large_diff("shell-reads-bound", FILES, LINES);
    let on_disk = std::fs::metadata(scratch.path_of("src/mod_0.rs"))
        .expect("stat")
        .len();
    let whole = on_disk * FILES as u64;
    assert!(
        many.cost.bytes >= whole && many.cost.bytes <= whole * 3,
        "one screen read {} bytes against {FILES} files of {on_disk} bytes each, \
         which is neither their two sides nor anything close to it",
        many.cost.bytes
    );
    // And the diffing half is still the window's, which is the claim this gate is
    // named for and the one the narrowing did not touch: one file fills the diff
    // region, the pinned list adds its rows, and the file both regions show is
    // built once.
    assert_eq!(
        many.cost.computed,
        many.read as u64 - 1,
        "{} diffs built for {} asks, so a file both regions draw was built twice",
        many.cost.computed,
        many.read
    );
    assert_eq!(
        many.cost.computed + many.cost.measured,
        FILES as u64,
        "{} built and {} counted over {FILES} changed files",
        many.cost.computed,
        many.cost.measured
    );
}

#[test]
fn a_tick_re_measures_only_what_changed() {
    // **I2a's rule, held over the height walk rather than over the diff.** I4's
    // narrowing admits one count per changed file per tick; it does not admit
    // *re-reading* a file that did not change, and until
    // [#101](https://github.com/breferrari/vigia/issues/101) that is exactly what
    // every tick did. `Frame::advance` dropped the span cache whole, so each of
    // the ninety-four files this screen never draws was read from disk again,
    // forever, at 12.90ms a tick on the reference machine.
    //
    // **Why eleven read-bounding gates were green over it.** Every one of them
    // opens with `settle`, and `settle` materialises: with a `FileDiff` cached
    // for every file the walk rebuilds every span from memory and reads nothing.
    // The setup deleted the cost. `settle_spans` is the same wait without the
    // materialisation, and it exists for this gate.
    //
    // Structural on purpose: a count, so it takes no slack and fails on any
    // machine. The wall clock is
    // `budgets.rs::ticking_over_an_undrawn_worktree_holds_the_frame_budget`.
    let scratch = Scratch::large_diff("shell-reads-tick", FILES, LINES);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    let primed = settle_spans(&mut frame);

    // The premise, in the direction that would make the assertion below vacuous:
    // if the priming tick measured nothing, every file was already diffed and
    // this fixture cannot tell "re-measure everything" from "re-measure what
    // changed".
    assert_eq!(
        primed, FILES as u64,
        "priming measured {primed} of {FILES} files, so the walk is not reading \
         the worktree and there is no cost here to make incremental"
    );

    let mut app = App::past_first_paint();
    let mut highlighter = Highlighter::new();
    let history = History::new();

    // One file changes. It is the file the viewport is on, so the frame has to
    // re-diff it and therefore has to re-measure it: one is the floor, not zero.
    scratch.edit_line("src/mod_0.rs", 3, "// edited");

    let before = frame.stats();
    frame.advance().expect("advance");
    let view = app
        .view(&mut frame, &mut highlighter, &history, layout())
        .expect("view");
    let cost = delta(before, frame.stats());

    assert_eq!(view.rows.len(), body(), "the screen did not fill");
    assert!(
        cost.measured <= 1,
        "a tick after one edit measured {} of {FILES} files, so the height walk \
         re-reads the whole changed set on every tick rather than the files that \
         changed. {} bytes were read",
        cost.measured,
        cost.bytes
    );
}

#[test]
fn a_tick_inside_the_settle_margin_stats_each_file_once() {
    // **The lazy fingerprint, held as a count.** `reusable` refuses an unsettled
    // observation *before* it asks for a fresh print, because no `stat` can
    // rescue evidence that was never proof. Asking eagerly is the obvious way to
    // write it and doubles the syscalls in the one window that can least afford
    // them: a bulk rewrite of files nothing has drawn leaves every carried span
    // unsettled at once, so every one of them would pay a pre-check stat that the
    // rule then ignores, on top of the post-read stat it already pays.
    //
    // **Written because a mutation survived.** Reordering the two terms of that
    // `&&` is invisible to every wall-clock gate here: it costs about 1.3ms over
    // a hundred files and
    // `budgets.rs::what_a_bulk_rewrite_of_undrawn_files_costs` has
    // more headroom than that. A count does not care about headroom.
    //
    // The **second** tick after the rewrite is the one to measure. On the first,
    // the carried evidence is still settled and its print really has moved, so a
    // stat is genuinely needed to discover that; from the second onward the
    // evidence is unsettled and the stat is pure waste.
    let scratch = Scratch::large_diff("shell-reads-margin", FILES, LINES);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    let primed = settle_spans(&mut frame);
    assert_eq!(
        primed, FILES as u64,
        "priming measured {primed} of {FILES} files, so there is no walk here"
    );

    // Every print moves at once, and nothing is drawn, so nothing can be proved.
    scratch.rewrite_all(FILES, LINES, 4);
    frame.advance().expect("advance");
    frame.height(vigia::rows_of).expect("height");

    let before = frame.stats();
    frame.advance().expect("advance");
    frame.height(vigia::rows_of).expect("height");
    let cost = delta(before, frame.stats());

    // Non-vacuity: the tick has to have actually re-measured, or there was no
    // stat to be lazy about.
    assert_eq!(
        cost.measured, FILES as u64,
        "the tick measured {} of {FILES} files, so it is not inside the margin \
         and this gate is not looking at the window it names",
        cost.measured
    );
    assert!(
        cost.probes <= FILES as u64,
        "the tick took {} stat calls over {FILES} files, so each one is being \
         stat'd twice. Two causes reach this and the message used to name only \
         the first: a pre-check fingerprint the rule would have answered without \
         one, or a type probe taken for a file the status walk had already \
         classified (`FileChange::maybe_symlink`, #15). This fixture is all \
         regular files, so the second should contribute nothing",
        cost.probes
    );
}

#[test]
fn an_uncommitted_gitattributes_does_not_re_read_the_worktree_every_tick() {
    // **A `.gitattributes` is a state, not an event, and the difference is
    // #101's whole invariant.** An attributes change invalidates every cached
    // artefact, because it changes what git's clean filter does to files it does
    // not touch, so `Frame::advance` drops both caches when one arrives. Testing
    // for *presence* rather than for *change* drops them on every tick instead,
    // for as long as the file sits uncommitted in the changed set, which is the
    // ordinary state of a repository being set up or of an agent that wrote one
    // and kept working.
    //
    // Measured with the presence test in place, over this fixture: **95 files
    // re-measured and 3.7 MiB read on every tick, 19.91ms p50 against 9.16ms**.
    // That is the pre-#101 shape exactly, reintroduced by #101's own fix for a
    // different defect and invisible to every gate here, because no fixture in
    // the suite had ever ticked with an uncommitted `.gitattributes` in it:
    // `Scratch::crlf_worktree` is the only family that writes one and it commits
    // it in the same breath.
    //
    // **The third tick is the one that matters.** Two ticks cannot tell "cleared
    // on arrival" from "cleared every time", which is one tick too few and is
    // why the gate that shipped with the guard did not catch this.
    let scratch = Scratch::large_diff("shell-reads-attrs", FILES, LINES);
    scratch.write(".gitattributes", "*.rs text\n");

    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    let primed = settle_spans(&mut frame);
    assert!(
        frame
            .files()
            .iter()
            .any(|change| change.path == ".gitattributes"),
        "the fixture's `.gitattributes` is not in the changed set, so this gate \
         is not looking at the state it names"
    );
    assert!(
        primed >= FILES as u64,
        "priming measured {primed} of at least {FILES} files, so there is no \
         walk here to make incremental"
    );

    // Two idle ticks with nothing on disk moving. Both must be free: the first
    // proves the guard is not firing on presence, the second that the first was
    // not a one-off.
    for tick in 1..=2 {
        let before = frame.stats();
        frame.advance().expect("advance");
        frame.height(vigia::rows_of).expect("height");
        let cost = delta(before, frame.stats());
        assert_eq!(
            cost.measured, 0,
            "idle tick {tick} re-measured {} files with an uncommitted \
             `.gitattributes` in the changed set, so the cache guard fires on \
             the file being present rather than on it having changed, and every \
             tick pays for the whole worktree",
            cost.measured
        );
        assert_eq!(cost.bytes, 0, "idle tick {tick} read {} bytes", cost.bytes);
    }

    // And it still fires when the file actually changes, or the fix above has
    // simply turned the guard off.
    scratch.write(".gitattributes", "*.rs -text\n");
    let before = frame.stats();
    frame.advance().expect("advance");
    frame.height(vigia::rows_of).expect("height");
    let cost = delta(before, frame.stats());
    // **At least the whole worktree, not merely "something".** `> 0` is satisfied
    // by one file, and one file is what an edited `.gitattributes` re-measures on
    // its *own* account: its span is invalidated by its own moved fingerprint
    // whether or not the guard exists. Deleting the guard entirely left this at
    // `measured == 1` and green; with the guard it is 101.
    assert!(
        cost.measured >= FILES as u64,
        "editing `.gitattributes` re-measured only {} files of {FILES}, which is \
         what the attributes file's own span costs on its own. The guard is not \
         dropping the caches, so every other artefact is still computed under \
         rules that have moved",
        cost.measured
    );
}

#[test]
fn a_deleted_gitattributes_does_not_re_read_the_worktree_every_tick() {
    // **The removal case, which is the one that looks most like an attributes
    // change and is the one the guard got wrong.** A deleted `.gitattributes` is
    // in the changed set with no working-tree side, so `fingerprint` reports
    // `None`. Reading that as "cannot be proved" holds the guard open for as long
    // as the removal sits there, and every tick clears both caches: the presence
    // test the guard was rewritten to remove, back again for the commonest way an
    // attributes file changes.
    //
    // An absent file is not a racily-clean hazard, because there is no content to
    // be stale about. Its absence *is* a change, and the map comparison sees that
    // on the tick it happens and on no tick after.
    //
    // Found by review rather than by any gate here, which is worth recording: the
    // sibling test one above covers a `.gitattributes` that is written and stays,
    // and nothing covered one that goes.
    let scratch = Scratch::large_diff("shell-reads-attrs-gone", FILES, LINES);
    scratch.write(
        ".gitattributes",
        "*.rs text
",
    );
    // Only the attributes file: `commit_all` would commit the hundred edits too
    // and leave nothing changed for the walk to be incremental over.
    scratch.git(&["add", ".gitattributes"]);
    scratch.git(&["commit", "-q", "-m", "attributes"]);
    scratch.remove(".gitattributes");

    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    let primed = settle_spans(&mut frame);
    assert!(
        frame
            .files()
            .iter()
            .any(|change| change.path == ".gitattributes"),
        "the deleted `.gitattributes` is not in the changed set, so this gate is          not looking at the state it names"
    );
    assert!(
        primed >= FILES as u64,
        "priming measured {primed} of at least {FILES} files, so there is \
         no walk here to make incremental"
    );

    for tick in 1..=2 {
        let before = frame.stats();
        frame.advance().expect("advance");
        frame.height(vigia::rows_of).expect("height");
        let cost = delta(before, frame.stats());
        assert_eq!(
            cost.measured, 0,
            "idle tick {tick} re-measured {} files with a deleted \
         `.gitattributes` in the changed set, so an absent file is being \
         read as an unprovable one and every tick pays for the whole \
         worktree",
            cost.measured
        );
    }
}

#[test]
fn a_redraw_with_nothing_changed_reads_nothing() {
    // The shell's half of I2a. The core can hold a diff between frames, and a
    // renderer that fetched content its own way would waste that entirely. The
    // frame is settled first because a file written moments ago cannot be proved
    // unchanged, so it is re-read by design: that is the engine being correct,
    // not the shell being wasteful, and measuring inside the margin would report
    // one as the other.
    let scratch = Scratch::large_diff("shell-reads-idle", FEW_FILES, LINES);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    settle(&mut frame);

    let mut app = App::new();
    let mut highlighter = Highlighter::new();
    let history = History::new();
    let before = frame.stats();
    let view = app
        .view(&mut frame, &mut highlighter, &history, layout())
        .expect("view");
    let cost = delta(before, frame.stats());

    assert_eq!(view.rows.len(), body(), "the screen did not fill");
    assert_eq!(
        cost.bytes, 0,
        "an idle redraw read {} bytes, so the shell is defeating the frame path",
        cost.bytes
    );
    assert_eq!(
        cost.computed, 0,
        "an idle redraw recomputed {} diffs",
        cost.computed
    );
    // One fewer reuse than ask, and the missing one is the file the pinned list
    // took from the walk rather than from the frame. See
    // `a_screen_a_single_file_fills_reads_that_single_file` for why that is a
    // reuse worth not making: inside the settle margin the same ask is a full
    // re-read, so the walk hands its entries to the list instead.
    assert_eq!(
        cost.reused,
        view.read as u64 - 1,
        "the {} files the view asked for produced {} reuses, so some other path \
         is fetching content",
        view.read,
        cost.reused
    );
}

/// Consecutive frames driven inside the settle margin.
///
/// Enough that a shell recomputing the whole worktree would be unmistakable
/// rather than arguable.
const MARGIN_FRAMES: usize = 20;

/// Frames between rewrites, so the window cannot outrun the margin.
///
/// "Drive frames for two seconds and they will all be inside the margin" is a
/// race against the machine, not an invariant: the margin is a fixed wall-clock
/// duration and the frame rate is whatever the hardware gives. A runner slow
/// enough would settle part-way through, those frames would reuse instead of
/// recompute, and the comparison below would hold for the one reason that proves
/// nothing. Rewriting on a fixed frame count instead means every frame sampled
/// is within this many frames of a write, whatever the machine, and it keeps the
/// two fixtures doing identical work in identical order rather than however much
/// each got through in a fixed time.
const REWRITE_EVERY: usize = 5;

/// What a bulk rewrite cost one fixture over [`MARGIN_FRAMES`] frames.
struct Margin {
    cost: FrameStats,
    /// Files the last frame asked the frame path for.
    read: usize,
    /// Rows the last frame drew, so a half-empty screen cannot pass as a full one.
    rows: usize,
    /// Changed files in the whole worktree, which is the term the two fixtures
    /// are supposed to differ in.
    ///
    /// Reported rather than trusted from the argument. The guard inside
    /// [`bulk_rewrite_window`] only proves a fixture matches *its own* argument,
    /// so it is blind to the one mistake that empties this gate: both call sites
    /// asking for the same size. That was confirmed by mutation, not by reading.
    files: usize,
}

/// Draw [`MARGIN_FRAMES`] screens without ever letting the fixture settle.
///
/// Deliberately **not** settled during the window, which is the whole point.
/// Every other structural gate in this file calls [`settle`] first, so the
/// window in which the frame path can prove nothing and recomputes by design is
/// the window none of them measure (`SPEC.md` §7). The fixture is settled
/// *before* the first rewrite so the margin under test is the one those rewrites
/// open and not the one building the fixture left behind.
///
/// Each rewrite carries a new round, because a rewrite with identical bytes
/// moves the modification time but leaves the diff alone: the frame path would
/// still recompute, and the highlighter would sit idle reusing a hunk that never
/// changed. `Scratch::rewrite_all` has the measurement.
fn bulk_rewrite_window(name: &str, files: usize) -> Margin {
    let scratch = Scratch::large_diff(name, files, LINES);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    settle(&mut frame);
    assert_eq!(
        frame.files().len(),
        files,
        "fixture {name} is not {files} files"
    );

    let mut app = App::new();
    let mut highlighter = Highlighter::new();
    let history = History::new();

    let before = frame.stats();
    let mut read = 0;
    let mut rows = 0;
    let mut files_seen = 0;
    for at in 0..MARGIN_FRAMES {
        if at % REWRITE_EVERY == 0 {
            scratch.rewrite_all(files, LINES, at / REWRITE_EVERY + 1);
        }
        frame.advance().expect("advance");
        let view = app
            .view(&mut frame, &mut highlighter, &history, layout())
            .expect("view");
        read = view.read;
        rows = view.rows.len();
        files_seen = view.files;
    }

    Margin {
        cost: delta(before, frame.stats()),
        read,
        rows,
        files: files_seen,
    }
}

#[test]
fn a_bulk_rewrite_recomputes_only_what_is_drawn() {
    // I4 held through the settle margin, which is where `SPEC.md` §10 claimed
    // the frame budget broke. It does break, over the *core* frame path, whose
    // fixture materialises every file so its own gate cannot pass vacuously. The
    // shell is a different shape and this is the assertion that says so: a
    // formatter or a branch switch invalidates every diff at once, and for the
    // whole margin nothing can be proved unchanged, so a shell that fetched
    // ahead would recompute a hundred files a frame for two seconds.
    //
    // Structural rather than wall-clock, for the reason the file header gives,
    // and two fixtures rather than one for the reason `one_screenful_costs_the_
    // same_however_much_else_changed` gives: a shell reading everything inflates
    // both sides of a single-fixture ratio and leaves it alone.
    let few = bulk_rewrite_window("shell-bulk-few", FEW_FILES);
    let many = bulk_rewrite_window("shell-bulk-many", FILES);

    // The two fixtures have to have really been different sizes. Without this the
    // whole two-fixture form is decorative: point both call sites at the same
    // count and every equality below still holds, which is not a hypothetical.
    assert_eq!(
        few.files, FEW_FILES,
        "the narrow fixture is not {FEW_FILES} files"
    );
    assert_eq!(many.files, FILES, "the wide fixture is not {FILES} files");

    // Non-vacuity, and this is the guard the gate actually needs. Frames that
    // settled would reuse rather than recompute, both sides would read zero, and
    // the equality below would hold for the one reason that proves nothing.
    // `REWRITE_EVERY` is what keeps that from being a race, and this is what says
    // it worked rather than assuming it.
    assert!(
        many.cost.computed >= MARGIN_FRAMES as u64,
        "{} diffs were recomputed across {MARGIN_FRAMES} frames, so frames \
         settled between rewrites and this gate measured settled frames",
        many.cost.computed
    );
    assert_eq!(
        few.rows,
        body(),
        "the narrow fixture did not fill the screen"
    );
    assert_eq!(
        many.rows,
        body(),
        "the wide fixture did not fill the screen"
    );

    assert_eq!(
        few.cost.computed, many.cost.computed,
        "{MARGIN_FRAMES} frames inside the margin recomputed {} diffs among \
         {FEW_FILES} changed files and {} among {FILES}, so what a frame \
         recomputes while nothing can be proved unchanged follows the worktree \
         rather than the screen",
        few.cost.computed, many.cost.computed
    );
    assert_eq!(
        few.cost.bytes, many.cost.bytes,
        "{MARGIN_FRAMES} frames inside the margin read {} bytes among \
         {FEW_FILES} changed files and {} among {FILES}",
        few.cost.bytes, many.cost.bytes
    );
    assert_eq!(
        few.read, many.read,
        "the view asked for {} files on the narrow fixture and {} on the wide \
         one",
        few.read, many.read
    );
}

#[test]
fn resolving_the_scroll_position_is_paid_once_and_not_every_frame() {
    // Why `App::view` writes `View::top` back. A position that overruns its file
    // has to be resolved by walking the files it crosses, and a shell that threw
    // the answer away would repeat that walk on every frame for as long as the
    // reader stayed there: with a hundred files that is a hundred `stat` calls a
    // frame to draw one.
    let scratch = Scratch::large_diff("shell-reads-resolve", FILES, 1);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    settle(&mut frame);

    let mut app = App::new();
    let mut highlighter = Highlighter::new();
    let history = History::new();
    // Each file here is one rewritten line: a file row, a hunk row and two
    // content rows. Scrolling by forty files' worth of rows lands well inside the
    // list rather than at either end.
    let span = vigia::rows_in(&mut frame, 0).expect("rows");
    assert_eq!(span, 4, "the fixture is not one line per file");
    app.apply(
        vigia::Action::Scroll((span * 40) as isize),
        &mut frame,
        body(),
    )
    .expect("scroll");

    let crossing = app
        .view(&mut frame, &mut highlighter, &history, layout())
        .expect("view");
    assert_eq!(
        crossing.top,
        Position { file: 40, row: 0 },
        "the scroll did not land where the fixture says it should"
    );
    assert!(
        crossing.read > 40,
        "the crossing frame read {} files, so it never walked to file 40 at all",
        crossing.read
    );

    // Ceiling, not division: the last file on screen is usually a partial one,
    // and it is still a file the frame had to be asked for.
    let drawn = body().div_ceil(span) + listed();
    let settled = app
        .view(&mut frame, &mut highlighter, &history, layout())
        .expect("view");
    assert_eq!(settled.top, crossing.top, "the position drifted while idle");
    assert_eq!(
        settled.read, drawn,
        "the frame after the scroll read {} files rather than the {drawn} it draws, \
         so the resolved position was thrown away",
        settled.read
    );
}

#[test]
fn a_taller_screen_reads_more_files_and_a_shorter_one_reads_fewer() {
    // The other direction of the same claim, and the one that catches a view
    // hardcoded to a single file. Without it, a renderer that only ever drew
    // `files[0]` would pass every assertion above.
    let scratch = Scratch::large_diff("shell-reads-height", FILES, 1);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);

    let mut app = App::new();
    let mut highlighter = Highlighter::new();
    let history = History::new();
    let span = 4;
    for height in [span, span * 2, span * 5] {
        // **List-free deliberately.** This gate is about the *diff* walk reading
        // in proportion to the rows it was given, and a pinned list would add a
        // constant to both sides that hides exactly the hardcoded-single-file
        // regression it exists to catch.
        let view = app
            .view(
                &mut frame,
                &mut highlighter,
                &history,
                Body::diff_only(height),
            )
            .expect("view");
        assert_eq!(
            view.read,
            height / span,
            "a {height}-row screen over {span}-row files read {} files",
            view.read
        );
        assert_eq!(view.rows.len(), height);
    }
}

#[test]
fn a_full_history_costs_the_frame_no_read_and_no_probe() {
    // I10 sits on the frame path now: every drawn heading asks the store what
    // that file's churn and recency are. This is the assertion that the answer
    // is free.
    //
    // It has to be free rather than cheap. I2a's whole claim is that a
    // revalidated frame reads **zero** bytes, and I4's is that the shell touches
    // only the files it draws. A store that answered by `stat`-ing, or by
    // reading, would break both while looking like a rendering detail, and the
    // failure would be invisible to every gate above: they all run with an empty
    // history, where a lookup that costs something is never made.
    //
    // Compared against the same frame drawn with an empty store rather than
    // against a literal, so the two runs differ in exactly one thing.
    let scratch = Scratch::large_diff("shell-reads-history", FEW_FILES, LINES);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    settle(&mut frame);

    let mut app = App::new();
    let mut highlighter = Highlighter::new();

    let empty = History::new();
    let before = frame.stats();
    let cold = app
        .view(&mut frame, &mut highlighter, &empty, layout())
        .expect("view");
    let without = delta(before, frame.stats());

    // Every path in the diff, recorded, so every drawn heading resolves to a
    // real entry rather than falling out of the store as untracked. A history
    // that answered `None` everywhere would be the cheap case, not the measured
    // one.
    let mut full = History::new();
    let paths: Vec<String> = frame
        .files()
        .iter()
        .map(|change| change.path.clone())
        .collect();
    full.record(paths.iter().map(String::as_str), Instant::now());
    assert!(full.tracked() > 0, "the store recorded nothing");

    let before = frame.stats();
    let warm = app
        .view(&mut frame, &mut highlighter, &full, layout())
        .expect("view");
    let with = delta(before, frame.stats());

    assert_eq!(cold.rows.len(), body(), "the screen did not fill");
    assert_eq!(
        warm.rows.len(),
        cold.rows.len(),
        "the two frames drew different screens, so their costs are not comparable"
    );
    assert_eq!(
        (with.bytes, with.probes, with.computed),
        (without.bytes, without.probes, without.computed),
        "a populated history changed the frame's cost from {without:?} to \
         {with:?}, so glance state is being answered from the filesystem"
    );
    assert_eq!(with.bytes, 0, "the frame read {} bytes", with.bytes);

    // Non-vacuity: the store has to have actually been consulted, or this
    // compares two identical empty lookups. The drawn headings carry the
    // recency the store holds, and with every path just recorded that is the
    // pulse.
    let pulsing = warm
        .rows
        .iter()
        .filter(|row| {
            matches!(
                row,
                Row::File(FileEntry {
                    recency: Recency::Pulse,
                    ..
                })
            )
        })
        .count();
    assert!(
        pulsing > 0,
        "no drawn heading took its recency from the store, so this gate compared \
         two frames that never consulted one"
    );
}

#[test]
fn a_heat_strip_is_drawn_from_a_reused_diff_without_reading() {
    // The whole of #39's risk, as one assertion. Locating change *within* a file
    // needs that file's total line count, and `SPEC.md` §5.2 records that as a
    // whole-file read: measured per frame it puts back exactly what I2a took
    // out.
    //
    // It is free instead, because `hunk::compute` interns both sides to diff
    // them at all and the count falls out of that. So it rides the cached
    // `FileDiff` and a revalidated frame still reads zero bytes.
    //
    // The byte assertion alone is satisfied by drawing no strip at all, so the
    // non-vacuity half below is what makes this a gate rather than a hope.
    let scratch = Scratch::large_diff("shell-reads-heat", FEW_FILES, LINES);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    settle(&mut frame);

    let mut app = App::new();
    let mut highlighter = Highlighter::new();
    let history = History::new();

    let before = frame.stats();
    let view = app
        .view(&mut frame, &mut highlighter, &history, layout())
        .expect("view");
    let cost = delta(before, frame.stats());

    assert_eq!(
        cost.bytes, 0,
        "drawing a heat strip over a settled frame read {} bytes, so the line \
         count is being measured rather than carried",
        cost.bytes
    );
    assert_eq!(
        cost.computed, 0,
        "the frame recomputed a diff it could reuse"
    );

    // And the strip has to have something in it. A renderer that handed back an
    // all-cool strip would pass every line above while showing a reader nothing.
    let headings: Vec<&[HeatBucket; HEAT_BUCKETS]> = view
        .rows
        .iter()
        .filter_map(|row| match row {
            Row::File(FileEntry { heat, .. }) => Some(heat),
            _ => None,
        })
        .collect();
    assert!(
        !headings.is_empty(),
        "the screen drew no file heading at all"
    );
    assert!(
        headings
            .iter()
            .any(|heat| heat.iter().any(|bucket| bucket.total() > 0)),
        "every drawn heading has an empty heat strip, so the projection ran on a \
         reused diff and found nothing to place"
    );
}

#[test]
fn the_branch_is_read_only_on_a_frame_that_will_draw_it() {
    // I4 for the one read the empty state added: never touch a file the frame
    // does not draw. Only a worktree with nothing in it names a branch, so a
    // frame with a diff must not go near `.git/HEAD`.
    //
    // **Driven by real frames rather than by a file count**, and that is the
    // whole reason this lives here instead of beside the code. When
    // `branch_for` took a count, the expression producing it sat at the call
    // site inside `Shell::draw`, which owns a terminal and which no test can
    // drive. Hardcoding that argument to `0` and to `1` both passed the entire
    // suite while the unit tests of the rule itself stayed green: the mutations
    // killed the consumer and never touched the producer. Taking the frame
    // moves the count inside the boundary, so what is asserted here is what
    // production computes.
    //
    // Counted rather than asserted on the result, because the failure is
    // invisible in the answer: returning `None` after reading and returning
    // `None` without reading are the same value at a different price.
    let clean = Scratch::new("shell-branch-clean");
    clean.write("a.txt", "one\n");
    clean.commit_all("first");
    let clean_tree = clean.worktree();
    let mut empty = clean_tree.frame();
    empty.advance().expect("advance");
    assert!(
        empty.files().is_empty(),
        "the clean fixture has a diff in it, so this proves nothing"
    );

    let dirty = Scratch::large_diff("shell-branch-dirty", FEW_FILES, LINES);
    let dirty_tree = dirty.worktree();
    let mut populated = dirty_tree.frame();
    populated.advance().expect("advance");
    assert_eq!(populated.files().len(), FEW_FILES, "fixture is not dirty");

    let reads = std::cell::Cell::new(0usize);
    let count = || {
        reads.set(reads.get() + 1);
        Some("main".to_owned())
    };

    assert_eq!(vigia::branch_for(&empty, count), Some("main".to_owned()));
    assert_eq!(reads.get(), 1, "the empty state did not ask for a branch");

    assert_eq!(vigia::branch_for(&populated, count), None);
    assert_eq!(
        reads.get(),
        1,
        "a frame with a diff in it read HEAD for a line it will not draw"
    );
}

#[test]
fn the_file_list_reads_only_the_rows_it_draws() {
    // The region's own I4 claim, stated directly rather than inferred from the
    // whole screen's byte count. `SPEC.md` §11.1 bounds the pinned list by its
    // height and never by the changed set, so doubling the worktree must not
    // move this number by one.
    //
    // Two fixtures both far past the cap, which is what makes the comparison an
    // equality rather than a ratio: below the cap the region is genuinely
    // shorter and a difference would mean nothing.
    let small = one_screen("shell-list-small", 200);
    let large = one_screen("shell-list-large", 400);

    assert_eq!(small.files, 200);
    assert_eq!(large.files, 400);

    assert_eq!(
        small.read, large.read,
        "one screen asked for {} files among 200 changed and {} among 400, so \
         the list follows the worktree rather than its own height",
        small.read, large.read
    );
    // Bytes are **not** equal any more and must not be asserted so: I4's
    // narrowing means the height is counted for every changed file, and there are
    // twice as many. What stays bounded by the region is the diffing, above, and
    // the counting scales with the changed set exactly once.
    assert_eq!(
        large.cost.bytes,
        small.cost.bytes * 2,
        "counting four hundred files read {} bytes against {} for two hundred,          so the counting pass is not linear in the changed set and something          else is reading",
        large.cost.bytes,
        small.cost.bytes
    );

    // And the number is the one the region actually asks for, not merely a
    // number that happens to match. One file fills this diff, so everything
    // above one is the list.
    assert_eq!(
        small.read,
        1 + listed(),
        "the screen asked for {} files, which is not one diff file plus {} list \
         rows",
        small.read,
        listed()
    );
}

#[test]
fn a_list_row_for_an_unchanged_file_reads_no_bytes() {
    // The half the issue asks for by name: a pinned row for a file the frame
    // already holds costs a `stat` and a cache hit, never a read. Under I2a that
    // is what makes the region affordable at all, because a monitor sits on a
    // settled tree for most of its life and the list is on screen the whole time.
    //
    // Settled first, deliberately. A file written moments ago cannot be proved
    // unchanged and is re-read by design, so measuring inside the margin would
    // report the engine being correct as the region being wasteful. The margin
    // gets its own gate below.
    let scratch = Scratch::large_diff("shell-list-idle", FILES, LINES);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    settle(&mut frame);

    let mut app = App::new();
    let mut highlighter = Highlighter::new();
    let history = History::new();

    // One view to warm whatever this screen touches, then measure the next.
    let warm = app
        .view(&mut frame, &mut highlighter, &history, layout())
        .expect("view");
    assert_eq!(
        warm.list.len(),
        listed(),
        "the fixture did not fill the list, so there are no rows to prove \
         anything about"
    );
    // Non-vacuity that matters here: the list has to be showing files the diff
    // region never draws, or this measures one file twice and calls it a region.
    assert!(
        warm.list.len()
            > warm
                .rows
                .iter()
                .filter(|row| matches!(row, Row::File(_)))
                .count(),
        "every listed file is also drawn in the diff, so no row is standing in \
         for an undiffed file"
    );

    let before = frame.stats();
    let again = app
        .view(&mut frame, &mut highlighter, &history, layout())
        .expect("view");
    let cost = delta(before, frame.stats());

    assert_eq!(
        again.list.len(),
        listed(),
        "the second view drew a short list"
    );
    assert_eq!(
        cost.bytes, 0,
        "a redraw of a settled tree read {} bytes for a list of files that did \
         not change",
        cost.bytes
    );
    assert_eq!(
        cost.computed, 0,
        "a redraw of a settled tree recomputed {} diffs",
        cost.computed
    );
}

#[test]
fn the_list_reads_no_more_when_its_window_has_moved() {
    // A gate over a windowed view taken only at the origin measures the cheapest
    // case, and this region has an origin worth leaving: at `list_top` zero the
    // walk starts on file zero, which is also where the diff starts, so the two
    // regions overlap and the list's own reads are hidden behind the body's.
    //
    // Driven to a deep window by scrolling the diff, since the window tracks it.
    let scratch = Scratch::large_diff("shell-list-window", FILES, 1);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);

    let mut app = App::new();
    let mut highlighter = Highlighter::new();
    let history = History::new();

    let at_origin = app
        .view(&mut frame, &mut highlighter, &history, layout())
        .expect("view");
    assert_eq!(
        at_origin.list_top, 0,
        "the fixture did not start at the top"
    );

    // Four rows a file in this fixture, and far enough in that the window has
    // long since left the top.
    for _ in 0..4 * 60 {
        app.apply(vigia::Action::Scroll(1), &mut frame, body())
            .expect("apply");
    }
    // **One view to resolve, then measure the next.** A position that has been
    // scrolled without being drawn names row 240 of file zero, so the first walk
    // crosses sixty files to find out where that is. That cost is real and is
    // already gated by `resolving_the_scroll_position_is_paid_once_and_not_every_
    // frame`; measuring it here would be that gate again wearing this one's name,
    // and would say nothing about the region.
    app.view(&mut frame, &mut highlighter, &history, layout())
        .expect("view");

    let before = frame.stats();
    let deep = app
        .view(&mut frame, &mut highlighter, &history, layout())
        .expect("view");
    let cost = delta(before, frame.stats());

    assert!(
        deep.list_top > 0,
        "the window never left the top, so this measured the origin twice"
    );
    assert_eq!(
        deep.read, at_origin.read,
        "the window at {} read {} files where the window at zero read {}",
        deep.list_top, deep.read, at_origin.read
    );
    // Bounded by what this screen asked for rather than by a constant. The
    // fixture is one line a file, so the diff region draws several files and not
    // the single tall one the other gates here use; a hardcoded bound would be
    // describing that fixture instead of the rule.
    assert!(
        cost.computed <= deep.read as u64,
        "a frame with a moved window recomputed {} diffs while asking for {} \
         files, so something is being recomputed that is not drawn",
        cost.computed,
        deep.read
    );
    assert!(
        deep.read < FILES,
        "the screen asked for {} of {FILES} files, which is the whole worktree",
        deep.read
    );
}

#[test]
fn a_list_inside_the_settle_margin_reads_only_what_it_draws() {
    // Every structural gate in this file opens with `settle()`, which means the
    // one window where the engine deliberately does the most work per frame is
    // the one window none of them enters. That was worth a rule in `SPEC.md` §7
    // for the diff; the region is new and owes the same proof.
    //
    // Inside the margin a file written moments ago is re-read every frame by
    // design, so the claim is not "reads nothing". It is that what it re-reads
    // is bounded by the two regions rather than by the worktree, which is the
    // same two-fixture shape the rest of this file uses.
    let few = margin_screen("shell-list-margin-few", FEW_FILES);
    let many = margin_screen("shell-list-margin-many", FILES);

    assert!(
        many > 0,
        "nothing was recomputed inside the margin, so the fixture left it"
    );
    assert_eq!(
        few, many,
        "inside the settle margin one screen recomputed {few} diffs among \
         {FEW_FILES} changed files and {many} among {FILES}",
    );
}

/// Diffs recomputed by one screen on a fixture that has just been written.
///
/// No `settle()`, which is the whole point: the frame cannot prove anything
/// unchanged, so every file it is asked about is recomputed and the count is
/// exactly "how many files did this screen ask for".
fn margin_screen(name: &str, files: usize) -> u64 {
    let scratch = Scratch::large_diff(name, files, LINES);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");

    let mut app = App::new();
    let mut highlighter = Highlighter::new();
    let history = History::new();
    let before = frame.stats();
    app.view(&mut frame, &mut highlighter, &history, layout())
        .expect("view");
    delta(before, frame.stats()).computed
}

/// What totalling the diff's rows would cost, which is what a row-exact
/// scrollbar needs.
///
/// **Ignored, diagnostic, not a gate.** It measures the thing `SPEC.md` §11.1
/// rules out so the ruling rests on a number rather than on an argument, and so
/// the number can be re-taken when the question comes back. Run it with:
///
/// ```text
/// cargo test --release --test reads -- --ignored --nocapture what_a_row_exact_scrollbar_would_cost
/// ```
///
/// The quantity a scrollbar needs is every changed file's **row count**, which is
/// `span_of(kind, diff)` and therefore the whole diff. There is no by-product to
/// reach for: `FileDiff` carries `lines`, `added` and `removed` already, but only
/// for files something has diffed, and the un-diffed ones are by definition the
/// paths that did not run.
#[test]
#[ignore = "diagnostic, not a gate"]
fn what_a_row_exact_scrollbar_would_cost() {
    for (label, files, lines) in [
        ("a working session", 20usize, 200usize),
        ("a formatter ran", 200, 200),
        ("the I4 fixture", FILES, LINES),
    ] {
        let scratch = Scratch::large_diff(&format!("cost-{files}-{lines}"), files, lines);
        let worktree = scratch.worktree();
        let mut frame = worktree.frame();
        frame.advance().expect("advance");

        // Built outside the timer, or the figure carries `Highlighter::new`'s
        // grammar loading and stops being a screen's cost.
        let mut app = App::new();
        let mut highlighter = Highlighter::new();
        let history = History::new();

        // What one screen costs today: the diff's file plus the region's rows.
        // A second screen, so the first one's cold-start is not in the number.
        app.view(&mut frame, &mut highlighter, &history, layout())
            .expect("view");
        let before = frame.stats();
        let began = Instant::now();
        app.view(&mut frame, &mut highlighter, &history, layout())
            .expect("view");
        let screen = began.elapsed();
        let screen_cost = delta(before, frame.stats());

        // Totalling through full diffs, which is what the obvious version does.
        // Its own frame, so the counts-only run below is not handed a cache this
        // one warmed.
        let mut naive_frame = worktree.frame();
        naive_frame.advance().expect("advance");
        let began = Instant::now();
        let mut total = 0usize;
        for index in 0..files {
            total += vigia::rows_in(&mut naive_frame, index).expect("rows");
        }
        let naive = began.elapsed();

        // And through the counts-only path, on a frame that has drawn one screen
        // exactly as the shipped one has.
        let before = frame.stats();
        let began = Instant::now();
        let counted = frame.height(vigia::rows_of).expect("height");
        let cold = began.elapsed();
        let cold_cost = delta(before, frame.stats());

        // Carried across ticks and re-proved by a `stat`, which is what makes
        // scrolling free.
        let began = Instant::now();
        frame.height(vigia::rows_of).expect("height");
        let warm = began.elapsed();

        assert_eq!(
            counted, total,
            "{label}: counting and diffing disagree about the diff's height"
        );

        println!(
            "{label}: {files} files x {lines} lines, {total} diff rows\n  \
             one screen        {screen:>10.2?}   {} files  {} bytes\n  \
             total, full diffs {naive:>10.2?}\n  \
             total, counted    {cold:>10.2?}   {} measured  {} bytes\n  \
             total, cached     {warm:>10.2?}   (until the next tick)",
            screen_cost.computed, screen_cost.bytes, cold_cost.measured, cold_cost.bytes,
        );
    }
}

#[test]
fn a_tick_recounts_the_height_and_a_redraw_does_not() {
    // **Both halves, because each is a different failure.** A span is derived
    // from content, so carrying one across a tick would leave a scrollbar scaled
    // against a diff that no longer exists — silently, and for as long as nothing
    // else touched that file. And recounting on every *paint* would put the one
    // unbounded walk in the frame path, which is exactly what I4's narrowing says
    // it does not do.
    //
    // Found by mutation: deleting the cache's clear in `Frame::advance` left the
    // whole suite green, which is the tell that nothing was asserting either way.
    // **Deliberately not `settle`d**, and that is what makes this gate mean
    // anything: `settle` materialises every file, so a frame that recounted on
    // every paint would take every span from a cached diff and the counters would
    // not move. The fixture is wide enough that the screen draws a handful and
    // leaves the rest un-diffed, which is the only state where "counted once per
    // tick" and "counted once per frame" produce different numbers.
    let scratch = Scratch::large_diff("shell-height-tick", FILES, LINES);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");

    let mut app = App::new();
    let mut highlighter = Highlighter::new();
    let history = History::new();

    let before = frame.stats();
    let first = app
        .view(&mut frame, &mut highlighter, &history, layout())
        .expect("view")
        .total_rows;
    let cold = delta(before, frame.stats());
    assert!(first > 0, "the fixture has no height to count");
    assert!(
        cold.measured > 0,
        "the first frame counted nothing, so every file was already diffed and          this fixture cannot tell a per-tick count from a per-frame one"
    );

    // A redraw with nothing changed counts nothing again.
    let before = frame.stats();
    let again = app
        .view(&mut frame, &mut highlighter, &history, layout())
        .expect("view");
    let idle = delta(before, frame.stats());
    assert_eq!(again.total_rows, first, "an idle redraw changed the height");
    assert_eq!(
        idle.measured, 0,
        "an idle redraw counted {} files again, so the height is being recomputed          per frame rather than per tick",
        idle.measured
    );

    // Now grow the diff and tick. The height has to follow, which it cannot do
    // from a cache carried across the advance.
    scratch.rewrite_all(FILES, LINES * 2, 7);
    frame.advance().expect("advance");
    let grown = app
        .view(&mut frame, &mut highlighter, &history, layout())
        .expect("view");
    assert!(
        grown.total_rows > first,
        "the diff doubled and the height stayed at {first}, so a stale span          survived the tick"
    );
}

#[test]
fn the_position_counts_the_rows_above_it_including_part_of_a_file() {
    // `rows_above` is the scrollbar's position, and it is two terms: every file
    // before the one the viewport is in, plus how far into that file it has
    // scrolled. Mutation found the second term untested — dropping it left every
    // gate green while the thumb stopped moving inside a file.
    const FILES: usize = 12;
    const SPAN: usize = 4;

    let scratch = Scratch::large_diff("shell-rows-above", FILES, 1);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);

    let mut app = App::new();
    let mut highlighter = Highlighter::new();
    let history = History::new();
    let split = layout();

    let mut seen = Vec::new();
    for step in 0..SPAN * 3 {
        let view = app
            .view(&mut frame, &mut highlighter, &history, split)
            .expect("view");
        seen.push(view.rows_above);
        assert_eq!(
            view.rows_above,
            view.top.file * SPAN + view.top.row,
            "step {step}: {} rows above a position of {:?} over {SPAN}-row files",
            view.rows_above,
            view.top
        );
        app.apply(vigia::Action::Scroll(1), &mut frame, split.diff)
            .expect("apply");
    }

    // Strictly increasing, which is what says the within-file term is there: a
    // position counting whole files only would repeat each value SPAN times.
    assert!(
        seen.windows(2).all(|pair| pair[0] < pair[1]),
        "the position did not move on every row: {seen:?}"
    );
}
