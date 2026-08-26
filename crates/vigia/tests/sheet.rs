//! The gestures sheet: `?`, the window it opens, and the one thing it must not do.
//!
//! `SPEC.md` §11.2's B12 ruled the keymap out of the footer and over the pane,
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
    Action, App, Chrome, Glyphs, Grabbed, Hovered, Pointing, Regions, Sheet, Theme, action_for,
    body_layout, regions, render,
};
use vigia_core::{Frame, Highlighter, History};

use support::{Scratch, materialise};

const WIDE: u16 = 80;

/// The pane height at which the sheet draws every gesture on one page.
///
/// **Derived rather than chosen, and it moves when the table does.** It was a `26`
/// written at six call sites, which is a threshold two dozen expressions agreed
/// about by hand — the shape `SPEC.md` §7 keeps finding. Adding B17's `a` row in
/// [#313](https://github.com/breferrari/vigia/issues/313) made every one of them
/// wrong at once, and each failed with a different message about a different
/// gesture rather than with "the table got taller".
///
/// **It is the pane height rather than a sheet height**, and the two differ by
/// more than the sheet's own chrome: the sheet is centred over a pane that also
/// carries a header, a footer and its own margins, and which rung the ladder picks
/// depends on the width as well. So this is calibrated rather than computed —
/// `the_whole_table_fits_on_one_page_at_the_calibrated_height` is the gate that
/// says it still means what it says, and it fails loudly if a future row makes
/// this number stale rather than leaving six other gates to fail obscurely.
///
/// **Was 26 with thirteen keyboard gestures**, and every added gesture moves it by
/// one row: B17's `a` ([#313](https://github.com/breferrari/vigia/issues/313)) is
/// what took it to 27.
const WHOLE_TABLE: u16 = 27;

/// Keyboard gestures the sheet's table holds, as a reader counts them on screen.
const KEYBOARD_ROWS: u16 = 14;
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
///
/// **Parameterised rather than open-coded**, which ten of #286's own call sites
/// were: each spelled `apply(ToggleSheet, ..)` against its own rect and dropped
/// the assertion below with it, so a `?` that asked the shell to quit would have
/// been read as a `?` that opened a sheet.
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
///
/// **One parser for three gates**, which read it in two idioms before #286's own
/// simplify pass: the frame gate sliced characters out of the top border and the
/// two new ones matched `" of "` against the whole box, which collides with
/// `jump to that row of the list` and needed a comment saying so. The title's
/// spelling is restated rather than imported for [`TITLE`]'s reason.
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

/// A pane the roomy rung fits on: a room of 68 columns and a body of 31 rows.
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
    // The roomy rung is the one with air in it: six of its twenty-nine interior
    // rows are blank, and a blank row is exactly a row the drawer writes nothing
    // over, so it is the shape most exposed to a background the blank pass failed
    // to clear. The rung that had this gate is the one with the least air.
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

        // **Non-vacuity, counted under the sheet's own rect rather than over the
        // pane.** The claim is about cells the sheet *covers*, so a pane whose
        // washed rows all sit outside it would satisfy a pane-wide count while
        // proving nothing, and the sheet is centred in a body that is mostly
        // context rows. Read off the closed buffer at the rect the open one
        // published, which is why the toggle happens before this rather than
        // after it.
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
    // **A control that never brightened is a glyph a reader has to guess at**
    // ([#211](https://github.com/breferrari/vigia/issues/211)), and B10's ladder had
    // the rungs for it: chrome at rest and `bar_hover` under the pointer.
    //
    // **Two rungs and not the step buttons' three, ruled by
    // [#298](https://github.com/breferrari/vigia/issues/298).** This gate asserted a
    // pressed rung by building a `Chrome` whose `pressed` is the control's own cell,
    // and nothing in the shell ever produces one: the control acts on `Down`, so the
    // sheet is gone before the next paint and no frame can draw it pressed.
    // `nothing_can_press_the_close_control` is the producer-side gate that replaced
    // that assertion, and `SPEC.md` §11.1 now states two.
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
    // **The bottom rung, which is the one of two nothing asserted positively.**
    // Every other assertion here is a comparison, so the resting weight was pinned
    // only by being *different* from the hover: painting the control permanently
    // `bar_active` keeps `at_rest != hovered`, keeps both `weight` equalities and
    // keeps the glyph, so the control could ship looking pressed on every frame,
    // or drawn in `chrome_dim` and reading as part of the frame that
    // [#166](https://github.com/breferrari/vigia/issues/166) rules it is not part
    // of.
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
///
/// **Both numbers, because a caller that wants a share needs the denominator too.**
/// `nothing_can_press_the_close_control` spelled its own `444` while this computed
/// `panes` from the same two arguments, so the two could drift with nothing to say
/// so. Round 3.
///
/// **A helper for three gates rather than the file's usual bespoke loop**, and the
/// difference is that these three are one claim at three call sites: #298's rule is
/// that *nothing* the loop asks `Regions` for may answer for a cell the sheet
/// covers, and it has to be asked of `step_at`, of `grab_at` and of the close
/// control's own cell. Written inline it was the same eight-line preamble three
/// times, and the failure that would matter is one of the three quietly sweeping a
/// different grid from the others.
///
/// The rest of this file keeps its inline loops on purpose: `walk_the_pages` and
/// the paging gates each sweep one axis for one gate, where this is one grid shared
/// by three.
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
    // **Bounded above as well as returned**, because every caller's floor is a
    // `>` and a `>` is blind upwards: a mutation that counted a pane twice, or
    // counted the ones that drew no sheet, makes every floor pass more easily.
    // Both directions matter here since `drew` is the only thing standing between
    // a sweep and a grid it has stopped covering. Named by the mutation round that
    // predicted it survives.
    // **Strictly fewer, not merely no more.** `<=` is satisfied by a count that
    // includes the panes which drew nothing, which is exactly the mutation round 2
    // predicted would survive and did: `drew` moved above the `continue` still
    // fits `<= panes`, and every caller's floor is a `>` that more only helps.
    //
    // **What guarantees a non-drawing pane is the short-and-narrow corner, and it
    // took two wrong answers and a measurement to say so.** The first draft cited
    // B13's thirty-column floor, which cannot fire here because both callers sweep
    // from thirty columns up. Round 3 replaced it with `sheet_plan`'s height floor
    // alone, and that is wrong in the other direction: a pane of eight rows draws a
    // sheet at almost every width.
    //
    // Counted rather than reasoned about, on the fixture both callers use: **11
    // panes of 444 draw none, every one of them at height 8 and at one of the
    // eleven narrowest widths**, and the larger grid finds the same 11 of 3663. So
    // it is the corner where a pane is short **and** narrow at once, which is the
    // case B13 names, and `<` is true with eleven panes of margin rather than one.
    // A grid that raised its minimum height past eight is what would make this fail.
    //
    // **So this is a precondition on the caller's grid, not a property of the
    // helper**, and it is stated because a future sweep is the thing most likely to
    // trip it: a grid of heights thirteen and up would never reach the corner, every
    // pane would draw, and this would fire on a `check` that is perfectly correct.
    // Named by round 4, which found nothing else.
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
    // **The producer half of a rule this suite already had the decider half of.**
    // `SPEC.md` §11.1 rules that a gesture landing on the sheet does nothing at all,
    // because *"falling through would let a click seek a scrollbar the reader cannot
    // see and a wheel scroll a diff the sheet is covering, which is the one way
    // something that moves no content could still move content"*. Three gates hold
    // that already and all three ask `action_for`.
    //
    // **The loop does not go through `action_for` to arm a hold.** It reads
    // `Regions::step_at` directly, and that had no sheet guard, so a press on a
    // covered step button armed a `Held`: `Held::fire` then applied `Scroll` every
    // `STEP_REPEAT` to a region under the sheet, for as long as the button was down.
    // The first step was *not* applied, because `action_for` correctly refused it,
    // which also made the arming site's own comment false: it says the hold is
    // *"armed from the same press that performs the first step"* and here no step
    // was performed at all.
    //
    // **The grid is the one the defect was measured over**, rather than the panes it
    // happened to land on: 30 to 140 columns against 8 to 40 rows found **85**
    // covered cells answering a step, all at widths 30, 32, 35 and 38, which are the
    // panes narrow enough for a centred sheet to reach a bar's own column. Those
    // four are deliberately **not** hard-coded here. A rung change moves which panes
    // cover a bar, and a fixed list would then sweep four panes that prove nothing
    // while reporting green, which is `SPEC.md` §7's own shape. The `guarded`
    // counter below is what stops that instead.
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
                // **Both halves, because the defect is that they disagreed.**
                // Asserting only `step_at` would pass on a build where the sheet
                // stopped covering a bar at all, and asserting only `action_for`
                // is what the three existing gates already do.
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
    // **The assertion that makes the loop above mean something.** Every cell in it
    // passes trivially on a build where no sheet ever reaches a bar, and that is not
    // a hypothetical: it is what a rung change does. This counts the cells the guard
    // actually took an answer away from, so a sweep that has stopped covering the
    // case reddens here rather than going quietly green.
    //
    // **A floor rather than the measured 85**, and the difference is deliberate. The
    // exact count is a fact about the sheet's box, which #297 and #288 both move, so
    // pinning it would make this gate fail for a reason that has nothing to do with
    // what it is about. What must not change is that the case is *reached*.
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
    // **Defence rather than behaviour, and it is written down as such because it
    // cannot be made to go red.** `Painter::sheet` drew `Theme::bar_active` when
    // `Chrome::pressed` was the control's own cell, and
    // [#298](https://github.com/breferrari/vigia/issues/298) found nothing produces
    // one. Two independent reasons, and both are asserted here rather than one being
    // trusted:
    //
    // - `Chrome::pressed` is `Held::at`, `Held` is armed from `Regions::step_at`,
    //   and the same sweep that found 85 covered cells answering a step found the
    //   close control's own cell among them **zero** times over 30 to 140 columns by
    //   8 to 40 rows. It is not a bar's column on any pane that draws a sheet.
    // - Since `a_press_under_the_sheet_arms_no_step`'s guard, no cell of the sheet
    //   answers at all, so the first reason is now a consequence of the rule rather
    //   than a fact about geometry. That is the direction that matters: it was true
    //   by coincidence and is true by construction.
    //
    // The drawn ladder is `the_close_control_brightens_under_the_pointer`'s; this is
    // the half no drawing test can reach, which is the producer-versus-decider split
    // #298 names.
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

    // **Proportional to the grid rather than a round number.** 111 widths against
    // four heights is 444 panes, and the ones that draw no sheet are the narrow and
    // short corner B13 rules out, so the great majority draw one. A floor of 100 was
    // the first spelling here and it left a silent range wide enough to hide a
    // regression that cut coverage to a quarter, which is the shape the sibling
    // sweep's own `guarded` counter exists to refuse. Three quarters is a bound the
    // ladder has room to move under without tripping.
    //
    // **Counted by `over_sheets` rather than here**, which is round 2's own
    // correction: a second counter incremented once per `check` call is equal to
    // `drew` by construction, so the assertion comparing them could not fail, and
    // deleting it left `drew` unread and the lint job red where a plain `cargo
    // test` stayed green.
    //
    // **And the denominator comes from the same call**, which is round 3's: this
    // spelled `444` while `over_sheets` computed the grid from the arguments it was
    // handed, so changing the sweep here would have left the constant stale with
    // nothing to catch it.
    assert!(
        drew * 4 > panes * 3,
        "only {drew} of {panes} panes drew a sheet, so this sweep has stopped \
         covering the ladder rather than proving anything about it"
    );
}

