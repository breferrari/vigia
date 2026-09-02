//! The gestures sheet: `?`, the window it opens, and the one thing it must not do.

#[path = "../../vigia-core/tests/support/mod.rs"]
mod support;

use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::Rect;
use ratatui::style::Color;
use vigia::{
    Action, App, Chrome, Glyphs, Grabbed, Hovered, Pointing, Regions, Sheet, Theme, action_for,
    body_layout, regions, render,
};
use vigia_core::{Frame, Highlighter, History};

use support::{Scratch, materialise};

const WIDE: u16 = 80;

/// The pane height at which the sheet draws every gesture on one page.
const WHOLE_TABLE: u16 = 31;

/// Keyboard gestures the sheet's table holds, as a reader counts them on screen.
const KEYBOARD_ROWS: u16 = 16;
const TALL: u16 = 24;
const FILES: usize = 3;

/// The word the sheet's own title bar spells, restated rather than imported.
const TITLE: &str = "gestures";

/// The close control's glyph, restated for [`TITLE`]'s reason.
const SHEET_CLOSE: char = '✕';

/// The glyph the sheet's frame and its group headings are ruled with, restated
/// for [`TITLE`]'s reason. No keys cell or verb holds one, which is what makes it
/// the way to tell furniture from a row.
const RULE: char = '─';

/// Keyboard rows the height ladder may never drop, restated for [`TITLE`]'s
/// reason. `SPEC.md` §11.1 names the three: `f`, `m` and `?`.
const KEEP: usize = 3;

fn area() -> Rect {
    Rect::new(0, 0, WIDE, TALL)
}

