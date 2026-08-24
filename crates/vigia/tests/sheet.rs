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
use ratatui::style::Color;
use vigia::{
    Action, App, Chrome, Glyphs, Hovered, Regions, Theme, action_for, body_layout, regions, render,
};
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

/// The close control's glyph, restated for [`TITLE`]'s reason.
const SHEET_CLOSE: char = '✕';

/// Keyboard rows the height ladder may never drop, restated for [`TITLE`]'s
/// reason. `SPEC.md` §11.1 names the three: `f`, `m` and `?`.
const KEEP: usize = 3;

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
    paint_with(app, frame, highlighter, history, at, &Theme::default())
}

/// [`paint`] with a palette of the caller's choosing.
///
/// One gate needs a theme that washes its rows, because the defect it covers is
/// about a background surviving underneath the sheet and `Theme::default` sets
/// none.
fn paint_with(
    app: &mut App,
    frame: &mut Frame<'_>,
    highlighter: &mut Highlighter,
    history: &History,
    at: Rect,
    theme: &Theme,
) -> (Buffer, Regions) {
    let chrome = chrome(app);
    let body = body_layout(at, &chrome, FILES);
    let view = app
        .view(frame, highlighter, history, body)
        .expect("collect a view");
    let mut buf = Buffer::empty(at);
    render(&mut buf, at, &view, theme, Glyphs::default(), &chrome);
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
    let mut highlighter = Highlighter::eager();
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
    //
    // **At two panes since [#285](https://github.com/breferrari/vigia/issues/285),
    // and the rung each takes is asserted.** This ran only at eighty by
    // twenty-four, which is the plain one-column rung. The roomy rung covers a
    // third more cells than any rung that existed when this was written and is
    // what a full-screen terminal now takes, so the pane most readers have was
    // the one B12's load-bearing claim had never been checked on.
    let mut app = App::past_first_paint();
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    for at in [area(), ROOMY_PANE] {
        let (before, _) = paint(&mut app, &mut frame, &mut highlighter, &history, at);
        toggle(&mut app, &mut frame);
        let (after, laid) = paint(&mut app, &mut frame, &mut highlighter, &history, at);
        let sheet = laid
            .sheet
            .expect("the sheet was not published to the pointer");

        // Non-vacuity: without this the two passes could draw the same rung and
        // the second would be a slower copy of the first.
        let drawn = text_of(
            &after,
            Rect::new(sheet.left, sheet.top, sheet.width, sheet.height),
        );
        assert_eq!(
            drawn.contains("moving"),
            at == ROOMY_PANE,
            "the {}x{} pane did not draw the rung this case is named for:\n{drawn}",
            at.width,
            at.height
        );

        let mut compared = 0;
        for y in 0..at.height {
            for x in 0..at.width {
                if sheet.covers(x, y) {
                    continue;
                }
                assert_eq!(
                    (before[(x, y)].symbol(), before[(x, y)].style()),
                    (after[(x, y)].symbol(), after[(x, y)].style()),
                    "cell {x},{y} changed under a sheet that is supposed to move \
                     nothing, on the {}x{} pane",
                    at.width,
                    at.height
                );
                compared += 1;
            }
        }
        assert!(compared > 0, "the sweep compared nothing");

        // Back down, so the next pane starts from the same state this one did.
        toggle(&mut app, &mut frame);
    }
}

/// A pane the roomy rung fits on: a room of 68 columns and a body of 29 rows.
///
/// Named because three gates run at both rungs now, and a pane size copied into
/// three places is three places that can disagree about which rung they are
/// asserting.
const ROOMY_PANE: Rect = Rect::new(0, 0, 120, 40);

#[test]
fn the_sheet_is_opaque() {
    // **The defect this shipped with, as a gate.** `Cell::set_style` *patches*: it
    // merges into whatever is already in the cell, so a background nothing
    // overwrites survives. `Theme::chrome_dim` carries a foreground and no
    // background, so every added and removed row under the sheet kept its wash and
    // the table drew as green and red bands. Reported from use within an hour of
    // the release that shipped it.
    let scratch = Scratch::large_diff("sheet-opaque", FILES, 40);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::past_first_paint();
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    // **The wash is injected rather than borrowed from the shipped palette**, and
    // that is the difference between gating the mechanism and gating a theme: what
    // failed in the field is `set_style` patching, which shows up wherever *any*
    // theme puts a background on a row. `Theme::default` happens not to, so a gate
    // written against it would have been green while the reported screen was
    // striped green and red.
    let mut washed_theme = Theme::default();
    washed_theme.added_row = washed_theme.added_row.bg(Color::Green);
    washed_theme.removed_row = washed_theme.removed_row.bg(Color::Red);

    // **At both rungs since [#285](https://github.com/breferrari/vigia/issues/285).**
    // The roomy rung is the one with air in it: six of its twenty-seven interior
    // rows are blank, and a blank row is exactly a row the drawer writes nothing
    // over, so it is the shape most exposed to a background the blank pass failed
    // to clear. The rung that had this gate is the one with the least air.
    for at in [area(), ROOMY_PANE] {
        // Non-vacuity: the pane has to be drawing washed rows, or a sheet with no
        // wash under it proves nothing about a sheet that covers one.
        let (closed, _) = paint_with(
            &mut app,
            &mut frame,
            &mut highlighter,
            &history,
            at,
            &washed_theme,
        );
        let washed = (0..at.height)
            .flat_map(|y| (0..at.width).map(move |x| (x, y)))
            .filter(|&(x, y)| !matches!(closed[(x, y)].style().bg, None | Some(Color::Reset)))
            .count();
        assert!(
            washed > 0,
            "no cell on the {}x{} pane carries a background, so this fixture \
             cannot show a wash through the sheet",
            at.width,
            at.height
        );

        toggle(&mut app, &mut frame);
        let (open, laid) = paint_with(
            &mut app,
            &mut frame,
            &mut highlighter,
            &history,
            at,
            &washed_theme,
        );
        let sheet = laid.sheet.expect("no sheet published");
        let drawn = text_of(
            &open,
            Rect::new(sheet.left, sheet.top, sheet.width, sheet.height),
        );
        assert_eq!(
            drawn.contains("moving"),
            at == ROOMY_PANE,
            "the {}x{} pane did not draw the rung this case is named for:\n{drawn}",
            at.width,
            at.height
        );

        for y in sheet.top..sheet.top + sheet.height {
            for x in sheet.left..sheet.left + sheet.width {
                assert!(
                    matches!(open[(x, y)].style().bg, None | Some(Color::Reset)),
                    "cell {x},{y} inside the {}x{} pane's sheet kept the \
                     background {:?} from what it covers, so the sheet is a tint \
                     rather than a window",
                    at.width,
                    at.height,
                    open[(x, y)].style().bg
                );
            }
        }

        toggle(&mut app, &mut frame);
    }
}

#[test]
fn closing_the_sheet_restores_every_cell() {
    // The other half of *it moves nothing*: not only must the pane outside the
    // sheet be untouched while it is up, the pane must come back **exactly** when
    // it goes. Cell for cell, symbol and style, including the washes the bug above
    // was destroying.
    let scratch = Scratch::large_diff("sheet-restores", FILES, 40);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::past_first_paint();
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    let (before, _) = paint(&mut app, &mut frame, &mut highlighter, &history, area());
    toggle(&mut app, &mut frame);
    let _ = paint(&mut app, &mut frame, &mut highlighter, &history, area());
    toggle(&mut app, &mut frame);
    let (after, _) = paint(&mut app, &mut frame, &mut highlighter, &history, area());

    for y in 0..TALL {
        for x in 0..WIDE {
            assert_eq!(
                (before[(x, y)].symbol(), before[(x, y)].style()),
                (after[(x, y)].symbol(), after[(x, y)].style()),
                "cell {x},{y} did not come back when the sheet closed"
            );
        }
    }
}

