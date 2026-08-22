//! What a view builds out of a real diff.
//!
//! `render.rs` builds its rows by hand, because `vigia_core::FileChange` keeps a
//! private field and cannot be constructed outside its crate. That makes the
//! renderer testable and leaves a hole exactly where the rows come from: every
//! snapshot in that file agrees with a derivation nothing checks.
//!
//! The hole is not hypothetical. `Row::Line` carries a line number, and the core
//! carries line numbers per *hunk*, so the shell counts the two sides forward
//! from each hunk header itself. Found by mutation: stopping the old side from
//! advancing over context lines left the whole suite green, and would have put
//! the wrong number against every removed line on screen.
//!
//! So the numbers here are checked against the file rather than against the
//! arithmetic that produced them. A row's number is right when the line it names
//! really is that line, on the side that line exists on.

use std::time::Instant;

#[path = "../../vigia-core/tests/support/mod.rs"]
mod support;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use vigia::{App, Body, Glyphs, Position, Row, Scale, Theme, View, Viewport, body_layout, render};
use vigia_core::{Highlighter, History, LineKind};

use support::Scratch;

/// Tall enough that nothing is scrolled off, so the whole diff is asserted.
const ALL_ROWS: usize = 500;

const PATH: &str = "src/a.rs";

/// A file whose every line names itself, so a line number has an oracle.
fn numbered(lines: usize) -> String {
    (1..=lines)
        .map(|n| format!("line {n}\n"))
        .collect::<String>()
}

/// A file whose lines are unique to it, so rename tracking cannot pair it with
/// another.
///
/// Worth its own helper. Rename detection is on by default and it works on
/// similarity, so an added file and a deleted one that happen to share boilerplate
/// are reported as one rename rather than two changes. Two four-line files of
/// `line 1`, `line 2` are similar enough to pair, which is not a defect in the
/// engine and is a trap for a fixture.
fn unique(tag: &str, lines: usize) -> String {
    (1..=lines)
        .map(|n| format!("{tag} line {n}\n"))
        .collect::<String>()
}

fn lines_of(text: &str) -> Vec<String> {
    text.lines().map(str::to_owned).collect()
}

/// Two changed files and a third left alone, which is the smallest tree with an
/// inter-file boundary in it.
///
/// **Named because two gates need the same shape and a fixture built twice
/// drifts.** `a_real_repository_draws` reads the cells this produces and
/// `a_files_block_ends_in_a_blank_row` reads the rows, so a change to one copy
/// would leave the other asserting against a screen nobody draws. Every sibling
/// test file here names its scratch shape for the same reason.
///
/// `README.md` is committed and never touched, so it is the file that must
/// **not** appear: it is what stops a walk over every tracked file passing.
fn two_changed(name: &str) -> Scratch {
    let scratch = Scratch::new(name);
    scratch.write("src/lib.rs", numbered(12));
    scratch.write("README.md", unique("readme", 3));
    scratch.commit_all("baseline");
    scratch.edit_line("src/lib.rs", 5, "let changed = true;");
    scratch.write("src/added.rs", unique("added", 2));
    scratch
}

