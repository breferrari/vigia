//! The gestures sheet: `?`, the window it opens, and the one thing it must not do.
//!
//! `SPEC.md` §11.1's B12 ruled the keymap out of the footer and over the pane,
//! and the ruling's load-bearing claim is not about space. It is that **an
//! overlay cannot move content**, which is the one property no answer living in
//! the footer has. So the gate that matters most here is not that the sheet draws
//! correctly, it is `the_sheet_moves_no_content`: every cell outside the sheet's
//! own rect is identical with it up and with it down.
//!
//! The rest are the constraints the ruling names, each written where it fails
//! rather than where it is convenient: a write under the sheet must not dismiss
//! it, the close control must dismiss it, the sheet must swallow what lands on it
//! rather than letting a click seek a scrollbar nobody can see, and it degrades on
//! two axes with a floor below which it is not drawn while `?` still toggles.
//!
//! Views are built from a **real repository** here rather than by hand, unlike
//! `render.rs`, and that is forced by the subject: two of these gates are about
//! what a *frame* does to the sheet, and a hand-built view has no frame behind it.

#[path = "../../vigia-core/tests/support/mod.rs"]
mod support;

use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::Rect;
use vigia::{Action, App, Chrome, Regions, Theme, action_for, body_layout, regions, render};
use vigia_core::{Frame, Highlighter, History};

use support::{Scratch, materialise};

const WIDE: u16 = 80;
const TALL: u16 = 24;
const FILES: usize = 3;

/// The word the sheet's own title bar spells, restated rather than imported.
///
/// A test that shared the renderer's constant would agree with it by
/// construction, which is the rule this suite already follows for the sparkline's
/// ramp, the rule glyph and the hint separator.
const TITLE: &str = "gestures";

fn area() -> Rect {
    Rect::new(0, 0, WIDE, TALL)
}

fn chrome(app: &App) -> Chrome {
    app.chrome("fixture", Some("main"), None, None, None, None)
}

fn press(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn click(column: u16, row: u16) -> Event {
    Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::NONE,
    })
}

fn wheel(column: u16, row: u16) -> Event {
    Event::Mouse(MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    })
}

/// Press `?`, through the app rather than around it.
fn toggle(app: &mut App, frame: &mut Frame<'_>) {
    let height = body_layout(area(), &chrome(app), FILES).diff;
    assert!(
        app.apply(Action::ToggleSheet, frame, height)
            .expect("toggle"),
        "the sheet's toggle asked the shell to quit"
    );
}

/// One painted frame at `at`, and the regions that frame published.
fn paint(
    app: &mut App,
    frame: &mut Frame<'_>,
    highlighter: &mut Highlighter,
    history: &History,
    at: Rect,
) -> (Buffer, Regions) {
    let chrome = chrome(app);
    let body = body_layout(at, &chrome, FILES);
    let view = app
        .view(frame, highlighter, history, body)
        .expect("collect a view");
    let mut buf = Buffer::empty(at);
    render(&mut buf, at, &view, &Theme::default(), &chrome);
    let laid = regions(at, &chrome, &view);
    (buf, laid)
}

