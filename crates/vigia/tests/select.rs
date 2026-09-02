//! `SPEC.md` §11.2 B20: what a drag over the diff selects, and what `y` then sends.
//!
//! The ruling turns on one distinction: a cell holds what was *drawn*, and the
//! copy is resolved against the row model instead. Everything here is either that
//! distinction or the gesture that reaches it.

#[path = "../../vigia-core/tests/support/mod.rs"]
mod support;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::Rect;
use vigia::{
    Action, App, Glyphs, Pointing, Region, Regions, Theme, body_layout, render, selection_after,
};
use vigia_core::{Frame, Highlighter, History};

use support::{Scratch, materialise};

/// Wide enough that nothing wraps by accident, narrow enough that the line below
/// cannot fit.
const PANE: Rect = Rect {
    x: 0,
    y: 0,
    width: 80,
    height: 24,
};

/// A content line the pane must clip: its tail is the thing no cell holds, and it
/// is the whole reason the copy does not come from cells.
const LONG: &str = "let tail = the_part_of_this_line_that_no_eighty_column_pane_will_ever_draw_in_full(argument, another_argument, a_third);";

/// A path the pane has to elide at any sensible width, so a heading row proves the
/// same distinction the line does.
const DEEP: &str = "crates/vigia-core/src/very/deeply/nested/module/frame.rs";

/// A diff region with no bar, so a press anywhere in it opens a selection.
fn regions_at(top: u16, rows: u16) -> Regions {
    Regions {
        diff: Region::bare(top, rows, 0, PANE.width, None),
        ..Regions::default()
    }
}

fn at(kind: MouseEventKind, column: u16, row: u16) -> Event {
    Event::Mouse(MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    })
}

fn press(column: u16, row: u16) -> Event {
    at(MouseEventKind::Down(MouseButton::Left), column, row)
}

fn drag(column: u16, row: u16) -> Event {
    at(MouseEventKind::Drag(MouseButton::Left), column, row)
}

/// A worktree holding `file`, written with `body`.
fn scratch_with(name: &str, file: &str, body: &str) -> Scratch {
    let scratch = Scratch::new(name);
    scratch.write(file, body);
    scratch
}

/// Collect one view with `span` selected, and hand back what `y` would send.
fn sent(
    app: &mut App,
    frame: &mut Frame,
    span: Option<(usize, usize)>,
    wrap: bool,
) -> Option<String> {
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    if wrap {
        app.apply(Action::ToggleWrap, frame, 24).expect("wrap");
    }
    app.select(span);
    let chrome = app.chrome("fixture", None, Pointing::default(), 0, "");
    let body = body_layout(PANE, &chrome, 1, 1);
    app.view(frame, &mut highlighter, &history, body)
        .expect("view");
    app.apply(Action::Yank, frame, 24).expect("yank");
    app.take_yank().map(|yank| yank.text)
}

