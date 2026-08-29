//! What a view builds out of a real diff.

use std::time::Instant;

#[path = "../../vigia-core/tests/support/mod.rs"]
mod support;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use vigia::{
    App, Body, Glyphs, Pointing, Position, Row, Scale, Theme, View, Viewport, body_layout, render,
};
use vigia_core::{HISTORY_SAMPLE, Highlighter, History, LineKind};

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
    // The fixture is built so the two sides cannot agree by accident: two lines are
    // deleted near the top, which offsets every later line, and a separate edit far
    // enough below to make a second hunk.
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
    // Git's letters, because they are the ones a reader already knows.
    let scratch = Scratch::new("shell-rows-letters");
    scratch.write("src/kept.rs", unique("kept", 4));
    scratch.write("src/gone.rs", unique("gone", 4));
    scratch.write("src/moved.rs", unique("moved", 40));
    scratch.commit_all("baseline");

    scratch.edit_line("src/kept.rs", 0, "changed");
    scratch.remove("src/gone.rs");
    scratch.write("src/fresh.rs", unique("fresh", 2));
    // `I` is the one letter here that is not git's.
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
    // The property that pins the windowing arithmetic.
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
            // Unanchored, because this slides a window and compares it against slices
            // of the whole.
            anchored: false,
            wrap: false,
            width: 0,
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
                wrap: false,
                width: 0,
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
    // scroll position, the rows, and the cells.
    let scratch = two_changed("shell-rows-draw");

    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    let mut terminal = Terminal::new(TestBackend::new(64, 18)).expect("terminal");
    let area = Rect::new(0, 0, 64, 18);
    // The shipped split, because this is the only whole-composition test.
    let split = body_layout(
        area,
        &app.chrome("fixture", None, Pointing::default(), 0, ""),
        frame.files().len(),
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
    let chrome = app.chrome("fixture", None, Pointing::default(), 0, "");
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
    /// Restated rather than read back from the store, because the view takes its
    /// scale straight from `History::scales` and comparing the two would assert
    /// nothing. It moves whenever the level filter's own constants do.
    ///
    /// The three are independent figures rather than one and its multiples: a
    /// burst landing only on the newest sample fills whole groups, and then the
    /// coarser two follow from the finest by arithmetic and assert nothing of
    /// their own. The older write below is what makes the lit region straddle a
    /// group boundary.
    const PINNED: [u32; 3] = [1_067_565, 1_868_239, 3_736_478];
    // The producer, not the decider.
    let scratch = Scratch::new("shell-rows-recorded-tick");
    scratch.write("src/lib.rs", numbered(12));
    scratch.commit_all("baseline");
    scratch.edit_line("src/lib.rs", 5, "let changed = true;");

    let now = Instant::now();
    // Opened before the first write, so the older one lands where it was made
    // rather than saturating into the newest sample.
    let began = now - HISTORY_SAMPLE * 13;
    let mut history = History::starting_at(began);
    // Sized writes rather than bare ticks, and spread far enough apart that the
    // levelled region does not end on a group boundary.
    history.record_sized([("src/lib.rs", Some(4_000))], began);
    history.record_sized([("src/lib.rs", Some(21_000))], began);
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
        &app.chrome("fixture", None, Pointing::default(), 0, ""),
        frame.files().len(),
        frame.files().len(),
    );
    let view = app
        .view(&mut frame, &mut highlighter, &history, split)
        .expect("view");

    // Non-vacuity, and it is the assertion that would have caught the hardcode
    // on its own: the store was asked and answered.
    assert_eq!(
        view.scale,
        Scale(PINNED),
        "the recorded ticks did not reach the view's shared scale"
    );

    let theme = Theme::default();
    let chrome = app.chrome("fixture", None, Pointing::default(), 0, "");
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
#[test]
fn every_rung_draws_from_the_stores_own_figures() {
    /// A pane per rung, widest first, with the buckets that rung must draw.
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
        // A fresh `App` per width, so each iteration is its own observation.
        let mut app = App::new();
        let mut terminal = Terminal::new(TestBackend::new(pane, 12)).expect("terminal");
        let area = Rect::new(0, 0, pane, 12);
        let chrome = app.chrome("fixture", None, Pointing::default(), 0, "");
        let split = body_layout(area, &chrome, frame.files().len(), frame.files().len());
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

        // And the heights came from the store rather than from nothing.
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
            wrap: false,
            width: 0,
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
    // And the last file does not get one.
    assert!(
        !matches!(view.rows.last(), Some(Row::Gap)),
        "the stream ends on a blank, so the gap has gone uniform and the bottom \
         of the diff is no longer content"
    );
}
