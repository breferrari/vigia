//! `SPEC.md` §11.2 B20: what a drag over the diff selects, and what `y` then sends.
//!
//! The ruling turns on one distinction: a cell holds what was *drawn*, and the
//! copy is resolved against the row model instead. Everything here is either that
//! distinction or the gesture that reaches it.

#[path = "../../vigia-core/tests/support/mod.rs"]
mod support;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{Event, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
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
    sent_on(app, frame, span, wrap, PANE)
}

/// The same, on a pane of a named size.
fn sent_on(
    app: &mut App,
    frame: &mut Frame,
    span: Option<(usize, usize)>,
    wrap: bool,
    pane: Rect,
) -> Option<String> {
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    if wrap {
        app.apply(Action::ToggleWrap, frame, 24).expect("wrap");
    }
    app.select(span);
    let chrome = app.chrome("fixture", None, Pointing::default(), 0, "");
    let body = body_layout(pane, &chrome, 1, 1);
    app.view(frame, &mut highlighter, &history, body)
        .expect("view");
    app.apply(Action::Yank, frame, 24).expect("yank");
    app.take_yank().map(|yank| yank.text)
}

/// The width I6 is named for, and the width a deep path has to be shortened at.
const NARROW: Rect = Rect {
    x: 0,
    y: 0,
    width: 40,
    height: 24,
};

/// Everything the pane draws, as text.
fn drawn(app: &mut App, frame: &mut Frame) -> String {
    drawn_on(app, frame, PANE)
}

/// The same, on a pane of a named size.
fn drawn_on(app: &mut App, frame: &mut Frame, pane: Rect) -> String {
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let chrome = app.chrome("fixture", None, Pointing::default(), 0, "");
    let body = body_layout(pane, &chrome, 1, 1);
    let view = app
        .view(frame, &mut highlighter, &history, body)
        .expect("view");
    let theme = Theme::default();
    let mut terminal = Terminal::new(TestBackend::new(pane.width, pane.height)).expect("terminal");
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

    // The precondition: wrapping really did split it, so `hits == 1` below cannot
    // pass by the line never having been cut at all.
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let mut probe = App::new();
    probe
        .apply(Action::ToggleWrap, &mut frame, 24)
        .expect("wrap");
    let chrome = probe.chrome("fixture", None, Pointing::default(), 0, "");
    let body = body_layout(PANE, &chrome, 1, 1);
    let view = probe
        .view(&mut frame, &mut highlighter, &history, body)
        .expect("view");
    assert!(
        view.rows.iter().any(vigia::Row::is_wrap),
        "nothing wrapped, so this gate is not the case it is named for"
    );

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

    // The precondition: at this width the pane really did shorten the label, so
    // a copy taken from the cells could not have reached the path.
    let screen = drawn_on(&mut app, &mut frame, NARROW);
    assert!(
        screen.contains('…'),
        "the pane drew {DEEP:?} whole at {} columns, so this gate is not the case it \
         is named for:\n{screen}",
        NARROW.width
    );
    let text =
        sent_on(&mut app, &mut frame, Some((0, 0)), false, NARROW).expect("`y` sent nothing");
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

/// A hunk header is sent in the spelling the pane draws, which drops the `,1` git
/// drops. `@@ -258,1 +25,1 @@` is not a verbose header, it is a different one.
#[test]
fn a_hunk_row_sends_the_header_the_pane_draws() {
    let scratch = scratch_with("select-hunk", "src/a.rs", "one\n");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();

    let screen = drawn(&mut app, &mut frame);
    let drawn_header = screen
        .lines()
        .find_map(|line| {
            line.split_whitespace()
                .find(|word| word.starts_with("@@"))
                .map(|_| line.trim().to_owned())
        })
        .expect("the pane drew no hunk header");
    let text = sent(&mut app, &mut frame, Some((0, 23)), false).expect("`y` sent nothing");
    let header = text
        .lines()
        .find(|line| line.starts_with("@@"))
        .expect("no hunk header was sent");
    assert!(
        drawn_header.contains(header),
        "the pane draws {drawn_header:?} and `y` sent {header:?}, so the copy spells \
         the header a second way"
    );
}

/// **The edge the first gate missed, and the one the ruling turns on.** A wrapped
/// line at the foot of the pane loses its last pieces from the row model, because
/// the walk emits only what fits. The copy carries them anyway.
///
/// The gate proves the loss before it asserts the recovery: without that it passes
/// on a pane that drew the whole line, which is how it passed the first time.
#[test]
fn a_wrapped_line_at_the_foot_of_the_pane_is_still_sent_whole() {
    // Nine, and the count is load bearing at both ends: fewer and every piece fits,
    // more and the line is taller than the pane, where the walk extends its last
    // piece to the end and loses nothing. The lossy window is only between.
    let huge = [LONG; 9].join(" ");
    let scratch = scratch_with(
        "select-foot",
        "src/long.rs",
        &format!("let a = 1;\nlet b = 2;\n    {huge}\n"),
    );
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    app.apply(Action::ToggleWrap, &mut frame, 24).expect("wrap");
    let chrome = app.chrome("fixture", None, Pointing::default(), 0, "");
    let body = body_layout(PANE, &chrome, 1, 1);
    let view = app
        .view(&mut frame, &mut highlighter, &history, body)
        .expect("view");

    // The precondition. Every piece the walk emitted, joined: if that is already the
    // whole line then nothing was dropped and the assertion below proves nothing.
    let on_screen: usize = view
        .rows
        .iter()
        .filter_map(|row| match row {
            vigia::Row::Line { text, .. } | vigia::Row::Wrap { text, .. } => Some(text.len()),
            _ => None,
        })
        .sum();
    assert!(
        on_screen < huge.len(),
        "the pane drew all {} bytes of the wrapped line, so the walk dropped nothing \
         and this gate is not the case it is named for",
        huge.len()
    );

    let lines = view
        .lines_in((0, view.rows.len() - 1))
        .expect("nothing resolved");
    assert!(
        lines.iter().any(|line| line.contains(&huge)),
        "the line at the foot of the pane was sent cut at a column: the walk kept \
         {on_screen} bytes of {} and the copy carried no more",
        huge.len()
    );
}

/// Losing focus is not one of the things B20 says clears a wash, and this pane is
/// built to sit beside one a reader types into.
#[test]
fn losing_focus_does_not_clear_the_wash() {
    let regions = regions_at(2, 20);
    let standing = selection_after(&press(10, 5), regions, None).expect("opened");
    assert_eq!(
        selection_after(&Event::FocusLost, regions, Some(standing)),
        Some(standing),
        "clicking into the agent's pane threw away a selection the reader was about to send"
    );
}

/// The sheet is drawn over the pane, so a press while it is up belongs to it. A
/// wash opened beside it would take `Esc` off the frontmost thing.
#[test]
fn a_press_while_the_sheet_is_up_opens_nothing() {
    let mut regions = regions_at(2, 20);
    regions.sheet = Some(vigia::Sheet {
        left: 30,
        top: 4,
        width: 20,
        height: 10,
        close: (48, 4),
    });
    assert!(
        selection_after(&press(2, 12), regions, None).is_none(),
        "a press on the diff beside the sheet opened a wash, so `Esc` would clear it \
         instead of closing the sheet"
    );
}

/// The footer names lines, not rows: a wrapped line is several rows and one line,
/// and a count that said otherwise would disagree with what was sent.
#[test]
fn the_footer_counts_lines_and_not_rows() {
    let scratch = scratch_with(
        "select-said",
        "src/long.rs",
        &format!("fn f() {{\n    {LONG}\n}}\n"),
    );
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);

    for (span, want) in [((2usize, 2usize), "1 line"), ((2, 4), "3 lines")] {
        let mut app = App::new();
        let mut highlighter = Highlighter::eager();
        let history = History::new();
        app.select(Some(span));
        let chrome = app.chrome("fixture", None, Pointing::default(), 0, "");
        let body = body_layout(PANE, &chrome, 1, 1);
        app.view(&mut frame, &mut highlighter, &history, body)
            .expect("view");
        app.apply(Action::Yank, &mut frame, 24).expect("yank");
        let yank = app.take_yank().expect("`y` yanked nothing");
        assert_eq!(
            yank.said,
            want,
            "a span of {span:?} is named {:?} on the footer and sent {} lines",
            yank.said,
            yank.text.lines().count()
        );
    }
}