#[test]
fn every_line_number_names_the_line_it_is_on() {
    // The fixture is built so the two sides cannot agree by accident: two lines
    // are deleted near the top, which offsets every later line, and a separate
    // edit far enough below to make a second hunk. A same-length one-line change
    // would leave old and new numbering identical and the assertion would hold
    // against a shell that counted only one side.
    let scratch = Scratch::new("shell-rows-numbers");
    scratch.write(PATH, numbered(40));
    scratch.commit_all("baseline");

    let mut after = lines_of(&numbered(40));
    after.remove(2);
    after.remove(2);
    after[24] = "changed".to_owned();
    scratch.write(PATH, after.join("\n") + "\n");

    let new_side = lines_of(&std::fs::read_to_string(scratch.path_of(PATH)).expect("read"));
    let old_side = lines_of(&scratch.git(&["show", &format!("HEAD:{PATH}")]));
    assert_eq!(old_side.len(), 40, "the baseline is not 40 lines");
    assert_eq!(new_side.len(), 38, "the edit did not remove two lines");

    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let view = app
        .view(
            &mut frame,
            &mut highlighter,
            &history,
            Body::diff_only(ALL_ROWS),
        )
        .expect("view");

    let mut counts = [0usize; 3];
    let mut hunks = 0usize;
    let mut diverged = false;

    for row in &view.rows {
        let Row::Line {
            kind, number, text, ..
        } = row
        else {
            if matches!(row, Row::Hunk { .. }) {
                hunks += 1;
            }
            continue;
        };

        // A removed line exists only in the index, everything else only or also
        // in the working tree. Numbering it against the other side is the whole
        // failure mode this test exists for.
        let (side, name) = match kind {
            LineKind::Removed => (&old_side, "the index"),
            _ => (&new_side, "the working tree"),
        };
        let index = *number as usize;
        assert_eq!(
            side.get(index.wrapping_sub(1)).map(String::as_str),
            Some(text.as_str()),
            "a {kind:?} row says line {number} is {text:?}, but line {number} of \
             {name} is {:?}",
            side.get(index.wrapping_sub(1))
        );

        counts[match kind {
            LineKind::Context => 0,
            LineKind::Added => 1,
            LineKind::Removed => 2,
        }] += 1;

        // Non-vacuity for the fixture's whole point: somewhere below the deletion
        // a context line must sit at a different number on each side, or old and
        // new never diverged and one counter could have served both.
        if *kind == LineKind::Context
            && old_side.iter().position(|line| line == text) != Some(index - 1)
        {
            diverged = true;
        }
    }

    assert!(hunks >= 2, "the fixture produced {hunks} hunks, not two");
    assert!(
        counts.iter().all(|n| *n > 0),
        "context/added/removed rows were {counts:?}, so some kind was never checked"
    );
    assert!(
        diverged,
        "no context line sat at a different number on the two sides, so the \
         fixture cannot tell one counter from two"
    );
}

#[test]
fn a_file_is_its_heading_then_its_hunks() {
    // The row sequence the renderer assumes. Getting it wrong draws content under
    // the wrong filename, which is worse than drawing nothing.
    let scratch = Scratch::new("shell-rows-shape");
    scratch.write(PATH, numbered(10));
    scratch.write("src/b.rs", numbered(10));
    scratch.commit_all("baseline");
    scratch.edit_line(PATH, 4, "changed");
    scratch.edit_line("src/b.rs", 4, "changed");

    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let view = app
        .view(
            &mut frame,
            &mut highlighter,
            &history,
            Body::diff_only(ALL_ROWS),
        )
        .expect("view");

    let mut headings = 0usize;
    for (index, row) in view.rows.iter().enumerate() {
        match row {
            Row::File(entry) => {
                let churn = &entry.churn;
                headings += 1;
                assert!(
                    matches!(view.rows.get(index + 1), Some(Row::Hunk { .. })),
                    "the row after a heading is {:?}, not a hunk header",
                    view.rows.get(index + 1)
                );
                assert_eq!(
                    *churn,
                    Some((1, 1)),
                    "a one-line replacement did not count as one added and one \
                     removed"
                );
            }
            Row::Hunk { .. } => assert!(
                headings > 0,
                "a hunk header appeared before any file heading, so its lines \
                 belong to nothing"
            ),
            _ => {}
        }
    }
    assert_eq!(headings, 2, "{headings} files were drawn, not two");
}

