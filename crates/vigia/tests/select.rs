//! `SPEC.md` §11.2 B20: what a drag over the diff washes, and what the button
//! coming up sends.
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
    Action, App, Glyphs, Pointing, Region, Regions, Selection, Theme, body_layout, render,
    selection_after,
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

fn release(column: u16, row: u16) -> Event {
    at(MouseEventKind::Up(MouseButton::Left), column, row)
}

/// The span still standing after `event`.
fn standing(event: &Event, regions: Regions, was: Option<Selection>) -> Option<Selection> {
    selection_after(event, regions, was).0
}

/// The span `event` ended, which is the one the release sends.
fn ended(event: &Event, regions: Regions, was: Option<Selection>) -> Option<Selection> {
    selection_after(event, regions, was).1
}

/// A worktree holding `file`, written with `body`.
fn scratch_with(name: &str, file: &str, body: &str) -> Scratch {
    let scratch = Scratch::new(name);
    scratch.write(file, body);
    scratch
}

/// Collect one view with `span` washed, and hand back what the release would send.
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
    let view = app
        .view(frame, &mut highlighter, &history, body)
        .expect("view");
    // The shell's own path, and the reason it is not `App`'s: the release resolves
    // against the frame last painted rather than against what a collect left behind.
    if let Some(lines) = span.and_then(|span| view.lines_in(span)) {
        app.send(&lines);
    }
    app.take_sending().map(|sending| sending.text)
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
    let opened = standing(&press(10, 5), regions, None).expect("a press opened nothing");
    assert_eq!(
        opened.rows(),
        (5, 5),
        "a press selected more than its own row"
    );

    let moved = standing(&drag(10, 9), regions, Some(opened)).expect("a drag ended it");
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
    let down = standing(
        &drag(10, 9),
        regions,
        standing(&press(10, 5), regions, None),
    )
    .expect("down");
    let up = standing(
        &drag(10, 5),
        regions,
        standing(&press(10, 9), regions, None),
    )
    .expect("up");
    assert_eq!(down.rows(), up.rows(), "the two directions do not agree");
}

/// A drag that leaves the region reaches its edge and stops, rather than reaching
/// into the chrome above or below it.
#[test]
fn a_drag_out_of_the_region_stops_at_its_edge() {
    let regions = regions_at(2, 20);
    let opened = standing(&press(10, 5), regions, None).expect("opened");
    let below = standing(&drag(10, 200), regions, Some(opened)).expect("dropped");
    assert_eq!(
        below.rows(),
        (5, 21),
        "the drag reached past the region's last row"
    );
    let above = standing(&drag(10, 0), regions, Some(opened)).expect("dropped");
    assert_eq!(
        above.rows(),
        (2, 5),
        "the drag reached above the region's first row"
    );
}

/// The wash lasts exactly as long as the gesture: the button coming up retires the
/// span, so nothing is left standing for a later key to send or for `Esc` to clear.
/// What it stood over reached the clipboard before this ran, in `Shell::send_wash`.
#[test]
fn the_button_coming_up_ends_the_span() {
    let regions = regions_at(2, 20);
    let opened = standing(&press(10, 5), regions, None).expect("opened");
    let dragged = standing(&drag(10, 9), regions, Some(opened)).expect("dragged");
    assert_ne!(
        dragged.rows(),
        (5, 5),
        "the drag moved no head, so the release below is not ending a real span"
    );
    assert_eq!(
        standing(&release(10, 9), regions, Some(dragged)),
        None,
        "the wash outlived the button, so it stands over rows the reader has \
         finished with and a later key could send them again"
    );
}

/// One event ends a drag and nothing else does. Anything that comes back as ended
/// is handed to `Shell::send_wash`, so anything answering here spends the reader's
/// clipboard.
#[test]
fn only_the_button_coming_up_ends_a_drag() {
    let regions = regions_at(2, 20);
    let had = standing(&press(10, 5), regions, None).expect("opened");
    assert_eq!(
        ended(&release(10, 5), regions, Some(had)),
        Some(had),
        "the release ended no drag, so nothing is sent and the gesture cannot finish"
    );
    for event in [
        press(10, 5),
        drag(10, 5),
        at(MouseEventKind::Up(MouseButton::Right), 10, 5),
        at(MouseEventKind::Moved, 10, 5),
        at(MouseEventKind::ScrollDown, 10, 5),
        Event::FocusLost,
        Event::Resize(80, 24),
    ] {
        assert_eq!(
            ended(&event, regions, Some(had)),
            None,
            "{event:?} ended a drag, so it sends whatever the wash was standing over"
        );
    }
}

