//! I4, held against the shell rather than against the core.

#[path = "../../vigia-core/tests/support/mod.rs"]
mod support;

use std::time::{Duration, Instant, SystemTime};

use ratatui::layout::Rect;
use vigia::{
    Action, App, Body, Deadlines, HEAT_BUCKETS, HeatBucket, LIST_SETTLED, Pointing, Position, Row,
    body_layout, diff_rows, due, patience,
};
use vigia_core::{Frame, FrameStats, HighlightStats, Highlighter, History, Recency};

use support::{Scratch, delta, materialise, settle, settle_spans};

/// The wide fixture: enough files that reading all of them is unmistakable.
const FILES: usize = 100;
/// The narrow one. Same content per file, so the per-file cost is comparable.
const FEW_FILES: usize = LIST_SETTLED + 2;
/// Lines per file, chosen so one file is far taller than any screen.
const LINES: usize = 500;

/// An ordinary terminal, which is where the row count comes from.
fn body() -> usize {
    layout().diff
}

/// Rows the pinned list takes on this screen.
fn listed() -> usize {
    layout().list
}

/// The shipped split, list included.
fn layout() -> Body {
    layout_at(ORDINARY)
}

/// An ordinary terminal, and the pane every gate in this file but one measures.
const ORDINARY: u16 = 24;

/// A pane tall enough that the list is deeper than the cap that shipped.
const DEEP: u16 = 50;

/// [`layout`] at a named pane height.
fn layout_at(height: u16) -> Body {
    layout_on(ORDINARY_WIDTH, height)
}

/// The pane every gate in this file is measured on unless it says otherwise.
const ORDINARY_WIDTH: u16 = 80;

/// A pane wide enough that the list is a rail beside the diff.
const RAIL_WIDTH: u16 = 160;

/// [`layout_at`] on a named pane.
fn layout_on(width: u16, height: u16) -> Body {
    body_layout(
        Rect::new(0, 0, width, height),
        &railed(App::new().chrome("fixture", None, Pointing::default(), 0, "")),
        FILES,
        FILES,
    )
}

/// A chrome that has asked for the rail.
fn railed(mut chrome: vigia::Chrome) -> vigia::Chrome {
    chrome.rail = true;
    chrome
}

/// What one screenful cost, and what it produced.
struct Screen {
    cost: FrameStats,
    /// What highlighting that same screenful cost.
    highlight: HighlightStats,
    /// Files the view asked the frame for.
    read: usize,
    rows: usize,
    files: usize,
}

/// Draw one screen over a fresh fixture and report what it cost.
fn one_screen(name: &str, files: usize) -> Screen {
    one_screen_at(name, files, ORDINARY)
}

/// [`one_screen`], drawn against a named pane height.
fn one_screen_at(name: &str, files: usize, height: u16) -> Screen {
    one_screen_on(name, files, ORDINARY_WIDTH, height)
}