#[test]
fn each_kind_of_change_gets_its_own_letter() {
    // Git's letters, because they are the ones a reader already knows. Asserted
    // against changes a real repository produces rather than against the mapping
    // function, so a kind the core reports and the shell does not recognise shows
    // up here as a wrong letter instead of never being noticed.
    // Every file's content is unique to it except the pair meant to be a rename.
    // With shared content the deletion and the addition below get paired into one
    // rename, and the assertion fails for a reason that is about the fixture and
    // not about the shell.
    let scratch = Scratch::new("shell-rows-letters");
    scratch.write("src/kept.rs", unique("kept", 4));
    scratch.write("src/gone.rs", unique("gone", 4));
    scratch.write("src/moved.rs", unique("moved", 40));
    scratch.commit_all("baseline");

    scratch.edit_line("src/kept.rs", 0, "changed");
    scratch.remove("src/gone.rs");
    scratch.write("src/fresh.rs", unique("fresh", 2));
    // `I` is the one letter here that is not git's. Git renders an intent-to-add
    // as a staged addition, and a monitor of the working tree has to distinguish
    // it from content that is really in the index, so the choice is ours and gets
    // its own case.
    scratch.write("src/promised.rs", unique("promised", 2));
    scratch.git(&["add", "-N", "--", "src/promised.rs"]);
    // A rename is a deletion paired with an addition, which the core reports as
    // one change because rename tracking is on by default. Identical content, so
    // the pairing is not in doubt.
    scratch.write("src/landed.rs", unique("moved", 40));
    scratch.remove("src/moved.rs");

    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let view = app
        .view(
            &mut frame,
            &mut highlighter,
            &history,
            Body::diff_only(ALL_ROWS),
        )
        .expect("view");

    let mut seen: Vec<(char, String, Option<String>)> = view
        .rows
        .iter()
        .filter_map(|row| match row {
            Row::File(entry) => Some((entry.kind, entry.path.clone(), entry.from.clone())),
            _ => None,
        })
        .collect();
    seen.sort_by(|a, b| a.1.cmp(&b.1));

    let letters: Vec<(char, &str)> = seen
        .iter()
        .map(|(kind, path, _)| (*kind, path.as_str()))
        .collect();
    assert_eq!(
        letters,
        vec![
            ('A', "src/fresh.rs"),
            ('D', "src/gone.rs"),
            ('M', "src/kept.rs"),
            ('R', "src/landed.rs"),
            ('I', "src/promised.rs"),
        ],
        "the letters do not match what the repository actually did"
    );

    let renamed = seen
        .iter()
        .find(|(kind, _, _)| *kind == 'R')
        .expect("a rename");
    assert_eq!(
        renamed.2.as_deref(),
        Some("src/moved.rs"),
        "the rename does not say where the content came from, which is the whole \
         content of a rename"
    );
}

#[test]
fn a_window_into_a_file_is_the_same_rows_the_whole_file_would_give() {
    // The property that pins the windowing arithmetic. Rows above the window are
    // counted rather than built, and a hunk entirely above it is skipped by
    // arithmetic rather than walked, because cloning a hundred thousand lines to
    // show twenty-four of them is a per-frame cost that grows with the file. Two
    // separate places to be off by one, and neither is reachable from a fixture
    // with one hunk or a view that starts at the top.
    //
    // Stepping every offset through a multi-hunk file and demanding each window
    // match the same slice of the full row list catches any of them, without the
    // test having to restate the arithmetic it is checking.
    let scratch = Scratch::new("shell-rows-window");
    scratch.write(PATH, numbered(60));
    scratch.commit_all("baseline");

    let mut after = lines_of(&numbered(60));
    // Far enough apart that their context windows cannot touch, so the core emits
    // one hunk each rather than merging them.
    for line in [5, 25, 45] {
        after[line] = format!("changed {line}");
    }
    scratch.write(PATH, after.join("\n") + "\n");

    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    let whole = View::collect(
        &mut frame,
        &mut highlighter,
        &history,
        Viewport {
            position: Position { file: 0, row: 0 },
            // Unanchored, because this slides a window and compares it against
            // slices of the whole. Letting the viewport back up to fill a short
            // tail would be comparing a different window from the one the offset
            // names.
            anchored: false,
            diff_rows: ALL_ROWS,
            ..Viewport::default()
        },
    )
    .expect("view");
    let hunks = whole
        .rows
        .iter()
        .filter(|row| matches!(row, Row::Hunk { .. }))
        .count();
    assert_eq!(hunks, 3, "the fixture produced {hunks} hunks, not three");
    assert!(
        whole.rows.len() < ALL_ROWS,
        "the whole file did not fit, so the slices below compare against a window"
    );

    let height = 4;
    for offset in 0..whole.rows.len() {
        let window = View::collect(
            &mut frame,
            &mut highlighter,
            &history,
            Viewport {
                position: Position {
                    file: 0,
                    row: offset,
                },
                anchored: false,
                diff_rows: height,
                ..Viewport::default()
            },
        )
        .expect("view");
        let end = (offset + height).min(whole.rows.len());
        assert_eq!(
            window.rows,
            whole.rows[offset..end],
            "a {height}-row window at offset {offset} is not the same rows as \
             offsets {offset}..{end} of the whole file"
        );
        assert_eq!(
            window.top,
            Position {
                file: 0,
                row: offset
            },
            "the window at offset {offset} reported starting somewhere else"
        );
    }
}