#[test]
fn a_press_on_a_track_under_the_sheet_grabs_nothing() {
    // **The sibling call site, and the one the first draft of #298 missed.** The
    // loop reaches into `Regions` for geometry in exactly two places, and guarding
    // only the first left the second holding the identical shape: `Regions::grab_at`
    // is asked on every left press, outside `action_for`, and had no sheet in it.
    //
    // **It is the worse of the two.** A hold repeats a bounded step every
    // `STEP_REPEAT`; a grab hands the whole gesture to `drag_action`, which ignores
    // the column by design so that a reader pulling a one-column bar does not lose
    // it, and the next motion therefore relocates a region the sheet is covering to
    // wherever the pointer happens to be. The sheet is centred on both axes, so at
    // the same narrow widths the step buttons were reachable at it covers the track
    // rows between them, which are more cells than the two the buttons occupy.
    //
    // Found by this pass's own `/simplify` round rather than by the issue, which
    // reported the drawn weight and not either producer.
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
///
/// The two spellings of an unset background compare unequal as `Style`s, and this
/// suite cares about which rung of B10's ladder a cell is on rather than about how
/// the drawer got there.
fn weight(style: ratatui::style::Style) -> (Option<Color>, ratatui::style::Modifier) {
    (style.fg, style.add_modifier)
}

/// The close control's glyph and style, with `hovered` handed to the chrome.
///
/// **No `pressed` parameter since [#298](https://github.com/breferrari/vigia/issues/298)**,
/// which ruled the control down to two rungs: it took one, every caller passed
/// `None` once the third rung's assertion went, and a parameter only ever given one
/// value is a knob that reads as a variable.
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
    // the layout rather than in the painter, which is #158's correction inherited.
    let scratch = Scratch::large_diff("sheet-ladder", FILES, 40);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    // **80 by 26 rather than the default pane since
    // [#288](https://github.com/breferrari/vigia/issues/288)**, and the failure it
    // caused is worth naming: at twenty-four rows the pane now takes the
    // two-column rung, whose *tight* spelling draws `q  Esc` where this gate looks
    // for `Ctrl+C`. Nothing had dropped a rung; the pane had changed which one it
    // was on.
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

    // A short pane keeps the keyboard group and loses the mouse group, which is
    // the height axis on its own. **Sixteen rows is still below #220's widening
    // rung** and the case survived it unchanged: the pane's footer is two rows
    // here, so the body is thirteen and the two-column rung needs sixteen. Three
    // rows taller and the mouse group comes back beside the keyboard group
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
    // **The other half of the table, and its own gate rather than a second loop
    // inside the keyboard one.** They fail for different reasons and are found by
    // different means: the keyboard half is a sweep of `action_for` and reddens
    // when a *binding* has no row, this one is `mouse_phrases`' exhaustive matches
    // and reddens when a *variant* has no phrase. One test covering both would
    // report either as the same failure.
    //
    // #288's two omissions were both on this side, and neither was a key.
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
///
/// **A range names its members, and finding that out is what the derived sweep
/// bought on its first run.** The digits are drawn `1  to  6`, one cell for six
/// bindings, so a check that only compared whole cells reported `2` as unfindable.
/// The list this gate replaced spelled `"1"` and `"6"` and never asked about the
/// four between them: it agreed with the sheet because both were written by the
/// same hand, which is precisely the failure #288 is about, one layer in from the
/// one it was filed for.
///
/// Only the shape the sheet actually draws, `a  to  b` over single characters.
/// Anything wider would be this gate inventing a spelling the table does not use.
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
///
/// **The drift path [#288](https://github.com/breferrari/vigia/issues/288) was
/// actually filed for, and the one nothing gated.** That row's title is *the sheet
/// omits a gesture the README teaches*: `README.md` carried `just point` and
/// `MOUSE` did not, so the sheet omitted it, and no test compared the two. The
/// gates beside this one check the sheet against the **keymap**, which is a
/// different pair and would never have caught it.
///
/// **Read from the file rather than restated**, which is the one place in this
/// suite where that is right and the rest of it is not. `TITLE`'s rule is that a
/// constant shared with the renderer makes a gate agree by construction; here the
/// renderer is not the other side. The other side is a document a human edits, and
/// the whole hazard is that the two say different things, so a restated copy would
/// be a third thing to keep in step.
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
    // **The comparison #288's own title names, and the one no gate made.** The
    // sheet is checked against the keymap by the two gates above; this checks it
    // against the **README**, which is where the reported omission lived: the
    // README taught `just point` and the sheet did not draw it, and both gates
    // beside this one would have stayed green forever.
    //
    // **Token by token rather than cell by cell**, because the two spell a set
    // differently on purpose: the README writes `j` `k` `↑` `↓` as four ticked
    // spans in one cell and the sheet draws `j  k  ↓  ↑` in one, and neither is
    // wrong. What must hold is that every key or word the README teaches is
    // findable on the sheet, which is what a reader who read one and opened the
    // other is entitled to.
    let scratch = Scratch::large_diff("sheet-readme", FILES, 40);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    // **Both one-column rungs, because the README writes the tight spelling.** It
    // says `drag a bar` and `click a file`, which are what this table draws at the
    // tight rung; the wide rung says `drag a scrollbar` and `click a listed file`.
    // Reading only the wide one made `bar` match inside `scrollbar`, which passed
    // by substring luck rather than because the sheet named the gesture. A cell has
    // to land on one row of one rung, and both are legitimate spellings of it.
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
        // **Every token of a cell on ONE drawn row, not each token anywhere on the
        // sheet.** The first spelling of this gate asked the second question and
        // both audit agents caught it independently: `q` and `point` each appear
        // somewhere, so a README cell reading `q point` would have passed while the
        // sheet named no such gesture. Checking the whole sheet text for a token is
        // nearly vacuous for a common word, and this row exists to close a drift
        // class rather than to add a check that cannot see one.
        let wanted: Vec<&str> = cell
            .split_whitespace()
            // `to` joins the digits' range on both sides and is not a gesture.
            .filter(|t| *t != "to")
            .collect();
        assert!(
            !wanted.is_empty(),
            "README.md has a gesture cell {cell:?} with nothing in it to look for"
        );
        // **`names` alone, with no `contains` fallback.** The fallback was round 1's
        // and round 2 found what it cost: a single-character token like `f`, `m`,
        // `r` or `s` is a substring of ordinary verb prose, so `f` matched
        // `half a page`, and deleting the whole `f` row from `KEYBOARD` left this
        // gate green. Those are the very keys `SPEC.md` records as having shipped
        // uncovered. `names` compares whitespace-delimited cells and understands
        // the digits' `1  to  6` range, which is the only spelling that is not one
        // cell, so the fallback bought nothing once both rungs are read.
        assert!(
            rows.iter().any(|row| wanted.iter().all(|t| names(row, t))),
            "README.md teaches {wanted:?} (in {cell:?}) as one gesture and no single \
             row of the sheet names all of it, so a reader who read one and opened \
             the other is missing a gesture:\n{drawn}"
        );
    }
}