/// A span is screen rows and the resolver indexes collected ones, so the origin is
/// the diff region's own first row. `Shell::send_wash` is the only place the two
/// coordinate systems meet and no test can drive it, so the arithmetic is driven here.
#[test]
fn a_span_resolves_from_the_diff_regions_first_row() {
    let regions = regions_at(2, 20);
    let top = regions.diff.top;
    let opened = standing(&press(10, top + 3), regions, None).expect("opened");
    let span = standing(&drag(10, top + 5), regions, Some(opened)).expect("dragged");
    assert_eq!(
        span.offsets(top),
        (3, 5),
        "the span did not resolve to offsets from the diff region's first row, so the \
         release would send rows the reader never crossed"
    );
    // A region that has moved below the span is a frame the wash no longer describes.
    // It clamps to the first collected row rather than wrapping to the last, which is
    // what `saturating_sub` is there for.
    assert_eq!(
        span.offsets(top + 9),
        (0, 0),
        "an origin below the span wrapped instead of clamping"
    );
}

/// A press that is not on the diff takes the wash with it, which is the clearing
/// rule reaching the one gesture that has no action behind it.
#[test]
fn a_press_off_the_diff_clears_the_selection() {
    let regions = regions_at(2, 20);
    let opened = standing(&press(10, 5), regions, None).expect("opened");
    assert!(
        standing(&press(10, 1), regions, Some(opened)).is_none(),
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
    assert!(
        !screen.contains(DEEP),
        "the pane drew the whole path, so a copy taken from the cells would have \
         reached it and this gate is answering a question nobody has:\n{screen}"
    );

    let text = sent_on(&mut app, &mut frame, Some((0, 0)), false, NARROW)
        .expect("the release sent nothing");
    assert_eq!(
        text, DEEP,
        "the heading row sent {text:?} rather than the path it stands for"
    );
    assert!(
        !text.contains('…'),
        "the sent string carries the renderer's elision mark, so it is a reading of \
         the screen rather than a path"
    );
}

/// A span past the collected rows is not a selection, so the release that ends it
/// sends nothing. There is no caret path to fall back to: the gesture that reaches
/// a path is a drag over the heading, and this drag reached no row at all.
#[test]
fn a_span_past_the_collected_rows_sends_nothing() {
    let scratch = scratch_with("select-past", DEEP, "one\n");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();

    assert_eq!(
        sent(&mut app, &mut frame, Some((900, 999)), false),
        None,
        "a span past the end of the collected rows put something on the clipboard"
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
    // Before the layout, so a layout that reserved a row for the wash is caught too.
    chrome.selected = selected;
    let body = body_layout(PANE, &chrome, 1, 1);
    let view = app
        .view(frame, &mut highlighter, &history, body)
        .expect("view");
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
    let opened = standing(&press(2, top + 2), laid, None).expect("opened");
    let span = standing(&drag(2, top + 4), laid, Some(opened)).expect("dragged");
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

/// And the frame after the button comes up carries none of it. The send happens
/// first, so a wash surviving here would be a second copy the reader could make of
/// rows they have already sent.
#[test]
fn the_frame_after_the_release_draws_no_wash() {
    let scratch = scratch_with(
        "select-wash-gone",
        "src/a.rs",
        "one\ntwo\nthree\nfour\nfive\nsix\n",
    );
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();

    let (plain, laid) = washes(&mut app, &mut frame, None);
    let top = laid.diff.top;
    let opened = standing(&press(2, top + 2), laid, None).expect("opened");
    let span = standing(&drag(2, top + 4), laid, Some(opened)).expect("dragged");
    let (washed, _) = washes(&mut app, &mut frame, Some(span));
    assert_ne!(
        washed, plain,
        "the drag washed nothing, so the release below has nothing to clear"
    );

    let after = standing(&release(2, top + 4), laid, Some(span));
    let (drawn, _) = washes(&mut app, &mut frame, after);
    assert_eq!(
        drawn, plain,
        "a row is still washed on the frame after the button came up"
    );
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
    let opened = standing(&press(2, plain.diff.top), plain, None).expect("opened");
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
    let opened = standing(&press(10, 5), regions, None).expect("opened");
    assert_eq!(
        standing(&Event::FocusLost, regions, Some(opened)),
        Some(opened),
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
        standing(&press(2, 12), regions, None).is_none(),
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

    for (wrap, span, want) in [
        (false, (2usize, 2usize), "1 line"),
        (false, (2, 4), "3 lines"),
        // Wrapped, so rows and lines come apart and a count of rows would say more.
        (true, (3, 4), "1 line"),
    ] {
        let mut app = App::new();
        let mut highlighter = Highlighter::eager();
        let history = History::new();
        if wrap {
            app.apply(Action::ToggleWrap, &mut frame, 24).expect("wrap");
        }
        app.select(Some(span));
        let chrome = app.chrome("fixture", None, Pointing::default(), 0, "");
        let body = body_layout(PANE, &chrome, 1, 1);
        let view = app
            .view(&mut frame, &mut highlighter, &history, body)
            .expect("view");
        // The wrapped case is only its own case while the span really covers a
        // continuation row: without this it is two more rows of the first case.
        assert_eq!(
            wrap,
            view.rows[span.0..=span.1].iter().any(vigia::Row::is_wrap),
            "the wrapped span covers no continuation row, so it proves nothing about \
             rows against lines"
        );
        let lines = view.lines_in(span).expect("the span resolved to nothing");
        app.send(&lines);
        let sending = app.take_sending().expect("the release sent nothing");
        assert_eq!(
            sending.said,
            want,
            "a span of {span:?} is named {:?} on the footer and sent {} lines",
            sending.said,
            sending.text.lines().count()
        );
    }
}

/// The count is the rows the drag covered, and it cannot be recovered from the
/// payload. `Sending` carries it rather than deriving it because a span ending on a
/// blank row joins to a string whose trailing newline `str::lines` does not count,
/// so a derived count would under-report exactly the drag that ends on one.
#[test]
fn a_span_ending_on_a_blank_row_is_still_counted_whole() {
    let scratch = scratch_with("select-trailing-blank", DEEP, "one\n\ntwo\n");
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
    assert!(
        blank > 0,
        "the blank row is the first, so there is no row above it"
    );

    let view = app
        .view(&mut frame, &mut highlighter, &history, body)
        .expect("view");
    let lines = view
        .lines_in((blank - 1, blank))
        .expect("the span resolved to nothing");
    app.send(&lines);
    let sending = app.take_sending().expect("the release sent nothing");
    assert_eq!(
        sending.text.lines().count(),
        1,
        "the payload's own line count is not the case this gate is named for"
    );
    assert_eq!(
        sending.said, "2 lines",
        "the footer named {:?} for a two-row span ending on a blank line",
        sending.said
    );
}

/// A wash cleared between paints takes its lines with it. Without that, a release
/// goes on sending rows nothing on screen is claiming, and after a tick that moved
/// the region they are not even the rows the reader crossed.
#[test]
fn a_cleared_wash_sends_nothing() {
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
    assert_eq!(
        sent(&mut app, &mut frame, None, false),
        None,
        "a release after the wash was cleared sent the lines it used to stand over"
    );
}

/// An empty payload is no payload. OSC 52 carrying nothing **clears** the reader's
/// clipboard, which is the one thing the surviving ownership ground refuses, and
/// the footer would have said it sent something.
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

    assert_eq!(
        sent(&mut app, &mut frame, Some((blank, blank)), false),
        None,
        "a selection with no text in it reached the clipboard; an empty OSC 52 write \
         wipes whatever the reader had on theirs"
    );
}

/// A span over rows the diff region owns and the walk had nothing for is not a
/// selection: it draws nothing, and the loop retires it on this answer. Left
/// standing it took `Esc`, which on a clean worktree is the whole keyboard a
/// reader has.
#[test]
fn a_span_the_walk_had_no_rows_for_holds_no_selection() {
    let scratch = scratch_with("select-thin", "src/a.rs", "one\n");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    let mut collect = |app: &mut App, frame: &mut Frame, span| {
        app.select(Some(span));
        let chrome = app.chrome("fixture", None, Pointing::default(), 0, "");
        let body = body_layout(PANE, &chrome, 1, 1);
        app.view(frame, &mut highlighter, &history, body)
            .expect("view")
    };

    let view = collect(&mut app, &mut frame, (0, 1));
    assert!(
        app.holds_a_selection(),
        "a span over rows the walk did fill resolved to nothing, so the case below \
         is not the one this gate is named for"
    );

    // The rows the region owns but the diff has no content for, which is every row
    // below the walk on a pane taller than the diff.
    let below = view.rows.len();
    collect(&mut app, &mut frame, (below, below + 3));
    assert!(
        !app.holds_a_selection(),
        "a span the walk had no rows for still counts as a selection, so a click on \
         an empty pane goes on swallowing `Esc`"
    );
}