#[test]
fn a_real_repository_draws() {
    // The only test that runs the whole composition: a working tree, a frame, the
    // scroll position, the rows, and the cells. Everything else in the suite cuts
    // it somewhere. `render.rs` hand-builds its rows because `FileChange` cannot
    // be constructed outside the core, so nothing there proves the rows a real
    // frame produces are the ones the renderer was designed against; the rest of
    // this file builds rows and never draws them.
    //
    // What it still does not cover is the terminal itself: raw mode, the
    // alternate screen, mouse capture and the panic hook are all outside a
    // `TestBackend`, so none of them can be reached from here.
    // [#8](https://github.com/breferrari/vigia/issues/8) proves them where they
    // live instead, in `crates/vigia/src/terminal.rs`, against a recorded console
    // rather than a real one.
    let scratch = two_changed("shell-rows-draw");

    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    let mut terminal = Terminal::new(TestBackend::new(64, 18)).expect("terminal");
    let area = Rect::new(0, 0, 64, 18);
    // **The shipped split, because this is the only whole-composition test.**
    // Everything else in the suite holds the pinned list out on purpose, to keep
    // some other measurement clean. If this one did too, no test anywhere would
    // draw a real frame's two regions together and the snapshot would be a
    // picture of a screen nobody gets.
    let split = body_layout(
        area,
        &app.chrome("fixture", None, None, None, None, None),
        frame.files().len(),
    );
    let view = app
        .view(&mut frame, &mut highlighter, &history, split)
        .expect("view");
    // Non-vacuity: the fixture has to have produced something to draw, or the
    // snapshot below is a picture of an empty pane.
    assert_eq!(view.files, 2, "the fixture is not two changed files");
    assert!(view.rows.len() > 4, "only {} rows to draw", view.rows.len());
    assert_eq!(
        view.list.len(),
        2,
        "both changed files should be pinned above the diff"
    );

    let theme = Theme::default();
    let chrome = app.chrome("fixture", None, None, None, None, None);
    terminal
        .draw(|f| {
            let area = f.area();
            render(
                f.buffer_mut(),
                area,
                &view,
                &theme,
                Glyphs::default(),
                &chrome,
            );
        })
        .expect("draw");
    insta::assert_snapshot!(terminal.backend());
}