/// One value of every [`Action`] variant, for the two gates that walk them.
///
/// **Written once, and the reason is this diff's own subject.** It was typed out
/// twice, in `mouse_phrases` and in `the_action_table_covers_every_variant`, which
/// is exactly the failure [#288](https://github.com/breferrari/vigia/issues/288)
/// exists to remove: two hand-written lists that agree because one hand wrote both,
/// and drift the moment a variant is added to one. Caught by this row's own
/// `/simplify` round, in the code written to fix it.
///
/// **The array is not what makes the classification safe and never was.**
/// [`reach_of`]'s `match` is, because it is exhaustive: a new variant fails to
/// compile there whatever this holds. What a shared const buys is that the two
/// callers cannot come to disagree about which variants exist.
///
/// The payloads are arbitrary. Both callers compare discriminants or ask
/// [`reach_of`], and neither reads a value.
const ALL_ACTIONS: [Action; 19] = [
    Action::Quit,
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
    Action::ToggleSheet,
    Action::CloseSheet,
    Action::ListTo(0),
    Action::ListRow(0),
    Action::DiffTo(0),
    Action::Redraw,
];

/// Where a gesture can be asked for, as the reason a variant is or is not on the
/// sheet's keyboard half.
///
/// **The compiler is the reminder, which is what
/// [#288](https://github.com/breferrari/vigia/issues/288) is for.** The gate this
/// replaced held a hand-written list of tokens, so a new binding was covered only
/// if somebody remembered to add it, and four did not get remembered: `r`
/// ([#295](https://github.com/breferrari/vigia/issues/295)) shipped a whole
/// release uncovered, the arrows ([#296](https://github.com/breferrari/vigia/issues/296))
/// a day, and `s` ([#297](https://github.com/breferrari/vigia/issues/297)) until that
/// row's own audit. A fifth and sixth fell through the mouse list beside it.
///
/// Matching exhaustively on [`Action`] means a variant added to the map **cannot
/// compile** until somebody says where it is reachable from. That is the property
/// a list cannot have at any length.
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
///
/// Written as a `match` on a value rather than a table so it is **exhaustive**: the
/// arms below are the whole of `Action`, and adding a variant reddens the build.
fn reach_of(action: &Action) -> Reach {
    match action {
        // The way out, and `q`, `Esc`, `Ctrl+C`, `Ctrl+D` are all keys.
        Action::Quit => Reach::Keyboard,
        // `a`, and the sheet's own row for it. No mouse gesture reaches it.
        Action::ToggleStaged => Reach::Keyboard,
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
        // **The close control, and the row #288 found named nowhere at all.** `?`
        // means *the sheet* and advances; this means *close* and is the pointer's
        // only escape, which is why it is its own variant since
        // [#286](https://github.com/breferrari/vigia/issues/286).
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
///
/// **Wider than the map on purpose.** The point is to find what `action_for`
/// answers to, so the space has to contain keys it refuses as well as keys it
/// binds; a space narrowed to the bindings would be the hand-written list again
/// wearing a sweep's clothes. `the_action_table_covers_every_variant` is what
/// fails if a binding ever lands outside it.
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
///
/// **A rendering rule, not a roster.** It answers for every key rather than for the
/// bound ones, so it is not the list this gate exists to delete: which keys are
/// bound comes from [`bound_keys`]' sweep, and this only says how to write one down.
///
/// Restated rather than imported for [`TITLE`]'s reason.
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
        // **Unreachable today, and carried rather than removed.** `bound_keys` only
        // spells events `action_for` already bound, and the map binds none of the
        // codes that would land here: `Enter`, `Backspace`, `Tab`, `Delete`,
        // `Insert` and `F1` to `F12`. `KeyCode` is an external and effectively open
        // enum, so a total match is the honest shape, and the `Debug` spelling
        // happens to match the arms above for the codes most likely to be bound
        // next. Confirmed unreachable by this row's `/simplify` round rather than
        // assumed.
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
///
/// **The set is derived**, which is the whole point: it is whatever the shell
/// answers to today, so a key added to the map is covered here without this file
/// being touched.
///
/// **A modifier that changes nothing is not a gesture**, and this rule was written
/// by the sweep rather than for it. Two cases turned up on its first runs and both
/// are the same shape: the terminal sends `Char('J')` for a shifted `j` and the map
/// reads the character, so `Shift+j` resolves exactly as `J` already does; and
/// `key_action` matches `KeyCode::Left` without consulting modifiers, so `Shift+←`
/// steps a file exactly as `←` does. Neither is a second gesture and the sheet must
/// not be made to name one twice.
///
/// So a modified event is kept only when it means something **different** from the
/// same key unmodified. `Shift+↑` survives, because it scrolls the list where `↑`
/// scrolls the diff; `Ctrl+D` survives, because it quits where `d` pages. That is
/// the distinction the sheet itself draws, derived rather than assumed.
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
///
/// **Compiler-forced rather than derived, and the difference is stated rather than
/// glossed.** The force is two exhaustive `match`es with no wildcard between them,
/// [`reach_of`]'s and this function's, so a new [`Action`] variant fails to build
/// until somebody both classifies it and says what teaches it. What [`ALL_ACTIONS`]
/// adds is only that the two callers walk the same set; it is data and was never
/// the guarantee. Round 1 of this row's audit found a `_ => panic!()` arm here that
/// made the second half a runtime failure, and made it one only for a variant
/// somebody had already added to that array. A key sweep can enumerate the keyboard because `action_for` answers
/// per key event. Nothing does that for the mouse: *wheel*, *drag a bar* and *click
/// a track* are event **shapes**, not enum variants, so there is no set to walk.
///
/// What the exhaustive matches below buy instead is that a new [`Action`],
/// [`Hovered`] or [`Grabbed`] variant **cannot fail to compile** until somebody
/// names the phrase that teaches it, or records that it has none. That is strictly
/// stronger than the hand-written list this replaced, which could be and was
/// forgotten twice, and strictly weaker than derivation. Saying so is the point: a
/// gate whose coverage is overstated is the shape #288 is about.
///
/// **And *cannot be forgotten* is what this said until round 2, which is one claim
/// too many.** Compiling is forced; being *exercised* is not. [`ALL_ACTIONS`] is a
/// hand-maintained array, so a variant can be classified in [`reach_of`], given an
/// arm here, and still reach neither gate if nobody adds it there. That is one
/// unenforced step, named rather than papered over.
///
/// **Round 2 claimed a length pin made missing it loud and round 3 showed it did
/// not**: comparing the array's length against a literal compares two things the
/// same edit would touch. What `the_action_table_covers_every_variant` checks now
/// is the array against the **sweep**, which is derived, so a variant bound to a
/// key cannot be left out of it. A variant reachable only by pointer still can be,
/// and that is the residue.
fn mouse_phrases() -> Vec<&'static str> {
    let mut phrases: Vec<&'static str> = Vec::new();

    // The `Action` half, from the same exhaustive table the keyboard half reads.
    for action in ALL_ACTIONS {
        if !matches!(reach_of(&action), Reach::Mouse | Reach::Both) {
            continue;
        }
        // **Exhaustive, with no wildcard.** The keyboard-only arms are listed and
        // return nothing rather than being swept up, because that is what makes the
        // match exhaustive: a new variant has to be put somewhere by hand. See this
        // function's docblock for what a `_` arm cost when it was here.
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
            | Action::ToggleSingle
            | Action::ToggleSheet
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
    // **What makes the sweep's coverage checkable.** `bound_keys` walks a candidate
    // space, and a space is only as good as what it contains: a binding on a
    // `KeyCode` the sweep never tries is invisible to it, and the failure looks
    // exactly like a key that is correctly absent.
    //
    // So the two are compared. Every `Action` that `reach_of` calls keyboard
    // reachable must actually have been produced by the sweep, and a mismatch means
    // either the space is too narrow or the table is wrong. Both want fixing and
    // neither is silent.
    let produced: Vec<Action> = bound_keys()
        .into_iter()
        .filter_map(|(event, _)| action_for(&Event::Key(event), Regions::default()))
        .collect();

    // **Every action the sweep found must be in the array**, which is the half of
    // `ALL_ACTIONS`' upkeep that can be checked against something other than
    // itself. `produced` is derived from `action_for`, so a variant bound to a key
    // and left out of the array reddens here.
    //
    // **Round 2 put a length pin here instead and round 3 showed it proved
    // nothing**: `ALL_ACTIONS.len() == 18` compares a hand-maintained array against
    // a hand-maintained literal, both of which the same edit would have to touch,
    // so it passes in exactly the scenario its own comment described. Deleted
    // rather than kept as reassurance.
    //
    // **What remains unenforced is narrower and is stated rather than papered
    // over**: a variant reachable only by *pointer*, classified and given a phrase
    // but never added to the array, still reaches no gate. No sweep produces it,
    // because the mouse side has none, which is `mouse_phrases`' own recorded
    // limit.
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
    // **The gate that fails when somebody adds a key and forgets the sheet**,
    // which is the whole reason the sheet is worth having: it is now where the
    // keymap is written down, so a binding missing from it is a binding nobody can
    // find. Spelled as the tokens a reader would look for rather than as key
    // codes, because tokens are what the sheet draws.
    // **80 by 26 rather than the default pane, and the two rows are why**
    // ([#288](https://github.com/breferrari/vigia/issues/288)). This ran at 80 by 24
    // until the mouse group grew to seven: twenty gestures no longer fit one column
    // in that body, so the pane takes the two-column rung, and this gate needs a
    // rung that is one column *and* draws no section headings. 80 by 26 is the
    // smallest that is, measured over the widths and heights around it, and it
    // draws the whole table on one page at 56 by 23.
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

    // **Inside the sheet's own rect, and on a rung that draws no headings.**
    // Both halves are load-bearing and neither was here. Read over the whole
    // pane, `q`, `f` and `?` score on the hint bar whether or not the sheet drew
    // them, which is `read_sheet`'s own recorded lesson one gate over. And these
    // are bare one-character tokens, so on the roomy rung the headings `moving`,
    // `files` and `view` alone satisfy `m`, `f`, `g` and `n`:
    // `render.rs`'s `no_section_label_hides_inside_a_cell_or_another_label` says
    // in prose that this gate must stay on a headingless rung, and prose is not
    // an instrument. [#286](https://github.com/breferrari/vigia/issues/286) moves
    // the floor next.
    let (_, drawn) = read_sheet(&buf, &laid);
    assert!(
        !drawn.contains("moving"),
        "this gate searches for bare one-character keys and the pane drew section \
         headings, so `m`, `f`, `g` and `n` are satisfied by `moving`, `files` \
         and `view` rather than by the rows:\n{drawn}"
    );

    // **The keys column of the gesture rows alone, because a bare token finds
    // anything.** Searching the sheet's whole text, `u` is satisfied by `jump to
    // that row of the list`, so `d  u` could become `d  y` (a key the map does not
    // bind, taught to the reader as one it does) with every gate in the suite
    // green: the verb is untouched, so the width, the row count, the frame and
    // every one of `GESTURES` stays exactly as it was.
    //
    // **The furniture has to go too, and leaving it in is the first thing that
    // went wrong here.** The title row carries `gestures` from column 3 and the
    // group heading carries `mouse` from column 2: between them they hold `u`,
    // `m`, `e`, `s` and `g`, so a keys column that included them was still
    // satisfied by the frame. Rule glyphs are what tells furniture from a row, and
    // no keys cell holds one.
    //
    // **And the tokens are matched whole, which is the third thing that went
    // wrong.** With `contains` over the joined column, a one-character token is
    // satisfied by any other row that happens to spell it: `g` by `PgDn` and by
    // `drag a scrollbar`, `d` by `listed`, `n` by `End`, `k` by `click`. So five
    // of the keys this gate lists could be changed in `KEYBOARD` with the whole
    // suite green. Every token here is whitespace-delimited in the wide spelling,
    // and no mouse keys cell yields one, so equality is both available and exact.
    //
    // The keys field of the one-column rung starts at column 2 and is twenty-two
    // wide, which `the_one_column_rung_places_its_cells_where_the_plan_says` pins.
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

    // **Derived from `action_for`, not listed here.** Every key the shell binds is
    // whatever the sweep below finds, so a binding added to the map is covered by
    // this gate with no edit to it. That is the whole of
    // [#288](https://github.com/breferrari/vigia/issues/288)'s case: the list this
    // replaced was a second hand-maintained copy of the table it checked, so it
    // failed only when somebody added a key, forgot the sheet, **and** remembered
    // the test. Four keys had already fallen through it, `r` for a whole release.
    for (event, token) in bound_keys() {
        assert!(
            names(&keys, &token),
            "`action_for` binds {event:?}, which the sheet's keys column does not \
             name, so that gesture is unfindable:\n{drawn}"
        );
    }

    // **And every keyboard verb in full, which nothing checked until #297's
    // mutation round.** The keys above say a gesture is *findable*; the verb is
    // what says what it does, and `GESTURES` matches only the prefix both
    // spellings share, so the wide rung's own wording was unchecked at every
    // rung. Shortening `s`'s wide verb from `one file, or the whole diff` to
    // `one file` moved no width, lost no gesture, kept every token, and left the
    // whole file green.
    //
    // This pane takes the wide one-column rung, so every row here draws its wide
    // spelling. Restated rather than imported for [`TITLE`]'s reason: a gate
    // reading `KEYBOARD` would agree with it by construction, and what this
    // exists to catch is the table changing without anybody meaning it to.
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
/// is last of the thirteen keyboard entries because the sheet draws it last, and
/// the ladder gives it up first. Several gates walk this against drawn rows, so
/// the order is load-bearing rather than decorative. The count is not spelled
/// out, for the reason [`LADDER_WIDTHS`]' own docblock gives.
const GESTURES: [&str; 21] = [
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
    // B17's row ([#313](https://github.com/breferrari/vigia/issues/313)), between
    // `s` and `?` because that is where the reader's own order puts it.
    "staged changes",
    "this sheet",
    "quit",
    "wheel",
    "drag a",
    "click a track",
    "click  ▲ ▼",
    "jump the diff to it",
    // The two [#288](https://github.com/breferrari/vigia/issues/288) added: the
    // sheet's own close control, and the hover mark the README already taught.
    "click  ✕",
    "just point",
];

#[test]
fn no_gesture_token_hides_inside_another() {
    // **The gate on the gate.** [`GESTURES`] is matched with `contains`, so an
    // entry that is a substring of another scores whenever the longer one draws,
    // and the height ladder gives rows up in `DROP_ORDER`, which is exactly where
    // the two meet: `page` sat inside `half a page`, and `Space  PgDn` is given up
    // one rung before `d  u`. (It read "drops from the top" until
    // [#285](https://github.com/breferrari/vigia/issues/285) separated the drop
    // order from the reader's; the hazard is the same and the direction is no
    // longer a direction through the table.)
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

    // **And no token may hide inside a drawn row either, which is the half this
    // gate was missing** ([#288](https://github.com/breferrari/vigia/issues/288)).
    // The loop above compares tokens against tokens, so it sees only a collision
    // between two entries of this list. The count is taken against the **drawn
    // sheet**, so a token hiding inside any row the renderer writes scores the same
    // way, whether or not that row is itself a token.
    //
    // It would have fired on this row's own first draft: the close control's verb
    // was spelled `close this sheet`, and `this sheet` is already the token for
    // `?`, so every page carrying the close row counted one gesture too many. What
    // noticed was `the_counter_is_right_where_a_page_spans_the_mouse_heading`,
    // a counter disagreeing with its own page, which is a long way from the cause.
    //
    // **Against the drawn sheet rather than a second restated table**, because the
    // hazard is exactly that a restated token and a *drawn* cell collide: two
    // restated lists were written by the same hand and would agree.
    let scratch = Scratch::large_diff("sheet-hiding", FILES, 40);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    // **One-column rungs only, and both spellings.** A row of the *two*-column rung
    // carries a keyboard gesture and a mouse gesture side by side, so two tokens on
    // one row is correct there and says nothing about hiding. Where a row is one
    // gesture, a second token on it can only be a collision. 80 by 26 draws the
    // wide one column and 43 by 25 the tight one, so both spellings are read.
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
///
/// **One place, because several gates read it.** They assert different things
/// about the same grid, and bounds that drifted between them would leave a rung
/// asserted monotone at a width no other gate had looked at. The count is
/// deliberately not spelled out: it read "three gates" while seven were reading
/// it, which is what a number in a comment does when the thing it counts is free
/// to grow.
///
/// The floor is at I6's forty columns and the ceiling is above every width the
/// two-column rung arrives at, so the sweep contains both ends of the ladder
/// rather than a slice of its middle.
const LADDER_WIDTHS: std::ops::RangeInclusive<u16> = 40..=144;

/// The height half of [`LADDER_WIDTHS`], from below the sheet's floor to above
/// the height at which the tallest rung fits.
///
/// **Raised from 32 by [#285](https://github.com/breferrari/vigia/issues/285)**,
/// and the old ceiling is why: the roomy rung needs a body of thirty-one rows,
/// which this fixture reaches at a pane of thirty-four, so a sweep stopping at
/// thirty-two would have covered the new rung at no height at all and called it
/// swept.
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
/// The scaffold the gates below shared by copy until #220's own audit pointed at
/// it. `name` is the fixture's, so each gate still gets an independent repository
/// and they keep running in parallel. The count is deliberately not spelled out:
/// it read "three" while four were calling it, which is what [`LADDER_WIDTHS`]'s
/// own docblock thirty lines up warns about.
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

/// The first height in `heights` at which the sheet draws the rung `marker`
/// names, walked a row at a time and asserted monotone from there on.
///
/// **[`arrival_of`] transposed, and it is not decoration.** The width boundary is
/// walked because #220's ruling shipped 80 for a rung arriving at 78. The height
/// boundary is the same shape and worse: a rung's height is spent against the
/// *body*, which is the pane less the header and less a footer whose own height
/// is a ladder in the **width**, so the pane height a rung arrives at is not the
/// number the rung is written as and cannot be read off the layout.
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
    // the twenty-one-row column but wide enough to put the mouse group beside the
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
            24,
            "a tall pane stopped drawing the twenty-four-row one-column sheet:\n{sheet}"
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
        // **The page counter sits here since #286**, between the title and the
        // rule, so the run of `─` no longer starts at `title + 1`. Validating its
        // shape is what stops "the rule starts wherever the counter ended" from
        // accepting any text at all.
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
        two_column > 0 && roomy_seen > 0 && sheets > two_column + roomy_seen,
        "the sweep saw {sheets} sheets, of which {two_column} were two-column and \
         {roomy_seen} roomy, so it did not cover all three shapes. The roomy arm \
         above asserts that a section heading carries no rule, and an arm no pane \
         in the sweep reaches is an arm that cannot fail"
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
/// **What the gate reading this cannot see, said rather than left implicit.** It
/// indexes a `Vec<char>` by column, so a cell holding a base character plus a
/// combining mark would shift every column assertion after it while the renderer
/// stayed correct, and a double-width glyph would shift them the other way. The
/// two-column rung's own placement gate has read columns the same way since #220
/// and this inherits the limit rather than adding it: no cell or label in either
/// table holds a combining mark today, and a table that gained one would be
/// caught by the renderer's own `width_of` against `set_stringn`'s clip rather
/// than here.
///
/// The sections carry **slices of [`GESTURES`]** rather than counts, so the mapping from a section to the rows
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
        // Five since B17: `r`, `s` and `a` join `f` and `m` in `view`, which is
        // the section for the things that change what the body is made of.
        ("view", &GESTURES[7..12]),
        // Seven since #288: the close control and the hover mark.
        ("mouse", &GESTURES[14..21]),
        ("leaving", &GESTURES[12..14]),
    ] {
        rows.push(RoomyRow::Heading(label));
        rows.extend(tokens.iter().map(|t| RoomyRow::Gesture(t)));
        rows.push(RoomyRow::Air);
    }
    rows
}

#[test]
fn the_roomy_rung_is_the_size_the_ruling_states() {
    // **`SPEC.md` §11.1 states 68 by 31, and Mock A drew 76 by 29.** The
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
            (68u16, 34u16),
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
fn the_roomy_rung_arrives_at_the_height_the_ruling_states() {
    // **The other axis, and the one no gate walked.** `SPEC.md` §11.1 states the
    // rung needs "a body of thirty-one rows", and a body is the pane less the
    // header and less a footer whose height is its own ladder in the width. So the
    // pane height it arrives at is not thirty-one and cannot be derived by
    // reading: on this fixture at a hundred columns it is thirty-four, three rows
    // above the number the ruling names.
    //
    // That gap is exactly what produced #220's wrong arrival width, where the
    // ruling said 80 for a rung that arrives at 78 because the probe sampled
    // instead of walking. Same instrument, other axis.
    let arrival;
    sweep!("sheet-roomy-height", |paint| {
        arrival = arrival_height_of(&mut paint, "moving", 24..=40, 100);
    });
    assert_eq!(
        arrival,
        Some(37),
        "the roomy rung does not arrive at the pane height a body of thirty-four \
         rows implies on this fixture"
    );
}

#[test]
fn the_roomy_rung_places_its_cells_where_the_plan_says() {
    // **Air is the one thing on this element that a count cannot see.** Every
    // gate above counts gestures or measures the frame, and a roomy rung that
    // drew its sections back to back with the blank rows all at the bottom would
    // satisfy every one of them: same width, same height, same eighteen gestures,
    // same closed box. The shape is what this pins, row by row.
    //
    // Columns are literals rather than derived, because a gate that computed them
    // from the layout would agree with it by construction.
    sweep!("sheet-roomy-cells", |paint| {
        let at = ROOMY_PANE;
        let (buf, laid) = paint(at);
        let (_, sheet) = read_sheet(&buf, &laid);
        let rows: Vec<Vec<char>> = sheet.lines().map(|r| r.chars().collect()).collect();
        assert_eq!(
            rows.len(),
            34,
            "the roomy rung is not thirty-four rows tall:\n{sheet}"
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
        //
        // **Collected from the drawn rows in row order**, not by filtering
        // `SECTIONS`: filtering yields `SECTIONS`' own order whatever the sheet
        // did, so the first version of this could fail on a label being *absent*
        // and never on the labels being in the wrong *order*, which is the half
        // it was written for.
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
    // **#285's own gate, in its own words: a heading costs a row and must never
    // cost a gesture.** The rung is inserted at the *head* of a monotone ladder,
    // which is the free case, and this is what turns that from an argument into
    // evidence.
    //
    // Two claims over one sweep, and they are not the same claim. Every pane that
    // takes the rung draws every gesture, so no pane can have lost one to
    // the air. And the shortest pane at each width that draws every one of them is
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
            // here survives that: it is still every gesture, still additive,
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
fn the_display_order_is_the_readers_and_the_narrow_floor_keeps_the_unguessable() {
    // **The two ends of #285's separation, on a screen.** `render.rs`'s
    // `sheet_tables` holds them on the tables; this holds them on what a reader
    // sees, and the two are not the same claim: a drawer free to iterate the
    // table backwards would satisfy every assertion over the constants.
    //
    // The reader's end: the full one-column rung draws every keyboard row in
    // the order Mock A reads them, `q` last. The ladder's end: at the floor, the
    // three rows left are `f`, `m` and `?`, and `q` is not among them. Conflate
    // the two orders again and the second fails, because dropping from the top of
    // the reader's order keeps `q`.
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

    // **The ladder's end moved from the height axis to the width one**
    // ([#286](https://github.com/breferrari/vigia/issues/286)). A paged rung drops
    // nothing, so `DROP_ORDER` no longer decides what a *short* pane sees; it
    // decides what a pane too **narrow** for the whole table sees, which is the
    // one place rows still disappear. Asserted as exact reachable sets rather than
    // as counts, because a count cannot tell a permutation from a reordering: this
    // is the mutation #285's two-orders separation exists to make visible.
    //
    // The sets are the tables' own, restated for [`TITLE`]'s reason, and the walk
    // is over every page because a narrow pane pages as well as drops.
    const NARROW: [(u16, &[&str]); 3] = [
        // Thirty columns, the narrowest sheet the ladder draws at all: what is
        // left after `DROP_ORDER` has given up seven, and every one of the three
        // the keep-set names is in it. `q` is the first to go, which is the whole
        // of #285's separation: the hint bar spells `q quit` on every frame.
        //
        // **Six since #297, five since #295, four before**, and which six is the
        // point rather than how many: `r` and `s` sit between `m` and `?` in the
        // reader's order and are given up at ranks eight and nine, both after
        // this rung's seven, so both survive here and are gone from a rung no
        // drawable pane reaches. That is the check B14's own reason failed and
        // this table is where it is made: the deepest rung a sheet draws at is
        // still `from = 7`, so neither reorder is behaviour.
        (
            30,
            &[
                "scroll the",
                "follow the newest",
                "churn band",
                "left rail",
                "one file",
                "staged changes",
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
    // **`SPEC.md` §11.1 states 104 by 16 wide and 71 by 16 tight, and until this
    // gate no test could fail on either.** The tight number was 76 until #286
    // shortened two tight mouse verbs so the whole table would fit I6's forty
    // columns in one column: the two-column rung measures the same cells, so it
    // narrowed by the same five. That is additive (a narrower rung arrives
    // earlier and draws every gesture where the pane used to page) and it is a
    // deviation from #286's plan, which predicted these numbers unmoved. That is the rail's own lesson one
    // element over: `RAIL_FLOOR` pins its composition in a const block precisely
    // because a claim no gate can fail is a wish, and #220's ruling shipped two
    // numbers with nothing holding them.
    //
    // It is not a restatement of the layout arithmetic. A mutation that moved the
    // mouse column one column right survived every other gate in this file: the
    // frame stayed closed because the sheet grew with it, every gesture still
    // drew, and the ladder stayed monotone. The only thing that changed was the
    // gap between the columns, and the only thing that can see it is a number.
    // **Both fixtures gained a row with the table** ([#297](https://github.com/breferrari/vigia/issues/297)).
    // The two-column rung is sixteen rows where it was fifteen, so a pane that
    // fitted it exactly no longer does and falls to the rung below, which is a
    // failure that reads as "the size changed" while the size is fine. The
    // `contains("keyboard")` guard above each assertion is what says which of the
    // two happened.
    let wide = Rect::new(0, 0, 120, 23);
    let tight = Rect::new(0, 0, 80, 20);

    sweep!("sheet-dimensions", |paint| {
        for (at, want, spelling) in [(wide, (104u16, 17u16), "wide"), (tight, (71, 17), "tight")] {
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
    // **73 since #286, 78 before it, and the first draft of the ruling said 80.**
    // `margin_of` returns 2 from 44 to 78, so a pane of 73 leaves 71 columns of room
    // and the tight two-column rung is exactly 71 wide. It was 76 until #286
    // shortened two tight mouse verbs, and this rung measures the same cells. The
    // probe that produced the ruling's original before-and-after sampled 60, 70 and
    // 80 and stepped over its own boundary, which is the same mistake as a
    // superlative bounded by a sweep that does not reach its falsifying case.
    //
    // Gated by walking the boundary a column at a time, because that is the only
    // instrument that can find it.
    let arrival;
    sweep!("sheet-arrival", |paint| {
        arrival = arrival_of(&mut paint, "keyboard", 70..=84, 20);
    });
    assert_eq!(
        arrival,
        Some(73),
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

                // **`SHEET_KEEP` is the smallest page, not a keep-set, since
                // #286.** Before B13 it named the three rows the height ladder
                // never dropped; a paged sheet drops none, so what the constant
                // decides is the height below which a page is too thin to be
                // worth drawing at all. Dropping it to 2 adds a rung two rows
                // tall, and this is where that fires. Every pane in this sweep is
                // wide enough for the whole table, so page one is always a full
                // page rather than a remainder.
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

    // Centring itself is pinned at named panes rather than swept, because the body
    // it is centred in is the pane less a footer whose height is its own ladder,
    // and a gate that reconstructed that would be reconstructing the layout. These
    // four were checked by hand against `margins_of` and the plan's own halving.
    sweep!("sheet-origin", |paint| {
        for (w, h, want) in [
            (120u16, 30u16, (32u16, 2u16, 56u16, 24u16)),
            // The roomy rung, which #285 put at the head of the ladder. A pane
            // this tall took the nineteen-row sheet at (22, 10, 56, 19) before it
            // existed, and the row it lost to air it had spare.
            (100, 40, (16, 2, 68, 34)),
            (120, 21, (8, 1, 104, 17)),
            // The tight two-column rung, five columns narrower since #286
            // shortened two tight mouse verbs and a row taller with each key added
            // since: `r` (#295) and `s` (#297). **The pane is 80x19 rather than
            // 80x18 for that second reason** — at eighteen rows this rung no
            // longer fits, and the case would have quietly become a one-column
            // one, still centred, still asserting a rect. The odd column of slack
            // it gained is what makes this case test the halving as well as the
            // width.
            (80, 20, (4, 1, 71, 17)),
            // Odd slack, which the first three above lack on both axes: halving
            // the slack the other way (`div_ceil`) or taking the trailing margin
            // instead of the leading one reproduces every one of them and misses
            // these, which is why this list is read as a set rather than case by
            // case.
            //
            // **This pane changed rung with [#288](https://github.com/breferrari/vigia/issues/288)**,
            // and the comment here described the old one until round 1 of that
            // row's audit read the two together. It took the one-column rung at
            // `(12, 1, 56, 21)`, and twenty-one gestures no longer fit one column
            // in a twenty-five-row pane, so it takes the two-column rung at
            // `(5, 3, 71, 17)`.
            //
            // **What this case demonstrates is no longer written here, and the
            // reason is worth more than the claim was.** Three spellings of this
            // comment in three rounds each asserted a slack arithmetic, and each
            // was wrong: two axes, then the height axis at nine splitting four and
            // five. The pinned tuple does not settle it, because a one-row and a
            // two-row footer both produce `top: 4` from this pane, so the height
            // slack is six or seven and no number here can say which without a
            // measurement nobody took. The width slack is ten and splits evenly at
            // five, which the tuple does show.
            //
            // So the case is kept for the rung it now covers and the list is read
            // as a set, which is what the paragraph above already said and what
            // three attempts at a per-case story failed to improve on.
            (81, 25, (5, 3, 71, 17)),
            // The whole table in one column reaches this width since #286, so
            // where this used to be a dropping rung of thirteen rows it is the
            // twenty-one-row sheet.
            //
            // **It stopped carrying the odd vertical slack when #295 added a
            // row and took it back when #297 added another**: the body is
            // twenty-two and the sheet is twenty-one, so the slack is one. Saying
            // which cases carry it beats leaving a comment that quietly stopped
            // describing its own line, and it has now stopped twice.
            (43, 25, (3, 1, 38, 22)),
            // The level probe's own boundary. `margin_of(58)` is 2, so the room is
            // exactly 56 and the wide one-column sheet is exactly 56: turning the
            // probe's `>` into `>=` flips this width and no other, dropping
            // `Ctrl+C`, `PgUp`, `Home`, `End` and the shifted arrows at a width
            // that fits them. Every count, every frame and every other origin is
            // identical.
            (58, 30, (1, 2, 56, 24)),
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
        (120u16, 23u16, [2usize, 26, 56, 77], 56usize, "wide"),
        (80, 20, [2, 15, 35, 50], 35, "tight"),
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
                // Fourteen since B17, and the number is the keyboard group's
                // own length rather than a constant that happens to match it: the
                // split is where `GESTURES` stops being keyboard rows.
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
    // **The worst regression available in this element, and nothing could see
    // it.** Before #286 the height axis dropped rows, and reversing the dropping
    // rungs made `from = 8` the first tried, so every short pane drew three
    // gestures instead of the eleven it could afford. Every other gate stayed
    // green: additivity reads a cell the reversal does not touch, monotonicity is
    // satisfied because three to sixteen is still non-decreasing, the degrade gate
    // asks only whether the mouse group is absent, and the origins, the size, the
    // arrival and the frame are all unchanged.
    //
    // The same regression is available under B13 one axis over: a page that took
    // `SHEET_KEEP` rows rather than the body's own would page a tall pane six
    // times and every gate above would stay green, because the union is still
    // every gesture and the sheet is still a closed box. So what is pinned is that
    // **a page is as full as the pane allows and the pages are as few as they can
    // be**, as literals rather than as a rule derived from the ladder.
    //
    // Pinned at fifty columns, too narrow for the two-column rung, so height is
    // the only axis moving. One row of pane buys one more gesture on page one all
    // the way up, with the single flat step at nineteen rows where the row it
    // buys is the mouse group's heading rather than a gesture.
    let expected = [
        (8u16, 3usize, 8usize),
        (9, 4, 6),
        (10, 5, 5),
        (11, 6, 4),
        (12, 7, 4),
        (13, 8, 3),
        (14, 9, 3),
        (15, 10, 3),
        (16, 11, 2),
        (17, 12, 2),
        (18, 13, 2),
        (19, 14, 2),
        // The flat step, one row later again since B17 gave the keyboard group a
        // fourteenth row ([#313](https://github.com/breferrari/vigia/issues/313)):
        // this is the height at which the row the body buys is the mouse group's
        // **heading**, which costs a row and names no gesture. It was 17 before
        // #295, 18 before #297 and 19 before this, and it moves with the table
        // rather than being a property of the ladder.
        (20, 14, 2),
        (21, 15, 2),
        (22, 16, 2),
        (23, 17, 2),
        (24, 18, 2),
        (25, 19, 2),
        (26, 20, 2),
        // **The whole table in one column, at twenty-seven since B17**, twenty-six
        // since #288 and twenty-four before that. Every gesture added to the table
        // is one more row of body before a single page holds them all, which is
        // what this last row measures and why it is [`WHOLE_TABLE`]'s own number.
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
        // **A cell has to hold something before its colour means anything**, and
        // the whole of this gate reads colours at fixed columns. The sheet's blank
        // pass paints its entire rect `chrome_dim`, so every "is dim" assertion
        // below passes on *air*: move the verb field and this would go on
        // reporting the right weight for a space. Only a glyph assertion in some
        // *other* gate rescues it today, which is precisely the coupling that
        // makes a gate unable to fail on its own terms.
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

    // **The roomy rung's five headings, which are plain text rather than rules and
    // so are the one heading shape no rule-reading gate reaches.** Repainting them
    // in the lit weight changes no character: `the_roomy_rung_places_its_cells_
    // where_the_plan_says` reads symbols and nothing else in the suite reads a
    // style inside this rung.
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
        // **The cell has to hold the label before its colour means anything.**
        // The blank pass paints every cell in the sheet's rect `chrome_dim`, so
        // reading a colour at a fixed column passes on *air*: move
        // `ROOMY_HEADING_INSET` and this would have gone on asserting the weight
        // of a space. The placement gate catches the move; this must not report
        // a pass while looking at nothing.
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
    let at = Rect::new(0, 0, 120, 21);
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
        // **The mouse heading is row fifteen since B17**, fourteen since #297,
        // thirteen since #295, twelve before: it is one past the keyboard group's
        // last row, and the number moves with the table by one every time a key is
        // added.
        // 80 by 27 since B17 and 26 since #288: below that this pane takes the
        // two-column rung instead.
        (80u16, WHOLE_TABLE, 2usize, 26usize, 'j', Some(15usize)),
        (120, 30, 2, 26, 'j', Some(15)),
        // A **paged** rung since #286, so its first row is the table's first
        // again: the height ladder no longer drops rows, it splits them, and page
        // one starts where the reader's order does. The fields are the whole
        // table's, mouse rows included, which is why the verb column sits two
        // further right than it did when this rung dropped the mouse group.
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

    // And the close control still dismisses at this rung. `CloseSheet` rather than
    // `ToggleSheet` since #286: `?` means *the sheet* and advances on a paged pane,
    // and a control that walked the reader through five more pages before letting
    // them out is not a close control.
    let close = action_for(&click(sheet.close.0, sheet.close.1), laid);
    assert_eq!(
        close,
        Some(Action::CloseSheet),
        "the close control does not dismiss the two-column rung"
    );
}

#[test]
fn the_roomy_rung_swallows_what_lands_on_it() {
    // **The same hole one rung up.** #220 wrote the gate below because every
    // behavioural gate ran at 80 by 24, where the sheet is 56 wide, so the
    // two-column rung was proven by geometry and by text alone. The roomy rung
    // arrived the same way: nothing clicks or wheels inside a 68 by 31 sheet, and
    // narrowing `SheetPlan::target` for `Shape::Roomy` alone would pass every
    // other gate in this file.
    //
    // It is the rung where it matters most now, because it is what a full-screen
    // pane draws, and it is thirty-one rows tall against the plain rung's
    // twenty-one: the rows a click can land on that no other rung reaches are
    // its own.
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
        (68, 34),
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
    // **This gate used to hold two branches nothing could make fire.**
    // `sheet_floor` raised any rung to the width its own title bar needed, and no
    // rung ever reached it: the floor was seventeen and the narrowest rung
    // twenty-four. It said so, and said that the day #288 added a `MOUSE` row one
    // of them would stop clearing and the behaviour behind the guard would become
    // reachable and untested.
    //
    // **#286 is what made it fire, and from the other direction.** The page
    // counter rides the title bar, so every rung now charges the widest counter
    // the tables can spell whether or not it draws one, and that is what keeps a
    // centred box the same size on every page of a pane. The floor is thirty, the
    // deepest dropping rungs are raised to it, and a pane narrower than thirty
    // draws no sheet at all. So the numbers below are a rung's rather than a
    // guard's slack, and they are asserted rather than described.
    //
    // Restated here rather than imported for [`TITLE`]'s reason. The counter's own
    // widest spelling is ` 18-18 of 18 `, which is the range form at the table's
    // own size: no page draws it, since a page showing every gesture draws no
    // counter, and it is the bound the box is sized against.
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
    // **#286's gate, and the promise B13 makes.** On any pane that draws a sheet
    // at all, walking `?` from the first page to the close reaches every gesture
    // the pane binds. Not *fits on one screen*: reaches.
    //
    // Forty is I6's own width and that is why it is the floor here. Below it the
    // mouse group has no spelling that fits any pane, so the promise is stated
    // where it can hold rather than everywhere and honoured nowhere.
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
    // **The floor its two sibling sweeps have and this one did not.** A
    // `sheet_plan` returning `None` everywhere makes every cell `continue` and the
    // gate passes having asserted nothing, which is the vacuous-superlative shape
    // `SPEC.md` §7 keeps finding.
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
    ///
    /// **All four, and the first version carried two.** The last page's content is
    /// a remainder, and sizing the frame from it slid the box down half the
    /// difference while `left` and `width` stayed put, so a gate reading those two
    /// was green over a control moving out from under the reader's pointer.
    frame: (u16, u16, u16, u16),
}

/// Every page `?` reaches on one pane, in order, before it closes.
///
/// **Driven through `App::apply` rather than through a plan**, because what #286
/// rules is the *input model*: a gate that asked the layout how many pages there
/// are would agree with the layout by construction and say nothing about what a
/// reader pressing `?` can reach.
///
/// **It returns the pages rather than a summary of them.** The first version
/// returned only the union and a press count, so three gates opened a second
/// `App` and repainted page one to look at it, and one of them re-implemented this
/// loop with a second copy of the bound.
fn walk_the_pages(
    frame: &mut Frame<'_>,
    highlighter: &mut Highlighter,
    history: &History,
    at: Rect,
) -> Vec<Page> {
    const BOUND: usize = 64;
    // **A fresh shell per pane, and the first version of this was not.** A pane
    // below the sheet's floor draws nothing and leaves the state open, which is
    // §11.1's own ruling and not a defect; carrying that state into the next pane
    // starts its walk on page two and hides page one from every gate here.
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
///
/// **The paint is what a reader's own pace looks like**, and it is one of the two
/// shapes this element has to be right in. `App::sheet_pages` is recorded by
/// `App::view`, so a walk with a frame between presses is a reader pressing `?`
/// and looking at what arrived. The other shape is a batch: the shell drains
/// several actions and paints once at the end, which is what
/// `two_presses_in_one_wake_reach_page_two` covers. Neither models the other, and
/// a suite with only the first cannot see a held key.
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
    // **The input model B13 rules, asserted as state rather than as pixels.**
    // Every gate above reads what was drawn; this reads what `?` did, because the
    // failure it covers is a sheet that pages forever (the reader can never close
    // it without the mouse) or one that closes early (the last page is
    // unreachable). Both draw perfectly well on every frame they do draw.
    let scratch = Scratch::large_diff("sheet-paging-state", FILES, 40);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    // A pane of seven pages and a pane of one, so both ends of the ladder are
    // here.
    for (at, pages) in [
        (Rect::new(0, 0, 50, 8), 8usize),
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
    // **B13's additivity claim, swept rather than sampled.** The ruling's whole
    // defence is that a reader whose pane already showed every gesture presses `?`
    // twice as before.
    //
    // **What it does not catch, said rather than implied.** The obvious mutation,
    // putting the paged rungs first in the ladder, leaves this green: a tall pane's
    // paged rung has capacity for all nineteen lines, so it is one page and closes
    // on the second press exactly as before. That mutation reddens nineteen other
    // gates here, every roomy-rung and two-column gate among them, because what it
    // actually destroys is the rungs it steps in front of. Verified by running it.
    // What this gate holds on its own is the counter's absence and the press count
    // on a pane that draws the whole table, which is the reader-facing half.
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
    // **The say-so half of B13, and the only thing on this element a reader on a
    // narrow pane can use to tell that gestures are missing.** Deleting it reddens
    // nothing else here: every gesture is still reachable wherever it was, the box
    // is still closed, and `the_sheet_is_a_closed_box_at_every_rung` only checks
    // the counter's *shape* when one is drawn. Verified by mutation.
    //
    // The ordinals are checked against the gestures actually on the page, which is
    // what makes this a claim about the counter's truth rather than its presence.
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
            vec!["1-3 of 21", "4-6 of 21", "7-9 of 21"],
        ),
        (Rect::new(0, 0, 32, 40), vec!["1-11 of 21"]),
        (Rect::new(0, 0, 30, 40), vec!["1-7 of 21"]),
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
    // **A centred box that changed width as the reader pressed `?` would shift
    // left under their eye**, and nothing else here can see it: every page is a
    // closed box of the right size *for itself*.
    //
    // **What holds it is not what this gate was first written to check.** The
    // claim was that `sheet_floor` charging the counter on every rung is what keeps
    // the width fixed; removing that charge leaves this green and reddens two width
    // gates instead. `sheet_fields` measures over the whole row set and every page
    // of a pane shares that set, so the width is page-independent by construction.
    // What the charge buys is that the counter fits inside the title bar at all,
    // which `the_floor_is_a_rung_now_and_the_narrowest_sheets_are_the_sizes_the_ruling_states`
    // is the gate for. Both claims are true and they are different claims.
    //
    // **All four edges, and this gate read two of them until #286's own audit.**
    // The last page's *content* is a remainder, but its box is not: `paged_fit`
    // sizes every page of a pane to `capacity + SHEET_FRAME` and lets the tail run
    // blank inside the frame, because `sheet_plan` centres on the height and a
    // shorter last page slid the box down half the difference. `left` and `width`
    // were unmoved by that, so the two-field version was green over a close control
    // walking out from under a resting pointer.
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
    // **`a_write_under_the_sheet_does_not_dismiss_it` one field over.** The page
    // is retained state for the same reason the sheet is: the pane wakes on
    // filesystem events, so an agent's build redraws the frame underneath it, and
    // a page that lived for one frame would send a reader back to the first one at
    // random. The existing gate holds the sheet and would stay green against a
    // page that reset, because page one is still a sheet.
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
    // **The one stale read in this design, exercised where it lands.** The page
    // count `?` is measured against is the previous frame's, because `Action`
    // carries no pane and the count is a fact about one. So a pane resized between
    // the last paint and the next press can be asked for a page it no longer has,
    // and `paged_fit` clamps to the last one. Without the clamp `lines - skip`
    // underflows on the frame path, which is a panic in a monitor somebody left
    // running.
    //
    // **Both panes have to be paged ones, and the first version of this gate used
    // an eighty by twenty-four target that is not.** Resizing into a rung that
    // draws the whole table never calls `paged_fit` at all, so deleting the clamp
    // left the gate green: it was asserting that a one-page sheet is one page.
    // Found by mutation, which is the only instrument that could see it.
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

    // Three pages of nine rows, so page seven does not exist and page three is
    // the last. **Two until #297's row**, and it is the mouse group's heading
    // that makes the third: nineteen lines over a capacity of nine is 9, 9 and a
    // remainder of one.
    let larger = Rect::new(0, 0, 50, 14);
    let (buf, laid) = paint(&mut app, &mut frame, &mut highlighter, &history, larger);
    let sheet = laid
        .sheet
        .expect("a resize past the last page closed a sheet nobody dismissed");
    let (count, drawn) = read_sheet(&buf, &laid);
    assert_eq!(
        counter_of(&drawn).unwrap_or_default().trim(),
        "18-21 of 21",
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
    // **The shell drains actions in a batch and paints once at the end of it**
    // (`lib.rs`'s `'awake` loop), so two `?` events that arrive together are
    // applied with no frame between them. Every other gate here paints between
    // presses, which is the shape production does *not* have: a held `?` or two
    // quick taps land in one batch, and `App::sheet_pages` is then whatever the
    // last *draw* measured rather than what the press before it would have.
    //
    // That is `SPEC.md` §7's own rule about a gate measuring the cheapest state,
    // one layer over: a suite that models one press per frame cannot see a defect
    // that needs two in one.
    //
    // What it costs when it is wrong: the sheet opens and closes inside one wake
    // and the reader sees nothing at all.
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
        "4-6 of 21",
        "the batched second press did not land on page two:\n{sheet}"
    );
}

#[test]
fn the_close_control_closes_from_any_page() {
    // **The sheet's only pointer escape, and it advanced.** A click on `✕` sent
    // `Action::ToggleSheet`, the same action `?` sends, so on a six-page pane the
    // control walked the reader forward a page at a time and took six clicks to let
    // them out. `SPEC.md` §11.1 and `Action::ToggleSheet`'s own docblock both said
    // it dismissed from any page while it did not.
    //
    // The gate that existed asserted the *action's identity* on an eighty by
    // twenty-four pane, which is one page, and never applied it. Both halves are
    // why it could not see this: one page has nothing to advance to, and an
    // identity is not an outcome.
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
    // **The one line in `column_lines` that is not a gesture**, and the ordinals
    // have to step over it. Dropping `filter(is_row)` from `shown_of`'s `before`
    // term survives the whole suite otherwise, because every page the counter gate
    // reads sits entirely above the heading.
    //
    // Six pages of three at fifty by eight: the heading is line 12 since #295 gave
    // the keyboard group a twelfth row, so **page five** is the one that carries it
    // and page six is the rest of the mouse group. The page it lands on moving is
    // the point: a gate that pinned page four would have followed the heading
    // rather than pinning the arithmetic.
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
            "1-3 of 21",
            "4-6 of 21",
            "7-9 of 21",
            "10-12 of 21",
            // The heading costs this page a row and no ordinal, so it names two
            // gestures where every page above it names three.
            "13-14 of 21",
            "15-17 of 21",
            "18-20 of 21",
            // And the twenty-first gesture is alone on a page of its own, which is
            // the tail a paged rung leaves rather than a defect: B17's row
            // ([#313](https://github.com/breferrari/vigia/issues/313)) made the
            // table one longer than seven pages of three can hold, exactly as
            // #297's did one page earlier.
            "21 of 21",
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
    // **`page.min(pages - 1)` and `page % pages` are the same answer for most
    // pairs**, so the resize gate cannot tell a clamp from a wrap: it goes from six
    // pages to two at page six, and `5 % 2` and `5.min(1)` are both one.
    //
    // Page **five** into a pane of four pages separates them: `4 % 4` is zero and
    // `4.min(3)` is three. A wrap would send a reader who shrank their pane back
    // to the start of the sheet; a clamp leaves them at the end, which is where
    // they were. (It was six pages into three before #297's row, `4 % 3` against
    // `4.min(2)`; the pair moved and the separation is what matters.)
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

    // Fifty by eleven: a body of eight, a capacity of six, nineteen lines, four
    // pages. Page five clamps to page four and would wrap to page one, which
    // still separates the two: `4 % 4` is zero and `4.min(3)` is three.
    let three = Rect::new(0, 0, 50, 11);
    let (buf, laid) = paint(&mut app, &mut frame, &mut highlighter, &history, three);
    assert!(laid.sheet.is_some(), "the resize closed the sheet");
    let (_, sheet) = read_sheet(&buf, &laid);
    assert_eq!(
        counter_of(&sheet).unwrap_or_default().trim(),
        "18-21 of 21",
        "the resize wrapped the page instead of clamping it:\n{sheet}"
    );
    assert_eq!(
        chrome(&app).sheet,
        Some(3),
        "the state kept a page the pane no longer has, so the screen and the state \
         disagree about which page is up"
    );
}

#[test]
fn every_page_is_a_closed_box_including_its_blank_tail() {
    // **`the_sheet_is_a_closed_box_at_every_rung` cannot reach this and its own
    // scaffold is why.** `walk_the_ladder` toggles once and paints, so every cell
    // of its 3,400-cell sweep reads **page one**, which is full. Only the *last*
    // page of a pane has a blank tail, because the box is the pane's `capacity` and
    // the content is a remainder, and the pipes down that tail are drawn by a
    // separate call. Deleting `sheet_pipes_over` from `Painter::sheet_column`
    // survives the entire suite otherwise: found by mutation, which is the only
    // instrument that could see it.
    //
    // The panes are chosen for their remainders rather than swept: 50x8 leaves one
    // blank row, 50x10 leaves three and 40x12 leaves four, so the tail is a row, a
    // few rows and most of a box.
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
    // **A pane too narrow for a sheet has zero pages, and the clamp read that as
    // "go to page one".** `App::view` clamps the page to what the pane has so the
    // state and the screen agree, and `pages.saturating_sub(1)` on a pane below the
    // thirty-column floor is zero, so a reader who dragged their pane narrow and
    // back found themselves on page one of a sheet they had left on page four.
    // Nothing was drawn while it was narrow, so nothing asked for the move.
    //
    // Found by the audit round that read the previous round's own fixes, which is
    // the class of defect that round exists for.
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
        "10-12 of 21",
        "the pane came back on a different page:\n{sheet}"
    );
}

#[test]
fn the_arrows_are_named_at_the_wide_spelling_and_not_the_tight_one() {
    // **The measured trade #296 took, pinned so it cannot be undone silently.**
    // The `n / p` row's keys cell names `→` and `←` at the wide spelling. It does
    // **not** at the tight one, and that is arithmetic rather than taste: the
    // tight keyboard keys field is eleven columns, the arrowed cell is thirteen,
    // and putting it there takes the keyboard-only rung from thirty-five columns
    // to thirty-seven, which costs panes of 35 and 36 their twelve gestures.
    //
    // Both halves are asserted, because only the pair says which trade was made.
    // Adding the arrows to the tight cell reddens the second and would otherwise
    // move two panes' reachability with nothing to see.
    let scratch = Scratch::large_diff("sheet-arrow-aliases", FILES, 40);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    // **One page, one paint.** The first draft reached for `walk_the_pages`, which
    // presses `?` through every page and repaints each, and then took `.first()`.
    // Both panes here draw the cell on their first page.
    let mut page_one = |frame: &mut Frame<'_>, at: Rect| {
        let mut app = App::new();
        toggle_at(&mut app, frame, at);
        let (buf, laid) = paint(&mut app, frame, &mut highlighter, &history, at);
        read_sheet(&buf, &laid).1
    };

    // Wide: a pane the whole table fits on in one column at the wide spelling.
    // 80 by 26 since #288: twenty gestures no longer fit one column in a
    // twenty-four-row pane, so that size takes the two-column rung and its tight
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
