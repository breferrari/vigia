//! The renderer, as text.
//!
//! `SPEC.md` §7 makes this the instrument the UI is asserted with: render into an
//! in-memory buffer and snapshot it, because that turns a screen into something a
//! diff can argue about. The suite goes through `TestBackend` and `Terminal::draw`
//! rather than calling into a bare `Buffer`, so the backend and the frame
//! plumbing are inside what is being tested and not beside it.
//!
//! Two things it deliberately does not cover.
//!
//! Colour. `TestBackend`'s `Display` writes symbols and drops styles, so a
//! snapshot cannot see a theme at all. The palette is checked by reading cells
//! instead, at the bottom of this file.
//!
//! I6. Legibility at 40, 80 and 120 columns is
//! [#7](https://github.com/breferrari/vigia/issues/7), and the assertions that
//! make it an invariant rather than a screenshot belong there. The 40-column
//! snapshot here is the shell's own baseline: something for that work to change
//! visibly rather than a claim that the invariant already holds.
//!
//! Views are built by hand, not from a repository. That is forced, and it is
//! worth knowing: `vigia_core::FileChange` keeps a private field, so no test
//! outside that crate can construct one. Whether the shell turns a real frame
//! into these rows is `reads.rs` and `scroll.rs`; whether these rows become the
//! right cells is here.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use vigia::{Chrome, Position, Row, Theme, View, body_height, render};
use vigia_core::LineKind;

/// Draw a view at `width` by `height` and hand back the backend to snapshot.
fn screen(width: u16, height: u16, view: &View, chrome: &Chrome) -> TestBackend {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
    let theme = Theme::default();
    terminal
        .draw(|f| {
            let area = f.area();
            render(f.buffer_mut(), area, view, &theme, chrome);
        })
        .expect("draw");
    terminal.backend().clone()
}

fn chrome() -> Chrome {
    Chrome {
        worktree: "vigia".to_owned(),
        notice: None,
    }
}

fn line(kind: LineKind, number: u32, text: &str) -> Row {
    Row::Line {
        kind,
        number,
        text: text.to_owned(),
    }
}

fn file(kind: char, path: &str, added: u32, removed: u32) -> Row {
    Row::File {
        path: path.to_owned(),
        from: None,
        kind,
        churn: Some((added, removed)),
    }
}

/// A view with the shape a real frame produces: a file, a hunk, mixed lines.
fn one_file() -> View {
    View {
        rows: vec![
            file('M', "crates/vigia-core/src/frame.rs", 3, 1),
            Row::Hunk {
                old_start: 258,
                old_lines: 7,
                new_start: 258,
                new_lines: 9,
            },
            line(LineKind::Context, 258, "    pub fn advance(&mut self) {"),
            line(
                LineKind::Context,
                259,
                "        let mut files = Vec::new();",
            ),
            line(
                LineKind::Removed,
                260,
                "        for change in self.changes() {",
            ),
            line(LineKind::Added, 260, "        for change in self.walk() {"),
            line(LineKind::Added, 261, "            // one per path"),
            line(LineKind::Added, 262, "            files.push(change?);"),
            line(LineKind::Context, 263, "        }"),
            line(LineKind::Context, 264, "    }"),
        ],
        files: 1,
        top: Position::default(),
        read: 1,
    }
}

#[test]
fn a_screenful_of_diff() {
    let view = one_file();
    insta::assert_snapshot!(screen(80, 14, &view, &chrome()));
}

#[test]
fn the_same_screenful_at_forty_columns() {
    // The baseline #7 will be measured against, not a claim that I6 holds.
    let view = one_file();
    insta::assert_snapshot!(screen(40, 14, &view, &chrome()));
}

#[test]
fn a_clean_worktree_says_so_rather_than_showing_nothing() {
    // A monitor is read by glancing at it, so "nothing has changed" and "I am
    // broken" must not look identical. An empty pane says both.
    let view = View {
        rows: Vec::new(),
        files: 0,
        top: Position::default(),
        read: 0,
    };
    insta::assert_snapshot!(screen(40, 6, &view, &chrome()));
}

#[test]
fn a_file_with_no_line_diff_says_why() {
    let view = View {
        rows: vec![
            Row::File {
                path: "assets/banner.jpg".to_owned(),
                from: None,
                kind: 'M',
                churn: None,
            },
            Row::Note("binary"),
            Row::File {
                path: "src/merge.rs".to_owned(),
                from: None,
                kind: 'U',
                churn: None,
            },
            Row::Note("unresolved conflict"),
            Row::File {
                path: "crates/vigia/src/shell.rs".to_owned(),
                from: Some("crates/vigia/src/main.rs".to_owned()),
                kind: 'R',
                churn: Some((0, 0)),
            },
        ],
        files: 3,
        top: Position::default(),
        read: 3,
    };
    insta::assert_snapshot!(screen(60, 8, &view, &chrome()));
}

#[test]
fn a_path_too_long_to_fit_keeps_the_end_that_names_the_file() {
    // Losing the tail would leave a column of `crates/vigia-core/…`, which names
    // nothing. This is the truncated-to-useless shape I6 forbids, and it is the
    // one part of I6 the renderer decides on its own rather than by layout.
    let view = View {
        rows: vec![file(
            'M',
            "crates/vigia-core/src/very/deeply/nested/module/frame.rs",
            12,
            3,
        )],
        files: 1,
        top: Position::default(),
        read: 1,
    };
    insta::assert_snapshot!(screen(40, 4, &view, &chrome()));
}