#[test]
fn a_recorded_tick_reaches_the_drawn_sparkline() {
    /// The figure the store answers with at each grouping, finest first.
    ///
    /// **Three since [#234](https://github.com/breferrari/vigia/issues/234)**,
    /// where this pinned one number. The middle entry is that number: `4_667` was
    /// measured over twelve buckets, and twelve buckets is what a drawn one holds
    /// at the settled rung, so the value did not move, it acquired an index.
    ///
    /// **And the set is what the gate is now for.** `418` and `837` are not one
    /// figure and its double, which is exactly the point: `scale_of` averages the
    /// non-empty values and grouping merges empties into their neighbours, so a
    /// coarser figure is never the finest one multiplied. A store that returned
    /// one figure three times, or one figure scaled by the grouping, fails here.
    ///
    /// **They were `[2_333, 4_667, 7_779]` until
    /// [#256](https://github.com/breferrari/vigia/issues/256)**, and they moved
    /// because the rule acquired an outlier cut. This fixture is one path with
    /// one write, so its levelled buckets are the kernel's own geometric ramp,
    /// `[5, 10, 22, 49, 114, 266, 622, 1489, 3785, 11590]`, where every value is
    /// about two and a half times its neighbour. A ramp has no bulk, so the cut
    /// reads its top as outlying and takes the two tallest out of the mean.
    ///
    /// **What that costs on screen is two cells, and on this pane none at all**,
    /// which is worth stating because the figure moved by 5.6x. At the finest
    /// rung the strip goes from `______________▁▁▁▁▁▁▃▆██` to
    /// `______________▁▁▁▁▃▆████`; at the rung eighty columns affords it does not
    /// move, and the snapshot below is unchanged.
    ///
    /// **The third entry equals the second, and that is the clamp rather than a
    /// coincidence.** `History::repeak` keeps a coarser rung's figure from
    /// falling below a finer rung's, and on this fixture the coarsest grouping
    /// has three buckets to find a bulk in and came out at 302. Delete the clamp
    /// and this reads `[418, 837, 302]`, which is a coarser rung drawing *taller*
    /// bars than a finer one.
    const PINNED: [u32; 3] = [418, 837, 837];
    // **The producer, not the decider.** `spark_of` and the painter are mutation
    // tested from every side in `render.rs`, and every one of those fixtures
    // hands `View` a `peak` by hand. Nothing drove a *recorded* one through
    // `App::view`, so `View::peak = history.scale()` was untested: hardcoding it
    // to zero passed the entire workspace suite, all 426 tests, while making
    // every sparkline on every screen draw pure track forever.
    //
    // That failure is invisible in the worst possible way after
    // [#78](https://github.com/breferrari/vigia/issues/78): a screen of pure
    // track is exactly what a correct launch looks like, so the tool would look
    // right and say nothing, which is the defect that issue exists to remove
    // rather than a new one it introduced.
    //
    // Asserted through the drawn cells rather than off `view.scale`, so it covers
    // the whole path a bucket takes from the store to the screen.
    let scratch = Scratch::new("shell-rows-recorded-tick");
    scratch.write("src/lib.rs", numbered(12));
    scratch.commit_all("baseline");
    scratch.edit_line("src/lib.rs", 5, "let changed = true;");

    let now = Instant::now();
    let mut history = History::starting_at(now);
    // **Sized writes rather than bare ticks.** A sample weighs a difference, and
    // the sparkline draws a *level* since #242, so two unsized ticks weigh one
    // each, spread to a scale of exactly 1: the value a hardcoded denominator
    // would reach for, which is the one thing this gate exists to tell apart.
    // A first write is a baseline, so the weight is the second one's growth.
    history.record_sized([("src/lib.rs", Some(4_000))], now);
    history.record_sized([("src/lib.rs", Some(28_000))], now);

    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut app = App::new();
    let mut highlighter = Highlighter::eager();

    let mut terminal = Terminal::new(TestBackend::new(80, 12)).expect("terminal");
    let area = Rect::new(0, 0, 80, 12);
    let split = body_layout(
        area,
        &app.chrome("fixture", None, None, None, None, None),
        frame.files().len(),
    );
    let view = app
        .view(&mut frame, &mut highlighter, &history, split)
        .expect("view");

    // Non-vacuity, and it is the assertion that would have caught the hardcode
    // on its own: the store was asked and answered.
    //
    // **Two recorded ticks rather than one**, so the expected peak is 2 and not
    // 1. A one-tick fixture asserts `peak == 1`, which a renderer hardcoding the
    // *ramp's* denominator to one would also satisfy: the value a store returns
    // has to differ from every constant a mutation would reach for, or the gate
    // that exists to kill a hardcode is one.
    //
    // **Thirteen rather than two since [#242](https://github.com/breferrari/vigia/issues/242).**
    // The sparkline draws a *level*, so its denominator is measured over the same
    // levelled buckets rather than over the raw ones: two ticks spread by the
    // kernel light many buckets at a low value, and the mean-based scale over
    // those is what the bars are actually divided by. Pinned rather than loosened
    // because this gate's whole point is that the value differs from every
    // constant a hardcode would reach for, and it still does.
    assert_eq!(
        view.scale,
        Scale(PINNED),
        "the recorded ticks did not reach the view's shared scale"
    );

    let theme = Theme::default();
    let chrome = app.chrome("fixture", None, None, None, None, None);
    terminal
        .draw(|f| {
            let drawn = f.area();
            render(
                f.buffer_mut(),
                drawn,
                &view,
                &theme,
                Glyphs::default(),
                &chrome,
            );
        })
        .expect("draw");

    // A written file draws at least one bucket. Matched on symbol and colour
    // together, because the heat strip on the same row draws the ramp's top
    // glyph too.
    let buffer = terminal.backend().buffer();
    let bars = (0..buffer.area.height)
        .flat_map(|y| (0..buffer.area.width).map(move |x| (x, y)))
        .filter(|&at| {
            let cell = &buffer[at];
            // Any stop of the ramp, which is three since #196.
            "▁▂▃▄▅▆▇█".contains(cell.symbol())
                && [theme.spark.fg, theme.spark_warm.fg, theme.spark_hot.fg]
                    .contains(&cell.style().fg)
        })
        .count();
    assert!(
        bars > 0,
        "a file with a recorded tick drew no sparkline bucket anywhere on \
         screen, so the store's scale is not reaching the renderer"
    );
}

