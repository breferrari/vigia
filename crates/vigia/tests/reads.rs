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
use vigia::{App, FileEntry, HEAT_BUCKETS, HeatBucket, Position, Row, body_height};
use vigia_core::{FrameStats, HighlightStats, Highlighter, History, Recency};

use support::{Scratch, delta, materialise, settle};

/// The wide fixture: enough files that reading all of them is unmistakable.
const FILES: usize = 100;
/// The narrow one. Same content per file, so the per-file cost is comparable.
const FEW_FILES: usize = 4;
/// Lines per file, chosen so one file is far taller than any screen.
const LINES: usize = 500;

/// An ordinary terminal, which is where the row count comes from.
///
/// Eighty columns, so the footer is one line whatever the state and the two
/// fixtures below are compared over the same number of rows. At forty this would
/// still be honest but the row count would be one lower, which is I6's business
/// rather than I4's.
fn body() -> usize {
    body_height(
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

    let mut app = App::new();
    let mut highlighter = Highlighter::new();
    let history = History::new();
    let before = frame.stats();
    let view = app
        .view(&mut frame, &mut highlighter, &history, body())
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

    assert_eq!(
        few.cost.bytes, many.cost.bytes,
        "one screen read {} bytes among {FEW_FILES} changed files and {} among \
         {FILES}, so what a frame reads follows the worktree rather than the \
         screen",
        few.cost.bytes, many.cost.bytes
    );
    assert_eq!(
        few.cost.computed, many.cost.computed,
        "one screen computed {} diffs among {FEW_FILES} changed files and {} \
         among {FILES}",
        few.cost.computed, many.cost.computed
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
            .view(&mut frame, &mut highlighter, &history, height)
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

    assert_eq!(
        many.read,
        1,
        "the view asked the frame for {} files to fill {} rows",
        many.read,
        body()
    );
    assert_eq!(
        many.cost.computed, 1,
        "{} diffs were computed for one screen of one file",
        many.cost.computed
    );

    // Bounded against the file on disk, so "one file" means one file's worth of
    // bytes and not a whole worktree that happened to sum to the same number.
    let scratch = Scratch::large_diff("shell-reads-bound", FILES, LINES);
    let on_disk = std::fs::metadata(scratch.path_of("src/mod_0.rs"))
        .expect("stat")
        .len();
    assert!(
        many.cost.bytes >= on_disk && many.cost.bytes <= on_disk * 4,
        "one screen read {} bytes against a {on_disk}-byte file, which is neither \
         one file's two sides nor anything close to it",
        many.cost.bytes
    );
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
        .view(&mut frame, &mut highlighter, &history, body())
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
    assert_eq!(
        cost.reused, view.read as u64,
        "the {} files the view asked for produced {} reuses, so some other path \
         is fetching content",
        view.read, cost.reused
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
    let height = body();

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
            .view(&mut frame, &mut highlighter, &history, height)
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
        .view(&mut frame, &mut highlighter, &history, body())
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
    let drawn = body().div_ceil(span);
    let settled = app
        .view(&mut frame, &mut highlighter, &history, body())
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
        let view = app
            .view(&mut frame, &mut highlighter, &history, height)
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
        .view(&mut frame, &mut highlighter, &empty, body())
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
        .view(&mut frame, &mut highlighter, &full, body())
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
        .view(&mut frame, &mut highlighter, &history, body())
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