#[test]
fn a_notice_takes_the_footer_from_the_key_hints() {
    let view = one_file();
    let chrome = Chrome {
        worktree: "vigia".to_owned(),
        notice: Some("the index entry for src/lib.rs points at a missing blob".to_owned()),
    };
    insta::assert_snapshot!(screen(80, 6, &view, &chrome));
}

#[test]
fn tabs_become_columns_and_control_characters_become_visible() {
    // Not cosmetic. A raw tab occupies one cell and advances nothing, so every
    // column after it is wrong; an escape character written through to the
    // terminal can move the cursor or open a sequence, which corrupts the whole
    // screen rather than one row. Both arrive from ordinary files that nobody
    // wrote for a display.
    let view = View {
        rows: vec![
            file('M', "Makefile", 1, 0),
            line(LineKind::Added, 1, "\tcargo build\ta\tb"),
            line(LineKind::Context, 2, "bell\u{7}esc\u{1b}[31mnul\u{0}"),
        ],
        files: 1,
        top: Position::default(),
        read: 1,
    };
    let backend = screen(60, 5, &view, &chrome());
    let rendered = format!("{backend}");
    assert!(
        !rendered.contains('\t') && !rendered.contains('\u{1b}') && !rendered.contains('\u{7}'),
        "a control character reached the buffer:\n{rendered}"
    );
    insta::assert_snapshot!(backend);
}

#[test]
fn the_gutter_gives_way_before_the_text_does() {
    // The rule is that line numbers go when they would leave the content less
    // than a readable column. Both sides are asserted, because a rule that only
    // ever fires one way is not a rule.
    let view = View {
        rows: vec![line(LineKind::Added, 1234, "let value = compute(input);")],
        files: 1,
        top: Position::default(),
        read: 1,
    };

    let wide = format!("{}", screen(40, 3, &view, &chrome()));
    assert!(
        wide.contains("1234"),
        "forty columns dropped the gutter, which is the width I6 is about:\n{wide}"
    );

    let narrow = format!("{}", screen(24, 3, &view, &chrome()));
    assert!(
        !narrow.contains("1234"),
        "the gutter survived into a width where it costs more than it explains:\n{narrow}"
    );
    assert!(
        narrow.contains("let value"),
        "the content lost its start, which is the wrong thing to spend:\n{narrow}"
    );
}

#[test]
fn any_area_renders_including_the_ones_that_fit_nothing() {
    // A pane being dragged narrow steps through every one of these sizes. A
    // monitor that panics on the way is worse than one that draws something
    // cramped, and `body_height` is what the caller uses to ask for rows, so the
    // two have to agree about a screen with no body at all.
    let view = one_file();
    for (width, height) in [(0, 0), (1, 1), (1, 2), (80, 1), (80, 2), (80, 3), (2, 30)] {
        let backend = screen(width, height, &view, &chrome());
        let area = ratatui::layout::Rect::new(0, 0, width, height);
        assert_eq!(
            body_height(area),
            usize::from(height).saturating_sub(2),
            "body_height disagreed with the renderer at {width}x{height}"
        );
        // Non-vacuity: the loop must actually have produced a buffer of the size
        // asked for, or it proved only that nothing was drawn.
        assert_eq!(backend.buffer().area.width, width);
        assert_eq!(backend.buffer().area.height, height);
    }
}

#[test]
fn the_palette_reaches_the_cells() {
    // Snapshots cannot see this: `TestBackend`'s `Display` writes symbols and
    // drops styles, so every colour in the theme is invisible to the rest of this
    // file. Without this test the palette could be one colour throughout and the
    // whole suite would stay green.
    let view = one_file();
    let backend = screen(80, 14, &view, &chrome());
    let buffer = backend.buffer();
    let theme = Theme::default();

    let row_of = |needle: char, y: u16| {
        let cell = &buffer[(0, y)];
        assert_eq!(
            cell.symbol(),
            needle.to_string(),
            "row {y} does not start with {needle:?}, so this test is reading the \
             wrong line"
        );
    };

    // Body rows start at y = 1: the header is y = 0. The gutter occupies the
    // first columns, so the sigil and its colour are found past it.
    let sigil_x = 4;
    let removed = &buffer[(sigil_x, 5)];
    let added = &buffer[(sigil_x, 6)];
    assert_eq!(removed.symbol(), "-", "expected the removed line at y=5");
    assert_eq!(added.symbol(), "+", "expected the added line at y=6");
    assert_eq!(removed.style().fg, theme.removed.fg);
    assert_eq!(added.style().fg, theme.added.fg);
    assert_ne!(
        removed.style().fg,
        added.style().fg,
        "added and removed lines are the same colour, which is the one thing the \
         palette exists to prevent"
    );
    assert_eq!(
        buffer[(0, 5)].style().fg,
        theme.gutter.fg,
        "the line number is not drawn in the gutter colour"
    );
    row_of('v', 0);
}