/// Every rung draws from the store's own figures, not a fixture's.
///
/// **The coverage-shape gap [#234](https://github.com/breferrari/vigia/issues/234)
/// left, closed here.** Every gate that reaches the twenty-four bucket rung does
/// it with a hand-set `Scale` on a literal `View`, because the sweeps live in
/// `tests/legibility.rs` and build their fixtures directly. The only test that
/// drives a *recorded* store all the way to a drawn row renders at eighty
/// columns, which is the twelve-bucket rung. So the widest rung had never been
/// drawn against a denominator the store actually computed, and the denominator
/// is the half of this feature that is not a width: `History::scales` returns one
/// figure per grouping precisely because multiplying the finest by the grouping
/// is wrong on a quiet worktree, and nothing end to end was checking that the
/// right one of the three arrives.
///
/// A hundred and sixty-four columns is the first pane whose share affords the
/// widest layout at the block rung, which
/// `tests/legibility.rs::the_glance_columns_collapse_in_one_order` derives and
/// pins.
#[test]
fn every_rung_draws_from_the_stores_own_figures() {
    /// A pane per rung, widest first, with the buckets that rung must draw.
    ///
    /// **Every grouping, not just the widest.** Round 1 closed this gap at the
    /// twenty-four end and left it open at the other: the six-bucket rung was
    /// reached only by `tests/legibility.rs` fixtures carrying a hand-set
    /// `Scale`, and `Scale::spread`'s own docblock says it is exact for the
    /// fixtures this suite writes rather than for what the store computes. The
    /// coarsest grouping is the one least like a multiplied figure, so it is the
    /// one a fixture is least able to stand in for.
    ///
    /// The widths are the ones
    /// `tests/legibility.rs::the_glance_columns_collapse_in_one_order` derives
    /// and pins.
    const RUNGS: [(u16, usize); 3] = [(164, 24), (80, 12), (45, 6)];

    let scratch = Scratch::new("shell-rows-every-rung");
    scratch.write("src/lib.rs", numbered(12));
    scratch.commit_all("baseline");
    scratch.edit_line("src/lib.rs", 5, "let changed = true;");

    let now = Instant::now();
    let mut history = History::starting_at(now);
    // Two sized writes, as `a_recorded_tick_reaches_the_drawn_sparkline` uses:
    // a first write is a baseline, so the weight is the second one's growth.
    history.record_sized([("src/lib.rs", Some(4_000))], now);
    history.record_sized([("src/lib.rs", Some(28_000))], now);

    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut highlighter = Highlighter::eager();
    let theme = Theme::default();
    let ramp = "▁▂▃▄▅▆▇█";
    let ink = [theme.spark.fg, theme.spark_warm.fg, theme.spark_hot.fg];

    for (pane, rung) in RUNGS {
        // **A fresh `App` per width, so each iteration is its own observation.**
        // One shared across the loop carries `App::paint` forward, so the first
        // pane draws plain and the rest draw coloured, and it carries the scroll
        // position and the follow state with it. None of that reaches a glance
        // slot today, which is exactly the kind of accident a later assertion
        // added to this loop would inherit without noticing.
        let mut app = App::new();
        let mut terminal = Terminal::new(TestBackend::new(pane, 12)).expect("terminal");
        let area = Rect::new(0, 0, pane, 12);
        let chrome = app.chrome("fixture", None, None, None, None, None);
        let split = body_layout(area, &chrome, frame.files().len());
        let view = app
            .view(&mut frame, &mut highlighter, &history, split)
            .expect("view");

        terminal
            .draw(|f| {
                let drawn = f.area();
                render(f.buffer_mut(), drawn, &view, &theme, Glyphs::Block, &chrome);
            })
            .expect("draw");

        // The whole slot, bars and track together, on the busiest row. At the
        // block rung a bucket is a cell, so this count is the rung itself.
        let buffer = terminal.backend().buffer();
        let bar = |cell: &ratatui::buffer::Cell| {
            ramp.contains(cell.symbol()) && ink.contains(&cell.style().fg)
        };
        let slot = |cell: &ratatui::buffer::Cell| {
            bar(cell) || (cell.symbol() == "_" && cell.style().fg == theme.spark_track.fg)
        };
        let count = |y: u16, want: &dyn Fn(&ratatui::buffer::Cell) -> bool| {
            (0..buffer.area.width)
                .filter(|x| want(&buffer[(*x, y)]))
                .count()
        };

        let widest = (0..buffer.area.height)
            .map(|y| count(y, &slot))
            .max()
            .expect("a row");
        assert_eq!(
            widest, rung,
            "at {pane} columns a recorded store drew {widest} sparkline buckets \
             rather than the {rung} its rung asks for"
        );

        // **And the heights came from the store rather than from nothing.** A
        // denominator of zero draws pure track, which is what a hardcode or the
        // wrong entry of `Scale` would most easily produce, and it is
        // indistinguishable from a correct launch by eye.
        let bars: usize = (0..buffer.area.height).map(|y| count(y, &bar)).sum();
        assert!(
            bars > 0,
            "at {pane} columns the {rung} bucket rung drew nothing at all, so \
             the store's figure for this grouping is not reaching the renderer"
        );
    }
}