/// [`one_screen_at`] on a named pane, so the read bound can be held beside a
/// rail as well as under a stacked list.
fn one_screen_on(name: &str, files: usize, width: u16, height: u16) -> Screen {
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
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let before = frame.stats();
    let view = app
        .view(
            &mut frame,
            &mut highlighter,
            &history,
            layout_on(width, height),
        )
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

    // What a frame BUILDS still follows the window, which is the half of I4
    // that was doing the real work and the one this file was written for. A diff
    // owns a `String` per drawn line; this is the number that bounds them.
    assert_eq!(
        few.cost.computed, many.cost.computed,
        "one screen computed {} diffs among {FEW_FILES} changed files and {} \
         among {FILES}, so what a frame builds follows the worktree rather than \
         the screen",
        few.cost.computed, many.cost.computed
    );

    // And what it counts does not.
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
    // I2b held against the shell rather than against the core, which is the same shape
    // `one_screenful_costs_the_same_however_much_else_changed` holds I4 in and is
    // needed for the same reason.
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
    // I3's shape, held against the shell, and it exists because a mutation survived
    // without it.
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
    let mut highlighter = Highlighter::eager();
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
    // And one fewer diff than asks, because the two regions overlap by a file.
    assert_eq!(
        many.cost.computed,
        many.read as u64 - 1,
        "{} diffs for {} asks, so the file both regions draw was computed twice",
        many.cost.computed,
        many.read
    );

    // Bounded against the files on disk.
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
    // I2a's rule, held over the height walk rather than over the diff.
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
    let mut highlighter = Highlighter::eager();
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
fn a_settled_worktree_and_nothing_held_means_no_timer_at_all() {
    // The invariant the settle clock is allowed under, asserted on the value the
    // loop's wait is given rather than on a behaviour observed around it, the way
    // the ageing clock's gate in `input.rs` is. Here rather than beside it because
    // this one drives a real worktree.
    let scratch = Scratch::large_diff("shell-settle-wake", 4, 40);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    settle_spans(&mut frame);
    frame.advance().expect("advance");
    let before = diff_rows(&mut frame).expect("height");
    let settling = |frame: &Frame| {
        patience(
            Deadlines {
                settling: frame.settles_in(SystemTime::now()),
                ..Deadlines::default()
            },
            Instant::now(),
        )
    };
    assert_eq!(
        settling(&frame),
        None,
        "a settled worktree and nothing held handed the loop a deadline, which is \
         a timer on an idle monitor"
    );

    // A file grows off screen, and the tick that reports it finds the file inside
    // the margin: the old height stands and the loop is asked to wake when the
    // file settles.
    scratch.write("src/mod_3.rs", "fn grown() {}\n".repeat(80));
    frame.advance().expect("advance");
    assert_eq!(
        diff_rows(&mut frame).expect("height"),
        before,
        "the height moved inside the margin, so a file still being written was read"
    );
    let armed = settling(&frame).expect(
        "a print that moved inside the margin did not arm the loop, so the total \
         stays stale until the next event",
    );
    assert!(
        armed <= Duration::from_secs(2),
        "the loop was asked to sleep {armed:?}, past the margin the wait is \
         measured against"
    );

    // The wake, and what the timeout arm does on it: the deadline is due, the
    // frame advances with no path list, and the walk reads the file once.
    std::thread::sleep(armed + Duration::from_millis(100));
    assert!(
        due(frame.settles_in(SystemTime::now())),
        "the deadline the loop slept for is not due when it wakes, so the arm \
         paints and asks again"
    );
    frame.advance().expect("advance");
    let after = diff_rows(&mut frame).expect("height");
    let mut cold = worktree.frame();
    cold.advance().expect("advance");
    let truth = diff_rows(&mut cold).expect("height");
    assert!(
        truth > before,
        "the file grew and a memoryless frame still counts {before} rows, so this \
         fixture proves nothing"
    );
    assert_eq!(
        after, truth,
        "the wake at the margin's end counted {after} rows where a frame with no \
         memory counts {truth}, so the recount waited for the next event"
    );

    // And it stops again, which is the bound the licence rests on.
    assert_eq!(
        settling(&frame),
        None,
        "every file settled and the loop is still on a clock, so it outlives the \
         thing that armed it"
    );
}

#[test]
fn a_tick_inside_the_settle_margin_stats_each_file_once() {
    // The lazy fingerprint, held as a count, and the read that waits for the margin.
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

    // Non-vacuity: the tick has to be inside the margin, which is every height kept
    // waiting and none read, or there was no stat to be lazy about.
    assert_eq!(
        cost.deferred, FILES as u64,
        "the tick kept {} of {FILES} heights waiting, so it is not inside the \
         margin and this gate is not looking at the window it names",
        cost.deferred
    );
    assert_eq!(
        cost.measured, 0,
        "the tick read {} files still being written, which the margin exists to prevent",
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

    // And the read comes once, when the files settle.
    let re_read = settle_spans(&mut frame);
    assert_eq!(
        re_read, FILES as u64,
        "the settled tick read {re_read} of {FILES} files"
    );
}

#[test]
fn an_uncommitted_gitattributes_does_not_re_read_the_worktree_every_tick() {
    // A `.gitattributes` is a state, not an event, and the difference is
    // The whole invariant: an attributes change invalidates every cached
    // artefact, because it changes what git's clean filter does to files it does
    // not touch, so `Frame::advance` drops both caches when one arrives. Testing
    // for *presence* rather than for *change* drops them on every tick instead,
    // for as long as the file sits uncommitted in the changed set, which is the
    // ordinary state of a repository being set up or of an agent that wrote one
    // and kept working.
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
        vigia::diff_rows(&mut frame).expect("height");
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
    // At least the whole worktree, not merely "something".
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
    // The removal case, which is the one that looks most like an attributes change and
    // is the one the guard got wrong.
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
    // The shell's half of I2a.
    let scratch = Scratch::large_diff("shell-reads-idle", FEW_FILES, LINES);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    settle(&mut frame);

    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
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
    // One fewer reuse than ask, and the missing one is the file the pinned list took
    // from the walk rather than from the frame.
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
const MARGIN_FRAMES: usize = 20;

/// Frames between rewrites, so the window cannot outrun the margin.
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
    files: usize,
}

/// Draw [`MARGIN_FRAMES`] screens without ever letting the fixture settle.
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
    let mut highlighter = Highlighter::eager();
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
    // I4 held through the settle margin, which is where `SPEC.md` §10 claimed the frame
    // budget broke.
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

    // Non-vacuity, and this is the guard the gate actually needs.
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
    // Why `App::view` writes `View::top` back.
    let scratch = Scratch::large_diff("shell-reads-resolve", FILES, 1);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    settle(&mut frame);

    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let block = vigia::rows_in(&mut frame, 0).expect("rows");
    assert_eq!(block, 5, "the fixture is not one line per file");
    app.apply(
        vigia::Action::Scroll((block * 40) as isize),
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
    let drawn = body().div_ceil(block) + listed();
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
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    // A whole block: heading, hunk header, two content rows and the closing
    // blank, so a screen of exactly `span` rows is exactly one file's worth.
    let block = 5;
    for height in [block, block * 2, block * 5] {
        // List-free deliberately.
        let view = app
            .view(
                &mut frame,
                &mut highlighter,
                &history,
                Body::diff_only(height),
            )
            .expect("view");
        // `span` is the whole block, the closing blank included, because that is
        // what a screen's worth of rows is spent on.
        assert_eq!(
            view.read,
            height / block,
            "a {height}-row screen over {block}-row blocks read {} files",
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
    let scratch = Scratch::large_diff("shell-reads-history", FEW_FILES, LINES);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    settle(&mut frame);

    let mut app = App::new();
    let mut highlighter = Highlighter::eager();

    let empty = History::new();
    let before = frame.stats();
    let cold = app
        .view(&mut frame, &mut highlighter, &empty, layout())
        .expect("view");
    let without = delta(before, frame.stats());

    // Every path in the diff, recorded, so every drawn heading resolves to a real entry
    // rather than falling out of the store as untracked.
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

    // Non-vacuity: the store has to have actually been consulted, or this compares two
    // identical empty lookups.
    let pulsing = warm
        .rows
        .iter()
        .filter(|row| {
            matches!(
                row,
                Row::File(entry) if entry.recency == Recency::Pulse
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
    let scratch = Scratch::large_diff("shell-reads-heat", FEW_FILES, LINES);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    settle(&mut frame);

    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
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
            Row::File(entry) => Some(&entry.heat),
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
fn the_file_list_reads_only_the_rows_it_draws() {
    // The region's own I4 claim, stated directly rather than inferred from the whole
    // screen's byte count.
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
    // Bytes are not equal any more and must not be asserted so: I4's narrowing means
    // the height is counted for every changed file, and there are twice as many.
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

    // And again on a pane where the region is deeper than the cap that shipped.
    let deep = one_screen_at("shell-list-deep", 200, DEEP);
    let deep_rows = layout_at(DEEP).list;
    assert!(
        deep_rows > LIST_SETTLED,
        "the deep pane drew {deep_rows} list rows, which is not deeper than the \
         {LIST_SETTLED} the ordinary one draws, so this half proves nothing"
    );
    assert_eq!(
        deep.read,
        1 + deep_rows,
        "on a {DEEP}-row pane the screen asked for {} files, which is not one \
         diff file plus {deep_rows} list rows",
        deep.read
    );

    // And once more beside a rail, which is the same argument on the other axis and the
    // one `SPEC.md` §11.1 names this file for.
    let beside = one_screen_on("shell-list-rail", 200, RAIL_WIDTH, DEEP);
    let rail = layout_on(RAIL_WIDTH, DEEP);
    assert!(
        rail.rail,
        "a {RAIL_WIDTH}-column pane did not draw a rail, so this half is the \
         stacked claim again under another name"
    );
    assert!(
        rail.list > deep_rows,
        "the rail drew {} list rows against the deep stacked pane's {deep_rows}, \
         which is not the deeper region this half is about",
        rail.list
    );
    assert_eq!(
        beside.read,
        1 + rail.list,
        "beside a rail the screen asked for {} files, which is not one diff file \
         plus {} list rows",
        beside.read,
        rail.list
    );
}

#[test]
fn a_list_row_for_an_unchanged_file_reads_no_bytes() {
    // The half the issue asks for by name: a pinned row for a file the frame already
    // holds costs a `stat` and a cache hit, never a read.
    let scratch = Scratch::large_diff("shell-list-idle", FILES, LINES);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    settle(&mut frame);

    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
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
    let scratch = Scratch::large_diff("shell-list-window", FILES, 1);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);

    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
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
    // One view to resolve, then measure the next.
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
    // Bounded by what this screen asked for rather than by a constant.
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
    // Every structural gate in this file opens with `settle()`, which means the one
    // window where the engine deliberately does the most work per frame is the one
    // window none of them enters.
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
fn margin_screen(name: &str, files: usize) -> u64 {
    let scratch = Scratch::large_diff(name, files, LINES);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");

    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let before = frame.stats();
    app.view(&mut frame, &mut highlighter, &history, layout())
        .expect("view");
    delta(before, frame.stats()).computed
}

/// What totalling the diff's rows would cost, which is what a row-exact
/// scrollbar needs.
///
/// Ignored, diagnostic, not a gate. It measures the thing `SPEC.md` §11.1
/// rules out so the ruling rests on a number rather than on an argument, and so
/// the number can be re-taken when the question comes back. Run it with:
///
/// ```text
/// cargo test --release --test reads -- --ignored --nocapture what_a_row_exact_scrollbar_would_cost
/// ```
///
/// The quantity a scrollbar needs is every changed file's row count, which is
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
        let mut highlighter = Highlighter::eager();
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
        let counted = vigia::diff_rows(&mut frame).expect("height");
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
    // Both halves, because each is a different failure.
    let scratch = Scratch::large_diff("shell-height-tick", FILES, LINES);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");

    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
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
        "the diff doubled and the height stayed at {first}, so a stale span \n         survived the tick"
    );
}

#[test]
fn a_pinned_frame_counts_no_height_at_all() {
    // The one place `SPEC.md` §11.2 B16 makes the frame path cheaper, and it
    // is worth a gate rather than a sentence because it is the opposite direction
    // to every other feature that has been added to this pane.
    let scratch = Scratch::large_diff("shell-reads-pinned", FILES, LINES);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");

    let mut highlighter = Highlighter::eager();
    let history = History::new();

    let mut loose = App::new();
    let before = frame.stats();
    let unpinned = loose
        .view(&mut frame, &mut highlighter, &history, layout())
        .expect("view");
    let walked = delta(before, frame.stats());
    assert!(
        walked.measured > 0,
        "the unpinned frame counted no heights, so this fixture cannot tell a \
         skipped walk from an absent one"
    );
    assert!(
        unpinned.total_rows > 0,
        "the unpinned frame drew a bar scaled against nothing"
    );

    // A frame of its own, with its spans unproven, and the first version of this gate
    // did not have one.
    let pinned_scratch = Scratch::large_diff("shell-reads-pinned-own", FILES, LINES);
    let pinned_worktree = pinned_scratch.worktree();
    let mut pinned_frame = pinned_worktree.frame();
    pinned_frame.advance().expect("advance");

    let mut app = App::new();
    app.apply(Action::ToggleSingle, &mut pinned_frame, 1)
        .expect("apply");
    let before = pinned_frame.stats();
    let pinned = app
        .view(&mut pinned_frame, &mut highlighter, &history, layout())
        .expect("view");
    let cost = delta(before, pinned_frame.stats());

    assert_eq!(
        cost.measured, 0,
        "a pinned frame counted {} files' heights for a total it can read off the \
         file it is pinned to",
        cost.measured
    );
    assert!(
        pinned.total_rows > 0,
        "the pinned bar is scaled against nothing, so the count was skipped by \
         losing the answer rather than by already having it"
    );
    assert!(
        pinned.total_rows < unpinned.total_rows,
        "the pinned total is the whole diff's, so the bar measures what the reader \
         cannot reach"
    );
    assert!(
        pinned.read <= unpinned.read,
        "the pinned frame asked the frame for {} files against the unpinned \
         frame's {}, so pinning bought work rather than saving it",
        pinned.read,
        unpinned.read
    );
}

#[test]
fn the_position_counts_the_rows_above_it_including_part_of_a_file() {
    // `rows_above` is the scrollbar's position, and it is two terms: every file before
    // the one the viewport is in, plus how far into that file it has scrolled.
    const FILES: usize = 12;
    /// A file's whole block: heading, hunk header, two content rows, and the blank that
    /// closes it.
    const BLOCK: usize = 5;

    let scratch = Scratch::large_diff("shell-rows-above", FILES, 1);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);

    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let split = layout();

    let mut seen = Vec::new();
    for step in 0..BLOCK * 3 {
        let view = app
            .view(&mut frame, &mut highlighter, &history, split)
            .expect("view");
        seen.push(view.rows_above);
        assert_eq!(
            view.rows_above,
            view.top.file * BLOCK + view.top.row,
            "step {step}: {} rows above a position of {:?} over {BLOCK}-row blocks",
            view.rows_above,
            view.top
        );
        app.apply(vigia::Action::Scroll(1), &mut frame, split.diff)
            .expect("apply");
    }

    // Strictly increasing, which is what says the within-file term is there: a
    // position counting whole files only would repeat each value BLOCK times.
    assert!(
        seen.windows(2).all(|pair| pair[0] < pair[1]),
        "the position did not move on every row: {seen:?}"
    );
}

/// A path's weight comes from the filesystem, and the cases that are not a size.
#[test]
fn a_path_weighs_its_bytes_and_a_missing_one_weighs_zero() {
    let scratch = Scratch::new("weigh-cases");
    scratch.write("src/present.rs", "0123456789");
    scratch.write("src/gone.rs", "gone");
    std::fs::create_dir_all(scratch.path_of("src/adir")).expect("a directory");
    std::fs::remove_file(scratch.path_of("src/gone.rs")).expect("remove");

    assert_eq!(
        vigia::sized(scratch.root(), &["src/present.rs".to_owned()])
            .next()
            .expect("a path")
            .1,
        Some(10),
        "a file that is there did not weigh its bytes"
    );
    assert_eq!(
        vigia::sized(scratch.root(), &["src/gone.rs".to_owned()])
            .next()
            .expect("a path")
            .1,
        Some(0),
        "a deleted file weighed 'no size' rather than zero bytes, so the store \
         keeps a baseline the file no longer has and the next write at that path \
         weighs the dead one"
    );
    assert_eq!(
        vigia::sized(scratch.root(), &["src/adir".to_owned()])
            .next()
            .expect("a path")
            .1,
        None,
        "a directory weighed a size, so a `mkdir` is charged as churn against a \
         path in no diff"
    );
}
