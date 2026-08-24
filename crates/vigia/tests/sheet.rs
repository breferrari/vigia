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
    let mut app = App::past_first_paint();
    let mut highlighter = Highlighter::eager();
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

    // Non-vacuity: the pane has to be drawing washed rows, or a sheet with no wash
    // under it proves nothing about a sheet that covers one.
    let (closed, _) = paint_with(
        &mut app,
        &mut frame,
        &mut highlighter,
        &history,
        area(),
        &washed_theme,
    );
    let washed = (0..TALL)
        .flat_map(|y| (0..WIDE).map(move |x| (x, y)))
        .filter(|&(x, y)| !matches!(closed[(x, y)].style().bg, None | Some(Color::Reset)))
        .count();
    assert!(
        washed > 0,
        "no cell on the pane carries a background, so this fixture cannot show a \
         wash through the sheet"
    );

    toggle(&mut app, &mut frame);
    let (open, laid) = paint_with(
        &mut app,
        &mut frame,
        &mut highlighter,
        &history,
        area(),
        &washed_theme,
    );
    let sheet = laid.sheet.expect("no sheet published");

    for y in sheet.top..sheet.top + sheet.height {
        for x in sheet.left..sheet.left + sheet.width {
            assert!(
                matches!(open[(x, y)].style().bg, None | Some(Color::Reset)),
                "cell {x},{y} inside the sheet kept the background {:?} from what \
                 it covers, so the sheet is a tint rather than a window",
                open[(x, y)].style().bg
            );
        }
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
) -> (char, ratatui::style::Style) {
    let chrome = app.chrome("fixture", Some("main"), None, None, hovered, None);
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
const GESTURES: [&str; 16] = [
    "quit",
    "scroll a row",
    "page",
    "half a page",
    "first / last",
    "next / prev",
    "jump to",
    "scroll the",
    "follow the newest",
    "churn band",
    "this sheet",
    "wheel",
    "drag a",
    "click a track",
    "click  ▲ ▼",
    "click a",
];

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
/// the height at which one column always fits.
const LADDER_HEIGHTS: std::ops::RangeInclusive<u16> = 6..=32;

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

/// How many of [`GESTURES`] the sheet draws, and the sheet's own rows.
///
/// **Counted inside the sheet's rect, never over the pane.** The hint bar spells
/// `q quit`, so a pane-wide count scores `GESTURES[0]` on every frame whether or
/// not the sheet drew that row, and every count-based gate here was one gesture
/// loose in exactly the region they exist to measure: the rungs that drop rows.
/// Measured at 120 by 8 the pane says four and the sheet draws three.
fn read_sheet(buf: &Buffer, laid: &Regions, at: Rect) -> (usize, String) {
    let _ = at;
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
        let (count, sheet) = read_sheet(&buf, &laid, short_and_wide);
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
        let (count, sheet) = read_sheet(&buf, &laid, tall);
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
        let (count, _) = read_sheet(buf, laid, at);
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

    walk_the_ladder("sheet-additive", |w, h, at, buf, laid| {
        let (count, sheet) = read_sheet(buf, laid, at);
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
        assert!(
            last.starts_with('└') && last.ends_with('┘'),
            "the sheet's bottom row is not a closed rule at {w}x{h}:\n{drawn}"
        );
        let span = last.chars().count() - 2;
        assert!(
            last.chars().skip(1).take(span).all(|c| c == '─'),
            "the sheet's bottom rule has a hole in it at {w}x{h}:\n{drawn}"
        );
        for (n, row) in rows[1..rows.len() - 1].iter().enumerate() {
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
            let (_, drawn) = read_sheet(&buf, &laid, at);
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
    let mut arrival = None;
    sweep!("sheet-arrival", |paint| {
        for w in 70u16..=84 {
            let at = Rect::new(0, 0, w, 19);
            let (buf, laid) = paint(at);
            let (_, sheet) = read_sheet(&buf, &laid, at);
            if sheet.contains("keyboard") && arrival.is_none() {
                arrival = Some(w);
            }
            if let Some(first) = arrival {
                assert!(
                    sheet.contains("keyboard"),
                    "the rung arrived at {first} and was gone again at {w}, so it \
                     is not monotone in width:\n{sheet}"
                );
            }
        }
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
    assert!(
        cleared > 0,
        "no pane in the sweep drew a hint bar, so the clearance assertion never ran"
    );

    // Centring itself is pinned at named panes rather than swept, because the body
    // it is centred in is the pane less a footer whose height is its own ladder,
    // and a gate that reconstructed that would be reconstructing the layout. These
    // four were checked by hand against `margins_of` and the plan's own halving.
    sweep!("sheet-origin", |paint| {
        for (w, h, want) in [
            (120u16, 30u16, (32u16, 5u16, 56u16, 19u16)),
            (100, 40, (22, 10, 56, 19)),
            (120, 21, (8, 3, 104, 14)),
            (80, 17, (2, 1, 76, 14)),
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
            let (_, sheet) = read_sheet(&buf, &laid, at);
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