#[test]
fn the_close_control_brightens_under_the_pointer() {
    // **A control that never brightened is a glyph a reader has to guess at**, and
    // B10's ladder already had the rungs: chrome at rest, `bar_hover` under the
    // pointer, `bar_active` while pressed. The same three the step buttons use.
    let scratch = Scratch::large_diff("sheet-hover", FILES, 40);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::past_first_paint();
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    toggle(&mut app, &mut frame);
    let (_, laid) = paint(&mut app, &mut frame, &mut highlighter, &history, area());
    let sheet = laid.sheet.expect("no sheet published");
    let (cx, cy) = sheet.close;

    // The pointer's own answer first: resting on the control marks it, and resting
    // anywhere else on the sheet marks nothing, because the sheet must not mark a
    // bar or a listed file it is covering.
    assert_eq!(
        laid.hover_at(cx, cy),
        Some(Hovered::Button(cx, cy)),
        "the close control does not answer the pointer"
    );
    assert_eq!(
        laid.hover_at(sheet.left + 1, sheet.top + 2),
        None,
        "a pointer on the sheet marked something underneath it"
    );

    let theme = Theme::default();
    let at_rest = drawn_close(&mut app, &mut frame, &mut highlighter, &history, None, None);
    let hovered = drawn_close(
        &mut app,
        &mut frame,
        &mut highlighter,
        &history,
        Some(Hovered::Button(cx, cy)),
        None,
    );
    // **Held, which is the rung nothing reached.** B10's ladder is three weights,
    // and this gate asserted two: no test in the suite ever built a `Chrome` whose
    // `pressed` is the control's own cell, so deleting the `bar_active` arm left
    // everything green. The step buttons' pressed rung is gated one element over
    // (`tests/render.rs`); the sheet's sibling was not.
    let held = drawn_close(
        &mut app,
        &mut frame,
        &mut highlighter,
        &history,
        Some(Hovered::Button(cx, cy)),
        Some((cx, cy)),
    );

    assert_ne!(
        at_rest, hovered,
        "the close control draws the same under the pointer as at rest, so nothing \
         says it is clickable"
    );
    // **Compared by weight rather than by whole `Style`**, because the drawer resets
    // the cell before styling it, so the drawn style carries an explicit
    // `Color::Reset` background where the theme's leaves it unset. Same ink, two
    // spellings, and it is the ink that is the ruling.
    assert_eq!(
        weight(hovered.1),
        weight(theme.bar_hover),
        "the hovered control is not on B10's hover rung"
    );
    assert_eq!(
        weight(held.1),
        weight(theme.bar_active),
        "the held control is not on B10's active rung, so pressing it says nothing"
    );
    assert_ne!(
        weight(held.1),
        weight(hovered.1),
        "the control draws the same held as merely hovered, so B10's ladder is two \
         rungs rather than three"
    );
    assert_eq!(
        at_rest.0, SHEET_CLOSE,
        "the control stopped being the glyph this gate is about"
    );
}

/// What a style says in ink: its foreground and its modifiers.
///
/// The two spellings of an unset background compare unequal as `Style`s, and this
/// suite cares about which rung of B10's ladder a cell is on rather than about how
/// the drawer got there.
fn weight(style: ratatui::style::Style) -> (Option<Color>, ratatui::style::Modifier) {
    (style.fg, style.add_modifier)
}