/// Everything the pane draws, as text.
fn drawn(app: &mut App, frame: &mut Frame) -> String {
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let chrome = app.chrome("fixture", None, Pointing::default(), 0, "");
    let body = body_layout(PANE, &chrome, 1, 1);
    let view = app
        .view(frame, &mut highlighter, &history, body)
        .expect("view");
    let theme = Theme::default();
    let mut terminal = Terminal::new(TestBackend::new(PANE.width, PANE.height)).expect("terminal");
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
    let buffer = terminal.backend().buffer().clone();
    (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol().to_owned())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// A press opens a selection of the one row it landed on, and a drag moves the
/// head without moving the anchor.
#[test]
fn a_drag_selects_the_rows_it_crosses() {
    let regions = regions_at(2, 20);
    let opened = selection_after(&press(10, 5), regions, None).expect("a press opened nothing");
    assert_eq!(
        opened.rows(),
        (5, 5),
        "a press selected more than its own row"
    );

    let moved = selection_after(&drag(10, 9), regions, Some(opened)).expect("a drag ended it");
    assert_eq!(
        moved.rows(),
        (5, 9),
        "the drag did not carry the head with it"
    );
}

/// The same drag upward covers the same rows, because the span is ordered rather
/// than anchored to whichever end the pointer reached last.
#[test]
fn a_drag_upward_selects_what_the_same_drag_downward_would() {
    let regions = regions_at(2, 20);
    let down = selection_after(
        &drag(10, 9),
        regions,
        selection_after(&press(10, 5), regions, None),
    )
    .expect("down");
    let up = selection_after(
        &drag(10, 5),
        regions,
        selection_after(&press(10, 9), regions, None),
    )
    .expect("up");
    assert_eq!(down.rows(), up.rows(), "the two directions do not agree");
}

/// A drag that leaves the region reaches its edge and stops, rather than reaching
/// into the chrome above or below it.
#[test]
fn a_drag_out_of_the_region_stops_at_its_edge() {
    let regions = regions_at(2, 20);
    let opened = selection_after(&press(10, 5), regions, None).expect("opened");
    let below = selection_after(&drag(10, 200), regions, Some(opened)).expect("dropped");
    assert_eq!(
        below.rows(),
        (5, 21),
        "the drag reached past the region's last row"
    );
    let above = selection_after(&drag(10, 0), regions, Some(opened)).expect("dropped");
    assert_eq!(
        above.rows(),
        (2, 5),
        "the drag reached above the region's first row"
    );
}

/// A press that is not on the diff takes the wash with it, which is the clearing
/// rule reaching the one gesture that has no action behind it.
#[test]
fn a_press_off_the_diff_clears_the_selection() {
    let regions = regions_at(2, 20);
    let standing = selection_after(&press(10, 5), regions, None).expect("opened");
    assert!(
        selection_after(&press(10, 1), regions, Some(standing)).is_none(),
        "a press above the diff left the wash standing"
    );
}

/// A window that lost focus has ended the gesture, exactly as it ends a hold.
#[test]
fn a_lost_focus_ends_it() {
    let regions = regions_at(2, 20);
    let standing = selection_after(&press(10, 5), regions, None).expect("opened");
    assert!(
        selection_after(&Event::FocusLost, regions, Some(standing)).is_none(),
        "the wash survived the window losing focus"
    );
    assert_eq!(
        selection_after(
            &Event::Key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE)),
            regions,
            Some(standing)
        ),
        Some(standing),
        "a key changed the span here, where the loop is what clears it"
    );
}

/// **The invariant the whole ruling rests on.** The pane clips a long line, so no
/// cell holds its tail; the copy comes from the row model and carries it anyway.
#[test]
fn the_sent_text_is_the_whole_line_where_the_pane_clipped_it() {
    let scratch = scratch_with(
        "select-clipped",
        "src/long.rs",
        &format!("fn f() {{\n    {LONG}\n}}\n"),
    );
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();

    let screen = drawn(&mut app, &mut frame);
    assert!(
        !screen.contains(LONG),
        "the pane drew the whole line, so a copy taken from cells would have reached \
         it and this gate proves nothing:\n{screen}"
    );

    // Every row of the collected screen, so whichever row the line landed on is in
    // the span. What is asserted is the payload, not where the drag started.
    let text = sent(&mut app, &mut frame, Some((0, 23)), false).expect("`y` sent nothing");
    assert!(
        text.contains(LONG),
        "`y` sent {text:?}, which does not carry the tail the pane clipped, so it is \
         a reading of the screen by another route"
    );
    assert!(
        !text.contains('…') && !text.contains('›'),
        "the sent text carries a renderer's mark, so it came from the cells: {text:?}"
    );
}

/// A wrapped line is several rows and one line, so it is sent once and whole
/// however many rows the reader's drag crossed.
#[test]
fn a_wrapped_line_is_sent_once_and_whole() {
    let scratch = scratch_with(
        "select-wrapped",
        "src/long.rs",
        &format!("fn f() {{\n    {LONG}\n}}\n"),
    );
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();

    let text = sent(&mut app, &mut frame, Some((0, 23)), true).expect("`y` sent nothing");
    let hits = text.matches(LONG).count();
    assert_eq!(
        hits, 1,
        "the wrapped line was sent {hits} times rather than once, so a continuation \
         row is being taken for a line of its own: {text:?}"
    );
}

/// A heading sends its repository-relative path, not the label the pane elided,
/// which is the same distinction the line above makes one region over.
#[test]
fn a_heading_row_sends_the_path_and_not_the_drawn_label() {
    let scratch = scratch_with("select-heading", DEEP, "one\ntwo\n");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();

    let text = sent(&mut app, &mut frame, Some((0, 0)), false).expect("`y` sent nothing");
    assert_eq!(
        text, DEEP,
        "the heading row sent {text:?} rather than the path it stands for"
    );
}