/// A wash cleared between paints takes its lines with it. Without that, `y` goes
/// on sending rows nothing on screen is claiming, and after a tick that moved the
/// region they are not even the rows the reader crossed.
#[test]
fn a_cleared_wash_sends_the_caret_path_again() {
    let scratch = scratch_with("select-retired", DEEP, "one\ntwo\nthree\n");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();

    // Resolved, so the lines are in hand the way a standing wash leaves them.
    assert!(
        sent(&mut app, &mut frame, Some((2, 3)), false).is_some_and(|text| text != DEEP),
        "the span resolved to nothing, so clearing it below proves nothing"
    );
    // Cleared with no collect after it, which is what a retired wash looks like.
    app.select(None);
    app.apply(Action::Yank, &mut frame, 24).expect("yank");
    assert_eq!(
        app.take_yank().map(|yank| yank.text).as_deref(),
        Some(DEEP),
        "`y` sent the cleared selection's lines rather than the caret file's path"
    );
}

/// An empty payload is no payload. OSC 52 carrying nothing **clears** the reader's
/// clipboard, which is the one thing B9's surviving ground refuses, and the footer
/// would have said it sent something.
#[test]
fn a_selection_with_no_text_in_it_never_reaches_the_clipboard() {
    let scratch = scratch_with("select-blank", DEEP, "one\n\ntwo\n");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    let chrome = app.chrome("fixture", None, Pointing::default(), 0, "");
    let body = body_layout(PANE, &chrome, 1, 1);
    let view = app
        .view(&mut frame, &mut highlighter, &history, body)
        .expect("view");
    let blank = view
        .rows
        .iter()
        .position(|row| matches!(row, vigia::Row::Line { text, .. } if text.is_empty()))
        .expect("the fixture drew no empty line, so this gate is not its own case");

    let text = sent(&mut app, &mut frame, Some((blank, blank)), false)
        .expect("`y` sent nothing at all, where it owes the caret path");
    assert_eq!(
        text, DEEP,
        "a selection with no text in it sent {text:?}; an empty OSC 52 write would \\
         have wiped whatever the reader had on their clipboard"
    );
}