fn text_of(buf: &Buffer, at: Rect) -> String {
    (at.y..at.y + at.height)
        .map(|y| {
            (at.x..at.x + at.width)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn the_shell_starts_with_no_sheet_and_question_mark_is_what_opens_one() {
    // The key map first, because it is the half a reader reaches for.
    assert_eq!(
        action_for(&press(KeyCode::Char('?')), Regions::default()),
        Some(Action::ToggleSheet),
        "`?` is not bound to the sheet"
    );
    // **`Esc` is still Quit**, which is B12's one refusal and the reason it gave:
    // teaching `Esc` to dismiss would put it one keystroke from ending the
    // program. A gate rather than a comment, because this is the arm a later
    // reader is likeliest to 'improve'.
    assert_eq!(
        action_for(&press(KeyCode::Esc), Regions::default()),
        Some(Action::Quit),
        "`Esc` stopped quitting, so the sheet has taught the wrong reflex"
    );

    let scratch = Scratch::large_diff("sheet-open", FILES, 40);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();
    let mut highlighter = Highlighter::new();
    let history = History::new();

    assert!(!chrome(&app).sheet, "a fresh shell drew a sheet");
    let (closed, laid) = paint(&mut app, &mut frame, &mut highlighter, &history, area());
    assert!(
        !text_of(&closed, area()).contains(TITLE),
        "the sheet was drawn before anybody asked for it"
    );
    assert!(
        laid.sheet.is_none(),
        "a sheet nobody opened is already eating gestures"
    );

    toggle(&mut app, &mut frame);
    assert!(chrome(&app).sheet, "`?` did not open the sheet");
    let (open, laid) = paint(&mut app, &mut frame, &mut highlighter, &history, area());
    assert!(
        text_of(&open, area()).contains(TITLE),
        "the sheet is on in the state and absent from the screen"
    );
    assert!(
        laid.sheet.is_some(),
        "the sheet is drawn and the pointer was not told"
    );

    toggle(&mut app, &mut frame);
    assert!(!chrome(&app).sheet, "`?` did not close the sheet again");
    let (shut, _) = paint(&mut app, &mut frame, &mut highlighter, &history, area());
    assert!(
        !text_of(&shut, area()).contains(TITLE),
        "closing the sheet left it drawn"
    );
}

#[test]
fn the_sheet_moves_no_content() {
    // **B12's load-bearing claim, and the reason it was ruled over anything living
    // in the footer.** Every cell outside the sheet's own rect is identical with it
    // up and with it down: the header, the footer, both regions, the scrollbars and
    // the rule. An overlay that resized anything fails here, and an answer that
    // grew the footer instead would have failed on the diff's last row.
    let scratch = Scratch::large_diff("sheet-still", FILES, 40);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    // **Past the first paint, and the first version of this gate was not.** I7
    // buys an imperceptible startup by drawing the opening frame *without
    // colour*, so comparing paint one against paint two compares plain against
    // coloured and reports every syntax span on the pane as having moved. It
    // failed exactly that way, on a `f` that was `Reset` before and `light_red`
    // after, which is I7 working rather than the sheet misbehaving.
    let mut app = App::past_first_paint();
    let mut highlighter = Highlighter::new();
    let history = History::new();

    let (before, _) = paint(&mut app, &mut frame, &mut highlighter, &history, area());
    toggle(&mut app, &mut frame);
    let (after, laid) = paint(&mut app, &mut frame, &mut highlighter, &history, area());
    let sheet = laid
        .sheet
        .expect("the sheet was not published to the pointer");

    let mut compared = 0;
    for y in 0..TALL {
        for x in 0..WIDE {
            if sheet.covers(x, y) {
                continue;
            }
            assert_eq!(
                (before[(x, y)].symbol(), before[(x, y)].style()),
                (after[(x, y)].symbol(), after[(x, y)].style()),
                "cell {x},{y} changed under a sheet that is supposed to move nothing"
            );
            compared += 1;
        }
    }
    assert!(compared > 0, "the sweep compared nothing");
}

#[test]
fn a_write_under_the_sheet_does_not_dismiss_it() {
    // The constraint B12 names as the one likeliest to be missed: the pane wakes on
    // filesystem events, so a sheet that lived for one frame would be dismissed at
    // random by the agent in the other pane.
    let scratch = Scratch::large_diff("sheet-survives", FILES, 40);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();
    let mut highlighter = Highlighter::new();
    let history = History::new();

    toggle(&mut app, &mut frame);
    let (open, _) = paint(&mut app, &mut frame, &mut highlighter, &history, area());
    assert!(
        text_of(&open, area()).contains(TITLE),
        "the sheet did not open"
    );

    // A real write and a real advance, which is what a wake does.
    scratch.rewrite_all(FILES, 40, 2);
    frame.advance().expect("advance");
    materialise(&mut frame);

    assert!(chrome(&app).sheet, "a write turned the sheet off");
    let (after, _) = paint(&mut app, &mut frame, &mut highlighter, &history, area());
    assert!(
        text_of(&after, area()).contains(TITLE),
        "a write under the sheet dismissed it"
    );
}

#[test]
fn the_close_control_dismisses_and_the_sheet_swallows_the_rest() {
    let scratch = Scratch::large_diff("sheet-mouse", FILES, 40);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();
    let mut highlighter = Highlighter::new();
    let history = History::new();

    toggle(&mut app, &mut frame);
    let (_, laid) = paint(&mut app, &mut frame, &mut highlighter, &history, area());
    let sheet = laid.sheet.expect("no sheet published");

    assert_eq!(
        action_for(&click(sheet.close.0, sheet.close.1), laid),
        Some(Action::ToggleSheet),
        "a click on the close control did not dismiss the sheet"
    );

    // Inside it and not on the control: nothing at all. Falling through is how a
    // click would seek a scrollbar the reader cannot see.
    let inside = (sheet.left + 1, sheet.top + 2);
    assert!(
        sheet.covers(inside.0, inside.1),
        "the fixture picked a cell outside the sheet"
    );
    assert_eq!(
        action_for(&click(inside.0, inside.1), laid),
        None,
        "a click inside the sheet reached what is behind it"
    );
    assert_eq!(
        action_for(&wheel(inside.0, inside.1), laid),
        None,
        "the wheel scrolled a diff the sheet is covering"
    );

    // Outside it the pane is what it always was, which is the other half: a sheet
    // that swallowed the whole screen would pass every assertion above.
    let outside = sheet.top.saturating_sub(1).max(1);
    assert!(
        !sheet.covers(1, outside),
        "the fixture picked a cell inside the sheet"
    );
    assert!(
        action_for(&wheel(1, outside), laid).is_some(),
        "a wheel outside the sheet stopped acting"
    );
}

#[test]
fn the_sheet_degrades_on_both_axes_and_has_a_floor() {
    // Width picks the spelling, height drops the groups, and both floors live in
    // the layout rather than in the painter, which is #158's correction inherited.
    let scratch = Scratch::large_diff("sheet-ladder", FILES, 40);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();
    let mut highlighter = Highlighter::new();
    let history = History::new();

    toggle(&mut app, &mut frame);

    let (buf, _) = paint(&mut app, &mut frame, &mut highlighter, &history, area());
    let wide = text_of(&buf, area());
    assert!(
        wide.contains("mouse") && wide.contains("Ctrl+C"),
        "an eighty column pane dropped a rung it can afford:\n{wide}"
    );

    // Forty columns: the tight spellings, and the mouse group gone with them.
    let narrow_at = Rect::new(0, 0, 40, TALL);
    let (buf, _) = paint(&mut app, &mut frame, &mut highlighter, &history, narrow_at);
    let narrow = text_of(&buf, narrow_at);
    assert!(
        narrow.contains(TITLE),
        "the sheet vanished at forty columns, which it fits:\n{narrow}"
    );
    assert!(
        !narrow.contains("Ctrl+C"),
        "the alias cells did not drop at forty columns:\n{narrow}"
    );

    // A short pane keeps the keyboard group and loses the mouse group, which is
    // the height axis on its own.
    let short_at = Rect::new(0, 0, WIDE, 16);
    let (buf, _) = paint(&mut app, &mut frame, &mut highlighter, &history, short_at);
    let short = text_of(&buf, short_at);
    assert!(
        short.contains(TITLE) && !short.contains("wheel"),
        "the height ladder did not drop the mouse group first:\n{short}"
    );

    // Below the floor nothing is drawn, the state stays true, and no gesture is
    // eaten, which is what `m` does on a pane that cannot carry the band.
    let tiny_at = Rect::new(0, 0, 12, 6);
    let (buf, laid) = paint(&mut app, &mut frame, &mut highlighter, &history, tiny_at);
    assert!(
        !text_of(&buf, tiny_at).contains(TITLE),
        "a pane too small for the sheet drew one anyway"
    );
    assert!(
        chrome(&app).sheet,
        "the state stopped being true because the pane was small"
    );
    assert!(
        laid.sheet.is_none(),
        "a sheet nobody can see is still eating gestures"
    );
}

#[test]
fn every_key_the_map_binds_is_named_on_the_sheet() {
    // **The gate that fails when somebody adds a key and forgets the sheet**,
    // which is the whole reason the sheet is worth having: it is now where the
    // keymap is written down, so a binding missing from it is a binding nobody can
    // find. Spelled as the tokens a reader would look for rather than as key
    // codes, because tokens are what the sheet draws.
    let scratch = Scratch::large_diff("sheet-covers", FILES, 40);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();
    let mut highlighter = Highlighter::new();
    let history = History::new();

    toggle(&mut app, &mut frame);
    let (buf, _) = paint(&mut app, &mut frame, &mut highlighter, &history, area());
    let drawn = text_of(&buf, area());

    for token in [
        "q",
        "Esc",
        "Ctrl+C",
        "Ctrl+D",
        "j",
        "k",
        "↓",
        "↑",
        "Space",
        "PgDn",
        "PgUp",
        "d",
        "u",
        "g",
        "Home",
        "G",
        "End",
        "n",
        "p",
        "1",
        "6",
        "J",
        "K",
        "Shift+↑",
        "Shift+↓",
        "f",
        "m",
        "?",
    ] {
        assert!(
            drawn.contains(token),
            "the sheet does not name {token:?}, so that gesture is unfindable:\n{drawn}"
        );
    }

    for gesture in [
        "wheel",
        "drag a scrollbar",
        "click a track",
        "click a listed file",
    ] {
        assert!(
            drawn.contains(gesture),
            "the sheet does not name the mouse gesture {gesture:?}"
        );
    }
}