/// The close control's glyph and style, with `hovered` handed to the chrome.
fn drawn_close(
    app: &mut App,
    frame: &mut Frame<'_>,
    highlighter: &mut Highlighter,
    history: &History,
    hovered: Option<Hovered>,
    pressed: Option<(u16, u16)>,
) -> (char, ratatui::style::Style) {
    let chrome = app.chrome("fixture", Some("main"), pressed, None, hovered, None);
    let body = body_layout(area(), &chrome, FILES);
    let view = app
        .view(frame, highlighter, history, body)
        .expect("collect a view");
    let mut buf = Buffer::empty(area());
    render(
        &mut buf,
        area(),
        &view,
        &Theme::default(),
        Glyphs::default(),
        &chrome,
    );
    let sheet = regions(area(), &chrome, &view)
        .sheet
        .expect("no sheet published");
    let cell = &buf[sheet.close];
    (cell.symbol().chars().next().expect("a glyph"), cell.style())
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
    let mut highlighter = Highlighter::eager();
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
    let mut highlighter = Highlighter::eager();
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
    let mut highlighter = Highlighter::eager();
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
    // the height axis on its own. **Sixteen rows is still below #220's widening
    // rung** and the case survived it unchanged: the pane's footer is two rows
    // here, so the body is thirteen and the two-column rung needs fourteen. One
    // row taller and the mouse group comes back beside the keyboard group
    // instead, which is what `the_sheet_spends_width_before_it_spends_gestures`
    // holds.
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
    let mut highlighter = Highlighter::eager();
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

/// Every gesture the sheet can draw, as the token a reader would look for.
///
/// One entry per row of `KEYBOARD` and `MOUSE`, restated rather than imported for
/// the reason [`TITLE`] is: a gate that shared the renderer's tables would agree
/// with them by construction, and this one exists to count what reaches a screen.
///
/// **Each entry is the longest prefix both spellings share**, which is why some
/// are shorter than the phrase the wide rung draws: `drag a` rather than `drag a
/// scrollbar`, because the tight rung spells it `drag a bar`. A gate that counts
/// gestures has to count them at every rung, so an entry that named only the wide
/// spelling would report a tight sheet as having lost the row.
///
/// `every_key_the_map_binds_is_named_on_the_sheet` keeps the full phrases and is
/// deliberately not folded into this: it runs at one pane size, and its job is
/// that the wide rung spells each gesture out rather than that the row exists.
///
/// **The order is the reader's, not the ladder's**, since
/// [#285](https://github.com/breferrari/vigia/issues/285) separated the two: `q`
/// is last of the eleven keyboard entries because the sheet draws it last, and
/// the ladder gives it up first. Two gates walk this against drawn rows, so the
/// order is load-bearing rather than decorative.
const GESTURES: [&str; 16] = [
    "scroll a row",
    "Space  PgDn",
    "half a page",
    "first / last",
    "next / prev",
    "jump to",
    "scroll the",
    "follow the newest",
    "churn band",
    "this sheet",
    "quit",
    "wheel",
    "drag a",
    "click a track",
    "click  ▲ ▼",
    "jump the diff to it",
];

#[test]
fn no_gesture_token_hides_inside_another() {
    // **The gate on the gate.** [`GESTURES`] is matched with `contains`, so an
    // entry that is a substring of another scores whenever the longer one draws,
    // and the height ladder drops from the top, which is exactly where the two
    // meet: `page` sat inside `half a page` and the row above it is dropped first.
    // Measured at 60 by 13 the sheet drew eight gestures and the count said nine.
    //
    // The same shape as the footer leak one line up, and both were invisible for
    // the same reason: a count that is wrong in the region the ladder degrades is
    // right everywhere a gate happened to assert an exact number.
    for (i, a) in GESTURES.iter().enumerate() {
        for (j, b) in GESTURES.iter().enumerate() {
            assert!(
                i == j || !b.contains(a),
                "GESTURES[{i}] {a:?} hides inside GESTURES[{j}] {b:?}, so every \
                 rung that draws the second and not the first counts one gesture \
                 too many"
            );
        }
    }
}

/// The extent every ladder gate walks: 40 to 144 columns by 1, 6 to 32 rows by 1.
///
/// **One place, because three gates read it.** They assert different things about
/// the same grid, and bounds that drifted between them would leave a rung
/// asserted monotone at a width no other gate had looked at.
///
/// The floor is at I6's forty columns and the ceiling is above every width the
/// two-column rung arrives at, so the sweep contains both ends of the ladder
/// rather than a slice of its middle.
const LADDER_WIDTHS: std::ops::RangeInclusive<u16> = 40..=144;

/// The height half of [`LADDER_WIDTHS`], from below the sheet's floor to above
/// the height at which the tallest rung fits.
///
/// **Raised from 32 by [#285](https://github.com/breferrari/vigia/issues/285)**,
/// and the old ceiling is why: the roomy rung needs a body of twenty-nine rows,
/// which this fixture reaches at a pane of thirty-two, so a sweep stopping there
/// would have covered the new rung at exactly one height of twenty-seven and
/// called it swept.
const LADDER_HEIGHTS: std::ops::RangeInclusive<u16> = 6..=38;

/// One materialised fixture, painted at many sizes.
///
/// **Through `render`, never through the layout alone.** The sheet's height is
/// spent against the *body*, and the body is the pane less the header and less a
/// footer whose own height is a ladder in the width. A sweep that assumed a
/// one-row footer would be measuring a pane that does not exist, and it did:
/// #220's first probe put the two-column rung one row lower than it lands.
///
/// The repository, the frame and the materialised diffs are built **once** and
/// the pane is resized around them. Rebuilding per cell cost forty seconds for
/// the sweep below, which is the shape that gets a gate deleted rather than run.
macro_rules! sweep {
    ($name:expr, |$paint:ident| $body:block) => {
        let scratch = Scratch::large_diff($name, FILES, 40);
        let worktree = scratch.worktree();
        let mut frame = worktree.frame();
        materialise(&mut frame);
        let mut app = App::new();
        let mut highlighter = Highlighter::eager();
        let history = History::new();
        toggle(&mut app, &mut frame);
        let mut $paint = |at: Rect| paint(&mut app, &mut frame, &mut highlighter, &history, at);
        $body
    };
}

/// Walk the ladder, handing each cell's painted pane to `each`.
///
/// The scaffold three gates below shared by copy until #220's own audit pointed
/// at it. `name` is the fixture's, so each gate still gets an independent
/// repository and the three keep running in parallel.
fn walk_the_ladder(name: &'static str, mut each: impl FnMut(u16, u16, Rect, &Buffer, &Regions)) {
    sweep!(name, |paint| {
        for w in LADDER_WIDTHS {
            for h in LADDER_HEIGHTS {
                let at = Rect::new(0, 0, w, h);
                let (buf, laid) = paint(at);
                each(w, h, at, &buf, &laid);
            }
        }
    });
}

/// The first width in `widths` at which the sheet draws the rung `marker` names,
/// walked a column at a time and asserted monotone from there on.
///
/// **The instrument two rulings needed and one of them shipped without.** #220's
/// arrival was recorded as 80 for a rung that arrives at 78, because the probe
/// that produced it sampled 60, 70 and 80 and stepped over its own boundary. A
/// boundary is only findable by walking it, and both rungs' arrivals are now
/// found by the same walk rather than by two copies of it that can drift on what
/// monotone means.
fn arrival_of(
    paint: &mut impl FnMut(Rect) -> (Buffer, Regions),
    marker: &str,
    widths: std::ops::RangeInclusive<u16>,
    height: u16,
) -> Option<u16> {
    let mut arrival = None;
    for w in widths {
        let (buf, laid) = paint(Rect::new(0, 0, w, height));
        let (_, sheet) = read_sheet(&buf, &laid);
        if sheet.contains(marker) && arrival.is_none() {
            arrival = Some(w);
        }
        if let Some(first) = arrival {
            assert!(
                sheet.contains(marker),
                "the rung arrived at {first} and was gone again at {w}, so it is \
                 not monotone in width:\n{sheet}"
            );
        }
    }
    arrival
}

/// How many of [`GESTURES`] the sheet draws, and the sheet's own rows.
///
/// **Counted inside the sheet's rect, never over the pane.** The hint bar spells
/// `q quit`, so a pane-wide count scores `GESTURES[0]` on every frame whether or
/// not the sheet drew that row, and every count-based gate here was one gesture
/// loose in exactly the region they exist to measure: the rungs that drop rows.
/// Measured at 120 by 8 the pane says four and the sheet draws three.
fn read_sheet(buf: &Buffer, laid: &Regions) -> (usize, String) {
    let sheet = laid.sheet.map_or_else(String::new, |s| {
        text_of(buf, Rect::new(s.left, s.top, s.width, s.height))
    });
    let count = if sheet.contains(TITLE) {
        GESTURES.iter().filter(|g| sheet.contains(*g)).count()
    } else {
        0
    };
    (count, sheet)
}

#[test]
fn the_sheet_spends_width_before_it_spends_gestures() {
    // **#220's whole claim, at the pane that reported it.** A pane too short for
    // the seventeen-row column but wide enough to put the mouse group beside the
    // keyboard group draws every gesture, where before this rung it dropped the
    // whole mouse group and said nothing about it.
    //
    // The pane sizes are named rather than derived, because a gate that computed
    // the boundary from the function under test would verify the function and
    // never the wiring.
    sweep!("sheet-width", |paint| {
        let short_and_wide = Rect::new(0, 0, 120, 21);
        let (buf, laid) = paint(short_and_wide);
        let (count, sheet) = read_sheet(&buf, &laid);
        assert_eq!(
            count,
            GESTURES.len(),
            "a pane with the columns to draw every gesture drew fewer:\n{sheet}"
        );
        assert!(
            sheet.contains("keyboard"),
            "the assertion above passed without the two-column rung, so it proves nothing:\n{sheet}"
        );

        // And a pane tall enough for one column is untouched: the widening rung
        // sits *below* the full one-column rung, which is what makes #220
        // additive rather than a relayout.
        let tall = Rect::new(0, 0, 120, 30);
        let (buf, laid) = paint(tall);
        let (count, sheet) = read_sheet(&buf, &laid);
        assert_eq!(
            sheet.lines().count(),
            19,
            "a tall pane stopped drawing the nineteen-row one-column sheet:\n{sheet}"
        );
        assert!(
            !sheet.contains("keyboard"),
            "a tall pane took the two-column rung, so the ladder is not additive:\n{sheet}"
        );
        assert_eq!(
            count,
            GESTURES.len(),
            "the untouched rung stopped drawing every gesture:\n{sheet}"
        );
    });
}

#[test]
fn the_widening_ladder_is_monotone_and_never_leaves_the_body() {
    // **Walks the ladder rather than sampling it**, which is #158's lesson and the
    // one `the_sheet_degrades_on_both_axes_and_has_a_floor` does not follow: it
    // asserts three fixed sizes, and a single fixture passes against an unfixed
    // ladder. #220 inserts a rung in the *middle* of this one, where a removal
    // from the top would have been free, so both axes are swept rather than
    // reasoned about.
    //
    // Two claims over one grid, because the sweep is what costs and the assertions
    // are free: growing a pane never takes a gesture away, and the sheet never
    // reaches the header or the footer at any rung.
    let widths: Vec<u16> = LADDER_WIDTHS.collect();
    let heights: Vec<u16> = LADDER_HEIGHTS.collect();
    let mut grid = vec![vec![0usize; heights.len()]; widths.len()];

    walk_the_ladder("sheet-ladder-sweep", |w, h, at, buf, laid| {
        let (count, _) = read_sheet(buf, laid);
        let i = usize::from(w - widths[0]);
        let j = usize::from(h - heights[0]);
        grid[i][j] = count;

        // B12's box rule, at every rung rather than at one: the sheet is centred
        // in the body, so a reader can still see the tool is alive behind it.
        if let Some(sheet) = laid.sheet {
            assert!(sheet.top > at.y, "the sheet reached the header at {w}x{h}");
            assert!(
                sheet.top + sheet.height < at.y + at.height,
                "the sheet reached the footer at {w}x{h}"
            );
        }
    });

    assert!(
        grid.iter().flatten().any(|&c| c == GESTURES.len()),
        "no pane in the sweep drew a whole sheet, so every assertion below is \
         vacuous and a `sheet_plan` that returned None unconditionally would pass"
    );

    for (i, &w) in widths.iter().enumerate() {
        for (j, &h) in heights.iter().enumerate() {
            if i + 1 < widths.len() {
                assert!(
                    grid[i + 1][j] >= grid[i][j],
                    "widening {w} to {} at height {h} took a gesture away: {} then {}",
                    widths[i + 1],
                    grid[i][j],
                    grid[i + 1][j]
                );
            }
            if j + 1 < heights.len() {
                assert!(
                    grid[i][j + 1] >= grid[i][j],
                    "growing height {h} to {} at width {w} took a gesture away: {} then {}",
                    heights[j + 1],
                    grid[i][j],
                    grid[i][j + 1]
                );
            }
        }
    }
}

#[test]
fn one_column_wins_wherever_one_column_fits() {
    // **The additivity claim as a gate, not as an argument.** #220's rung is
    // inserted in the *middle* of a monotone ladder, where removing one from the
    // top would have been free, so the property that keeps it from being a
    // relayout is that a pane on which one column already fits never reaches it.
    //
    // Stated so a screen can answer it: at each width, find the shortest pane that
    // draws every gesture in **one** column, and require every two-column pane at
    // that width to be shorter than it. A pane that could have had the shape it
    // has today and was given a different one fails here.
    //
    // **The first version of this gate could not fail.** It skipped a width whose
    // one-column pane did not exist, and putting the two-column rung first is
    // exactly what makes it not exist, so the mutation it was written for passed
    // straight through it. A width that draws two columns and never falls back to
    // one is now the failure rather than the exemption.
    let widths: Vec<u16> = LADDER_WIDTHS.collect();
    let heights: Vec<u16> = LADDER_HEIGHTS.collect();
    // Per cell: whether the sheet drew two columns, and how many gestures reached
    // the screen. `None` where no sheet was drawn at all.
    let mut cells = vec![vec![None; heights.len()]; widths.len()];

    walk_the_ladder("sheet-additive", |w, h, _at, buf, laid| {
        let (count, sheet) = read_sheet(buf, laid);
        let i = usize::from(w - widths[0]);
        let j = usize::from(h - heights[0]);
        cells[i][j] = laid.sheet.map(|_| (sheet.contains("keyboard"), count));
    });

    let mut seen = 0;
    for (i, &w) in widths.iter().enumerate() {
        let two_column: Vec<u16> = heights
            .iter()
            .enumerate()
            .filter(|(j, _)| matches!(cells[i][*j], Some((true, _))))
            .map(|(_, h)| *h)
            .collect();
        if two_column.is_empty() {
            continue;
        }
        seen += two_column.len();

        // The shortest pane at this width that draws every gesture in one column.
        let full = heights
            .iter()
            .enumerate()
            .find(|(j, _)| cells[i][*j] == Some((false, GESTURES.len())))
            .map(|(_, h)| *h);
        let full = full.unwrap_or_else(|| {
            panic!(
                "width {w} draws two columns at {two_column:?} and never falls \
                 back to one, so the two-column rung is being taken ahead of a \
                 shape that fits"
            )
        });
        for h in two_column {
            assert!(
                h < full,
                "a pane of {w}x{h} took the two-column rung although one column \
                 draws every gesture from {w}x{full}, so the ladder is a relayout \
                 rather than additive"
            );
        }
    }
    assert!(
        seen > 0,
        "the sweep found no two-column pane at all, so it proves nothing"
    );
}

#[test]
fn the_sheet_is_a_closed_box_at_every_rung() {
    // **The gate that was missing, found by the audit that introduced its
    // defect.** Restructuring the two-column rung so the layout places each group
    // (rather than the drawer re-deriving where the second one starts) made the
    // sheet two columns narrower than the row it draws, so a long verb in the
    // right-hand column overwrote the frame and the box read as open on four rows
    // of thirteen. **Every other gate stayed green**: they count gestures and look
    // for tokens, and none of them had ever looked at the border.
    //
    // The frame is the one part of this element with no content in it, so it can
    // be asserted exactly: four corners, a closed rule along the bottom, and a
    // pipe at both ends of every row between. Swept, because a width that clips is
    // a width the ladder chose and one screen cannot find it.
    let mut sheets = 0;
    let mut two_column = 0;

    walk_the_ladder("sheet-frame", |w, h, _at, buf, laid| {
        let Some(sheet) = laid.sheet else { return };
        let rect = Rect::new(sheet.left, sheet.top, sheet.width, sheet.height);
        let drawn = text_of(buf, rect);
        let rows: Vec<&str> = drawn.lines().collect();
        sheets += 1;
        two_column += usize::from(drawn.contains("keyboard"));

        let (first, last) = (rows[0], rows[rows.len() - 1]);
        assert!(
            first.starts_with('┌') && first.ends_with('┐'),
            "the sheet's top row is not a closed rule at {w}x{h}:\n{drawn}"
        );
        // The title bar's own rule, which nothing looked at: replacing its fill
        // with spaces left `┌ gestures      ┐` green everywhere. The run starts
        // after the title and stops before the three cells the close control sits
        // in, which is the geometry `Painter::sheet` writes.
        let title = "─ gestures ".chars().count();
        let top: Vec<char> = first.chars().collect();
        assert!(
            top[1..=title].iter().collect::<String>() == "─ gestures ",
            "the sheet's title is not where the frame puts it at {w}x{h}:\n{drawn}"
        );
        assert!(
            top[title + 1..top.len() - 4].iter().all(|c| *c == '─'),
            "the sheet's title rule has a hole in it at {w}x{h}:\n{drawn}"
        );
        // The control's own cell, which the rule assertion above deliberately
        // stops short of and nothing else pinned: the hover and dismiss gates both
        // read `sheet.close` out of the plan, so they follow it wherever it goes.
        assert_eq!(
            top[top.len() - 3],
            SHEET_CLOSE,
            "the close control is not three in from the right edge at {w}x{h}:\n{drawn}"
        );
        assert!(
            last.starts_with('└') && last.ends_with('┘'),
            "the sheet's bottom row is not a closed rule at {w}x{h}:\n{drawn}"
        );
        let span = last.chars().count() - 2;
        assert!(
            last.chars().skip(1).take(span).all(|c| c == '─'),
            "the sheet's bottom rule has a hole in it at {w}x{h}:\n{drawn}"
        );
        // Pane-dependent, not row-dependent: read once rather than per row, and
        // the five headed labels built once rather than five `format!`s per row
        // over a sweep of 105 widths by 33 heights.
        let roomy = drawn.contains("moving");
        let headings: Vec<String> = SECTIONS.iter().map(|l| format!("  {l}")).collect();
        for (n, row) in rows[1..rows.len() - 1].iter().enumerate() {
            // **A heading's own fill, which only the title bar's and the bottom
            // border's were checked.** Deleting the rule run leaves
            // `│ keyboard        │`, and the reset pass has already written a dim
            // space into every one of those cells, so both the glyph and the
            // weight survive: the pipes are `put` explicitly either side, the
            // label offsets do not move, and no gate compares a heading row
            // wholesale. A heading is furniture that runs to the frame, or it is
            // a label floating in a gap.
            //
            // **The roomy rung is the exception and it is the opposite claim**
            // ([#285](https://github.com/breferrari/vigia/issues/285)): there the
            // separator is a blank row, so its headings are plain text standing
            // back from their own rows. A rule under a blank row reads as a
            // divider rather than as a heading, and ruling one there would be the
            // same defect this block guards against, pointing the other way.
            let cells: Vec<char> = row.chars().collect();
            if !roomy && (row.contains(" keyboard ") || row.contains(" mouse ")) {
                assert_eq!(
                    cells[cells.len() - 3],
                    '─',
                    "a heading of the sheet does not rule to its frame at \
                     {w}x{h}, so it is a label in a gap:\n{drawn}"
                );
                assert!(
                    cells.iter().filter(|c| **c == '─').count() >= 3,
                    "a heading of the sheet has almost no rule in it at {w}x{h}:\n{drawn}"
                );
            }
            if roomy && headings.iter().any(|l| row.contains(l)) {
                assert!(
                    !row.contains('─'),
                    "a section heading of the roomy rung rules to its frame at \
                     {w}x{h}, so the air and a rule are both separating the same \
                     two sections:\n{drawn}"
                );
            }
            assert!(
                row.starts_with('│') && row.ends_with('│'),
                "row {n} of the sheet is not closed at both ends at {w}x{h}, so a \
                 cell has overwritten the frame:\n{drawn}"
            );
        }
    });

    assert!(
        two_column > 0 && sheets > two_column,
        "the sweep saw {sheets} sheets of which {two_column} were two-column, so \
         it did not cover both shapes"
    );
}

/// The roomy rung's five labels, in the order Mock A draws them.
///
/// Restated rather than imported for [`TITLE`]'s reason, and the order is the
/// ruling: `leaving` at the bottom is what
/// [#285](https://github.com/breferrari/vigia/issues/285) asked for and
/// `SPEC.md` §11.1 now states.
const SECTIONS: [&str; 5] = ["moving", "files", "view", "mouse", "leaving"];

/// One row of the roomy rung, as the gate expects to read it.
///
/// **Three cases as a type rather than as an `Option<&str>` whose empty string
/// meant a gesture row.** A heading spelled `""` silently became a gesture row
/// under that encoding, and the walk needed a running counter and three index
/// offsets to say which gesture belonged where.
#[derive(Debug, Clone, Copy)]
enum RoomyRow {
    /// A blank row. The one thing on this rung no count can see.
    Air,
    /// A section heading, standing back from its own rows.
    Heading(&'static str),
    /// A gesture row, carrying the token a reader would look for.
    Gesture(&'static str),
}

/// The roomy rung's shape, row by row, in the order it is drawn.
///
/// One place, because three gates read it. The sections carry **slices of
/// [`GESTURES`]** rather than counts, so the mapping from a section to the rows
/// under it is written where it can be read instead of being re-derived by hand
/// as an offset. `GESTURES` is itself restated rather than imported, so this
/// cannot agree with the renderer by construction.
///
/// The two keyboard slices either side of the mouse group are what makes the
/// order Mock A's: `leaving` is the tail of the keyboard table and is drawn last,
/// after the mouse group rather than before it.
fn roomy_shape() -> Vec<RoomyRow> {
    let mut rows = vec![RoomyRow::Air];
    for (label, tokens) in [
        ("moving", &GESTURES[0..3]),
        ("files", &GESTURES[3..7]),
        ("view", &GESTURES[7..9]),
        ("mouse", &GESTURES[11..16]),
        ("leaving", &GESTURES[9..11]),
    ] {
        rows.push(RoomyRow::Heading(label));
        rows.extend(tokens.iter().map(|t| RoomyRow::Gesture(t)));
        rows.push(RoomyRow::Air);
    }
    rows
}

#[test]
fn the_roomy_rung_is_the_size_the_ruling_states() {
    // **`SPEC.md` §11.1 states 68 by 29, and Mock A drew 76 by 29.** The
    // difference is the twelve blank columns the mock leaves after its verb field
    // against the four before its keys, which is the box the reader drew rather
    // than a table they designed: §11.1's own rule for a mockup and a drawer that
    // disagree about a width is that *the widths are the shipped drawer's*, and it
    // was written when B12's own mockup was corrected the same way.
    //
    // Pinned as a number for the reason the two-column rung's size is: a mutation
    // moving the verb column keeps the frame closed, keeps every gesture on
    // screen, and keeps the ladder monotone, so the only thing that can see it is
    // a width.
    sweep!("sheet-roomy-size", |paint| {
        let at = Rect::new(0, 0, 120, 40);
        let (buf, laid) = paint(at);
        let sheet = laid.sheet.expect("the pane draws no sheet at all");
        let (count, drawn) = read_sheet(&buf, &laid);
        assert!(
            drawn.contains("moving"),
            "the pinned pane stopped taking the roomy rung, so the size below is \
             not the one being pinned:\n{drawn}"
        );
        assert_eq!(
            (sheet.width, sheet.height),
            (68u16, 29u16),
            "the roomy rung is not the size SPEC.md §11.1 states:\n{drawn}"
        );
        assert_eq!(
            count,
            GESTURES.len(),
            "the roomy rung drew fewer than every gesture:\n{drawn}"
        );

        // **The wide spelling, which the size alone does not say.** The rung is
        // measured at spelling 0 and drawn at whatever `Fit::level` carries, and
        // those are two expressions: setting the level to 1 while the fields stay
        // wide leaves a sixty-eight column sheet whose cells are the tight
        // spellings sitting left-aligned in fields sized for the wide ones. The
        // frame stays closed, every column stays where it was, and `GESTURES` is
        // written as the prefix both spellings share, so nothing else here can
        // see it. These three phrases exist only at the wide spelling.
        for wide in [
            "q  Esc  Ctrl+C  Ctrl+D",
            "first / last changed file",
            "scroll the pinned file list",
        ] {
            assert!(
                drawn.contains(wide),
                "the roomy rung does not spell {wide:?}, so it is drawing the \
                 tight spelling on a pane that has the columns for the wide \
                 one:\n{drawn}"
            );
        }
    });
}

#[test]
fn the_roomy_rung_arrives_at_the_width_the_ruling_states() {
    // **70, and it is not the sheet's own 68.** `margin_of` leaves 68 columns of
    // room at a pane of 70 and 67 at a pane of 69, so the boundary sits two
    // columns above the width the rung measures. That is exactly the gap the
    // two-column rung's arrival fell into when its ruling shipped 80 for a rung
    // that arrives at 78, and the instrument is the same: walk the boundary a
    // column at a time, because nothing else can find it.
    let arrival;
    sweep!("sheet-roomy-arrival", |paint| {
        arrival = arrival_of(&mut paint, "moving", 64..=80, 40);
    });
    assert_eq!(
        arrival,
        Some(70),
        "the roomy rung does not arrive where SPEC.md §11.1 says it does"
    );
}

#[test]
fn the_roomy_rung_places_its_cells_where_the_plan_says() {
    // **Air is the one thing on this element that a count cannot see.** Every
    // gate above counts gestures or measures the frame, and a roomy rung that
    // drew its sections back to back with the blank rows all at the bottom would
    // satisfy every one of them: same width, same height, same sixteen gestures,
    // same closed box. The shape is what this pins, row by row.
    //
    // Columns are literals rather than derived, because a gate that computed them
    // from the layout would agree with it by construction.
    sweep!("sheet-roomy-cells", |paint| {
        let at = Rect::new(0, 0, 120, 40);
        let (buf, laid) = paint(at);
        let (_, sheet) = read_sheet(&buf, &laid);
        let rows: Vec<Vec<char>> = sheet.lines().map(|r| r.chars().collect()).collect();
        assert_eq!(
            rows.len(),
            29,
            "the roomy rung is not twenty-nine rows tall:\n{sheet}"
        );

        // Interior rows only: the title bar and the bottom border are the frame's.
        let shape = roomy_shape();
        assert_eq!(
            shape.len(),
            rows.len() - 2,
            "the shape this gate walks is not the height the rung draws"
        );

        let mut gestures = 0;
        for (n, want) in shape.iter().enumerate() {
            let row = &rows[n + 1];
            let text: String = row.iter().collect();
            assert!(
                row[0] == '│' && row[row.len() - 1] == '│',
                "row {n} of the roomy rung is not closed by the frame:\n{sheet}"
            );
            match want {
                // A blank row is blank between the pipes. Nothing else in this
                // file can tell air from a row that happens to draw nothing.
                RoomyRow::Air => assert!(
                    row[1..row.len() - 1].iter().all(|c| *c == ' '),
                    "row {n} of the roomy rung should be air and reads {text:?}"
                ),
                RoomyRow::Heading(label) => {
                    let head: String = row[1..].iter().collect();
                    assert!(
                        head.starts_with(&format!("  {label}")),
                        "row {n} should be the {label:?} heading, standing two \
                         columns in from the frame, and reads {text:?}"
                    );
                }
                // A gesture row: its keys cell starts at column 5, its verb at
                // column 35, and it carries the token its own section names.
                RoomyRow::Gesture(token) => {
                    assert!(
                        row[5] != ' ' && row[4] == ' ',
                        "row {n}'s keys cell does not start at column 5: {text:?}"
                    );
                    assert!(
                        row[35] != ' ' && row[34] == ' ',
                        "row {n}'s verb does not start at column 35: {text:?}"
                    );
                    // Read from the keys cell rather than from the verb field:
                    // `GESTURES` carries whichever half identifies the row, and
                    // `Space  PgDn` is a keys cell because `page` hides inside
                    // `half a page`.
                    let cell: String = row[5..].iter().collect();
                    assert!(
                        cell.contains(token),
                        "row {n} should carry {token:?} and reads {text:?}, so the \
                         sections are not in the reader's order or their rows are \
                         not in the table's"
                    );
                    gestures += 1;
                }
            }
        }
        assert_eq!(
            gestures,
            GESTURES.len(),
            "the roomy rung drew {gestures} gesture rows, not {}",
            GESTURES.len()
        );

        // The labels themselves, in Mock A's order, which the walk above checks
        // one at a time and cannot check as a sequence.
        let drawn: Vec<&str> = SECTIONS
            .iter()
            .copied()
            .filter(|label| sheet.contains(*label))
            .collect();
        assert_eq!(
            drawn,
            SECTIONS.to_vec(),
            "the roomy rung does not draw all five sections:\n{sheet}"
        );
    });
}

#[test]
fn the_roomy_rung_is_additive_and_costs_no_pane_a_gesture() {
    // **#285's own gate, in its own words: a heading costs a row and must never
    // cost a gesture.** The rung is inserted at the *head* of a monotone ladder,
    // which is the free case, and this is what turns that from an argument into
    // evidence.
    //
    // Two claims over one sweep, and they are not the same claim. Every pane that
    // takes the rung draws all sixteen gestures, so no pane can have lost one to
    // the air. And the shortest pane at each width that draws all sixteen is
    // still a pane that takes no headings, so no pane was made to wait longer for
    // a full sheet than it did before the rung existed. The second is what fails
    // if the rung is ever measured smaller than the one-column sheet it sits
    // above.
    let widths: Vec<u16> = LADDER_WIDTHS.collect();
    let heights: Vec<u16> = LADDER_HEIGHTS.collect();
    let mut cells = vec![vec![None; heights.len()]; widths.len()];

    walk_the_ladder("sheet-roomy-additive", |w, h, _at, buf, laid| {
        let (count, sheet) = read_sheet(buf, laid);
        let i = usize::from(w - widths[0]);
        let j = usize::from(h - heights[0]);
        cells[i][j] = laid.sheet.map(|_| (sheet.contains("moving"), count));
        if let Some((true, count)) = cells[i][j] {
            assert_eq!(
                count,
                GESTURES.len(),
                "a {w}x{h} pane took the roomy rung and drew {count} gestures, so \
                 a heading cost it one:\n{sheet}"
            );
            // **The rung is the wide spelling's, and nothing else could see
            // that.** Measured at the pane's own `level` instead of at 0 it
            // becomes reachable at exactly one room width, fifty-five, where it
            // would trade the spelled-out verbs for air. Every other assertion
            // here survives that: it is still sixteen gestures, still additive,
            // still monotone. Its width is the only thing that moves.
            assert_eq!(
                laid.sheet.map(|s| s.width),
                Some(68),
                "a {w}x{h} pane drew a roomy rung that is not the wide \
                 spelling's sixty-eight columns:\n{sheet}"
            );
        }
    });

    let mut seen = 0;
    for (i, &w) in widths.iter().enumerate() {
        let roomy: Vec<u16> = heights
            .iter()
            .enumerate()
            .filter(|(j, _)| matches!(cells[i][*j], Some((true, _))))
            .map(|(_, h)| *h)
            .collect();
        if roomy.is_empty() {
            continue;
        }
        seen += roomy.len();

        // The shortest pane at this width that draws every gesture with no
        // headings at all.
        let full = heights
            .iter()
            .enumerate()
            .find(|(j, _)| cells[i][*j] == Some((false, GESTURES.len())))
            .map(|(_, h)| *h);
        let full = full.unwrap_or_else(|| {
            panic!(
                "width {w} draws the roomy rung at {roomy:?} and never falls back \
                 to a headingless sheet, so the rung is being taken ahead of a \
                 shape that fits"
            )
        });
        for h in roomy {
            assert!(
                h > full,
                "a pane of {w}x{h} took the roomy rung although a headingless \
                 sheet draws every gesture from {w}x{full}, so the rung is a \
                 relayout rather than additive"
            );
        }
    }
    assert!(
        seen > 0,
        "the sweep found no roomy pane at all, so it proves nothing"
    );
}

#[test]
fn the_display_order_is_the_readers_and_the_floor_keeps_the_unguessable() {
    // **The two ends of #285's separation, on a screen.** `render.rs`'s
    // `sheet_tables` holds them on the tables; this holds them on what a reader
    // sees, and the two are not the same claim: a drawer free to iterate the
    // table backwards would satisfy every assertion over the constants.
    //
    // The reader's end: the full one-column rung draws the eleven keyboard rows in
    // the order Mock A reads them, `q` last. The ladder's end: at the floor, the
    // three rows left are `f`, `m` and `?`, and `q` is not among them. Conflate
    // the two orders again and the second fails, because dropping from the top of
    // the reader's order keeps `q`.
    sweep!("sheet-orders", |paint| {
        let at = Rect::new(0, 0, 80, 24);
        let (buf, laid) = paint(at);
        let (_, sheet) = read_sheet(&buf, &laid);
        let rows: Vec<&str> = sheet.lines().collect();
        for (n, token) in GESTURES.iter().take(11).enumerate() {
            assert!(
                rows[n + 1].contains(token),
                "row {} of the one-column rung does not carry {token:?}, so the \
                 keyboard table is not in the reader's order:\n{sheet}",
                n + 1
            );
        }

        // The floor: the shortest pane in the sweep that still draws a sheet.
        let mut floor = None;
        for h in LADDER_HEIGHTS {
            let at = Rect::new(0, 0, 44, h);
            let (buf, laid) = paint(at);
            let (count, sheet) = read_sheet(&buf, &laid);
            if laid.sheet.is_some() && floor.is_none() {
                floor = Some((h, count, sheet));
            }
        }
        let (h, count, sheet) = floor.expect("no pane in the sweep drew a floor sheet");
        assert_eq!(
            count, KEEP,
            "the floor rung at 44x{h} draws {count} gestures, not the {KEEP} the \
             keep-set names:\n{sheet}"
        );
        for kept in ["follow the newest", "churn band", "this sheet"] {
            assert!(
                sheet.contains(kept),
                "the floor rung at 44x{h} dropped {kept:?}:\n{sheet}"
            );
        }
        assert!(
            !sheet.contains("quit"),
            "the floor rung at 44x{h} kept `q`, which the hint bar already names \
             on every frame, so the two orders have been conflated again and the \
             unguessable is what got dropped:\n{sheet}"
        );
    });
}

#[test]
fn the_two_column_rung_is_the_size_the_ruling_states() {
    // **`SPEC.md` §11.1 states 104 by 14 wide and 76 by 14 tight, and until this
    // gate no test could fail on either.** That is the rail's own lesson one
    // element over: `RAIL_FLOOR` pins its composition in a const block precisely
    // because a claim no gate can fail is a wish, and #220's ruling shipped two
    // numbers with nothing holding them.
    //
    // It is not a restatement of the layout arithmetic. A mutation that moved the
    // mouse column one column right survived every other gate in this file: the
    // frame stayed closed because the sheet grew with it, every gesture still
    // drew, and the ladder stayed monotone. The only thing that changed was the
    // gap between the columns, and the only thing that can see it is a number.
    let wide = Rect::new(0, 0, 120, 21);
    let tight = Rect::new(0, 0, 80, 17);

    sweep!("sheet-dimensions", |paint| {
        for (at, want, spelling) in [(wide, (104u16, 14u16), "wide"), (tight, (76, 14), "tight")] {
            let (buf, laid) = paint(at);
            let sheet = laid.sheet.expect("the pane draws no sheet at all");
            let (_, drawn) = read_sheet(&buf, &laid);
            assert!(
                drawn.contains("keyboard"),
                "the {spelling} case stopped taking the two-column rung, so the \
                 size below is not the one being pinned:\n{drawn}"
            );
            assert_eq!(
                (sheet.width, sheet.height),
                want,
                "the {spelling} two-column rung is not the size SPEC.md §11.1 \
                 states:\n{drawn}"
            );
        }
    });
}

#[test]
fn the_two_column_rung_arrives_at_the_width_the_ruling_states() {
    // **78, and the first draft of this ruling said 80.** `margin_of` returns 2 at
    // 78, 3 at 79 and 4 at 80, so the room a pane leaves is 76 at all three and
    // the tight two-column rung is exactly 76 wide. The probe that produced the
    // ruling's before-and-after sampled 60, 70, 80 and stepped over the boundary,
    // which is the same mistake as a superlative bounded by a sweep that does not
    // reach its falsifying case.
    //
    // Gated by walking the boundary a column at a time, because that is the only
    // instrument that can find it.
    let arrival;
    sweep!("sheet-arrival", |paint| {
        arrival = arrival_of(&mut paint, "keyboard", 70..=84, 19);
    });
    assert_eq!(
        arrival,
        Some(78),
        "the two-column rung does not arrive where SPEC.md §11.1 says it does"
    );
}

#[test]
fn the_sheet_is_centred_and_clears_the_footer_at_every_rung() {
    // **Two claims no gate could fail before.** Bottom-aligning the sheet, or
    // pinning it to the left margin, left every other gate in this file green: the
    // frame is still closed, the gestures are all there, the size is right. And
    // "never leaves the body" was bounded against the pane's **last row** rather
    // than the footer's first, so it could not fail for footer encroachment at
    // all. B12's reason for a box is that a reader must still see the tool is
    // alive behind it, and the footer is half of what they are looking at.
    //
    // Clearance is swept, because the footer's own height is a ladder in the width
    // and one screen cannot find where the two ladders meet. The footer is located
    // by the `q quit` the hint bar always spells, read off the screen rather than
    // asked of the layout: a gate that imported the layout's idea of where the
    // footer starts would agree with it by construction.
    let mut seen = 0;
    let mut cleared = 0;
    sweep!("sheet-centred", |paint| {
        for w in LADDER_WIDTHS {
            for h in LADDER_HEIGHTS {
                let at = Rect::new(0, 0, w, h);
                let (buf, laid) = paint(at);
                let Some(sheet) = laid.sheet else { continue };
                seen += 1;
                assert!(sheet.top > at.y, "the sheet reached the header at {w}x{h}");

                // **The three rows `SHEET_KEEP` promises survive.** `KEYBOARD`'s
                // docblock names them and nothing asserted them: dropping
                // `SHEET_KEEP` to 2 adds a rung that loses `f`, and it fires only
                // where the body is four rows, which is one height in this whole
                // sweep. The unguessable outlives the reflexive, or it does not.
                let (_, drawn) = read_sheet(&buf, &laid);
                for kept in ["follow the newest", "churn band", "this sheet"] {
                    assert!(
                        drawn.contains(kept),
                        "a {w}x{h} pane drew a sheet without {kept:?}, which \
                         `SHEET_KEEP` promises is never dropped:\n{drawn}"
                    );
                }

                let pane = text_of(&buf, at);
                if let Some(hint) = pane.lines().position(|r| r.contains("q quit")) {
                    cleared += 1;
                    let hint = u16::try_from(hint).expect("a pane taller than u16");
                    assert!(
                        hint >= sheet.top + sheet.height,
                        "the sheet covers the hint bar at {w}x{h}: it ends at row \
                         {} and the bar is on row {hint}",
                        sheet.top + sheet.height
                    );
                }
            }
        }
    });
    assert!(
        seen > 0,
        "the sweep drew no sheet at all, so it proves nothing"
    );
    assert_eq!(
        cleared,
        seen,
        "the clearance assertion skipped {} of {seen} panes. `HINT_RUNGS` drops \
         `q quit` at rung 1, so a pane narrow enough to lose it would leave the \
         narrow half of this gate unchecked while a `cleared > 0` total still \
         looked healthy. Every pane in this sweep that draws a sheet also draws \
         rung 0, and that is the fact this asserts rather than assumes",
        seen - cleared
    );

    // Centring itself is pinned at named panes rather than swept, because the body
    // it is centred in is the pane less a footer whose height is its own ladder,
    // and a gate that reconstructed that would be reconstructing the layout. These
    // four were checked by hand against `margins_of` and the plan's own halving.
    sweep!("sheet-origin", |paint| {
        for (w, h, want) in [
            (120u16, 30u16, (32u16, 5u16, 56u16, 19u16)),
            // The roomy rung, which #285 put at the head of the ladder. A pane
            // this tall took the nineteen-row sheet at (22, 10, 56, 19) before it
            // existed, and the row it lost to air it had spare.
            (100, 40, (16, 5, 68, 29)),
            (120, 21, (8, 3, 104, 14)),
            (80, 17, (2, 1, 76, 14)),
            // Odd slack, which all four above lack on both axes: halving the slack
            // the other way (`div_ceil`) or taking the trailing margin instead of
            // the leading one reproduces every one of them and misses these.
            (81, 25, (12, 2, 56, 19)),
            (43, 25, (4, 5, 35, 13)),
            // The level probe's own boundary. `margin_of(58)` is 2, so the room is
            // exactly 56 and the wide one-column sheet is exactly 56: turning the
            // probe's `>` into `>=` flips this width and no other, dropping
            // `Ctrl+C`, `PgUp`, `Home`, `End` and the shifted arrows at a width
            // that fits them. Every count, every frame and every other origin is
            // identical.
            (58, 30, (1, 5, 56, 19)),
        ] {
            let at = Rect::new(0, 0, w, h);
            let (_, laid) = paint(at);
            let sheet = laid.sheet.expect("a pane that draws no sheet at all");
            assert_eq!(
                (sheet.left, sheet.top, sheet.width, sheet.height),
                want,
                "the sheet is not where centring in the body puts it at {w}x{h}"
            );
        }
    });
}

#[test]
fn the_two_column_rung_places_its_cells_where_the_plan_says() {
    // **The gap between a keys cell and its verb is spent in two expressions**,
    // one summing the sheet's width and one placing the row. Changing only the
    // second moved every verb column in both shapes and left the width, the frame
    // and the gesture count exactly as planned, so nothing in this file could see
    // it. The same is true of the ` mouse ` label's column, which is round 0's
    // surviving mutant relocated onto the heading row.
    //
    // Columns are stated as literals rather than derived, because a gate that
    // computed them from the layout would agree with it by construction.
    for (w, h, cols, label, spelling) in [
        (120u16, 21u16, [2usize, 26, 56, 77], 56usize, "wide"),
        (80, 17, [2, 15, 35, 50], 35, "tight"),
    ] {
        sweep!("sheet-columns", |paint| {
            let at = Rect::new(0, 0, w, h);
            let (buf, laid) = paint(at);
            let (_, sheet) = read_sheet(&buf, &laid);
            assert!(
                sheet.contains("keyboard"),
                "the {spelling} case is not the two-column rung:\n{sheet}"
            );
            let rows: Vec<Vec<char>> = sheet.lines().map(|r| r.chars().collect()).collect();

            // The heading row: the label sits one column back from the keys cells
            // it names, so the space it opens with lands on the rule.
            let heading: String = rows[1].iter().collect();
            assert_eq!(
                heading
                    .char_indices()
                    .nth(label)
                    .map(|(i, _)| &heading[i..])
                    .map(|t| t.starts_with("mouse ")),
                Some(true),
                "the {spelling} mouse label does not start at column {label}:\n{sheet}"
            );

            // **Which group is in which column, and which row is on top.**
            // Swapping the two groups' contents, or reversing the rows inside
            // one, left every column assertion below satisfied: the pair pins a
            // field *boundary*, not *which row*. The keyboard group opens with
            // `j` and the mouse group with `wheel`, in the tables' own order.
            // `j` rather than `q` since #285: the reader's order opens on
            // `moving` and `q` is the last row of the table, not the first.
            let row2: String = rows[2].iter().collect();
            let keyboard_first: String = rows[2][cols[0]..].iter().collect();
            let mouse_first: String = rows[2][cols[2]..].iter().collect();
            assert!(
                keyboard_first.starts_with('j'),
                "the {spelling} left column does not open with the keyboard \
                 group's first row:\n{sheet}"
            );
            assert!(
                mouse_first.starts_with("wheel"),
                "the {spelling} right column does not open with the mouse group's \
                 first row:\n{sheet}"
            );
            assert!(
                !row2.is_empty(),
                "the {spelling} rung drew no first row at all:\n{sheet}"
            );

            // The keyboard label's own column, which only the mouse label's was
            // pinned against.
            let heading_head: String = rows[1][1..].iter().collect();
            assert!(
                heading_head.starts_with(" keyboard "),
                "the {spelling} keyboard label is not against the frame:\n{sheet}"
            );

            // **Every row of both groups, in order, not merely the ends.**
            // Pinning the first and the last leaves the rows between them free
            // to permute: swapping the middle two mouse rows changed no count,
            // no column and no frame. `KEYBOARD`'s docblock calls its order "the
            // order a reader meets it" and `MOUSE`'s is the ladder's drop order,
            // so the sequence is a property rather than an accident.
            //
            // Walked against [`GESTURES`], which is already one entry per row in
            // table order and is restated here rather than imported, so this
            // cannot agree with the renderer by construction.
            for (n, token) in GESTURES.iter().enumerate() {
                let (col, row) = if n < 11 {
                    (cols[0], 2 + n)
                } else {
                    (cols[2], 2 + (n - 11))
                };
                let text: String = rows[row][col..].iter().collect();
                assert!(
                    text.contains(token),
                    "the {spelling} rung's row {row} does not carry {token:?}, so \
                     the two groups' rows are not in the order their tables \
                     declare:\n{sheet}"
                );
            }

            // The first gesture row carries all four fields.
            let first = &rows[2];
            for at_col in cols {
                assert!(
                    first[at_col] != ' ',
                    "the {spelling} rung has no cell at column {at_col}, so a field \
                     moved without the sheet's width moving:\n{sheet}"
                );
                assert!(
                    first[at_col - 1] == ' ',
                    "the {spelling} rung's cell at column {at_col} is not the start \
                     of its field:\n{sheet}"
                );
            }
        });
    }
}

#[test]
fn the_height_ladder_drops_the_fewest_rows_it_can() {
    // **The worst regression available in this element, and nothing could see
    // it.** Reversing the dropping rungs makes `from = 8` the first rung tried, so
    // it fits almost any pane and every short one draws three gestures instead of
    // the eleven it can afford. Every other gate stayed green: additivity reads a
    // cell the reversal does not touch, monotonicity is satisfied because three to
    // sixteen is still non-decreasing, the degrade gate asks only whether the
    // mouse group is absent, and the origins, the size, the arrival and the frame
    // are all unchanged.
    //
    // What no gate stated is that the ladder drops **as little as it can**. Pinned
    // at a width too narrow for the two-column rung, so height is the only axis
    // moving, and as literals rather than as a rule derived from the ladder: one
    // row of pane buys exactly one more gesture until the keyboard group is whole
    // at eleven, then nothing until the mouse group fits.
    let expected = [
        (8u16, 3usize),
        (9, 4),
        (10, 5),
        (11, 6),
        (12, 7),
        (13, 8),
        (14, 9),
        (15, 10),
        (16, 11),
        (17, 11),
        (21, 11),
        (22, GESTURES.len()),
    ];
    sweep!("sheet-drop-order", |paint| {
        for (h, want) in expected {
            let at = Rect::new(0, 0, 50, h);
            let (buf, laid) = paint(at);
            let (count, sheet) = read_sheet(&buf, &laid);
            assert_eq!(
                count, want,
                "a 50 by {h} pane draws {count} gestures where the ladder should \
                 leave {want}:\n{sheet}"
            );
        }
    });
}

#[test]
fn the_keys_cell_is_lit_and_the_verb_is_dim() {
    // B12 rules the keys cell lit against a dim verb, and swapping the two weights
    // changes no glyph, so every gate reading `text_of` is blind to it. The close
    // control has had this treatment since #211; the rows never did. No other test
    // in the suite reads a foreground inside the sheet's rect.
    let theme = Theme::default();
    let scratch = Scratch::large_diff("sheet-weights", FILES, 40);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    toggle(&mut app, &mut frame);

    for (w, h, keys_at, verb_at, spelling) in [
        (120u16, 21u16, 2u16, 26u16, "two columns"),
        (120, 30, 2, 26, "one column"),
    ] {
        let at = Rect::new(0, 0, w, h);
        let (buf, laid) = paint(&mut app, &mut frame, &mut highlighter, &history, at);
        let sheet = laid.sheet.expect("a pane that draws no sheet");
        let row = sheet.top + 2;
        assert_eq!(
            buf[(sheet.left + keys_at, row)].fg,
            theme.chrome.fg.expect("the chrome weight carries a colour"),
            "the {spelling} keys cell is not drawn lit"
        );
        assert_eq!(
            buf[(sheet.left + verb_at, row)].fg,
            theme
                .chrome_dim
                .fg
                .expect("the dim weight carries a colour"),
            "the {spelling} verb cell is not drawn dim"
        );

        // **The furniture's own weight, which nothing read.** Every frame gate in
        // this file compares glyphs, `the_sheet_is_opaque` reads backgrounds only,
        // and `closing_the_sheet_restores_every_cell` looks outside the sheet. So
        // repainting the whole frame, both headings and the pipes in the lit
        // weight rather than the dim one changed no character and reddened
        // nothing. §11.1 draws this element as chrome behind its own content.
        let dim = theme
            .chrome_dim
            .fg
            .expect("the dim weight carries a colour");
        let right = sheet.left + sheet.width - 1;
        let bottom = sheet.top + sheet.height - 1;
        for (x, y, what) in [
            (sheet.left, sheet.top, "the top-left corner"),
            (sheet.left + 1, sheet.top, "the title bar's rule"),
            (sheet.left, row, "the left pipe"),
            (right, row, "the right pipe"),
            (sheet.left, bottom, "the bottom-left corner"),
            (sheet.left + 1, bottom, "the bottom border"),
        ] {
            assert_eq!(
                buf[(x, y)].fg,
                dim,
                "{what} of the {spelling} sheet is not drawn in the chrome's dim \
                 weight, so the frame competes with the table inside it"
            );
        }
    }

    // The headings are furniture too, and the two-column rung has two of them.
    let at = Rect::new(0, 0, 120, 21);
    let (buf, laid) = paint(&mut app, &mut frame, &mut highlighter, &history, at);
    let sheet = laid.sheet.expect("a pane that draws no sheet");
    let dim = theme
        .chrome_dim
        .fg
        .expect("the dim weight carries a colour");
    for (col, what) in [(2u16, "the keyboard label"), (56, "the mouse label")] {
        assert_eq!(
            buf[(sheet.left + col, sheet.top + 1)].fg,
            dim,
            "{what} is not drawn in the chrome's dim weight"
        );
    }
}

#[test]
fn the_one_column_rung_places_its_cells_where_the_plan_says() {
    // **The rung every reader sees, and its columns were never pinned.** The
    // two-column rung has had absolute columns since #220's round 1; the one that
    // draws on a default 80 by 24 pane had none, so moving `Group { at: 1 }` to
    // `at: 0` shifted the whole table against the left pipe and survived every
    // gate in this file. Width comes from `sheet_fields` rather than from `at`, so
    // the size and origin gates see nothing; the frame gate sees an under-fill
    // rather than an overwrite; and the weights gate reads two cells that are
    // still lit and still dim one column over.
    for (w, h, keys_at, verb_at, first_key, mouse_from) in [
        // `j` rather than `q` since #285: the reader's order starts at `moving`
        // and `q` is the row the ladder gives up first, at the bottom of the table.
        (80u16, 24u16, 2usize, 26usize, 'j', Some(12usize)),
        (120, 30, 2, 26, 'j', Some(12)),
        // A dropping rung, so its first row is not the table's first: at nine
        // gestures the ladder has already dropped `q` and `j k`, and the mouse
        // group is gone entirely.
        (50, 14, 2, 15, 'S', None),
    ] {
        sweep!("sheet-one-column-cells", |paint| {
            let at = Rect::new(0, 0, w, h);
            let (buf, laid) = paint(at);
            let (_, sheet) = read_sheet(&buf, &laid);
            assert!(
                !sheet.contains("keyboard"),
                "the {w}x{h} case is not the one-column rung:\n{sheet}"
            );
            let rows: Vec<Vec<char>> = sheet.lines().map(|r| r.chars().collect()).collect();
            let first = &rows[1];
            assert_eq!(
                first[keys_at], first_key,
                "the one-column keys field does not start at column {keys_at} at \
                 {w}x{h}:\n{sheet}"
            );
            assert_eq!(
                first[keys_at - 1],
                ' ',
                "the one-column keys field is not inset from the frame at \
                 {w}x{h}:\n{sheet}"
            );
            assert!(
                first[verb_at] != ' ' && first[verb_at - 1] == ' ',
                "the one-column verb field does not start at column {verb_at} at \
                 {w}x{h}:\n{sheet}"
            );

            // **The mouse group's rows too, not only the keyboard group's.**
            // Both are drawn by the same loop with the same `Group`, but from
            // two call sites, and only one of them was read: sliding the mouse
            // call's origin one column right moved the whole group on the
            // default 80 by 24 pane and reddened nothing. Every gate with an
            // absolute mouse column ran on the two-column rung only, and the
            // weights gate reads a keyboard row.
            if let Some(heading) = mouse_from {
                let rule: String = rows[heading].iter().collect();
                assert!(
                    rule.contains("mouse"),
                    "row {heading} is not the mouse group's heading at {w}x{h}, \
                     so the rows below it are not what this pins:\n{sheet}"
                );
                let mouse_first = &rows[heading + 1];
                assert_eq!(
                    mouse_first[keys_at], 'w',
                    "the one-column mouse group's keys field does not start at \
                     column {keys_at} at {w}x{h}:\n{sheet}"
                );
                assert_eq!(
                    mouse_first[keys_at - 1],
                    ' ',
                    "the one-column mouse group is not inset from the frame at \
                     {w}x{h}:\n{sheet}"
                );
                assert!(
                    mouse_first[verb_at] != ' ' && mouse_first[verb_at - 1] == ' ',
                    "the one-column mouse group's verb field does not start at \
                     column {verb_at} at {w}x{h}:\n{sheet}"
                );
            }
        });
    }
}

#[test]
fn the_two_column_rung_swallows_what_lands_on_it() {
    // **Every behavioural gate on this element runs at 80 by 24, which takes the
    // one-column rung.** The two-column rung was proven only by geometry and by
    // what the text says. It matters most here: the sheet went from 56 columns to
    // 104, so a click at column 90 used to reach a diff row and is now the
    // sheet's, and nothing resolved a gesture at that shape.
    //
    // B12's rule is that the sheet swallows what lands on it rather than letting
    // it fall through, because a click seeking a scrollbar the reader cannot see
    // is the one way something that moves no content could still move content.
    let scratch = Scratch::large_diff("sheet-beside-input", FILES, 40);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let at = Rect::new(0, 0, 120, 21);

    let height = body_layout(at, &chrome(&app), FILES).diff;
    assert!(
        app.apply(Action::ToggleSheet, &mut frame, height)
            .expect("toggle"),
        "the sheet's toggle asked the shell to quit"
    );

    let (_, laid) = paint(&mut app, &mut frame, &mut highlighter, &history, at);
    let sheet = laid.sheet.expect("a 120 by 21 pane draws no sheet");
    assert_eq!(
        sheet.width, 104,
        "this gate is not looking at the two-column rung"
    );

    // A column the one-column sheet never covered, which this rung does.
    let inside = sheet.left + 90;
    assert!(
        sheet.covers(inside, sheet.top + 3),
        "column {inside} is not inside the two-column sheet, so the case below \
         proves nothing"
    );
    assert!(
        action_for(&click(inside, sheet.top + 3), laid).is_none(),
        "a click inside the two-column rung fell through to whatever is beneath it"
    );
    assert!(
        action_for(&wheel(inside, sheet.top + 3), laid).is_none(),
        "a wheel inside the two-column rung scrolled a region the sheet is covering"
    );

    // And the close control still dismisses at this rung.
    let close = action_for(&click(sheet.close.0, sheet.close.1), laid);
    assert_eq!(
        close,
        Some(Action::ToggleSheet),
        "the close control does not dismiss the two-column rung"
    );
}

#[test]
fn the_two_guards_no_rung_reaches_are_still_the_right_size() {
    // **Two branches in the layout that nothing can currently make fire**, both
    // documented as guards rather than rungs, and one table edit from going live:
    // #288 adds a `MOUSE` row.
    //
    // **#285 was named here as the other trigger and it was not one.** The
    // prediction was that it would *shrink* the tables; what it did was reorder
    // them and add a rung above them, and a reorder moves no field width at all.
    // The narrowest rung is still the deepest dropping one, whose kept rows are
    // still `f`, `m` and `?`, so both numbers below are unmoved. Corrected rather
    // than deleted, because a prediction that quietly vanished would read as one
    // still pending.
    //
    // A guard whose slack nobody states is a guard nobody will notice binding, so
    // the numbers are asserted rather than described. They are restated here
    // rather than imported for [`TITLE`]'s reason.
    //
    // **What this gate cannot do, said plainly rather than implied.** A branch
    // that never fires has no observable value, so no gate can pin the production
    // constants themselves: `sheet_floor`'s `+ 6` could become `+ 13` and the
    // `.max` could be deleted outright, and nothing here would redden, because
    // neither changes a cell. What is pinned is the *slack* — that the narrowest
    // rung the ladder draws clears the floor, and that the keyboard block clears
    // its own label — which is the property that decides whether the guard is
    // live. The day #288 adds a `MOUSE` row, one of these stops clearing, this gate reddens, and the behaviour behind the guard
    // becomes reachable and untested. That is the moment to gate the value; until
    // then there is nothing to gate.
    //
    // The roomy rung is not a candidate either: it is the *widest* rung on the
    // ladder at sixty-eight columns, so it moves neither of the numbers below.
    //
    // `sheet_floor` raises any rung to the width its own title bar needs. The
    // title is `─ gestures ` at eleven columns, plus a border and a space at each
    // end and the two-of-gap the fields already carry, so the floor is 17.
    let floor = "─ gestures ".chars().count() + 6;
    assert_eq!(floor, 17, "the title bar's floor is not what §11.1 states");

    // The narrowest sheet the ladder can actually produce, and the narrowest
    // two-column one. Both are well clear of the floor, which is why it is a
    // guard: measured, not assumed.
    sweep!("sheet-guards", |paint| {
        let mut narrowest = u16::MAX;
        let mut narrowest_beside = u16::MAX;
        for w in LADDER_WIDTHS {
            for h in LADDER_HEIGHTS {
                let at = Rect::new(0, 0, w, h);
                let (buf, laid) = paint(at);
                let Some(sheet) = laid.sheet else { continue };
                let (_, drawn) = read_sheet(&buf, &laid);
                narrowest = narrowest.min(sheet.width);
                if drawn.contains("keyboard") {
                    narrowest_beside = narrowest_beside.min(sheet.width);
                }
            }
        }
        // 24 is a property of the tables, not of this sweep's floor: probed
        // across every pane from 1x1 to 39x39 as well, the narrowest sheet drawn
        // anywhere is the same 24, first appearing at a pane exactly that wide.
        assert_eq!(
            narrowest, 24,
            "the narrowest rung the ladder draws is not the 24 columns §11.1 \
             names, so `sheet_floor`'s slack is not what it says"
        );
        assert_eq!(
            narrowest_beside, 76,
            "the narrowest two-column rung is not the 76 columns §11.1 names"
        );
        assert!(
            narrowest > floor as u16,
            "`sheet_floor` has stopped being a guard and become a rung: the \
             narrowest sheet is {narrowest} against a floor of {floor}"
        );
    });

    // The keyboard block must be wide enough for its own heading label, or
    // ` mouse ` would land inside the word `keyboard`. The label needs
    // `mouse.at >= 1 + 10`; the keyboard block puts it at 55 wide and 34 tight.
    let label = " keyboard ".chars().count();
    assert_eq!(
        label, 10,
        "the keyboard label is not the width the floor assumes"
    );
    for (w, h, at) in [(120u16, 21u16, 55usize), (80, 17, 34)] {
        sweep!("sheet-label-floor", |paint| {
            let pane = Rect::new(0, 0, w, h);
            let (buf, laid) = paint(pane);
            let (_, sheet) = read_sheet(&buf, &laid);
            let heading = sheet
                .lines()
                .nth(1)
                .expect("a two-column sheet has a heading");
            // By character rather than by byte: the heading is mostly box-drawing
            // glyphs, so `str::find` returns an offset three times the column.
            let cells: Vec<char> = heading.chars().collect();
            let mouse_at = (0..cells.len())
                .find(|&i| cells[i..].starts_with(&['m', 'o', 'u', 's', 'e']))
                .expect("the mouse label is drawn");
            assert_eq!(
                mouse_at - 1,
                at,
                "the mouse label sits at {mouse_at} rather than {at} at {w}x{h}, \
                 so the heading floor's stated slack is wrong"
            );
            assert!(
                at > label,
                "the keyboard block no longer clears its own label, so the floor \
                 has gone live and its behaviour is untested"
            );
        });
    }
}