#[test]
fn a_binary_file_gets_a_reason_instead_of_hunks() {
    // Otherwise it draws as a heading with nothing under it, which reads as a
    // file the monitor failed to open rather than one it decided not to.
    let scratch = Scratch::new("shell-rows-binary");
    scratch.write("assets/blob.bin", [0u8, 1, 2, 3, 0, 5]);
    scratch.commit_all("baseline");
    scratch.write("assets/blob.bin", [0u8, 9, 9, 9, 0, 9]);

    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let view = app
        .view(
            &mut frame,
            &mut highlighter,
            &history,
            Body::diff_only(ALL_ROWS),
        )
        .expect("view");

    assert!(
        matches!(
            view.rows.first(),
            Some(Row::File(entry)) if entry.churn.is_none()
        ),
        "a binary file's heading is {:?}, and a +/- count for it would be a lie",
        view.rows.first()
    );
    assert_eq!(
        view.rows.get(1),
        Some(&Row::Note("binary")),
        "a binary file drew {:?} where its reason should be",
        view.rows.get(1)
    );
    assert_eq!(view.rows.len(), 2, "a binary file drew extra rows");
}

#[test]
fn a_files_block_ends_in_a_blank_row() {
    // [#165](https://github.com/breferrari/vigia/issues/165). A file's last
    // content row and the next file's heading sat on adjacent rows, and a
    // heading is a `Painter::file_row` carrying the kind letter, the path, the
    // pulse, the heat strip, the sparkline and the counters, so a dense row
    // landed directly under a dense row and the only thing marking the boundary
    // was the content itself.
    //
    // **Trailing, and on every file but the last**, which is the ruling rather
    // than an implementation detail, so the gate is written against all three of
    // its halves: every heading after the first is preceded by a gap, the
    // **first** row of the stream is a heading and not a gap, and the **last**
    // file does **not** get one. Dropping any of the three is a different ruling
    // that would otherwise pass this test, and the third is the one that was
    // reversed while this was being built (the reason is at the assertion).
    //
    // Against a real frame rather than hand-built rows, because the claim is
    // about what `View::take_file` produces from a diff, which is exactly what
    // this file exists to cover.
    let scratch = two_changed("shell-rows-gap");

    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    let view = View::collect(
        &mut frame,
        &mut highlighter,
        &history,
        Viewport {
            position: Position { file: 0, row: 0 },
            anchored: false,
            diff_rows: ALL_ROWS,
            ..Viewport::default()
        },
    )
    .expect("view");

    let headings: Vec<usize> = view
        .rows
        .iter()
        .enumerate()
        .filter_map(|(at, row)| matches!(row, Row::File(_)).then_some(at))
        .collect();

    // Non-vacuity first: with one changed file there is no boundary to draw and
    // every assertion below holds for a renderer that draws no gap at all.
    assert!(
        headings.len() >= 2,
        "the fixture drew {} headings, so there is no inter-file boundary in it \
         and this gate proves nothing",
        headings.len()
    );
    assert!(
        view.rows.len() < ALL_ROWS,
        "the whole diff did not fit, so the last row below is a window edge \
         rather than the end of the stream"
    );

    assert_eq!(
        headings.first(),
        Some(&0),
        "the stream does not open on a heading, so a gap was drawn above the \
         first file: {:?}",
        view.rows.first()
    );
    for &at in &headings[1..] {
        assert_eq!(
            view.rows.get(at - 1),
            Some(&Row::Gap),
            "the row above the heading at {at} is {:?} rather than the blank \
             that ends the file before it",
            view.rows.get(at - 1)
        );
    }
    // **And the last file does not get one.** That is the exception rather than
    // an oversight, and it is the half of this ruling that was reversed while it
    // was being built: `SPEC.md` §11.1 rules the bottom of the diff is
    // **content**, `scroll.rs::the_bottom_of_the_diff_is_content_rather_than_blank`
    // is the gate over it, and that gate carries its own warning about having
    // once been weakened. A blank there would separate the diff from nothing,
    // since the footer is chrome with a row of its own. Asserted here as well,
    // because this is the file that would notice a uniform gap coming back.
    assert!(
        !matches!(view.rows.last(), Some(Row::Gap)),
        "the stream ends on a blank, so the gap has gone uniform and the bottom \
         of the diff is no longer content"
    );
}