fn chrome(app: &App) -> Chrome {
    app.chrome("fixture", Some("main"), Pointing::default(), 0, "")
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

/// Press `?` on the default pane, through the app rather than around it.
fn toggle(app: &mut App, frame: &mut Frame<'_>) {
    toggle_at(app, frame, area());
}

/// [`toggle`] on a pane of the caller's choosing.
fn toggle_at(app: &mut App, frame: &mut Frame<'_>, at: Rect) {
    let height = body_layout(at, &chrome(app), FILES, FILES).diff;
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
fn paint_with(
    app: &mut App,
    frame: &mut Frame<'_>,
    highlighter: &mut Highlighter,
    history: &History,
    at: Rect,
    theme: &Theme,
) -> (Buffer, Regions) {
    let chrome = chrome(app);
    let body = body_layout(at, &chrome, FILES, FILES);
    let view = app
        .view(frame, highlighter, history, body)
        .expect("collect a view");
    let mut buf = Buffer::empty(at);
    render(&mut buf, at, &view, theme, Glyphs::default(), &chrome);
    let laid = regions(at, &chrome, &view);
    (buf, laid)
}

/// The page counter on the sheet's title bar, or `None` when it draws none.
fn counter_of(sheet: &str) -> Option<String> {
    let top: Vec<char> = sheet.lines().next()?.chars().collect();
    let title = "─ gestures ".chars().count();
    if top.len() < title + 5 {
        return None;
    }
    let counter: String = top[title + 1..top.len() - 4]
        .iter()
        .take_while(|c| **c != RULE)
        .collect();
    (!counter.is_empty()).then_some(counter)
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
    // `Esc` asks to leave the frontmost thing, and B12's refusal was reversed by its
    // own reason.
    assert_eq!(
        action_for(&press(KeyCode::Esc), Regions::default()),
        Some(Action::Escape),
        "`Esc` no longer asks to leave the frontmost thing"
    );

    let scratch = Scratch::large_diff("sheet-open", FILES, 40);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    assert!(chrome(&app).sheet.is_none(), "a fresh shell drew a sheet");
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
    assert_eq!(
        chrome(&app).sheet,
        Some(0),
        "`?` did not open the sheet at its first page"
    );
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
    assert!(
        chrome(&app).sheet.is_none(),
        "`?` did not close the sheet again, so an eighty by twenty-four pane, whose \
         sheet is one page, has stopped being a toggle"
    );
    let (shut, _) = paint(&mut app, &mut frame, &mut highlighter, &history, area());
    assert!(
        !text_of(&shut, area()).contains(TITLE),
        "closing the sheet left it drawn"
    );
}

#[test]
fn the_sheet_moves_no_content() {
    // B12's load-bearing claim, and the reason it was ruled over anything living in the
    // footer.
    let scratch = Scratch::large_diff("sheet-still", FILES, 40);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    // Past the first paint, and the first version of this gate was not.
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

/// A pane the roomy rung fits on: a room of 68 columns and a body of 31 rows.
const ROOMY_PANE: Rect = Rect::new(0, 0, 120, 41);

#[test]
fn the_sheet_is_opaque() {
    // The defect this shipped with, as a gate.
    let scratch = Scratch::large_diff("sheet-opaque", FILES, 40);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::past_first_paint();
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    // The wash is injected rather than borrowed from the shipped palette, and that is
    // the difference between gating the mechanism and gating a theme: what failed in
    // the field is `set_style` patching, which shows up wherever *any* theme puts a
    // background on a row.
    let mut washed_theme = Theme::default();
    washed_theme.added_row = washed_theme.added_row.bg(Color::Green);
    washed_theme.removed_row = washed_theme.removed_row.bg(Color::Red);

    // At both rungs.
    for at in [area(), ROOMY_PANE] {
        let (closed, _) = paint_with(
            &mut app,
            &mut frame,
            &mut highlighter,
            &history,
            at,
            &washed_theme,
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

        // Non-vacuity, counted under the sheet's own rect rather than over the pane.
        let washed = (sheet.top..sheet.top + sheet.height)
            .flat_map(|y| (sheet.left..sheet.left + sheet.width).map(move |x| (x, y)))
            .filter(|&(x, y)| !matches!(closed[(x, y)].style().bg, None | Some(Color::Reset)))
            .count();
        assert!(
            washed > 0,
            "no cell the {}x{} pane's sheet covers carried a background before it \
             opened, so this fixture cannot show a wash through the sheet",
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
    // The other half of *it moves nothing*: not only must the pane outside the sheet be
    // untouched while it is up, the pane must come back exactly when it goes.
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
    // A control that never brightens is a glyph a reader has to guess at,
    // and B10's ladder has the rungs for it: chrome at rest and `bar_hover`
    // under the pointer.
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
    let at_rest = drawn_close(&mut app, &mut frame, &mut highlighter, &history, None);
    let hovered = drawn_close(
        &mut app,
        &mut frame,
        &mut highlighter,
        &history,
        Some(Hovered::Button(cx, cy)),
    );
    assert_ne!(
        at_rest, hovered,
        "the close control draws the same under the pointer as at rest, so nothing \
         says it is clickable"
    );
    // Compared by weight rather than by whole `Style`, because the drawer resets the
    // cell before styling it, so the drawn style carries an explicit `Color::Reset`
    // background where the theme's leaves it unset.
    assert_eq!(
        weight(hovered.1),
        weight(theme.bar_hover),
        "the hovered control is not on B10's hover rung"
    );
    // The bottom rung, which is the one of two nothing asserted positively.
    assert_eq!(
        weight(at_rest.1),
        weight(theme.chrome),
        "the resting control is not on B10's chrome rung, so it is drawn as \
         something other than a control at rest"
    );
    assert_eq!(
        at_rest.0, SHEET_CLOSE,
        "the control stopped being the glyph this gate is about"
    );
}

/// Walk every pane in `widths` x `heights` that draws a sheet, and hand each one's
/// published regions and sheet to `check`. Returns how many drew one, and how many
/// panes the grid held.
fn over_sheets(
    name: &str,
    widths: std::ops::RangeInclusive<u16>,
    heights: &[u16],
    mut check: impl FnMut(u16, u16, Regions, Sheet),
) -> (usize, usize) {
    let scratch = Scratch::large_diff(name, FILES, 40);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    let panes = widths.clone().count() * heights.len();
    let mut drew = 0usize;
    for width in widths {
        for &height in heights {
            let at = Rect::new(0, 0, width, height);
            let mut app = App::past_first_paint();
            toggle_at(&mut app, &mut frame, at);
            let (_, laid) = paint(&mut app, &mut frame, &mut highlighter, &history, at);
            // A pane below the sheet's own floor draws none, which is B13's ruling
            // rather than a gap: `?` still toggles and nothing is drawn.
            let Some(sheet) = laid.sheet else { continue };
            drew += 1;
            check(width, height, laid, sheet);
        }
    }
    // Bounded above as well as returned, because every caller's floor is a `>` and a
    // `>` is blind upwards: a mutation that counted a pane twice, or counted the ones
    // that drew no sheet, makes every floor pass more easily.
    assert!(
        drew < panes,
        "the sweep counted {drew} sheets over {panes} panes, so it is counting \
         panes that drew none and the floors below it mean nothing"
    );
    (drew, panes)
}

/// Every cell the sheet covers, as a flat iterator.
fn cells_of(sheet: Sheet) -> impl Iterator<Item = (u16, u16)> {
    (sheet.top..sheet.top.saturating_add(sheet.height)).flat_map(move |row| {
        (sheet.left..sheet.left.saturating_add(sheet.width)).map(move |column| (column, row))
    })
}

#[test]
fn a_press_under_the_sheet_arms_no_step() {
    // The producer half of a rule this suite already had the decider half of.
    let mut guarded = 0usize;
    let heights: Vec<u16> = (8..=40).collect();
    let (drew, _) = over_sheets(
        "sheet-press-under",
        30..=140,
        &heights,
        |width, height, laid, sheet| {
            // The same regions with the sheet taken out, which is the only way to
            // ask what a cell would have answered without it.
            let bare = Regions {
                sheet: None,
                ..laid
            };
            for (column, row) in cells_of(sheet) {
                // Both halves, because the defect is that they disagreed.
                assert_eq!(
                    laid.step_at(column, row),
                    None,
                    "on a {width} by {height} pane, ({column},{row}) is under the \
                     sheet and still arms a hold, so holding it repeats a scroll \
                     on a region the reader cannot see"
                );
                assert!(
                    action_for(&click(column, row), laid).is_none() || (column, row) == sheet.close,
                    "on a {width} by {height} pane, a click at ({column},{row}) \
                     fell through the sheet"
                );
                guarded += usize::from(bare.step_at(column, row).is_some());
            }
        },
    );

    assert!(
        drew > 1000,
        "only {drew} panes drew a sheet, so this sweep is thin"
    );
    // The assertion that makes the loop above mean something.
    assert!(
        guarded > 0,
        "no cell under the sheet would have armed a hold without the guard, so \
         this gate is passing over panes that prove nothing: the sheet no longer \
         reaches a scrollbar at any size swept, and the 85 cells #298 measured \
         are gone for some other reason"
    );
}

#[test]
fn nothing_can_press_the_close_control() {
    // Defence rather than behaviour, and it is written down as such because it cannot
    // be made to go red.
    let (drew, panes) = over_sheets(
        "sheet-close-press",
        30..=140,
        &[8, 13, 24, 40],
        |width, height, laid, sheet| {
            assert_eq!(
                laid.step_at(sheet.close.0, sheet.close.1),
                None,
                "on a {width} by {height} pane the close control arms a hold, so \
                 `Chrome::pressed` can carry its cell and the weight #298 deleted \
                 was reachable after all"
            );
            // And the control still does the one thing it is for, so this gate
            // cannot pass by the sheet having no control on it.
            assert_eq!(
                action_for(&click(sheet.close.0, sheet.close.1), laid),
                Some(Action::CloseSheet),
                "on a {width} by {height} pane the close control stopped dismissing"
            );
        },
    );

    // Proportional to the grid rather than a round number. 111 widths against four
    // heights is 444 panes, and the ones that draw no sheet are the narrow and short
    // corner B13 rules out, so the great majority draw one.
    assert!(
        drew * 4 > panes * 3,
        "only {drew} of {panes} panes drew a sheet, so this sweep has stopped \
         covering the ladder rather than proving anything about it"
    );
}

#[test]
fn a_press_on_a_track_under_the_sheet_grabs_nothing() {
    // The sibling call site, and the one most easily missed.
    let mut guarded = 0usize;
    let heights: Vec<u16> = (8..=40).collect();
    let (drew, _) = over_sheets(
        "sheet-grab-under",
        30..=140,
        &heights,
        |width, height, laid, sheet| {
            let bare = Regions {
                sheet: None,
                ..laid
            };
            for (column, row) in cells_of(sheet) {
                assert_eq!(
                    laid.grab_at(column, row),
                    None,
                    "on a {width} by {height} pane, ({column},{row}) is under the \
                     sheet and still takes hold of a bar, so the next drag moves a \
                     region the reader cannot see"
                );
                guarded += usize::from(bare.grab_at(column, row).is_some());
            }
        },
    );

    assert!(
        drew > 1000,
        "only {drew} panes drew a sheet, so this sweep is thin"
    );
    // The same vacuity floor its sibling carries, and for the same reason: without
    // it a rung change that moved the box clear of every bar would leave this gate
    // green over panes that prove nothing.
    assert!(
        guarded > 0,
        "no cell under the sheet would have taken hold of a bar without the \
         guard, so this gate is passing over panes that prove nothing"
    );
}

/// What a style says in ink: its foreground and its modifiers.
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
) -> (char, ratatui::style::Style) {
    let chrome = app.chrome(
        "fixture",
        Some("main"),
        Pointing {
            hovered,
            ..Pointing::default()
        },
        0,
        "",
    );
    let body = body_layout(area(), &chrome, FILES, FILES);
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

    assert!(chrome(&app).sheet.is_some(), "a write turned the sheet off");
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
        Some(Action::CloseSheet),
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
    // the layout rather than in the painter, on the band's own recorded reason.
    let scratch = Scratch::large_diff("sheet-ladder", FILES, 40);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    // 80 by 26 rather than the default pane, and the failure that causes is worth
    // naming: at twenty-four rows the pane takes the two-column rung, whose *tight*
    // spelling draws `q Esc` where this gate looks for `Ctrl+C`.
    let wide_at = Rect::new(0, 0, WIDE, WHOLE_TABLE);
    toggle_at(&mut app, &mut frame, wide_at);

    let (buf, _) = paint(&mut app, &mut frame, &mut highlighter, &history, wide_at);
    let wide = text_of(&buf, wide_at);
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

    // A short pane keeps the keyboard group and loses the mouse group, which is the
    // height axis on its own.
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
        chrome(&app).sheet.is_some(),
        "the state stopped being true because the pane was small"
    );
    assert!(
        laid.sheet.is_none(),
        "a sheet nobody can see is still eating gestures"
    );
}

#[test]
fn every_mouse_gesture_the_pane_answers_is_named_on_the_sheet() {
    // The other half of the table, and its own gate rather than a second loop inside
    // the keyboard one.
    let scratch = Scratch::large_diff("sheet-mouse-covers", FILES, 40);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    // The pane its sibling uses, and for the same reason: one column, no section
    // headings, the whole table on one page.
    let at = Rect::new(0, 0, WIDE, WHOLE_TABLE);

    toggle_at(&mut app, &mut frame, at);
    let (buf, laid) = paint(&mut app, &mut frame, &mut highlighter, &history, at);
    let (_, drawn) = read_sheet(&buf, &laid);

    for phrase in mouse_phrases() {
        assert!(
            drawn.contains(phrase),
            "the sheet does not name the mouse gesture {phrase:?}, which the pane \
             answers:\n{drawn}"
        );
    }
}

/// Whether the sheet's keys column names `token`, as a cell or inside a range.
fn names(keys: &str, token: &str) -> bool {
    if keys.split_whitespace().any(|cell| cell == token) {
        return true;
    }
    let Some(ch) = token.chars().next().filter(|_| token.chars().count() == 1) else {
        return false;
    };
    keys.lines().any(|row| {
        let cells: Vec<&str> = row.split_whitespace().collect();
        cells.windows(3).any(|w| {
            w[1] == "to"
                && w[0].chars().count() == 1
                && w[2].chars().count() == 1
                && (w[0].chars().next().unwrap_or('\0')..=w[2].chars().next().unwrap_or('\0'))
                    .contains(&ch)
        })
    })
}

/// The gestures `README.md`'s two tables teach, as the cells a reader meets there.
fn readme_gestures() -> Vec<String> {
    let readme = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../README.md"),
    )
    .expect("README.md is where the crate says it is");

    // The two tables live inside one `<td>` each under a bold caption. A row is
    // `| cell | cell |`, and the left cell is what a reader looks for.
    let mut out = Vec::new();
    let mut inside = false;
    for line in readme.lines() {
        let trimmed = line.trim();
        if trimmed == "**Keys**" || trimmed == "**Mouse**" {
            inside = true;
            continue;
        }
        if inside && trimmed.starts_with("</td>") {
            inside = false;
            continue;
        }
        if !inside || !trimmed.starts_with('|') {
            continue;
        }
        // The header separator and the empty header row carry no gesture.
        if trimmed.contains("---") || trimmed.trim_matches(['|', ' ']).is_empty() {
            continue;
        }
        let Some(cell) = trimmed.split('|').nth(1) else {
            continue;
        };
        let cell = cell.trim().replace('`', "");
        if !cell.is_empty() {
            out.push(cell);
        }
    }
    assert!(
        out.len() > 10,
        "README.md's key and mouse tables parsed to {} rows, so this gate is \
         reading the wrong thing and would pass over anything",
        out.len()
    );
    out
}

#[test]
fn every_gesture_the_readme_teaches_is_named_on_the_sheet() {
    // The comparison the reported defect names, and the one no gate made.
    let scratch = Scratch::large_diff("sheet-readme", FILES, 40);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    // Both one-column rungs, because the README writes the tight spelling.
    let mut rows: Vec<String> = Vec::new();
    for (w, h) in [(WIDE, WHOLE_TABLE), (43u16, WHOLE_TABLE - 1)] {
        let at = Rect::new(0, 0, w, h);
        let mut app = App::new();
        toggle_at(&mut app, &mut frame, at);
        let (buf, laid) = paint(&mut app, &mut frame, &mut highlighter, &history, at);
        let (_, drawn) = read_sheet(&buf, &laid);
        rows.extend(drawn.lines().map(str::to_owned));
    }
    let drawn = rows.join("\n");

    for cell in readme_gestures() {
        // Every token of a cell on ONE drawn row, not each token anywhere on the sheet.
        let wanted: Vec<&str> = cell
            .split_whitespace()
            // `to` joins the digits' range on both sides and is not a gesture.
            .filter(|t| *t != "to")
            .collect();
        assert!(
            !wanted.is_empty(),
            "README.md has a gesture cell {cell:?} with nothing in it to look for"
        );
        // `names` alone, with no `contains` fallback.
        assert!(
            rows.iter().any(|row| wanted.iter().all(|t| names(row, t))),
            "README.md teaches {wanted:?} (in {cell:?}) as one gesture and no single \
             row of the sheet names all of it, so a reader who read one and opened \
             the other is missing a gesture:\n{drawn}"
        );
    }
}

/// One value of every [`Action`] variant, for the two gates that walk them.
const ALL_ACTIONS: [Action; 22] = [
    Action::Quit,
    Action::Escape,
    Action::Scroll(1),
    Action::ScrollList(1),
    Action::Page(1),
    Action::HalfPage(1),
    Action::File(1),
    Action::Top,
    Action::Bottom,
    Action::ToggleFollow,
    Action::ToggleMasthead,
    Action::ToggleRail,
    Action::ToggleSingle,
    Action::ToggleStaged,
    Action::ToggleWrap,
    Action::Yank,
    Action::ToggleSheet,
    Action::CloseSheet,
    Action::ListTo(0),
    Action::ListRow(0),
    Action::DiffTo(0),
    Action::Redraw,
];

/// Where a gesture can be asked for, as the reason a variant is or is not on the
/// sheet's keyboard half.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reach {
    /// A key binds it, so [`bound_keys`] must find it.
    Keyboard,
    /// The pointer produces it, so [`mouse_phrases`] must teach it.
    Mouse,
    /// Both, and both halves apply.
    Both,
    /// Neither asks for it: the shell raises it itself. Carries why, so that a
    /// reader can disagree with the classification rather than only read it.
    Shell(&'static str),
}

/// Where each [`Action`] can be asked for.
fn reach_of(action: &Action) -> Reach {
    match action {
        // The way out, and `q`, `Ctrl+C`, `Ctrl+D` are all keys.
        Action::Quit => Reach::Keyboard,
        // `Esc`, which leaves the sheet if one is up and the program if none
        // is. A key, and the sheet's `leaving` group names it.
        Action::Escape => Reach::Keyboard,
        // `a`, and the sheet's own row for it. No mouse gesture reaches it.
        Action::ToggleStaged => Reach::Keyboard,
        // `w`, and the sheet's own row for it. No mouse gesture reaches it.
        Action::ToggleWrap => Reach::Keyboard,
        // `y`. The pointer has no yank: a drag is the terminal's own selection,
        // which the pane does not answer today, and whether it should is open.
        Action::Yank => Reach::Keyboard,
        // `j`/`k`/arrows, and the wheel and the step buttons.
        Action::Scroll(_) => Reach::Both,
        // `J`/`K`/`Shift`+arrows, and the list's own bar.
        Action::ScrollList(_) => Reach::Both,
        Action::Page(_) => Reach::Keyboard,
        Action::HalfPage(_) => Reach::Keyboard,
        // `n`/`p` and the horizontal arrows.
        Action::File(_) => Reach::Keyboard,
        Action::Top => Reach::Keyboard,
        Action::Bottom => Reach::Keyboard,
        Action::ToggleFollow => Reach::Keyboard,
        Action::ToggleMasthead => Reach::Keyboard,
        Action::ToggleRail => Reach::Keyboard,
        Action::ToggleSingle => Reach::Keyboard,
        Action::ToggleSheet => Reach::Keyboard,
        // The close control, and the row most easily named nowhere at all.
        Action::CloseSheet => Reach::Mouse,
        // Dragging or clicking a bar. No key seeks to a fraction.
        Action::ListTo(_) => Reach::Mouse,
        Action::DiffTo(_) => Reach::Mouse,
        // The digits, and a click on a listed file.
        Action::ListRow(_) => Reach::Both,
        Action::Redraw => Reach::Shell(
            "a resize, which the terminal reports and no reader asks for, so there \
             is nothing to teach",
        ),
    }
}

/// The candidate key space the sweep walks.
fn candidate_keys() -> Vec<KeyEvent> {
    let mut codes: Vec<KeyCode> = (b' '..=b'~').map(|c| KeyCode::Char(c as char)).collect();
    codes.extend([
        KeyCode::Up,
        KeyCode::Down,
        KeyCode::Left,
        KeyCode::Right,
        KeyCode::Home,
        KeyCode::End,
        KeyCode::PageUp,
        KeyCode::PageDown,
        KeyCode::Enter,
        KeyCode::Esc,
        KeyCode::Backspace,
        KeyCode::Tab,
        KeyCode::Delete,
        KeyCode::Insert,
    ]);
    codes.extend((1..=12).map(KeyCode::F));
    let mods = [
        KeyModifiers::NONE,
        KeyModifiers::SHIFT,
        KeyModifiers::CONTROL,
        KeyModifiers::ALT,
    ];
    codes
        .into_iter()
        .flat_map(|code| mods.iter().map(move |m| KeyEvent::new(code, *m)))
        .collect()
}

/// How the sheet spells one key event in its keys column.
fn token_of(event: KeyEvent) -> String {
    let base = match event.code {
        KeyCode::Char(c) => c.to_string(),
        KeyCode::Up => "↑".into(),
        KeyCode::Down => "↓".into(),
        KeyCode::Left => "←".into(),
        KeyCode::Right => "→".into(),
        KeyCode::Home => "Home".into(),
        KeyCode::End => "End".into(),
        KeyCode::PageUp => "PgUp".into(),
        KeyCode::PageDown => "PgDn".into(),
        KeyCode::Esc => "Esc".into(),
        // Unreachable today, and carried rather than removed.
        other => format!("{other:?}"),
    };
    // `Space` is drawn by name, because a blank cell is not a cell.
    let base = if base == " " { "Space".into() } else { base };
    match event.modifiers {
        KeyModifiers::CONTROL => format!("Ctrl+{}", base.to_uppercase()),
        KeyModifiers::SHIFT => format!("Shift+{base}"),
        _ => base,
    }
}

/// Every key `action_for` binds, with the token the sheet must spell for it.
fn bound_keys() -> Vec<(KeyEvent, String)> {
    let plain = |code: KeyCode| {
        action_for(
            &Event::Key(KeyEvent::new(code, KeyModifiers::NONE)),
            Regions::default(),
        )
    };
    let mut found: Vec<(KeyEvent, String)> = Vec::new();
    for event in candidate_keys() {
        let Some(action) = action_for(&Event::Key(event), Regions::default()) else {
            continue;
        };
        if event.modifiers != KeyModifiers::NONE && plain(event.code) == Some(action) {
            continue;
        }
        let token = token_of(event);
        if !found.iter().any(|(_, t)| *t == token) {
            found.push((event, token));
        }
    }
    found
}

/// The phrases the sheet must carry for the gestures a pointer produces.
fn mouse_phrases() -> Vec<&'static str> {
    let mut phrases: Vec<&'static str> = Vec::new();

    // The `Action` half, from the same exhaustive table the keyboard half reads.
    for action in ALL_ACTIONS {
        if !matches!(reach_of(&action), Reach::Mouse | Reach::Both) {
            continue;
        }
        // Exhaustive, with no wildcard.
        phrases.extend(match action {
            // The wheel scrolls whichever region is under the pointer, and the
            // step buttons move one row.
            Action::Scroll(_) | Action::ScrollList(_) => vec!["wheel", "click  ▲ ▼"],
            Action::ListTo(_) | Action::DiffTo(_) => vec!["click a track"],
            Action::ListRow(_) => vec!["click a listed file"],
            Action::CloseSheet => vec!["click  ✕"],
            Action::Quit
            | Action::Page(_)
            | Action::HalfPage(_)
            | Action::File(_)
            | Action::Top
            | Action::Bottom
            | Action::ToggleFollow
            | Action::ToggleMasthead
            | Action::ToggleRail
            | Action::ToggleStaged
            | Action::ToggleWrap
            | Action::ToggleSingle
            | Action::ToggleSheet
            | Action::Yank
            | Action::Escape
            | Action::Redraw => Vec::new(),
        });
    }

    // The two pointer states that are not `Action`s at all.
    for hovered in [
        Hovered::Button(0, 0),
        Hovered::Track(Grabbed::Diff),
        Hovered::Row(0),
    ] {
        phrases.push(match hovered {
            // One mark, one phrase: `SPEC.md` §11.1 gives every marked surface the
            // same reading, so three variants share it rather than each naming a
            // row of its own.
            Hovered::Button(..) | Hovered::Track(_) | Hovered::Row(_) => "just point",
        });
    }
    for grabbed in [Grabbed::List, Grabbed::Diff] {
        phrases.push(match grabbed {
            Grabbed::List | Grabbed::Diff => "drag a scrollbar",
        });
    }

    phrases.sort_unstable();
    phrases.dedup();
    phrases
}

#[test]
fn the_action_table_covers_every_variant() {
    // What makes the sweep's coverage checkable.
    let produced: Vec<Action> = bound_keys()
        .into_iter()
        .filter_map(|(event, _)| action_for(&Event::Key(event), Regions::default()))
        .collect();

    // Every action the sweep found must be in the array, which is the half of
    // `ALL_ACTIONS`' upkeep that can be checked against something other than itself.
    for action in &produced {
        assert!(
            ALL_ACTIONS
                .iter()
                .any(|known| std::mem::discriminant(known) == std::mem::discriminant(action)),
            "`action_for` binds a key to {action:?} and `ALL_ACTIONS` does not \
             carry that variant, so both gates walk a set that is no longer the type"
        );
    }

    for action in ALL_ACTIONS {
        let wanted = matches!(reach_of(&action), Reach::Keyboard | Reach::Both);
        let seen = produced
            .iter()
            .any(|got| std::mem::discriminant(got) == std::mem::discriminant(&action));
        assert_eq!(
            seen,
            wanted,
            "`reach_of` says {action:?} is {:?}, and the key sweep {} it: either \
             the candidate space in `candidate_keys` is too narrow to reach its \
             binding, or the table is wrong",
            reach_of(&action),
            if seen { "found" } else { "did not find" }
        );
    }
}

#[test]
fn every_key_the_map_binds_is_named_on_the_sheet() {
    // The gate that fails when somebody adds a key and forgets the sheet, which is the
    // whole reason the sheet is worth having: it is now where the keymap is written
    // down, so a binding missing from it is a binding nobody can find.
    let scratch = Scratch::large_diff("sheet-covers", FILES, 40);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let at = Rect::new(0, 0, WIDE, WHOLE_TABLE);

    toggle_at(&mut app, &mut frame, at);
    let (buf, laid) = paint(&mut app, &mut frame, &mut highlighter, &history, at);

    // Inside the sheet's own rect, and on a rung that draws no headings.
    let (_, drawn) = read_sheet(&buf, &laid);
    assert!(
        !drawn.contains("moving"),
        "this gate searches for bare one-character keys and the pane drew section \
         headings, so `m`, `f`, `g` and `n` are satisfied by `moving`, `files` \
         and `view` rather than by the rows:\n{drawn}"
    );

    // The keys column of the gesture rows alone, because a bare token finds anything.
    let keys: String = drawn
        .lines()
        .filter(|row| !row.contains(RULE))
        .filter_map(|row| {
            let cells: Vec<char> = row.chars().collect();
            (cells.len() > 24).then(|| cells[2..24].iter().collect::<String>())
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        keys.split_whitespace().any(|cell| cell == "Ctrl+C"),
        "the keys column this gate reads is not where the keys are:\n{keys}"
    );

    // Derived from `action_for`, not listed here.
    for (event, token) in bound_keys() {
        assert!(
            names(&keys, &token),
            "`action_for` binds {event:?}, which the sheet's keys column does not \
             name, so that gesture is unfindable:\n{drawn}"
        );
    }

    // And every keyboard verb in full, which is easily left unchecked.
    for verb in [
        "scroll a row",
        "page",
        "half a page",
        "first / last changed file",
        "next / previous changed file",
        "jump to that row of the list",
        "scroll the pinned file list",
        "follow the newest change",
        "show or hide the churn band",
        "show or hide the left rail",
        "one file, or the whole diff",
        "this sheet",
        "quit",
    ] {
        assert!(
            drawn.contains(verb),
            "the wide sheet does not spell the verb {verb:?}, so a reader is \
             being told less than the table says:\n{drawn}"
        );
    }
}

/// A page counter as the sheet spells it.
///
/// The total is derived rather than re-typed, because it *is* `GESTURES.len()`
/// by definition and carries no independent verification: hand-writing it once
/// per assertion is what made adding one gesture a fifteen-line edit. The range
/// stays literal, since that is the pagination arithmetic under test.
fn counter(range: &str) -> String {
    format!("{range} of {}", GESTURES.len())
}

/// Every gesture the sheet can draw, as the token a reader would look for.
const GESTURES: [&str; 25] = [
    "scroll a row",
    "Space  PgDn",
    "half a page",
    "first / last",
    "next / prev",
    "jump to",
    "scroll the",
    "follow the newest",
    "churn band",
    "left rail",
    "one file",
    // B17's row, between `s` and `?` because that is where the reader's own
    // order puts it.
    "staged changes",
    // B19's row, after `a` because that is where the reader's own order puts it.
    "wrap",
    // B9's row, last of `view` because it is the one key that spends something
    // outside this program. The token is the verb's shared half: the wide
    // spelling is `copy this file's path` and the tight one `copy the path`.
    "copy",
    "this sheet",
    "quit",
    "wheel",
    "drag a",
    "click a track",
    "click  ▲ ▼",
    "jump the diff to it",
    // B20's row, after the click it shares a region with. The keys cell is the
    // token because it is the half that keeps one spelling at both rungs.
    "drag the diff",
    // The two the README teaches and the sheet most easily omits: its own close
    // control, and the hover mark.
    "click  ✕",
    "just point",
    // The terminal's row, and the only one here the pane does not answer: it
    // gives selection back while the modifier is held.
    "Shift+drag",
];

#[test]
fn no_gesture_token_hides_inside_another() {
    // The gate on the gate.
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

    // And no token may hide inside a drawn row either, which is the half most easily
    // missed.
    let scratch = Scratch::large_diff("sheet-hiding", FILES, 40);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    // One-column rungs only, and both spellings.
    for (w, h) in [(WIDE, WHOLE_TABLE), (43u16, WHOLE_TABLE - 1)] {
        let at = Rect::new(0, 0, w, h);
        let mut app = App::new();
        toggle_at(&mut app, &mut frame, at);
        let (buf, laid) = paint(&mut app, &mut frame, &mut highlighter, &history, at);
        let (_, sheet) = read_sheet(&buf, &laid);
        for row in sheet.lines() {
            let carried: Vec<&&str> = GESTURES.iter().filter(|g| row.contains(**g)).collect();
            assert!(
                carried.len() <= 1,
                "the drawn row {row:?} on a {w}x{h} pane carries {} tokens at once \
                 ({carried:?}), so every page holding it counts a gesture too many",
                carried.len()
            );
        }
    }
}

/// The extent every ladder gate walks: 40 to 144 columns by 1, 6 to 38 rows by 1.
const LADDER_WIDTHS: std::ops::RangeInclusive<u16> = 40..=144;

/// The height half of [`LADDER_WIDTHS`], from below the sheet's floor to above
/// the height at which the tallest rung fits.
const LADDER_HEIGHTS: std::ops::RangeInclusive<u16> = 6..=44;

/// One materialised fixture, painted at many sizes.
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

/// The first height in `heights` at which the sheet draws the rung `marker`
/// names, walked a row at a time and asserted monotone from there on.
fn arrival_height_of(
    paint: &mut impl FnMut(Rect) -> (Buffer, Regions),
    marker: &str,
    heights: std::ops::RangeInclusive<u16>,
    width: u16,
) -> Option<u16> {
    let mut arrival = None;
    for h in heights {
        let (buf, laid) = paint(Rect::new(0, 0, width, h));
        let (_, sheet) = read_sheet(&buf, &laid);
        if sheet.contains(marker) && arrival.is_none() {
            arrival = Some(h);
        }
        if let Some(first) = arrival {
            assert!(
                sheet.contains(marker),
                "the rung arrived at {first} rows and was gone again at {h}, so it \
                 is not monotone in height:\n{sheet}"
            );
        }
    }
    arrival
}

/// How many of [`GESTURES`] the sheet draws, and the sheet's own rows.
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
    // The widening rung's whole claim, at the pane that reported it.
    sweep!("sheet-width", |paint| {
        let short_and_wide = Rect::new(0, 0, 120, 22);
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
        // sits *below* the full one-column rung, which is what makes it
        // additive rather than a relayout.
        let tall = Rect::new(0, 0, 120, 32);
        let (buf, laid) = paint(tall);
        let (count, sheet) = read_sheet(&buf, &laid);
        assert_eq!(
            sheet.lines().count(),
            28,
            "a tall pane stopped drawing the twenty-eight-row one-column sheet:\n{sheet}"
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
    // Walks the ladder rather than sampling it, which
    // `the_sheet_degrades_on_both_axes_and_has_a_floor` does not: it asserts three
    // fixed sizes, and a single fixture passes against an unfixed ladder.
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
    // The additivity claim as a gate, not as an argument.
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
    // The gate that was missing, found by the audit that introduced its defect.
    let mut sheets = 0;
    let mut two_column = 0;
    let mut roomy_seen = 0;

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
        // The title bar's own rule, which nothing looked at: replacing its fill with
        // spaces left `┌ gestures ┐` green everywhere.
        let title = "─ gestures ".chars().count();
        let top: Vec<char> = first.chars().collect();
        assert!(
            top[1..=title].iter().collect::<String>() == "─ gestures ",
            "the sheet's title is not where the frame puts it at {w}x{h}:\n{drawn}"
        );
        // The page counter sits here, between the title and the rule, so the run of `─`
        // does not start at `title + 1`.
        let counter = counter_of(&drawn).unwrap_or_default();
        if !counter.is_empty() {
            let body = counter
                .strip_prefix(' ')
                .and_then(|c| c.strip_suffix(' '))
                .unwrap_or_default();
            let (run, total) = body.split_once(" of ").unwrap_or_default();
            let ordinals: Vec<&str> = run.split('-').collect();
            assert!(
                (1..=2).contains(&ordinals.len())
                    && ordinals.iter().all(|n| n.parse::<usize>().is_ok())
                    && total.parse::<usize>().is_ok(),
                "the sheet's title bar carries {counter:?} at {w}x{h}, which is \
                 not the ` a of N ` or ` a-b of N ` the page counter spells:\n{drawn}"
            );
        }
        assert!(
            top[title + 1 + counter.chars().count()..top.len() - 4]
                .iter()
                .all(|c| *c == '─'),
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
        if roomy {
            roomy_seen += 1;
        }
        let headings: Vec<String> = SECTIONS.iter().map(|l| format!("  {l}")).collect();
        for (n, row) in rows[1..rows.len() - 1].iter().enumerate() {
            // A heading's own fill, which only the title bar's and the bottom border's
            // were checked.
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
        two_column > 0 && roomy_seen > 0 && sheets > two_column + roomy_seen,
        "the sweep saw {sheets} sheets, of which {two_column} were two-column and \
         {roomy_seen} roomy, so it did not cover all three shapes. The roomy arm \
         above asserts that a section heading carries no rule, and an arm no pane \
         in the sweep reaches is an arm that cannot fail"
    );
}

/// The roomy rung's five labels, in the order Mock A draws them.
const SECTIONS: [&str; 5] = ["moving", "files", "view", "mouse", "leaving"];

/// One row of the roomy rung, as the gate expects to read it.
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
fn roomy_shape() -> Vec<RoomyRow> {
    let mut rows = vec![RoomyRow::Air];
    for (label, tokens) in [
        ("moving", &GESTURES[0..3]),
        ("files", &GESTURES[3..7]),
        // Six since B19: `r`, `s`, `a` and `w` join `f` and `m` in `view`, which
        // is the section for the things that change what the body is made of.
        ("view", &GESTURES[7..14]),
        // Nine, the close control, the hover mark and the terminal's modifier included.
        ("mouse", &GESTURES[16..25]),
        ("leaving", &GESTURES[14..16]),
    ] {
        rows.push(RoomyRow::Heading(label));
        rows.extend(tokens.iter().map(|t| RoomyRow::Gesture(t)));
        rows.push(RoomyRow::Air);
    }
    rows
}

#[test]
fn the_roomy_rung_is_the_size_the_ruling_states() {
    // `SPEC.md` §11.1 states 68 by 35, and Mock A drew 76 by 29.
    sweep!("sheet-roomy-size", |paint| {
        let at = ROOMY_PANE;
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
            (68u16, 38u16),
            "the roomy rung is not the size SPEC.md §11.1 states:\n{drawn}"
        );
        assert_eq!(
            count,
            GESTURES.len(),
            "the roomy rung drew fewer than every gesture:\n{drawn}"
        );

        // The wide spelling, which the size alone does not say.
        for wide in [
            "q  Ctrl+C  Ctrl+D",
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
    // 70, and it is not the sheet's own 68.
    let arrival;
    sweep!("sheet-roomy-arrival", |paint| {
        arrival = arrival_of(&mut paint, "moving", 64..=80, 41);
    });
    assert_eq!(
        arrival,
        Some(70),
        "the roomy rung does not arrive where SPEC.md §11.1 says it does"
    );
}

#[test]
fn the_roomy_rung_arrives_at_the_height_the_ruling_states() {
    // The other axis, and the one no gate walked.
    let arrival;
    sweep!("sheet-roomy-height", |paint| {
        arrival = arrival_height_of(&mut paint, "moving", 24..=41, 100);
    });
    assert_eq!(
        arrival,
        Some(41),
        "the roomy rung does not arrive at the pane height a body of thirty-eight \
         rows implies on this fixture"
    );
}

#[test]
fn the_roomy_rung_places_its_cells_where_the_plan_says() {
    // Air is the one thing on this element that a count cannot see.
    sweep!("sheet-roomy-cells", |paint| {
        let at = ROOMY_PANE;
        let (buf, laid) = paint(at);
        let (_, sheet) = read_sheet(&buf, &laid);
        let rows: Vec<Vec<char>> = sheet.lines().map(|r| r.chars().collect()).collect();
        assert_eq!(
            rows.len(),
            38,
            "the roomy rung is not thirty-eight rows tall:\n{sheet}"
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
        let drawn: Vec<&str> = sheet
            .lines()
            .filter_map(|row| {
                SECTIONS
                    .iter()
                    .find(|label| row.starts_with(&format!("│  {label}")))
                    .copied()
            })
            .collect();
        assert_eq!(
            drawn,
            SECTIONS.to_vec(),
            "the roomy rung does not draw all five sections in Mock A's \
             order:\n{sheet}"
        );
    });
}

#[test]
fn the_roomy_rung_is_additive_and_costs_no_pane_a_gesture() {
    // The roomy rung's own claim: a heading costs a row and must never cost a gesture.
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
            // The rung is the wide spelling's, and nothing else could see that.
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
fn the_display_order_is_the_readers_and_the_narrow_floor_keeps_the_unguessable() {
    // The two ends of the display/drop separation, on a screen.
    sweep!("sheet-orders", |paint| {
        let at = Rect::new(0, 0, WIDE, WHOLE_TABLE);
        let (buf, laid) = paint(at);
        let (_, sheet) = read_sheet(&buf, &laid);
        let rows: Vec<&str> = sheet.lines().collect();
        for (n, token) in GESTURES.iter().take(KEYBOARD_ROWS as usize).enumerate() {
            assert!(
                rows[n + 1].contains(token),
                "row {} of the one-column rung does not carry {token:?}, so the \
                 keyboard table is not in the reader's order:\n{sheet}",
                n + 1
            );
        }
    });

    // The ladder's end is the width axis rather than the height one.
    const NARROW: [(u16, &[&str]); 3] = [
        // Thirty columns, the narrowest sheet the ladder draws at all: what is left
        // after `DROP_ORDER` has given up seven, and every one of the three the
        // keep-set names is in it.
        (
            30,
            &[
                "scroll the",
                "follow the newest",
                "churn band",
                "left rail",
                "one file",
                "staged changes",
                "wrap",
                "copy",
                "this sheet",
            ],
        ),
        (
            32,
            &[
                "half a page",
                "first / last",
                "next / prev",
                "jump to",
                "scroll the",
                "follow the newest",
                "churn band",
                "left rail",
                "one file",
                "staged changes",
                "wrap",
                "copy",
                "this sheet",
            ],
        ),
        // Thirty-five: every keyboard row, `q` back among them, and only the
        // mouse group short of the whole table.
        (
            35,
            &[
                "scroll a row",
                "Space  PgDn",
                "half a page",
                "first / last",
                "next / prev",
                "jump to",
                "scroll the",
                "follow the newest",
                "churn band",
                "left rail",
                "one file",
                "staged changes",
                "wrap",
                "copy",
                "this sheet",
                "quit",
            ],
        ),
    ];

    let scratch = Scratch::large_diff("sheet-orders-narrow", FILES, 40);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    for (w, want) in NARROW {
        let at = Rect::new(0, 0, w, 40);
        let seen = reached(&walk_the_pages(&mut frame, &mut highlighter, &history, at));
        let want: std::collections::BTreeSet<&str> = want.iter().copied().collect();
        assert_eq!(
            seen, want,
            "a {w} column pane reaches a different set of gestures than \
             DROP_ORDER states, so the two orders have been conflated again"
        );
    }

    // And below the narrowest rung there is no sheet at all, which is what a box
    // that cannot say how much of the table it is hiding costs.
    let at = Rect::new(0, 0, 29, 40);
    let pages = walk_the_pages(&mut frame, &mut highlighter, &history, at);
    assert!(
        pages.is_empty(),
        "a twenty-nine column pane drew a sheet, which is below the width its own \
         page counter needs"
    );
}

#[test]
fn the_two_column_rung_is_the_size_the_ruling_states() {
    // `SPEC.md` §11.1 states 104 by 16 wide and 71 by 16 tight, and without this gate
    // no test can fail on either.
    let wide = Rect::new(0, 0, 120, 23);
    let tight = Rect::new(0, 0, 80, 22);

    sweep!("sheet-dimensions", |paint| {
        for (at, want, spelling) in [(wide, (104u16, 19u16), "wide"), (tight, (71, 19), "tight")] {
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
    // 73, and neither 78 nor the 80 the ruling first named.
    let arrival;
    sweep!("sheet-arrival", |paint| {
        // Twenty-two rows rather than twenty, B19's `w` and B9's `y` each having
        // added one: the rung grows with the table, so a shorter pane takes it at no
        // width at all and the probe returns `None` at every column of the sweep.
        arrival = arrival_of(&mut paint, "keyboard", 70..=84, 22);
    });
    assert_eq!(
        arrival,
        Some(73),
        "the two-column rung does not arrive where SPEC.md §11.1 says it does"
    );
}

#[test]
fn the_sheet_is_centred_and_clears_the_footer_at_every_rung() {
    // Two claims no gate could fail before.
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

                // `SHEET_KEEP` is the smallest page, not a keep-set.
                let (count, drawn) = read_sheet(&buf, &laid);
                assert!(
                    count >= KEEP,
                    "a {w}x{h} pane drew a page of {count} gestures, below the \
                     {KEEP} `SHEET_KEEP` names as the thinnest page worth \
                     drawing:\n{drawn}"
                );

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

    // Centring itself is pinned at named panes rather than swept, because the body it
    // is centred in is the pane less a footer whose height is its own ladder, and a
    // gate that reconstructed that would be reconstructing the layout.
    sweep!("sheet-origin", |paint| {
        for (w, h, want) in [
            (120u16, 31u16, (32u16, 1u16, 56u16, 28u16)),
            // The roomy rung, at the head of the ladder. A pane this tall takes
            // the nineteen-row sheet at (22, 10, 56, 19) without it, and the row
            // it loses to air it has spare.
            (100, 42, (16, 1, 68, 38)),
            (120, 22, (8, 1, 104, 19)),
            // The tight two-column rung, five columns narrower for the shortened tight
            // mouse verbs and a row taller for each of `r`, `s` and `w`.
            (80, 22, (4, 1, 71, 19)),
            // Odd slack, which the first three above lack on both axes: halving
            // the slack the other way (`div_ceil`) or taking the trailing margin
            // instead of the leading one reproduces every one of them and misses
            // these, which is why this list is read as a set rather than case by
            // case.
            (81, 25, (5, 2, 71, 19)),
            // The whole table in one column reaches this width, so this is the
            // twenty-one-row sheet rather than a dropping rung of thirteen rows.
            (43, 25, (3, 1, 38, 22)),
            // The level probe's own boundary.
            (58, 30, (1, 1, 56, 27)),
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
    // The gap between a keys cell and its verb is spent in two expressions, one summing
    // the sheet's width and one placing the row.
    for (w, h, cols, label, spelling) in [
        (120u16, 23u16, [2usize, 26, 56, 77], 56usize, "wide"),
        // Twenty-two rows rather than twenty, B19's `w` and B9's `y` each having
        // added one: the rung grows with the table, so a shorter pane falls to the
        // one-column rung and the `contains("keyboard")` guard above says which of
        // the two happened.
        (80, 22, [2, 15, 35, 50], 35, "tight"),
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

            // Which group is in which column, and which row is on top.
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

            // Every row of both groups, in order, not merely the ends.
            for (n, token) in GESTURES.iter().enumerate() {
                // The keyboard group's own length rather than a constant that
                // happens to match it: the split is where `GESTURES` stops being
                // keyboard rows.
                let split = KEYBOARD_ROWS as usize;
                let (col, row) = if n < split {
                    (cols[0], 2 + n)
                } else {
                    (cols[2], 2 + (n - split))
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
fn the_height_ladder_pages_rather_than_dropping_and_fills_every_page_it_can() {
    // The worst regression available in this element, and nothing could see it.
    let expected = [
        (8u16, 3usize, 9usize),
        (9, 4, 7),
        (10, 5, 6),
        (11, 6, 5),
        (12, 7, 4),
        (13, 8, 4),
        (14, 9, 3),
        (15, 10, 3),
        (16, 11, 3),
        // Three pages, and it is the table's length rather than the ladder that moves
        // it: twelve rows a page over twenty-four table rows would be two, and the
        // headings the pages step over are what buy a third.
        (17, 12, 3),
        (18, 13, 2),
        (19, 14, 2),
        (20, 15, 2),
        // The flat step, one row later for each keyboard row added: this is the height
        // at which the row the body buys is the mouse group's heading, which costs a
        // row and names no gesture.
        (21, 16, 2),
        (22, 16, 2),
        (23, 17, 2),
        (24, 18, 2),
        (25, 19, 2),
        (26, 20, 2),
        (27, 21, 2),
        // The whole table in one column, at thirty with B9's row.
        (WHOLE_TABLE, GESTURES.len(), 1),
    ];

    let scratch = Scratch::large_diff("sheet-paging", FILES, 40);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    for (h, first_page, pages) in expected {
        let at = Rect::new(0, 0, 50, h);

        let walked = walk_the_pages(&mut frame, &mut highlighter, &history, at);
        let drawn = walked.first().expect("a pane that draws no sheet at all");
        assert_eq!(
            drawn.count, first_page,
            "the first page of a 50 by {h} pane draws {} gestures where the body \
             has room for {first_page}:\n{}",
            drawn.count, drawn.text
        );
        assert_eq!(
            walked.len(),
            pages,
            "a 50 by {h} pane takes {} pages where the body affords {pages}",
            walked.len()
        );
        let seen = reached(&walked);
        assert_eq!(
            seen.len(),
            GESTURES.len(),
            "a 50 by {h} pane reaches {} gestures of {} across its {pages} pages",
            seen.len(),
            GESTURES.len()
        );
    }
}

#[test]
fn the_keys_cell_is_lit_and_the_verb_is_dim() {
    // B12 rules the keys cell lit against a dim verb, and swapping the two weights
    // changes no glyph, so every gate reading `text_of` is blind to it.
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
        (120u16, 22u16, 2u16, 26u16, "two columns"),
        (120, 30, 2, 26, "one column"),
        // The roomy rung's own columns, which are its own: keys five in and verbs
        // thirty-five in, against two and twenty-six at every other rung.
        (ROOMY_PANE.width, ROOMY_PANE.height, 5, 35, "roomy"),
    ] {
        let at = Rect::new(0, 0, w, h);
        let (buf, laid) = paint(&mut app, &mut frame, &mut highlighter, &history, at);
        let sheet = laid.sheet.expect("a pane that draws no sheet");
        // The roomy rung's first gesture row is one lower: air, then a heading,
        // then the row. Non-vacuity below asserts the rung this case is named for.
        let row = sheet.top + if spelling == "roomy" { 3 } else { 2 };
        let (_, drawn) = read_sheet(&buf, &laid);
        assert_eq!(
            drawn.contains("moving"),
            spelling == "roomy",
            "the {w}x{h} case is not the {spelling} rung:\n{drawn}"
        );
        // A cell has to hold something before its colour means anything, and the whole
        // of this gate reads colours at fixed columns.
        for (x, what) in [(keys_at, "keys cell"), (verb_at, "verb")] {
            assert_ne!(
                buf[(sheet.left + x, row)].symbol(),
                " ",
                "column {x} of the {spelling} rung's first row is blank, so the \
                 weight read below is a blank cell's rather than a {what}'s"
            );
        }
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

        // The furniture's own weight, which nothing read.
        let dim = theme
            .chrome_dim
            .fg
            .expect("the dim weight carries a colour");
        let right = sheet.left + sheet.width - 1;
        let bottom = sheet.top + sheet.height - 1;
        // Each carries the glyph it is named for, asserted before its weight for
        // the reason above: `└` and `─` are as unreadable in the wrong colour as
        // in the wrong place, and a blank cell reports the right colour either way.
        for (x, y, glyph, what) in [
            (sheet.left, sheet.top, '┌', "the top-left corner"),
            (sheet.left + 1, sheet.top, RULE, "the title bar's rule"),
            (sheet.left, row, '│', "the left pipe"),
            (right, row, '│', "the right pipe"),
            (sheet.left, bottom, '└', "the bottom-left corner"),
            (sheet.left + 1, bottom, RULE, "the bottom border"),
        ] {
            assert_eq!(
                buf[(x, y)].symbol().chars().next(),
                Some(glyph),
                "{what} of the {spelling} sheet does not hold {glyph:?}, so the \
                 weight read below is some other cell's"
            );
            assert_eq!(
                buf[(x, y)].fg,
                dim,
                "{what} of the {spelling} sheet is not drawn in the chrome's dim \
                 weight, so the frame competes with the table inside it"
            );
        }
    }

    // The roomy rung's five headings, which are plain text rather than rules and so are
    // the one heading shape no rule-reading gate reaches.
    let (buf, laid) = paint(&mut app, &mut frame, &mut highlighter, &history, ROOMY_PANE);
    let sheet = laid.sheet.expect("the roomy pane draws no sheet");
    let dim = theme
        .chrome_dim
        .fg
        .expect("the dim weight carries a colour");
    let (_, drawn) = read_sheet(&buf, &laid);
    let rows: Vec<&str> = drawn.lines().collect();
    let mut headed = 0;
    for (n, row) in rows.iter().enumerate() {
        let Some(label) = SECTIONS
            .iter()
            .find(|l| row.contains(&format!("  {l}")))
            .copied()
        else {
            continue;
        };
        headed += 1;
        let at = (sheet.left + 3, sheet.top + n as u16);
        // The cell has to hold the label before its colour means anything.
        assert_eq!(
            buf[at].symbol(),
            label[..1].to_string(),
            "column 3 of the {label:?} heading row does not hold the label, so \
             the weight read below is a blank cell's:\n{drawn}"
        );
        assert_eq!(
            buf[at].fg, dim,
            "a section heading of the roomy rung is not drawn in the chrome's dim \
             weight, so a label competes with the rows under it:\n{drawn}"
        );
    }
    assert_eq!(
        headed,
        SECTIONS.len(),
        "the roomy pane drew {headed} of {} section headings, so the assertion \
         above skipped some:\n{drawn}",
        SECTIONS.len()
    );

    // The headings are furniture too, and the two-column rung has two of them.
    let at = Rect::new(0, 0, 120, 22);
    let (buf, laid) = paint(&mut app, &mut frame, &mut highlighter, &history, at);
    let sheet = laid.sheet.expect("a pane that draws no sheet");
    let dim = theme
        .chrome_dim
        .fg
        .expect("the dim weight carries a colour");
    for (col, glyph, what) in [
        (2u16, "k", "the keyboard label"),
        (56, "m", "the mouse label"),
    ] {
        assert_eq!(
            buf[(sheet.left + col, sheet.top + 1)].symbol(),
            glyph,
            "{what} does not start at column {col}, so the weight read below is a \
             blank cell's"
        );
        assert_eq!(
            buf[(sheet.left + col, sheet.top + 1)].fg,
            dim,
            "{what} is not drawn in the chrome's dim weight"
        );
    }
}

#[test]
fn the_one_column_rung_places_its_cells_where_the_plan_says() {
    // The rung every reader sees, and the one whose columns are easiest to leave
    // unpinned.
    for (w, h, keys_at, verb_at, first_key, mouse_from) in [
        // `j` rather than `q`: the reader's order starts at `moving` and `q` is the row
        // the ladder gives up first, at the bottom of the table.
        (80u16, WHOLE_TABLE, 2usize, 26usize, 'j', Some(17usize)),
        (120, 31, 2, 26, 'j', Some(17)),
        // A paged rung, so its first row is the table's first: the height ladder splits
        // rows rather than dropping them, and page one starts where the reader's order
        // does.
        (50, 14, 2, 17, 'j', None),
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

            // The mouse group's rows too, not only the keyboard group's.
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
    // Every behavioural gate on this element runs at 80 by 24, which takes the
    // one-column rung.
    let scratch = Scratch::large_diff("sheet-beside-input", FILES, 40);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let at = Rect::new(0, 0, 120, 22);

    let height = body_layout(at, &chrome(&app), FILES, FILES).diff;
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
        Some(Action::CloseSheet),
        "the close control does not dismiss the two-column rung"
    );
}

#[test]
fn the_roomy_rung_swallows_what_lands_on_it() {
    // The same hole one rung up.
    let scratch = Scratch::large_diff("sheet-roomy-input", FILES, 40);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    let height = body_layout(ROOMY_PANE, &chrome(&app), FILES, FILES).diff;
    assert!(
        app.apply(Action::ToggleSheet, &mut frame, height)
            .expect("toggle"),
        "the sheet's toggle asked the shell to quit"
    );

    let (_, laid) = paint(&mut app, &mut frame, &mut highlighter, &history, ROOMY_PANE);
    let sheet = laid.sheet.expect("the roomy pane draws no sheet");
    assert_eq!(
        (sheet.width, sheet.height),
        (68, 38),
        "this gate is not looking at the roomy rung"
    );

    // A row no other rung reaches: the plain rung is twenty-one rows tall and the
    // two-column rung sixteen, so row 25 of this sheet is the roomy rung's alone.
    let deep = sheet.top + 25;
    let inside = sheet.left + 34;
    assert!(
        sheet.covers(inside, deep),
        "{inside},{deep} is not inside the roomy sheet, so the case below proves \
         nothing"
    );
    assert!(
        action_for(&click(inside, deep), laid).is_none(),
        "a click inside the roomy rung fell through to whatever is beneath it"
    );
    assert!(
        action_for(&wheel(inside, deep), laid).is_none(),
        "a wheel inside the roomy rung scrolled a region the sheet is covering"
    );

    // A blank row is still the sheet's. Air is the one part of this rung that
    // draws nothing, and a target derived from what is drawn rather than from the
    // plan would let a click through exactly there.
    let air = sheet.top + 1;
    assert!(
        action_for(&click(inside, air), laid).is_none(),
        "a click on the roomy rung's air fell through, so the pointer is reading \
         ink rather than the plan"
    );

    let close = action_for(&click(sheet.close.0, sheet.close.1), laid);
    assert_eq!(
        close,
        Some(Action::CloseSheet),
        "the close control does not dismiss the roomy rung"
    );
}

#[test]
fn the_floor_is_a_rung_now_and_the_narrowest_sheets_are_the_sizes_the_ruling_states() {
    // Two branches here are easy to write and impossible to fire.
    let floor = "─ gestures ".chars().count() + " 18-18 of 18 ".chars().count() + 6;
    assert_eq!(floor, 30, "the title bar's floor is not what §11.1 states");

    sweep!("sheet-guards", |paint| {
        let mut narrowest = u16::MAX;
        let mut narrowest_beside = u16::MAX;
        let mut narrowest_at_i6 = u16::MAX;
        // Wider than `LADDER_WIDTHS`' floor at the bottom end, because the rungs
        // this gate is about are the ones only a pane below I6's forty reaches.
        for w in 20..=*LADDER_WIDTHS.end() {
            for h in LADDER_HEIGHTS {
                let at = Rect::new(0, 0, w, h);
                let (buf, laid) = paint(at);
                let Some(sheet) = laid.sheet else { continue };
                let (_, drawn) = read_sheet(&buf, &laid);
                narrowest = narrowest.min(sheet.width);
                if w >= *LADDER_WIDTHS.start() {
                    narrowest_at_i6 = narrowest_at_i6.min(sheet.width);
                }
                if drawn.contains("keyboard") {
                    narrowest_beside = narrowest_beside.min(sheet.width);
                }
            }
        }
        assert_eq!(
            narrowest, floor as u16,
            "the narrowest rung the ladder draws is not the {floor} columns \
             §11.1 names, so `sheet_floor` is not binding where the ruling says"
        );
        // The whole table in one column, which is what a pane at I6's forty
        // columns draws and the number the two shortened tight verbs bought.
        assert_eq!(
            narrowest_at_i6, 38,
            "the narrowest sheet a pane of forty columns and up draws is not the \
             38 columns §11.1 names for the whole table at the tight spelling"
        );
        assert_eq!(
            narrowest_beside, 71,
            "the narrowest two-column rung is not the 71 columns §11.1 names"
        );
    });
}

#[test]
fn every_gesture_is_reachable_at_forty_columns_and_up() {
    // B13's own promise as a gate. On any pane that draws a sheet
    // at all, walking `?` from the first page to the close reaches every gesture
    // the pane binds. Not *fits on one screen*: reaches.
    let scratch = Scratch::large_diff("sheet-reachable", FILES, 40);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    let mut panes = 0;
    for w in LADDER_WIDTHS {
        for h in LADDER_HEIGHTS {
            let at = Rect::new(0, 0, w, h);
            let seen = reached(&walk_the_pages(&mut frame, &mut highlighter, &history, at));
            if seen.is_empty() {
                continue;
            }
            panes += 1;
            let missing: Vec<_> = GESTURES.iter().filter(|g| !seen.contains(*g)).collect();
            assert!(
                missing.is_empty(),
                "a {w}x{h} pane draws a sheet and {} of its {} gestures are \
                 unreachable however often `?` is pressed: {missing:?}",
                missing.len(),
                GESTURES.len()
            );
        }
    }
    // The floor its two sibling sweeps have and this one did not.
    assert!(
        panes > 0,
        "no pane in the sweep drew a sheet at all, so this proves nothing"
    );
}

/// One page of a walk: what it drew and where its box was.
struct Page {
    /// How many of [`GESTURES`] this page carries.
    count: usize,
    /// The sheet's own cells, frame included.
    text: String,
    /// The sheet's whole box: left, top, width and height.
    frame: (u16, u16, u16, u16),
}

/// Every page `?` reaches on one pane, in order, before it closes.
fn walk_the_pages(
    frame: &mut Frame<'_>,
    highlighter: &mut Highlighter,
    history: &History,
    at: Rect,
) -> Vec<Page> {
    const BOUND: usize = 64;
    // A fresh shell per pane, and the first version of this was not.
    let mut app = App::new();
    let mut pages = vec![];
    loop {
        toggle_at(&mut app, frame, at);
        let (buf, laid) = paint(&mut app, frame, highlighter, history, at);
        let Some(sheet) = laid.sheet else { break };
        let (count, text) = read_sheet(&buf, &laid);
        pages.push(Page {
            count,
            text,
            frame: (sheet.left, sheet.top, sheet.width, sheet.height),
        });
        assert!(
            pages.len() < BOUND,
            "`?` was pressed {} times at {}x{} and the sheet never closed",
            pages.len(),
            at.width,
            at.height
        );
    }
    pages
}

/// Press `?` `times` times, painting between presses the way the shell does.
fn press_pages(
    app: &mut App,
    frame: &mut Frame<'_>,
    highlighter: &mut Highlighter,
    history: &History,
    at: Rect,
    times: usize,
) {
    for _ in 0..times {
        toggle_at(app, frame, at);
        let _ = paint(app, frame, highlighter, history, at);
    }
}

/// Every gesture the walk reached, across all of its pages.
fn reached(pages: &[Page]) -> std::collections::BTreeSet<&'static str> {
    pages
        .iter()
        .flat_map(|page| GESTURES.iter().copied().filter(|g| page.text.contains(g)))
        .collect()
}

#[test]
fn paging_closes_after_the_last_page_and_never_before() {
    // The input model B13 rules, asserted as state rather than as pixels.
    let scratch = Scratch::large_diff("sheet-paging-state", FILES, 40);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    // A pane of nine pages and a pane of one, so both ends of the ladder are
    // here.
    for (at, pages) in [
        (Rect::new(0, 0, 50, 8), 9usize),
        (Rect::new(0, 0, WIDE, WHOLE_TABLE), 1),
    ] {
        let mut app = App::new();
        let _ = paint(&mut app, &mut frame, &mut highlighter, &history, at);
        assert!(
            chrome(&app).sheet.is_none(),
            "a fresh shell has a sheet up at {}x{}",
            at.width,
            at.height
        );

        for page in 0..pages {
            toggle_at(&mut app, &mut frame, at);
            let (_, laid) = paint(&mut app, &mut frame, &mut highlighter, &history, at);
            assert_eq!(
                chrome(&app).sheet,
                Some(page),
                "`?` did not advance to page {} at {}x{}",
                page + 1,
                at.width,
                at.height
            );
            assert!(
                laid.sheet.is_some(),
                "page {} is in the state and absent from the screen at {}x{}",
                page + 1,
                at.width,
                at.height
            );
        }

        toggle_at(&mut app, &mut frame, at);
        let (buf, laid) = paint(&mut app, &mut frame, &mut highlighter, &history, at);
        assert!(
            chrome(&app).sheet.is_none() && laid.sheet.is_none(),
            "`?` past the last of {pages} pages did not close the sheet at {}x{}",
            at.width,
            at.height
        );
        assert!(
            !text_of(&buf, at).contains(TITLE),
            "the closed sheet is still drawn at {}x{}",
            at.width,
            at.height
        );
    }
}

#[test]
fn a_single_page_sheet_still_toggles_in_one_press() {
    // B13's additivity claim, swept rather than sampled. The ruling's whole
    // defence is that a reader whose pane already showed every gesture presses `?`
    // twice as before.
    let scratch = Scratch::large_diff("sheet-additive", FILES, 40);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    let mut whole = 0;
    for w in LADDER_WIDTHS {
        for h in LADDER_HEIGHTS {
            let at = Rect::new(0, 0, w, h);
            let walked = walk_the_pages(&mut frame, &mut highlighter, &history, at);
            let Some(first) = walked.first() else {
                continue;
            };
            if first.count != GESTURES.len() {
                continue;
            }
            whole += 1;
            assert_eq!(
                walked.len(),
                1,
                "a {w}x{h} pane draws every gesture on its first page and still \
                 takes {} pages:\n{}",
                walked.len(),
                first.text
            );
            assert_eq!(
                counter_of(&first.text),
                None,
                "a {w}x{h} pane draws every gesture and still carries a page \
                 counter:\n{}",
                first.text
            );
        }
    }
    assert!(
        whole > 0,
        "the sweep found no pane drawing the whole table, so it proves nothing"
    );
}

#[test]
fn the_counter_names_what_the_pane_reaches() {
    // The say-so half of B13, and the only thing on this element a reader on a narrow
    // pane can use to tell that gestures are missing.
    let scratch = Scratch::large_diff("sheet-counter", FILES, 40);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    // Seven pages at fifty columns, and two narrow panes whose ordinals must stop
    // below the eighteen the tables hold.
    for (at, want) in [
        (
            Rect::new(0, 0, 50, 8),
            vec![counter("1-3"), counter("4-6"), counter("7-9")],
        ),
        (Rect::new(0, 0, 32, 40), vec![counter("1-13")]),
        (Rect::new(0, 0, 30, 40), vec![counter("1-9")]),
    ] {
        let walked = walk_the_pages(&mut frame, &mut highlighter, &history, at);
        for (n, spelling) in want.iter().enumerate() {
            let page = walked.get(n).unwrap_or_else(|| {
                panic!("a {}x{} pane has no page {}", at.width, at.height, n + 1)
            });
            let drawn = counter_of(&page.text).unwrap_or_default();
            assert_eq!(
                drawn.trim(),
                *spelling,
                "page {} of a {}x{} pane spells its counter {drawn:?}:\n{}",
                n + 1,
                at.width,
                at.height,
                page.text
            );
            // And the counter is not decoration: the run it names is as long as
            // the gestures the page actually drew.
            let run = spelling.split(" of ").next().unwrap_or_default();
            let (first, last) = run.split_once('-').unwrap_or((run, run));
            let span = last.parse::<usize>().expect("an ordinal")
                - first.parse::<usize>().expect("an ordinal")
                + 1;
            assert_eq!(
                span,
                page.count,
                "the counter on page {} of a {}x{} pane names {span} gestures and \
                 the page draws {}:\n{}",
                n + 1,
                at.width,
                at.height,
                page.count,
                page.text
            );
        }
    }
}

#[test]
fn the_box_does_not_resize_between_pages() {
    // A centred box that changed width as the reader pressed `?` would shift
    // left under their eye, and nothing else here can see it: every page is a
    // closed box of the right size *for itself*.
    let scratch = Scratch::large_diff("sheet-box", FILES, 40);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    let mut paged = 0;
    for (w, h) in [(50u16, 8u16), (50, 10), (40, 12), (120, 9), (30, 40)] {
        let at = Rect::new(0, 0, w, h);
        let walked = walk_the_pages(&mut frame, &mut highlighter, &history, at);
        if walked.len() > 1 {
            paged += 1;
        }
        let first = walked
            .first()
            .unwrap_or_else(|| panic!("a {w}x{h} pane draws no sheet"))
            .frame;
        let boxes: Vec<(u16, u16, u16, u16)> = walked.iter().map(|page| page.frame).collect();
        assert!(
            boxes.iter().all(|b| *b == first),
            "the sheet's box moves between pages at {w}x{h}: {boxes:?}"
        );
    }
    assert!(
        paged > 1,
        "fewer than two of the panes in this gate actually paged, so it proves \
         nothing about pages"
    );
}

#[test]
fn a_write_under_a_paged_sheet_does_not_move_the_page() {
    // `a_write_under_the_sheet_does_not_dismiss_it` one field over.
    let scratch = Scratch::large_diff("sheet-page-write", FILES, 40);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    let at = Rect::new(0, 0, 50, 8);
    press_pages(&mut app, &mut frame, &mut highlighter, &history, at, 3);
    let (before, _) = paint(&mut app, &mut frame, &mut highlighter, &history, at);
    assert_eq!(
        chrome(&app).sheet,
        Some(2),
        "three presses is not page three"
    );

    scratch.rewrite_all(FILES, 40, 2);
    frame.advance().expect("advance");
    materialise(&mut frame);

    let (after, laid) = paint(&mut app, &mut frame, &mut highlighter, &history, at);
    assert_eq!(
        chrome(&app).sheet,
        Some(2),
        "a write under the sheet sent the reader back a page"
    );
    let sheet = laid.sheet.expect("a write dismissed the sheet");
    let rect = Rect::new(sheet.left, sheet.top, sheet.width, sheet.height);
    assert_eq!(
        text_of(&after, rect),
        text_of(&before, rect),
        "a write under the sheet redrew a different page"
    );
}

#[test]
fn a_resize_clamps_the_page_rather_than_closing_the_sheet() {
    // The one stale read in this design, exercised where it lands.
    let scratch = Scratch::large_diff("sheet-resize", FILES, 40);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    // Seven pages of three rows.
    let small = Rect::new(0, 0, 50, 8);
    press_pages(&mut app, &mut frame, &mut highlighter, &history, small, 7);
    assert_eq!(
        chrome(&app).sheet,
        Some(6),
        "seven presses is not page seven"
    );

    // Three pages of nine rows, so page seven does not exist and page three is the
    // last.
    let larger = Rect::new(0, 0, 50, 14);
    let (buf, laid) = paint(&mut app, &mut frame, &mut highlighter, &history, larger);
    let sheet = laid
        .sheet
        .expect("a resize past the last page closed a sheet nobody dismissed");
    let (count, drawn) = read_sheet(&buf, &laid);
    assert_eq!(
        counter_of(&drawn).unwrap_or_default().trim(),
        counter("18-25"),
        "the clamped page is not the pane's last one:\n{drawn}"
    );
    assert!(count > 0, "the clamped page draws nothing:\n{drawn}");
    assert!(
        sheet.width > 0 && sheet.height > 0,
        "the clamped page has no box"
    );

    // And the next press closes it, rather than walking four pages that are gone.
    toggle_at(&mut app, &mut frame, larger);
    let (_, laid) = paint(&mut app, &mut frame, &mut highlighter, &history, larger);
    assert!(
        laid.sheet.is_none(),
        "`?` on the clamped last page did not close the sheet"
    );
}

#[test]
fn two_presses_in_one_wake_reach_page_two() {
    // The shell drains actions in a batch and paints once at the end of it (`lib.rs`'s
    // `'awake` loop), so two `?` events that arrive together are applied with no frame
    // between them.
    let scratch = Scratch::large_diff("sheet-batch", FILES, 40);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    // A pane of six pages, painted once with the sheet down, which is the frame a
    // reader is looking at when they reach for `?`.
    let at = Rect::new(0, 0, 50, 8);
    let _ = paint(&mut app, &mut frame, &mut highlighter, &history, at);

    // Both presses inside one batch, no paint between them.
    toggle_at(&mut app, &mut frame, at);
    toggle_at(&mut app, &mut frame, at);
    assert_eq!(
        chrome(&app).sheet,
        Some(1),
        "two `?` events in one wake opened the sheet and closed it again, so a \
         held key or two quick taps show the reader nothing"
    );

    let (buf, laid) = paint(&mut app, &mut frame, &mut highlighter, &history, at);
    assert!(
        laid.sheet.is_some(),
        "the batched second press lost the sheet"
    );
    let (_, sheet) = read_sheet(&buf, &laid);
    assert_eq!(
        counter_of(&sheet).unwrap_or_default().trim(),
        counter("4-6"),
        "the batched second press did not land on page two:\n{sheet}"
    );
}

#[test]
fn the_close_control_closes_from_any_page() {
    // The sheet's only pointer escape, and it advanced.
    let scratch = Scratch::large_diff("sheet-close-any", FILES, 40);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    let at = Rect::new(0, 0, 50, 8);
    press_pages(&mut app, &mut frame, &mut highlighter, &history, at, 3);
    let (_, laid) = paint(&mut app, &mut frame, &mut highlighter, &history, at);
    let sheet = laid.sheet.expect("no sheet published on page three");
    assert_eq!(
        chrome(&app).sheet,
        Some(2),
        "the fixture is not on page three"
    );

    let close = action_for(&click(sheet.close.0, sheet.close.1), laid)
        .expect("the close control published no action");
    let height = body_layout(at, &chrome(&app), FILES, FILES).diff;
    app.apply(close, &mut frame, height).expect("close");
    assert_eq!(
        chrome(&app).sheet,
        None,
        "the close control on page three did not close the sheet"
    );
    let (buf, laid) = paint(&mut app, &mut frame, &mut highlighter, &history, at);
    assert!(
        laid.sheet.is_none() && !text_of(&buf, at).contains(TITLE),
        "the closed sheet is still drawn"
    );
}

#[test]
fn the_counter_is_right_where_a_page_spans_the_mouse_heading() {
    // The one line in `column_lines` that is not a gesture, and the ordinals have to
    // step over it.
    let scratch = Scratch::large_diff("sheet-heading-ordinals", FILES, 40);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    let at = Rect::new(0, 0, 50, 8);
    let walked = walk_the_pages(&mut frame, &mut highlighter, &history, at);
    let spellings: Vec<String> = walked
        .iter()
        .map(|page| counter_of(&page.text).unwrap_or_default().trim().to_owned())
        .collect();
    assert_eq!(
        spellings,
        [
            counter("1-3"),
            counter("4-6"),
            counter("7-9"),
            counter("10-12"),
            counter("13-15"),
            // The heading costs this page a row and no ordinal, so it names two
            // gestures where every page above it names three.
            counter("16-17"),
            counter("18-20"),
            // The tail takes whatever the table's length modulo three leaves, so
            // each added gesture moves it: this is a full page where the row before
            // the modifier's was a short one.
            counter("21-23"),
            counter("24-25"),
        ],
        "the ordinals do not step over the mouse group's heading"
    );
    // And the count on each page agrees with what the counter claims, which is what
    // makes this a claim about the arithmetic rather than about seven strings.
    for (page, spelling) in walked.iter().zip(&spellings) {
        let run = spelling.split(" of ").next().unwrap_or_default();
        let (first, last) = run.split_once('-').unwrap_or((run, run));
        let span = last.parse::<usize>().expect("an ordinal")
            - first.parse::<usize>().expect("an ordinal")
            + 1;
        assert_eq!(
            span, page.count,
            "the counter says {spelling} and the page draws {} gestures:\n{}",
            page.count, page.text
        );
    }
}

#[test]
fn a_resize_clamps_rather_than_wrapping() {
    // `page.min(pages - 1)` and `page % pages` are the same answer for most
    // pairs, so the resize gate cannot tell a clamp from a wrap: it goes from six
    // pages to two at page six, and `5 % 2` and `5.min(1)` are both one.
    let scratch = Scratch::large_diff("sheet-clamp-not-wrap", FILES, 40);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    let six = Rect::new(0, 0, 50, 8);
    press_pages(&mut app, &mut frame, &mut highlighter, &history, six, 5);
    assert_eq!(chrome(&app).sheet, Some(4), "five presses is not page five");

    // Fifty by eleven is five pages. Page six clamps to page five and would wrap
    // to page one, which is what separates a clamp from a wrap: `5 % 5` is zero and
    // `5.min(4)` is four.
    let three = Rect::new(0, 0, 50, 11);
    let (buf, laid) = paint(&mut app, &mut frame, &mut highlighter, &history, three);
    assert!(laid.sheet.is_some(), "the resize closed the sheet");
    let (_, sheet) = read_sheet(&buf, &laid);
    assert_eq!(
        counter_of(&sheet).unwrap_or_default().trim(),
        counter("24-25"),
        "the resize wrapped the page instead of clamping it:\n{sheet}"
    );
    assert_eq!(
        chrome(&app).sheet,
        Some(4),
        "the state kept a page the pane no longer has, so the screen and the state \
         disagree about which page is up"
    );
}

#[test]
fn every_page_is_a_closed_box_including_its_blank_tail() {
    // `the_sheet_is_a_closed_box_at_every_rung` cannot reach this and its own scaffold
    // is why.
    let scratch = Scratch::large_diff("sheet-tail-frame", FILES, 40);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    let mut tails = 0;
    for (w, h) in [(50u16, 8u16), (50, 10), (40, 12)] {
        let at = Rect::new(0, 0, w, h);
        let walked = walk_the_pages(&mut frame, &mut highlighter, &history, at);
        assert!(
            walked.len() > 1,
            "a {w}x{h} pane draws one page, so it has no tail to close"
        );
        for (n, page) in walked.iter().enumerate() {
            let rows: Vec<&str> = page.text.lines().collect();
            let drawn = page
                .text
                .lines()
                .skip(1)
                .take(rows.len().saturating_sub(2))
                .filter(|row| row.chars().nth(1) != Some(' ') || row.contains(RULE))
                .count();
            if n + 1 == walked.len() && drawn < rows.len() - 2 {
                tails += 1;
            }
            for (r, row) in rows.iter().enumerate().take(rows.len() - 1).skip(1) {
                assert!(
                    row.starts_with('│') && row.ends_with('│'),
                    "row {r} of page {} at {w}x{h} is not closed at both edges, so \
                     the blank tail runs out of the frame:\n{}",
                    n + 1,
                    page.text
                );
            }
            assert!(
                rows[0].starts_with('┌') && rows[0].ends_with('┐'),
                "page {} at {w}x{h} has no top rule:\n{}",
                n + 1,
                page.text
            );
            assert!(
                rows[rows.len() - 1].starts_with('└') && rows[rows.len() - 1].ends_with('┘'),
                "page {} at {w}x{h} has no bottom rule:\n{}",
                n + 1,
                page.text
            );
        }
    }
    assert!(
        tails > 0,
        "no pane in this gate drew a last page shorter than its box, so the tail \
         it exists for was never on screen"
    );
}

#[test]
fn a_pane_dragged_below_the_floor_and_back_keeps_its_page() {
    // A pane too narrow for a sheet has zero pages, and the clamp read that as "go to
    // page one".
    let scratch = Scratch::large_diff("sheet-below-floor", FILES, 40);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    let at = Rect::new(0, 0, 50, 8);
    press_pages(&mut app, &mut frame, &mut highlighter, &history, at, 4);
    assert_eq!(chrome(&app).sheet, Some(3), "four presses is not page four");

    // Twenty-eight columns: under the floor, so nothing is drawn and the state
    // stays true, which is §11.1's own ruling and is what `m` does on a pane that
    // cannot carry the band.
    let narrow = Rect::new(0, 0, 28, 8);
    let (buf, laid) = paint(&mut app, &mut frame, &mut highlighter, &history, narrow);
    assert!(
        laid.sheet.is_none() && !text_of(&buf, narrow).contains(TITLE),
        "a twenty-eight column pane drew a sheet"
    );

    let (buf, laid) = paint(&mut app, &mut frame, &mut highlighter, &history, at);
    assert_eq!(
        chrome(&app).sheet,
        Some(3),
        "a pane dragged below the sheet's floor and back moved the reader's page"
    );
    let (_, sheet) = read_sheet(&buf, &laid);
    assert_eq!(
        counter_of(&sheet).unwrap_or_default().trim(),
        counter("10-12"),
        "the pane came back on a different page:\n{sheet}"
    );
}

#[test]
fn the_arrows_are_named_at_the_wide_spelling_and_not_the_tight_one() {
    // The measured trade the arrows took, pinned so it cannot be undone silently.
    let scratch = Scratch::large_diff("sheet-arrow-aliases", FILES, 40);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    // One page, one paint. `walk_the_pages` presses `?` through every page
    // and repaints each, and both panes here draw the cell on their first.
    let mut page_one = |frame: &mut Frame<'_>, at: Rect| {
        let mut app = App::new();
        toggle_at(&mut app, frame, at);
        let (buf, laid) = paint(&mut app, frame, &mut highlighter, &history, at);
        read_sheet(&buf, &laid).1
    };

    // Wide: a pane the whole table fits on in one column at the wide spelling.
    // 80 by 26, because twenty gestures do not fit one column in a
    // twenty-four-row pane: that size takes the two-column rung and its tight
    // spelling, which is not what this gate is about.
    let drawn = page_one(&mut frame, Rect::new(0, 0, WIDE, WHOLE_TABLE));
    assert!(
        drawn.contains("n  →  /  p  ←"),
        "the wide spelling does not name the arrows beside `n` and `p`:\n{drawn}"
    );

    // Tight: the widest pane that still takes the tight spelling, so this is the
    // cell a forty-column reader sees rather than a hypothetical one.
    let drawn = page_one(&mut frame, Rect::new(0, 0, 40, 30));
    assert!(
        drawn.contains("n  /  p"),
        "the tight spelling lost the `n / p` row entirely:\n{drawn}"
    );
    assert!(
        !drawn.contains('→') && !drawn.contains('←'),
        "the tight spelling names the arrows, which takes the keyboard-only rung \
         from 35 columns to 37 and costs panes of 35 and 36 their gestures:\n{drawn}"
    );
}

/// `Esc` closes the sheet when one is up and quits when none is, which is the
/// half a keymap gate cannot see: `input::key_action` answers the same action
/// either way, and
/// which thing is frontmost is `App`'s question.
#[test]
fn escape_puts_the_sheet_away_before_it_puts_the_program_away() {
    let scratch = Scratch::large_diff("sheet-escape", FILES, 40);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();
    let height = 20;

    app.apply(Action::ToggleSheet, &mut frame, height)
        .expect("open the sheet");
    assert!(chrome(&app).sheet.is_some(), "the fixture opened no sheet");

    // With a sheet up, `Esc` takes the sheet and leaves the program running.
    let running = app
        .apply(Action::Escape, &mut frame, height)
        .expect("escape from the sheet");
    assert!(
        running,
        "`Esc` ended the program while the gestures sheet was open"
    );
    assert!(
        chrome(&app).sheet.is_none(),
        "`Esc` left the sheet up, so it dismissed nothing"
    );

    // With none up, it is still the way out: a reader who has learned `Esc`
    // does not have to learn a second key to leave.
    let running = app
        .apply(Action::Escape, &mut frame, height)
        .expect("escape from the pane");
    assert!(!running, "`Esc` no longer leaves the program");
}