/// With nothing selected `y` is what B9 shipped, which is the half of this ruling
/// that must not have moved.
#[test]
fn with_no_selection_y_still_sends_the_caret_path() {
    let scratch = scratch_with("select-fallback", DEEP, "one\n");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();

    assert_eq!(
        sent(&mut app, &mut frame, None, false).as_deref(),
        Some(DEEP),
        "`y` with nothing selected stopped sending the caret file's path"
    );
}

/// A span nothing was collected for sends nothing at all, rather than an empty
/// string the reader would have to notice on the footer.
#[test]
fn a_span_past_the_collected_rows_sends_the_caret_path_instead() {
    let scratch = scratch_with("select-past", DEEP, "one\n");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();

    assert_eq!(
        sent(&mut app, &mut frame, Some((900, 999)), false).as_deref(),
        Some(DEEP),
        "a span past the end of the collected rows did not fall back to the caret"
    );
}

/// Everything the pane draws, as one background per row of the diff, plus the
/// regions the frame laid out.
fn washes(
    app: &mut App,
    frame: &mut Frame,
    selected: Option<vigia::Selection>,
) -> (Vec<ratatui::style::Color>, Regions) {
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let mut chrome = app.chrome("fixture", None, Pointing::default(), 0, "");
    let body = body_layout(PANE, &chrome, 1, 1);
    let view = app
        .view(frame, &mut highlighter, &history, body)
        .expect("view");
    chrome.selected = selected;
    // Named rather than defaulted: `ansi` reverses the row instead of colouring
    // it, so a background comparison there would pass by drawing nothing.
    let theme = Theme::named("dark").expect("the dark palette");
    let laid = vigia::regions(PANE, &chrome, &view);
    let mut terminal = Terminal::new(TestBackend::new(PANE.width, PANE.height)).expect("terminal");
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
    let buffer = terminal.backend().buffer().clone();
    let rows = (0..PANE.height).map(|y| buffer[(1, y)].bg).collect();
    (rows, laid)
}

/// The wash reaches every row the drag crossed and stops there. Without it a
/// selection is a copy nobody can see they have made.
#[test]
fn the_wash_covers_the_selected_rows_and_no_others() {
    let scratch = scratch_with(
        "select-wash",
        "src/a.rs",
        "one\ntwo\nthree\nfour\nfive\nsix\n",
    );
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();

    let (plain, laid) = washes(&mut app, &mut frame, None);
    let top = laid.diff.top;
    let opened = selection_after(&press(2, top + 2), laid, None).expect("opened");
    let span = selection_after(&drag(2, top + 4), laid, Some(opened)).expect("dragged");
    let (washed, _) = washes(&mut app, &mut frame, Some(span));

    let (from, to) = span.rows();
    for y in 0..PANE.height {
        let inside = y >= from && y <= to;
        if inside {
            assert_ne!(
                washed[y as usize], plain[y as usize],
                "row {y} is inside the selection and its background did not change"
            );
        } else {
            assert_eq!(
                washed[y as usize], plain[y as usize],
                "row {y} is outside the selection and its background changed anyway"
            );
        }
    }
}

/// §11.1: a transient mark may not move content. The regions a frame lays out are
/// the same with a selection standing as without one.
#[test]
fn a_selection_moves_no_rect() {
    let scratch = scratch_with("select-rects", "src/a.rs", "one\ntwo\nthree\n");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();

    let (_, plain) = washes(&mut app, &mut frame, None);
    let opened = selection_after(&press(2, plain.diff.top), plain, None).expect("opened");
    let (_, laid) = washes(&mut app, &mut frame, Some(opened));
    assert_eq!(
        (
            plain.diff.top,
            plain.diff.rows,
            plain.diff.left,
            plain.diff.width
        ),
        (
            laid.diff.top,
            laid.diff.rows,
            laid.diff.left,
            laid.diff.width
        ),
        "the diff region moved because a selection was standing"
    );
    assert_eq!(
        (plain.list.top, plain.list.rows),
        (laid.list.top, laid.list.rows),
        "the list region moved because a selection was standing"
    );
}
