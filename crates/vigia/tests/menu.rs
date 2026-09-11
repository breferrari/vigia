//! The config menu: `m`, the mode it opens, and the two things it must not do.

#[path = "../../vigia-core/tests/support/mod.rs"]
mod support;

use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::Rect;
use vigia::{
    Action, App, Chrome, Glyphs, Hovered, MenuRoute, Pointing, Regions, SETTINGS, Setting, Theme,
    action_for, body_layout, menu_route, regions, render,
};
use vigia_core::{Frame, Highlighter, History};

use support::{Scratch, materialise};

const WIDE: u16 = 80;
const TALL: u16 = 24;
const FILES: usize = 3;

/// The word the menu's own title bar spells, restated rather than imported: a
/// gate that reads the constant it is checking cannot fail when the constant is
/// wrong.
const TITLE: &str = "config menu";

/// The close control's glyph, restated for [`TITLE`]'s reason.
const CLOSE: char = '✕';

/// The mark on the row the caret is on, restated for [`TITLE`]'s reason.
const CARET: char = '▸';

fn area() -> Rect {
    Rect::new(0, 0, WIDE, TALL)
}

fn chrome(app: &App) -> Chrome {
    app.chrome(
        "fixture",
        Some("main"),
        "current",
        Pointing::default(),
        Default::default(),
        "",
    )
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

/// Drive one action through the app, on a pane of the caller's choosing.
fn apply(app: &mut App, frame: &mut Frame<'_>, at: Rect, action: Action) {
    let height = body_layout(at, &chrome(app), FILES, FILES).diff;
    assert!(
        app.apply(action, frame, height).expect("apply"),
        "{action:?} asked the shell to quit"
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
    let body = body_layout(at, &chrome, FILES, FILES);
    let view = app
        .view(frame, highlighter, history, body)
        .expect("collect a view");
    let mut buf = Buffer::empty(at);
    render(
        &mut buf,
        at,
        &view,
        &Theme::default(),
        Glyphs::default(),
        &chrome,
    );
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

/// A fixture, an app and the machinery one paint needs.
struct Pane {
    scratch: Scratch,
    app: App,
    highlighter: Highlighter,
    history: History,
}

impl Pane {
    fn open(name: &str) -> Self {
        Self {
            scratch: Scratch::large_diff(name, FILES, 40),
            app: App::new(),
            highlighter: Highlighter::eager(),
            history: History::new(),
        }
    }

    /// Run `body` with a walked frame in hand.
    fn with<T>(
        &mut self,
        body: impl FnOnce(&mut App, &mut Frame<'_>, &mut Highlighter, &History) -> T,
    ) -> T {
        let worktree = self.scratch.worktree();
        let mut frame = worktree.frame();
        materialise(&mut frame);
        body(
            &mut self.app,
            &mut frame,
            &mut self.highlighter,
            &self.history,
        )
    }
}

/// The word the row carrying `label` ends in, frame and padding taken off.
///
/// Found by the label rather than by the word: a gate that locates its subject
/// by the value it expects cannot fail when the value is wrong.
fn state_of(text: &str, label: &str) -> String {
    text.lines()
        .find(|line| line.contains(label))
        .unwrap_or_else(|| panic!("no row for {label:?}:\n{text}"))
        .trim_end_matches(['\u{2502}', '|'])
        .trim_end()
        .rsplit(' ')
        .next()
        .expect("a row with a word in it")
        .to_owned()
}

/// The box as it was painted, found in the buffer by its own frame, and held
/// against what `Regions` published.
///
/// Taking the rect from the region alone measures what the pointer is told and
/// says nothing about what was drawn. The two are built by separate calls, so a
/// gate reading only the region stays green while the drawn box moves under it:
/// `no_toggle_the_menu_draws_moves_the_box_it_is_drawn_in` did exactly that
/// against a mutation that tied the drawn box's height to the rail.
fn drawn(buf: &Buffer, laid: &Regions) -> (String, Rect) {
    let area = *buf.area();
    let corners =
        |x: u16, y: u16, of: [char; 2]| of.iter().any(|c| buf[(x, y)].symbol() == c.to_string());
    let (top, left) = (area.y..area.y + area.height)
        .flat_map(|y| (area.x..area.x + area.width).map(move |x| (y, x)))
        .find(|&(y, x)| {
            corners(x, y, ['┌', '╭'])
                && text_of(buf, Rect::new(x, y, area.width - x, 1)).contains(TITLE)
        })
        .expect("no config menu is painted anywhere on this screen");
    let width = (left + 1..area.x + area.width)
        .find(|&x| corners(x, top, ['┐', '╮']))
        .map(|right| right + 1 - left)
        .expect("the painted box has no closing corner on its title bar");
    let height = (top + 1..area.y + area.height)
        .find(|&y| corners(left, y, ['└', '╰']))
        .map(|bottom| bottom + 1 - top)
        .expect("the painted box has no bottom edge");
    let at = Rect::new(left, top, width, height);

    let menu = laid
        .menu
        .expect("the menu was painted and published no region");
    assert_eq!(
        (menu.left, menu.top, menu.width, menu.height),
        (at.x, at.y, at.width, at.height),
        "the region the pointer is told about is not the box that was drawn"
    );
    (text_of(buf, at), at)
}

#[test]
fn m_is_what_opens_the_config_menu() {
    assert_eq!(
        action_for(&press(KeyCode::Char('m')), Regions::default()),
        Some(Action::ToggleMenu),
        "`m` is not bound to the config menu"
    );

    let mut pane = Pane::open("menu-open");
    pane.with(|app, frame, highlighter, history| {
        let (_, laid) = paint(app, frame, highlighter, history, area());
        assert!(
            laid.menu.is_none(),
            "the pane opened with a menu already up"
        );

        apply(app, frame, area(), Action::ToggleMenu);
        let (buf, laid) = paint(app, frame, highlighter, history, area());
        let (text, _) = drawn(&buf, &laid);
        assert!(text.contains(TITLE), "the box is not titled:\n{text}");
        assert!(
            text.contains(CLOSE),
            "the box carries no close control:\n{text}"
        );

        apply(app, frame, area(), Action::ToggleMenu);
        let (_, laid) = paint(app, frame, highlighter, history, area());
        assert!(laid.menu.is_none(), "`m` a second time left the menu up");
    });
}

#[test]
fn the_menu_draws_every_view_toggle_and_the_word_for_where_it_stands() {
    // Driven through the real `App` rather than a hand-built `Chrome`, so the
    // assignment that fills each row from the pane's own state is inside what
    // this gate can see.
    let mut pane = Pane::open("menu-rows");
    pane.with(|app, frame, highlighter, history| {
        apply(app, frame, area(), Action::ToggleMenu);
        let (buf, laid) = paint(app, frame, highlighter, history, area());
        let (text, _) = drawn(&buf, &laid);
        for setting in SETTINGS {
            assert!(
                text.contains(setting.label()),
                "the menu never draws {:?}:\n{text}",
                setting.label()
            );
        }

        // The wire, not the painter: `f` is on at launch and `r` is off, so the
        // two words have to differ on the rows that carry them.
        assert_eq!(
            state_of(&text, Setting::Follow.label()),
            "on",
            "follow is on at launch and its row does not say so:\n{text}"
        );
        assert_eq!(
            state_of(&text, Setting::Rail.label()),
            "off",
            "the rail is off at launch and its row does not say so:\n{text}"
        );

        // And it moves with the pane rather than being drawn once.
        apply(app, frame, area(), Action::ToggleRail);
        let (buf, laid) = paint(app, frame, highlighter, history, area());
        let (text, _) = drawn(&buf, &laid);
        assert_eq!(
            state_of(&text, Setting::Rail.label()),
            "on",
            "the rail was turned on and its row still reads off:\n{text}"
        );
    });
}

#[test]
fn every_settings_action_moves_its_own_row_and_no_other() {
    // `Setting` carries a label and an action in one table, and a row wired to its
    // neighbour's action is invisible to every per-row check: flipping it still
    // changes *a* setting, and the assertion that some state moved still passes.
    // Found by mutation, not by reading: pointing `Icons` at `Action::ToggleLinks`
    // left all twelve gates green.
    let mut pane = Pane::open("menu-wiring");
    pane.with(|app, frame, highlighter, _history| {
        let _ = highlighter;
        for setting in SETTINGS {
            let before = app.settings();
            apply(app, frame, area(), setting.action());
            let after = app.settings();
            for other in SETTINGS {
                let moved = other.of(before) != other.of(after);
                assert_eq!(
                    moved,
                    other == setting,
                    "flipping {:?} moved {:?}, so the two rows do not have                      disjoint state",
                    setting.label(),
                    other.label()
                );
            }
            // Back, so each setting is measured from the same pane.
            apply(app, frame, area(), setting.action());
            assert_eq!(
                app.settings(),
                before,
                "flipping {:?} twice did not put the pane back",
                setting.label()
            );
        }
    });
}

#[test]
fn the_caret_moves_and_space_flips_the_row_it_is_on() {
    let mut pane = Pane::open("menu-caret");
    pane.with(|app, frame, highlighter, history| {
        apply(app, frame, area(), Action::ToggleMenu);
        let (buf, laid) = paint(app, frame, highlighter, history, area());
        let (text, _) = drawn(&buf, &laid);
        let carets = |text: &str| {
            text.lines()
                .position(|line| line.contains(CARET))
                .expect("no caret is drawn")
        };
        let first = carets(&text);

        apply(app, frame, area(), Action::MenuMove(1));
        let (buf, laid) = paint(app, frame, highlighter, history, area());
        let (text, _) = drawn(&buf, &laid);
        assert_eq!(
            carets(&text),
            first + 1,
            "the caret did not move a row down:\n{text}"
        );

        // The second row is the rail, which is off, and `Space` is what flips it.
        assert!(!app.settings().rail, "the rail started on");
        apply(app, frame, area(), Action::MenuFlip);
        assert!(app.settings().rail, "the flip did not reach the rail");

        // And it clamps rather than wrapping.
        for _ in 0..SETTINGS.len() * 2 {
            apply(app, frame, area(), Action::MenuMove(1));
        }
        let (buf, laid) = paint(app, frame, highlighter, history, area());
        let (text, _) = drawn(&buf, &laid);
        assert_eq!(
            carets(&text),
            first + SETTINGS.len() - 1,
            "the caret wrapped past the last row instead of stopping:\n{text}"
        );
    });
}

#[test]
fn space_and_enter_are_the_menus_and_every_letter_still_reaches_the_pane() {
    let over = None;
    assert_eq!(
        menu_route(&press(KeyCode::Char(' ')), over),
        MenuRoute::Flip
    );
    assert_eq!(menu_route(&press(KeyCode::Enter), over), MenuRoute::Flip);
    assert_eq!(menu_route(&press(KeyCode::Down), over), MenuRoute::Move(1));
    assert_eq!(menu_route(&press(KeyCode::Up), over), MenuRoute::Move(-1));
    assert_eq!(menu_route(&press(KeyCode::Esc), over), MenuRoute::Close);

    // The mode is narrow, and this is the half that says so. `r` still flips the
    // rail from inside the menu, and `j` still scrolls the diff behind it.
    for code in [
        KeyCode::Char('r'),
        KeyCode::Char('j'),
        KeyCode::Char('k'),
        KeyCode::Char('q'),
        KeyCode::Char('?'),
        KeyCode::Char('a'),
    ] {
        assert_eq!(
            menu_route(&press(code), over),
            MenuRoute::Through,
            "{code:?} was swallowed by the menu's mode"
        );
    }
}

#[test]
fn esc_closes_the_menu_and_not_the_program() {
    let mut pane = Pane::open("menu-esc");
    pane.with(|app, frame, highlighter, history| {
        apply(app, frame, area(), Action::ToggleMenu);
        let height = body_layout(area(), &chrome(app), FILES, FILES).diff;
        assert!(
            app.apply(Action::Escape, frame, height).expect("escape"),
            "Esc over the menu asked the shell to quit"
        );
        let (_, laid) = paint(app, frame, highlighter, history, area());
        assert!(laid.menu.is_none(), "Esc left the menu up");

        // And with nothing up it still leaves.
        assert!(
            !app.apply(Action::Escape, frame, height).expect("escape"),
            "Esc over a bare pane no longer quits"
        );
    });
}

#[test]
fn the_menu_and_the_sheet_are_never_both_drawn() {
    let mut pane = Pane::open("menu-sheet");
    pane.with(|app, frame, highlighter, history| {
        apply(app, frame, area(), Action::ToggleSheet);
        apply(app, frame, area(), Action::ToggleMenu);
        let (_, laid) = paint(app, frame, highlighter, history, area());
        assert!(laid.menu.is_some(), "`m` under the sheet drew no menu");
        assert!(
            laid.sheet.is_none(),
            "`m` under the sheet left the sheet up"
        );

        apply(app, frame, area(), Action::ToggleSheet);
        let (_, laid) = paint(app, frame, highlighter, history, area());
        assert!(laid.sheet.is_some(), "`?` under the menu drew no sheet");
        assert!(laid.menu.is_none(), "`?` under the menu left the menu up");
    });
}

#[test]
fn a_click_anywhere_on_a_row_flips_that_row() {
    let mut pane = Pane::open("menu-click");
    pane.with(|app, frame, highlighter, history| {
        apply(app, frame, area(), Action::ToggleMenu);
        let (_, laid) = paint(app, frame, highlighter, history, area());
        let menu = laid.menu.expect("a menu region");

        // The second drawn row is the rail. The name is the target, not the state
        // cell: three columns is a hostile thing to aim at.
        let row = menu.top + 3;
        let on_name = click(menu.left + 6, row);
        assert_eq!(
            menu_route(&on_name, Some(menu)),
            MenuRoute::Row(1),
            "a press on a row's name is not that row's"
        );

        apply(app, frame, area(), Action::MenuRow(1));
        assert!(app.settings().rail, "the click did not reach the rail");
        // The caret follows the pointer, so arrowing after a click does not jump
        // somewhere stale.
        apply(app, frame, area(), Action::MenuFlip);
        assert!(!app.settings().rail, "the caret did not follow the click");
    });
}

#[test]
fn a_pointer_marks_the_row_it_rests_on_and_leaves_the_caret_where_it_was() {
    let mut pane = Pane::open("menu-hover");
    pane.with(|app, frame, highlighter, history| {
        apply(app, frame, area(), Action::ToggleMenu);
        let (_, laid) = paint(app, frame, highlighter, history, area());
        let menu = laid.menu.expect("a menu region");

        // The third drawn row, which the caret is not on.
        let row = menu.top + 4;
        let over = Regions {
            menu: Some(menu),
            ..Regions::default()
        };
        assert_eq!(
            over.hover_at(menu.left + 6, row),
            Some(Hovered::MenuRow(row)),
            "a pointer resting on a row is not told it is over one"
        );
        // Its own variant, so a listed file underneath the overlay is not marked
        // by a pointer that is nowhere near the list.
        assert_ne!(
            over.hover_at(menu.left + 6, row),
            Some(Hovered::Row(row)),
            "the menu's rows report themselves as listed files"
        );
        // And the frame is not a row.
        assert_eq!(
            over.hover_at(menu.left, menu.top + 1),
            None,
            "the box's own frame reports itself as a row"
        );

        let mut ink = |hovered: Option<Hovered>| {
            let chrome = Chrome {
                hovered,
                ..chrome(app)
            };
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
            buf[(menu.left + 6, row)].fg
        };
        let theme = Theme::default();
        assert_eq!(
            ink(Some(Hovered::MenuRow(row))),
            theme.path_hover.fg.expect("the hover ink names a colour"),
            "a pointer resting on a row does not mark it"
        );
        assert_ne!(
            ink(None),
            theme.path_hover.fg.expect("the hover ink names a colour"),
            "the row is drawn in the hover ink with no pointer on it, so the              assertion above proves nothing"
        );
    });
}

#[test]
fn a_press_outside_closes_it_and_the_control_closes_it() {
    let mut pane = Pane::open("menu-dismiss");
    pane.with(|app, frame, highlighter, history| {
        apply(app, frame, area(), Action::ToggleMenu);
        let (_, laid) = paint(app, frame, highlighter, history, area());
        let menu = laid.menu.expect("a menu region");

        assert_eq!(
            menu_route(&click(menu.close.0, menu.close.1), Some(menu)),
            MenuRoute::Close,
            "the close control does not close it"
        );
        assert_eq!(
            menu_route(&click(0, 0), Some(menu)),
            MenuRoute::Close,
            "a press outside it does not close it"
        );
        // Its own frame is inert: a press there is not a row and is not a dismissal.
        assert_eq!(
            menu_route(&click(menu.left, menu.top + 1), Some(menu)),
            MenuRoute::Inert,
            "a press on the box's own frame did something"
        );
    });
}

#[test]
fn no_toggle_the_menu_draws_moves_the_box_it_is_drawn_in() {
    // `SPEC.md` §11.2 B22's "nothing moves the box", as a gate rather than a
    // sentence. `render::menu_plan` takes the pane and the footer's height, and
    // never the body's split, so a toggle that reshapes the body underneath
    // cannot reach it. The two most likely to be pressed from a menu, `r` and
    // `o`, are exactly the two that reshape it.
    let mut pane = Pane::open("menu-still");
    pane.with(|app, frame, highlighter, history| {
        apply(app, frame, area(), Action::ToggleMenu);
        let (buf, laid) = paint(app, frame, highlighter, history, area());
        // Off the painted cells, not off the published region: the region is where
        // the pointer is told the box is, and the claim is about where it was drawn.
        let (_, first) = drawn(&buf, &laid);

        for setting in SETTINGS {
            apply(app, frame, area(), setting.action());
            let (buf, laid) = paint(app, frame, highlighter, history, area());
            let (_, now) = drawn(&buf, &laid);
            assert_eq!(
                now,
                first,
                "flipping {:?} moved the box under the reader's hand",
                setting.label()
            );
            // Put it back, so each toggle is measured from the same pane.
            apply(app, frame, area(), setting.action());
        }
    });
}

#[test]
fn a_pane_too_short_scrolls_and_says_how_many_rows_it_is_hiding() {
    let mut pane = Pane::open("menu-short");
    pane.with(|app, frame, highlighter, history| {
        // Tall enough for a box, too short for nine names.
        let short = Rect::new(0, 0, WIDE, 12);
        apply(app, frame, short, Action::ToggleMenu);
        let (buf, laid) = paint(app, frame, highlighter, history, short);
        let (text, _) = drawn(&buf, &laid);
        let drawn_rows = SETTINGS
            .iter()
            .filter(|setting| text.contains(setting.label()))
            .count();
        assert!(
            drawn_rows < SETTINGS.len(),
            "the pane was not short enough to hide a row:\n{text}"
        );
        assert!(
            text.contains(&format!("of {}", SETTINGS.len())),
            "the menu is hiding rows and its title bar does not say so:\n{text}"
        );

        // And the window follows the caret rather than dropping what is under it.
        for _ in 0..SETTINGS.len() {
            apply(app, frame, short, Action::MenuMove(1));
        }
        let (buf, laid) = paint(app, frame, highlighter, history, short);
        let (text, _) = drawn(&buf, &laid);
        assert!(
            text.contains(SETTINGS[SETTINGS.len() - 1].label()),
            "the caret reached the last row and the window did not follow:\n{text}"
        );
        assert!(
            text.contains(CARET),
            "the caret is off the drawn window:\n{text}"
        );
    });
}

#[test]
fn the_menu_draws_inside_the_pane_at_forty_columns() {
    let mut pane = Pane::open("menu-narrow");
    pane.with(|app, frame, highlighter, history| {
        let narrow = Rect::new(0, 0, 40, TALL);
        apply(app, frame, narrow, Action::ToggleMenu);
        let (buf, laid) = paint(app, frame, highlighter, history, narrow);
        let (text, at) = drawn(&buf, &laid);
        assert!(
            at.x + at.width <= narrow.width,
            "the box runs off a forty-column pane"
        );
        for setting in SETTINGS {
            assert!(
                text.contains(setting.label()),
                "{:?} is cut at forty columns:\n{text}",
                setting.label()
            );
        }
        // I6's floor takes the legend's shorter rung rather than dropping it.
        assert!(
            text.lines().last().is_some_and(|edge| edge.contains("Esc")),
            "the bottom edge names no keys at forty columns:\n{text}"
        );
    });
}

/// Every pane the box could be asked to draw in, including the ones below its
/// floor.
///
/// The bounds are the drawer's, not the product's: `SPEC.md` I6 names forty
/// columns, and `menu_plan` is asked for a box at every size a terminal can be,
/// so the region between one column and forty is exactly where an underflow or a
/// rect past the buffer would hide from every other gate here.
const SWEEP_WIDTHS: std::ops::RangeInclusive<u16> = 1..=144;
const SWEEP_HEIGHTS: std::ops::RangeInclusive<u16> = 1..=40;

#[test]
fn the_box_never_leaves_the_pane_at_any_size_a_terminal_can_be() {
    let mut pane = Pane::open("menu-sweep");
    pane.with(|app, frame, highlighter, history| {
        apply(app, frame, area(), Action::ToggleMenu);
        let (mut drew, mut declined) = (0usize, 0usize);
        for w in SWEEP_WIDTHS {
            for h in SWEEP_HEIGHTS {
                let at = Rect::new(0, 0, w, h);
                let (buf, laid) = paint(app, frame, highlighter, history, at);
                let Some(menu) = laid.menu else {
                    declined += 1;
                    continue;
                };
                drew += 1;
                assert!(
                    menu.left + menu.width <= w && menu.top + menu.height <= h,
                    "at {w}x{h} the box runs from ({}, {}) for {}x{}, past the pane",
                    menu.left,
                    menu.top,
                    menu.width,
                    menu.height
                );
                // The header owns row zero and the footer owns the rows under the
                // body, so a box over either is a box the reader cannot read past.
                assert!(menu.top >= 1, "at {w}x{h} the box covers the header row");
                // And what was published is what was painted, which is `drawn`'s
                // own assertion. It also proves the box is really on the screen
                // rather than only in a rect.
                let (_, painted) = drawn(&buf, &laid);
                assert_eq!(
                    (painted.x, painted.y, painted.width, painted.height),
                    (menu.left, menu.top, menu.width, menu.height),
                    "at {w}x{h} the painted box is not the published one"
                );
            }
        }
        // Non-vacuity, both ways: a sweep that drew nothing would pass every
        // assertion above, and one that drew everywhere would never reach the
        // floor this is aimed at.
        assert!(
            drew > 0 && declined > 0,
            "the sweep drew {drew} boxes and declined {declined}, so it never              crossed the floor it exists to cross"
        );
    });
}

#[test]
fn every_label_is_one_column_per_byte() {
    // `render::MENU_LABEL` sizes the label field with `str::len`, which is bytes,
    // because `width_of` is not const. That is only the same number while every
    // label is ASCII, and this is what says so.
    for setting in SETTINGS {
        assert!(
            setting.label().is_ascii(),
            "{:?} is not ASCII, so the label field is sized in bytes and drawn in \
             columns",
            setting.label()
        );
    }
}
