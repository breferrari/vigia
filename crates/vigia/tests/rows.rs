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

#[path = "../../vigia-core/tests/support/mod.rs"]
mod support;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use vigia::{App, FileEntry, Position, Row, Theme, View, body_height, render};
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
    let mut highlighter = Highlighter::new();
    let history = History::new();
    let view = app
        .view(&mut frame, &mut highlighter, &history, ALL_ROWS)
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
    let mut highlighter = Highlighter::new();
    let history = History::new();
    let view = app
        .view(&mut frame, &mut highlighter, &history, ALL_ROWS)
        .expect("view");

    let mut headings = 0usize;
    for (index, row) in view.rows.iter().enumerate() {
        match row {
            Row::File(FileEntry { churn, .. }) => {
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
    let mut highlighter = Highlighter::new();
    let history = History::new();
    let view = app
        .view(&mut frame, &mut highlighter, &history, ALL_ROWS)
        .expect("view");

    let mut seen: Vec<(char, String, Option<String>)> = view
        .rows
        .iter()
        .filter_map(|row| match row {
            Row::File(FileEntry {
                kind, path, from, ..
            }) => Some((*kind, path.clone(), from.clone())),
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
    let mut highlighter = Highlighter::new();
    let history = History::new();

    let whole = View::collect(
        &mut frame,
        &mut highlighter,
        &history,
        Position { file: 0, row: 0 },
        ALL_ROWS,
        // Unanchored, because this slides a window and compares it against slices
        // of the whole. Letting the viewport back up to fill a short tail would be
        // comparing a different window from the one the offset names.
        false,
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
            Position {
                file: 0,
                row: offset,
            },
            height,
            false,
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
    let scratch = Scratch::new("shell-rows-draw");
    scratch.write("src/lib.rs", numbered(12));
    scratch.write("README.md", unique("readme", 3));
    scratch.commit_all("baseline");
    scratch.edit_line("src/lib.rs", 5, "let changed = true;");
    scratch.write("src/added.rs", unique("added", 2));

    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut app = App::new();
    let mut highlighter = Highlighter::new();
    let history = History::new();

    let mut terminal = Terminal::new(TestBackend::new(64, 18)).expect("terminal");
    let area = Rect::new(0, 0, 64, 18);
    let height = body_height(area, &app.chrome("fixture", None), frame.files().len());
    let view = app
        .view(&mut frame, &mut highlighter, &history, height)
        .expect("view");
    // Non-vacuity: the fixture has to have produced something to draw, or the
    // snapshot below is a picture of an empty pane.
    assert_eq!(view.files, 2, "the fixture is not two changed files");
    assert!(view.rows.len() > 4, "only {} rows to draw", view.rows.len());

    let theme = Theme::default();
    let chrome = app.chrome("fixture", None);
    terminal
        .draw(|f| {
            let area = f.area();
            render(f.buffer_mut(), area, &view, &theme, &chrome);
        })
        .expect("draw");
    insta::assert_snapshot!(terminal.backend());
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
    let mut highlighter = Highlighter::new();
    let history = History::new();
    let view = app
        .view(&mut frame, &mut highlighter, &history, ALL_ROWS)
        .expect("view");

    assert!(
        matches!(
            view.rows.first(),
            Some(Row::File(FileEntry { churn: None, .. }))
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
