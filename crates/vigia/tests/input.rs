//! The key and mouse map, as a table.
//!
//! `SPEC.md` §4 puts "scroll (keyboard + mouse wheel)" in scope and nothing else
//! about input, so what this file gates is narrow and worth stating: every event
//! the terminal can deliver resolves to exactly one intention or to none, and the
//! events that mean nothing to a monitor stay meaning nothing. A shell that
//! redrew on key releases and mouse movement would have an idle cost, which is
//! I1's whole subject.
//!
//! It is a separate binary from `render.rs` because the question is different:
//! this one has no buffer, no area and no view.

use ratatui::crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use std::time::{Duration, Instant};

use vigia::{
    Action, Grabbed, Held, Hovered, LIST_SETTLED, Region, Regions, SCROLL_LINGER, STEP_DELAY,
    STEP_REPEAT, Sheet, TRACK_SCALE, WHEEL_ROWS, action_for, drag_action, hover_after,
    hover_repainted, patience, scroll_mark, settled,
};
use vigia_core::{HISTORY_SAMPLE, HISTORY_WINDOW, History};

/// A key press with no modifiers, which is what a terminal sends for a letter.
fn press(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn with(modifiers: KeyModifiers, code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, modifiers))
}

fn wheel(kind: MouseEventKind) -> Event {
    Event::Mouse(MouseEvent {
        kind,
        column: 4,
        row: 9,
        modifiers: KeyModifiers::NONE,
    })
}

#[test]
fn every_way_out_is_a_way_out() {
    // Three, because three habits exist. `q` is the pager reflex, `Esc` is the
    // dialog reflex, and Ctrl-C is what a reader does when they have decided the
    // program is not listening. In raw mode Ctrl-C is not a signal at all, it is
    // an ordinary key event, so if this map drops it nothing else catches it.
    for event in [
        press(KeyCode::Char('q')),
        press(KeyCode::Esc),
        with(KeyModifiers::CONTROL, KeyCode::Char('c')),
        with(KeyModifiers::CONTROL, KeyCode::Char('d')),
    ] {
        assert_eq!(
            action_for(&event, Regions::default()),
            Some(Action::Quit),
            "{event:?} did not quit, so a reader pressing it would be stuck in the \
             alternate screen"
        );
    }
}

#[test]
fn arrows_and_their_vi_equivalents_agree() {
    for event in [press(KeyCode::Down), press(KeyCode::Char('j'))] {
        assert_eq!(
            action_for(&event, Regions::default()),
            Some(Action::Scroll(1)),
            "{event:?}"
        );
    }
    for event in [press(KeyCode::Up), press(KeyCode::Char('k'))] {
        assert_eq!(
            action_for(&event, Regions::default()),
            Some(Action::Scroll(-1)),
            "{event:?}"
        );
    }
    for event in [press(KeyCode::PageDown), press(KeyCode::Char(' '))] {
        assert_eq!(
            action_for(&event, Regions::default()),
            Some(Action::Page(1)),
            "{event:?}"
        );
    }
    assert_eq!(
        action_for(&press(KeyCode::PageUp), Regions::default()),
        Some(Action::Page(-1))
    );
    for event in [press(KeyCode::Home), press(KeyCode::Char('g'))] {
        assert_eq!(
            action_for(&event, Regions::default()),
            Some(Action::Top),
            "{event:?}"
        );
    }
    for event in [press(KeyCode::End), press(KeyCode::Char('G'))] {
        assert_eq!(
            action_for(&event, Regions::default()),
            Some(Action::Bottom),
            "{event:?}"
        );
    }
}

#[test]
fn f_toggles_follow_and_shift_f_does_not() {
    // The key the mockup published, and the one nobody would guess: `q` and
    // `jk` are pager reflexes and `f` is not.
    assert_eq!(
        action_for(&press(KeyCode::Char('f')), Regions::default()),
        Some(Action::ToggleFollow)
    );
    // Case matters here because it already matters next door: `g` and `G` are
    // two different actions, so folding case would hand `F` a meaning by
    // accident beside a key where the distinction is load bearing.
    assert_eq!(
        action_for(&press(KeyCode::Char('F')), Regions::default()),
        None,
        "shift-f did something, next to a key map where `g` and `G` differ"
    );
}

#[test]
fn d_and_u_are_the_half_page_and_ctrl_d_still_quits() {
    // `less`'s own bindings, and the collision this map has to survive: `Ctrl-D`
    // is one of four ways out, so a plain `d` gaining a meaning is one arm's
    // ordering away from making the quit key scroll instead. The CONTROL branch
    // returns before the plain match, and this is what says so.
    assert_eq!(
        action_for(&press(KeyCode::Char('d')), Regions::default()),
        Some(Action::HalfPage(1))
    );
    assert_eq!(
        action_for(&press(KeyCode::Char('u')), Regions::default()),
        Some(Action::HalfPage(-1))
    );
    assert_eq!(
        action_for(
            &with(KeyModifiers::CONTROL, KeyCode::Char('d')),
            Regions::default()
        ),
        Some(Action::Quit),
        "a plain `d` took the quit key with it"
    );

    // The two the issue refused, held as refusals rather than left to chance.
    // `Ctrl-U` is not a second spelling of `u`: nothing here rebinds a control
    // key to a scroll. `D` and `U` are unbound for the reason `F` is, one key
    // map over from `g` and `G`.
    for event in [
        with(KeyModifiers::CONTROL, KeyCode::Char('u')),
        press(KeyCode::Char('D')),
        press(KeyCode::Char('U')),
    ] {
        assert_eq!(
            action_for(&event, Regions::default()),
            None,
            "{event:?} became an action, on a map where case and control are both \
             load bearing"
        );
    }
}

#[test]
fn n_and_p_are_the_file_step() {
    // The granularity between a row and the whole diff, and the unit the pinned
    // list draws. Both letters were free: no search exists to claim `n`, and a
    // `less` reader carries the next-file reflex from `:n`/`:p` already.
    assert_eq!(
        action_for(&press(KeyCode::Char('n')), Regions::default()),
        Some(Action::File(1))
    );
    assert_eq!(
        action_for(&press(KeyCode::Char('p')), Regions::default()),
        Some(Action::File(-1))
    );

    // Held as refusals rather than left to chance, exactly as `D` and `U` are
    // one test up: `g`/`G` already teach that case is load bearing here, so an
    // upper-case letter that quietly gained a meaning would be a surprise this
    // map has avoided everywhere else.
    for event in [press(KeyCode::Char('N')), press(KeyCode::Char('P'))] {
        assert_eq!(
            action_for(&event, Regions::default()),
            None,
            "{event:?} became an action, on a map where case is load bearing"
        );
    }
}

#[test]
fn the_digits_cover_the_settled_cap_and_stop_there() {
    // **The gate that makes the restated bound safe.** `input.rs` spells the
    // digits `'1'..='6'` rather than importing `LIST_SETTLED`, because everything
    // there is a pure function of a key code and reaching into the renderer for a
    // layout constant would end that. The cost of restating is drift, and this is
    // what pays it: move the settled cap and the loop goes red here instead of
    // leaving a row every pane draws with no key that reaches it.
    //
    // **This was `every_row_the_list_can_draw_has_a_digit` and the name stopped
    // being true** ([#160](https://github.com/breferrari/vigia/issues/160)). The
    // list is deeper than the settled cap on a pane of 28 rows or more, and those
    // rows are addressed by `J`/`K`, `n`/`p` and the pointer rather than by a
    // digit. What the digits cover is the rows **every** pane drawing a list has,
    // so a digit means the same thing at every height, and that is the claim this
    // gate holds now.
    for row in 0..LIST_SETTLED {
        let digit = char::from_digit(row as u32 + 1, 10).expect("a digit for the row");
        assert_eq!(
            action_for(&press(KeyCode::Char(digit)), Regions::default()),
            Some(Action::ListRow(row as u16)),
            "`{digit}` does not name row {row}, which the list draws"
        );
    }

    // And the boundary in the other direction. `0` has no row to name because
    // rows are counted from one on screen, and the digit past the settled cap
    // names a row that some panes draw and most do not. Both stay **unbound**
    // rather than becoming out-of-range jumps: an unbound key is no action at all,
    // where a bound one is a jump that lands nowhere and spends the reader's
    // follow mode doing it, and a key live only above some pane height would be
    // the intermittent affordance §11.1 refuses one region over.
    //
    // **This is the only place either is asserted, deliberately.** The inert list
    // in `nothing_a_reader_did_not_ask_for_becomes_an_action` holds keys with no
    // home of their own, which is why `D`, `U`, `N` and `P` are not in it either.
    // Restating `'7'` there would hardcode what this loop derives, so moving the
    // settled cap would redden a test named for idle cost and send the next reader
    // to the wrong file to find out why.
    let past = char::from_digit(LIST_SETTLED as u32 + 1, 10).expect("a digit past the cap");
    for digit in ['0', past] {
        assert_eq!(
            action_for(&press(KeyCode::Char(digit)), Regions::default()),
            None,
            "`{digit}` became an action, and it names no row the list can draw"
        );
    }
}

#[test]
fn only_the_actions_that_move_the_viewport_disengage_follow() {
    // `SPEC.md` §11.1 hangs follow mode on this split, and both sides are a way
    // for I5 to be quietly wrong rather than loudly broken. Too eager and a
    // resize switches following off for free, on a pane that is resized
    // constantly. Too lax and a reader who scrolled away gets dragged back on
    // the next write.
    for action in [
        Action::Scroll(1),
        Action::Scroll(-1),
        Action::Page(1),
        Action::Page(-1),
        Action::HalfPage(1),
        Action::HalfPage(-1),
        // A file step moves the diff, so it belongs on this side whichever end
        // it is pressed at: `Top` at the top and `Bottom` at the last file are
        // already here and already move nothing.
        Action::File(1),
        Action::File(-1),
        Action::Top,
        Action::Bottom,
    ] {
        assert!(
            action.is_manual_scroll(),
            "{action:?} does not disengage follow mode, so it drags the reader \
             back on the next write"
        );
    }

    for action in [Action::Quit, Action::Redraw, Action::ToggleFollow] {
        assert!(
            !action.is_manual_scroll(),
            "{action:?} disengages follow mode, and none of these moved a viewport"
        );
    }
}

#[test]
fn the_wheel_scrolls_both_ways_by_the_same_amount() {
    // Symmetry is the claim. A wheel that moves three rows down and one up feels
    // broken in a way that is hard to name and easy to ship.
    assert_eq!(
        action_for(&wheel(MouseEventKind::ScrollDown), Regions::default()),
        Some(Action::Scroll(WHEEL_ROWS))
    );
    assert_eq!(
        action_for(&wheel(MouseEventKind::ScrollUp), Regions::default()),
        Some(Action::Scroll(-WHEEL_ROWS))
    );
}

#[test]
fn a_key_release_is_not_a_keypress() {
    // Windows reports press and release; Unix terminals report press only. Acting
    // on both doubles every keystroke on exactly one platform, which is a bug
    // that only ever reproduces for the person who cannot debug it.
    for kind in [KeyEventKind::Release, KeyEventKind::Repeat] {
        let event = Event::Key(KeyEvent::new_with_kind(
            KeyCode::Char('j'),
            KeyModifiers::NONE,
            kind,
        ));
        let action = action_for(&event, Regions::default());
        if kind == KeyEventKind::Release {
            assert_eq!(
                action, None,
                "a release scrolled, so every keystroke moves twice on Windows"
            );
        } else {
            // A held key repeating *should* scroll: that is what holding it is
            // for. Asserted here so the release filter cannot be widened into
            // one that also swallows repeats.
            assert_eq!(action, Some(Action::Scroll(1)));
        }
    }
}

#[test]
fn nothing_a_reader_did_not_ask_for_becomes_an_action() {
    // Each of these is delivered by a real terminal in ordinary use, and every
    // one of them turning into a frame is how a monitor acquires an idle cost.
    //
    // **`Moved` is the one with a ruling behind it, so read `SPEC.md` §11.2 B10
    // before deciding this line is stale.** It is delivered because the mouse
    // bundle sets `?1003h`, any-event tracking, which nothing here consumes and
    // which cannot portably be switched off; `RULINGS.md`'s I1 section carries
    // what it costs. B10 was reversed on 2026-08-16 and a hover mark is adopted,
    // which does **not** make this line stale in either direction: §11.1 rules
    // the mark to be view state resolved in the loop, so `Moved` means no action
    // here whether or not the mark has been built yet.
    //
    // `FocusGained` and `FocusLost` sit below for the same reason and will keep
    // a second one once #186 lands. How often they arrive today is per platform
    // rather than rare: `TAKEOVER` does not request `?1004h`, so on Unix they
    // never arrive at all, while on Windows the console API delivers a
    // `FOCUS_EVENT` on every focus change whether or not anyone asked. Once
    // #186 asks, `FocusLost` is what clears a hover mark, and clearing it is the
    // *loop's* job. An arm for either one here would be that job in the wrong
    // module.
    //
    // **This fixture cannot be the whole tripwire, and the sibling below is
    // why.** `Regions::default()` is a screen with no region and no bars, so a
    // hover arm written the way the click arm above it is written — gated on
    // `over_list` — returns `None` here and leaves this green. The proof that
    // the fixture is doing the work is one line down: `Down(Left)` is asserted
    // inert here and is emphatically not inert in production. So this list
    // catches only a region-blind hover, and
    // [`pointer_motion_over_a_laid_out_screen_is_still_no_action`] catches the
    // one anybody would actually build.
    let inert = [
        wheel(MouseEventKind::Moved),
        wheel(MouseEventKind::Down(MouseButton::Left)),
        wheel(MouseEventKind::Up(MouseButton::Left)),
        wheel(MouseEventKind::Drag(MouseButton::Left)),
        wheel(MouseEventKind::ScrollLeft),
        wheel(MouseEventKind::ScrollRight),
        Event::FocusGained,
        Event::FocusLost,
        Event::Paste("pasted".to_owned()),
        press(KeyCode::Tab),
        press(KeyCode::Enter),
        press(KeyCode::Char('x')),
        with(KeyModifiers::CONTROL, KeyCode::Char('q')),
    ];
    for event in inert {
        assert_eq!(
            action_for(&event, Regions::default()),
            None,
            "{event:?} became an action, so the shell redraws for it"
        );
    }
}

/// `SPEC.md` §11.2 B9, and it needs its own test rather than a line in the inert
/// list above.
///
/// > B9 — a yank key over OSC 52. Ruled 2026-08-15: no.
///
/// That list holds keys with **no home of their own**, which is why `D`, `U`,
/// `N` and `P` are not in it. `y` has one: a ruling refused it, on the ground
/// that it would be the first key on this map to destroy something the reader
/// owns, with no way for the program to confirm it happened. A key that is
/// refused for a reason is not the same thing as a key nobody thought of, and
/// the difference is exactly what a later reader needs.
///
/// **This is the gate B9 was missing.** Its sibling,
/// `legibility.rs::a_drawn_path_carries_no_escape_sequence_of_its_own`, holds
/// B8 by forbidding an escape inside a cell, and cannot hold this one: an OSC 52
/// write draws nothing, touches no cell, and leaves the buffer identical, so the
/// whole rendering suite stays green while the ruling is violated. The keymap is
/// where B9 is decidable, because a clipboard write needs a key to arrive on.
///
/// **What it reaches and what it does not.** It reaches the shape anyone would
/// actually build, which is a plain `y`, and the two neighbouring spellings the
/// map's own conventions would suggest next. It does not reach a clipboard write
/// hung on a key that already has a meaning, and nothing here can: that would be
/// a change to an existing binding's behaviour rather than a new binding, and
/// `only_the_actions_that_move_the_viewport_disengage_follow` is the closest
/// thing to a guard over it.
#[test]
fn the_yank_key_is_refused_rather_than_unbound() {
    for event in [
        press(KeyCode::Char('y')),
        press(KeyCode::Char('Y')),
        with(KeyModifiers::CONTROL, KeyCode::Char('y')),
    ] {
        assert_eq!(
            action_for(&event, Regions::default()),
            None,
            "{event:?} became an action, and `SPEC.md` §11.2 B9 refused the yank \
             key rather than leaving it unassigned"
        );
    }
}

/// `SPEC.md` §11.2 B10's tripwire, and the reason the inert list above is not
/// it.
///
/// > B10 — a hover highlight on what the pointer is over. Ruled 2026-08-15: no.
/// > Reversed 2026-08-16: yes.
///
/// **This gate outlived the reversal unchanged, and that is the thing to
/// understand before touching it.** It was written to catch a hover being built
/// while B10 said no. B10 now says yes, and the assertion is still exactly
/// right, because §11.1 rules that **hover is view state and never an action**:
/// the mark is resolved from `Regions` in the loop and drawn, the way a pressed
/// step button is, and `action_for` never learns about it. So what this forbids
/// is not the feature, it is the feature arriving in the wrong place. A gate
/// that forbids a *mechanism* survives its ruling changing sign; one that
/// forbids an *outcome* would have had to be deleted here.
///
/// **The fixture is the other half.** [`nothing_a_reader_did_not_ask_for_becomes_an_action`]
/// hands `action_for` a `Regions::default()`, which is a screen with no list, no
/// diff and no bar, so every region-gated arm in the map returns `None` against
/// it whatever it does in production — `Down(Left)` sits in that same list and
/// puts the diff at a file on a real screen. A hover written the way the click
/// arm is written, `Moved if regions.over_list(row)`, is therefore invisible
/// there. Here the screen is laid out, and the pointer is placed over the list,
/// over the diff and on the scrollbar column in turn: the three places a hover
/// would have something to say.
///
/// **What this does and does not hold.** It holds that pointer motion produces
/// no *action*. It does **not** hold that motion produces no *paint*, and
/// nothing in this suite does: `vigia::run` draws once per drained batch whether
/// or not any wake in it produced an action, so the honest count today is one
/// paint per motion batch rather than zero. `RULINGS.md`'s I1 section carries
/// that finding and [#154](https://github.com/breferrari/vigia/issues/154)
/// tracks it; #123's exit criterion asked for a zero-paints gate and this is
/// deliberately not one, because writing it today would assert something untrue.
/// The reversal does not change that either: hover adds no paint, so the count
/// it would have to assert is the same one.
#[test]
fn pointer_motion_over_a_laid_out_screen_is_still_no_action() {
    for (place, row) in [("the list", 2), ("the diff", 9), ("the scrollbar", 6)] {
        let event = at(MouseEventKind::Moved, 79, row);
        assert_eq!(
            action_for(&event, two_regions()),
            None,
            "motion over {place} became an action. `SPEC.md` §11.2 B10 adopts a \
             hover mark, so this is not a stale refusal: §11.1 rules the mark to \
             be view state resolved in the loop, never a keymap entry, and a \
             hover arm inside `action_for` gives B4's `nothing is remembered` a \
             second meaning to answer for"
        );
    }
}

#[test]
fn a_shifted_key_still_carries_its_meaning() {
    // `G` cannot be typed without shift, and a terminal reports the modifier
    // alongside it. Only CONTROL is special-cased in the map, so this is really
    // asserting that the special case did not grow to swallow every modifier:
    // filtering on `modifiers.is_empty()` anywhere would make the End binding
    // unreachable on every terminal that reports SHIFT, which is all of them.
    assert_eq!(
        action_for(
            &with(KeyModifiers::SHIFT, KeyCode::Char('G')),
            Regions::default()
        ),
        Some(Action::Bottom)
    );
    assert_eq!(
        action_for(&with(KeyModifiers::SHIFT, KeyCode::End), Regions::default()),
        Some(Action::Bottom)
    );
    // ALT is not a modifier this map assigns meaning to, so an alt-arrow keeps
    // the arrow's meaning rather than becoming inert.
    assert_eq!(
        action_for(&with(KeyModifiers::ALT, KeyCode::Down), Regions::default()),
        Some(Action::Scroll(1))
    );
}

#[test]
fn a_resize_redraws_without_moving_anything() {
    // A resize changes what fits without changing where the reader is, so it is
    // the one event that has to cost a frame and must not cost a scroll.
    assert_eq!(
        action_for(&Event::Resize(40, 12), Regions::default()),
        Some(Action::Redraw)
    );
}

#[test]
fn shift_scrolls_the_list_by_letter_and_by_arrow() {
    // Two bindings for one intention, and both are needed. A terminal that never
    // reports a modified arrow would leave `Shift-↓` indistinguishable from `↓`,
    // so the letters are the path that always works; a reader whose hands are on
    // the arrows should not have to learn a letter to use the region.
    //
    // `SPEC.md` §11.1 rules Shift as the modifier: `Ctrl-J` is LF, `Ctrl-C` and
    // `Ctrl-D` already quit, and Alt is intercepted by terminal emulators and by
    // macOS Option.
    for event in [
        press(KeyCode::Char('J')),
        with(KeyModifiers::SHIFT, KeyCode::Down),
    ] {
        assert_eq!(
            action_for(&event, Regions::default()),
            Some(Action::ScrollList(1)),
            "{event:?} did not scroll the list down"
        );
    }
    for event in [
        press(KeyCode::Char('K')),
        with(KeyModifiers::SHIFT, KeyCode::Up),
    ] {
        assert_eq!(
            action_for(&event, Regions::default()),
            Some(Action::ScrollList(-1)),
            "{event:?} did not scroll the list up"
        );
    }
}

#[test]
fn a_shifted_arrow_does_not_fall_through_to_the_diff() {
    // The specific defect the ordering in `key_action` exists to prevent: the
    // plain `Down` arm matches whatever the modifiers are, so a shifted arrow
    // tested *after* it silently scrolls the diff instead of the list. Worth its
    // own gate rather than trusting the test above, because that one would pass
    // on a terminal shape where the letters carry it.
    assert_ne!(
        action_for(
            &with(KeyModifiers::SHIFT, KeyCode::Down),
            Regions::default()
        ),
        Some(Action::Scroll(1)),
        "Shift-Down fell through to a diff scroll"
    );
    assert_ne!(
        action_for(&with(KeyModifiers::SHIFT, KeyCode::Up), Regions::default()),
        Some(Action::Scroll(-1)),
        "Shift-Up fell through to a diff scroll"
    );

    // And the unshifted arrows still mean the diff, which is the direction a
    // careless fix breaks.
    assert_eq!(
        action_for(&press(KeyCode::Down), Regions::default()),
        Some(Action::Scroll(1))
    );
    assert_eq!(
        action_for(&press(KeyCode::Up), Regions::default()),
        Some(Action::Scroll(-1))
    );
}

#[test]
fn scrolling_the_list_is_not_a_manual_scroll() {
    // `SPEC.md` §11.1's ruling, held where the code reads it. Follow is a claim
    // about the diff viewport; moving a window over the map of it expresses no
    // intent about what the diff should show, exactly as a resize does not.
    //
    // Asserted beside the actions that *do* disengage, because the value of this
    // is the contrast: a gate saying only "list scrolling returns false" would
    // pass against a predicate that returned false for everything.
    assert!(!Action::ScrollList(1).is_manual_scroll());
    assert!(!Action::ScrollList(-1).is_manual_scroll());
    assert!(Action::Scroll(1).is_manual_scroll());
    assert!(Action::Page(1).is_manual_scroll());
    assert!(Action::HalfPage(1).is_manual_scroll());
    assert!(Action::Bottom.is_manual_scroll());

    // And it is not measured in screens, so the loop never pays for a terminal
    // size to apply one. Asserted beside two that are, for the same reason the
    // block above is a contrast rather than a single claim.
    assert!(!Action::ScrollList(1).needs_height());
    assert!(Action::Page(1).needs_height());
    assert!(Action::HalfPage(1).needs_height());
}

/// A screen with a pinned list on rows 1..4, a diff on 5..20, and bars at 79.
///
/// **One bare bar and one stepped one, which is the geometry `render::regions`
/// actually produces for these heights.** The list's three rows are below the
/// step floor, so its track is its region and every gate written before there
/// were buttons still reads the rows it always read. The diff's fifteen are
/// above it, so rows 5 and 19 are its buttons and its track is 6..19.
///
/// Stating both here rather than in each test is deliberate: a fixture that gave
/// every region a track equal to itself would be a screen the renderer cannot
/// draw, and gates written against it would pass while agreeing with nothing.
/// The same screen with both bars in `column`.
///
/// **A helper because the column is per region now**
/// ([#251](https://github.com/breferrari/vigia/issues/251)), where a fixture used
/// to say `bar: Some(60)` once. Every screen these gates describe is the stacked
/// layout, where the two bars share the pane's right edge, so moving them
/// together is what "the bar moved" means here.
fn bars_at(regions: Regions, column: u16) -> Regions {
    Regions {
        list: Region {
            bar: Some(column),
            ..regions.list
        },
        diff: Region {
            bar: Some(column),
            ..regions.diff
        },
        ..regions
    }
}

/// The same screen with no bar drawn in either region.
fn without_bars(regions: Regions) -> Regions {
    Regions {
        list: Region {
            bar: None,
            ..regions.list
        },
        diff: Region {
            bar: None,
            ..regions.diff
        },
        ..regions
    }
}

fn two_regions() -> Regions {
    Regions {
        list: Region::bare(1, 3, 0, 80, Some(79)),
        diff: Region {
            top: 5,
            rows: 15,
            left: 0,
            width: 80,
            track: (6, 13),
            bar: Some(79),
        },
        sheet: None,
    }
}

/// Two regions **beside** each other: one first row, one row count, told apart
/// by their columns alone.
///
/// The layout [#252](https://github.com/breferrari/vigia/issues/252) draws, and
/// the shape every model in this module has to be able to express before that
/// row can be built. The shipped ladder never produces it, which is exactly why
/// it is worth a name: a fixture that only ever appears inline reads as a local
/// quirk of whichever test spelled it, and this is the case three of them share.
///
/// The list takes the left thirty columns with its bar at 29, the diff takes the
/// remaining seventy with its bar at 99. Both start on row 1 and hold 18 rows, so
/// **no row distinguishes them and no assertion here may rest on one**.
fn beside() -> Regions {
    Regions {
        list: Region::bare(1, 18, 0, 30, Some(29)),
        diff: Region::bare(1, 18, 30, 70, Some(99)),
        sheet: None,
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

#[test]
fn the_wheel_scrolls_whichever_region_it_is_over() {
    // The one place this shell reads a pointer's position rather than only its
    // kind. `SPEC.md` §2 makes `btop` the reference and this is what `btop` does:
    // a reader hovering the map and turning the wheel means the map.
    let regions = two_regions();

    for row in 1..4u16 {
        assert_eq!(
            action_for(&at(MouseEventKind::ScrollDown, 10, row), regions),
            Some(Action::ScrollList(1)),
            "row {row} is inside the list and did not scroll it"
        );
        assert_eq!(
            action_for(&at(MouseEventKind::ScrollUp, 10, row), regions),
            Some(Action::ScrollList(-1))
        );
    }

    // The header, the rule and the diff all belong to the diff, which is what a
    // reader gets today and must keep getting.
    for row in [0u16, 4, 5, 12, 19] {
        assert_eq!(
            action_for(&at(MouseEventKind::ScrollDown, 10, row), regions),
            Some(Action::Scroll(WHEEL_ROWS)),
            "row {row} is outside the list and did not scroll the diff"
        );
    }

    // And with no region at all, every row is the diff's, which is the screen
    // before the first paint and on a pane too short to hold a list.
    assert_eq!(
        action_for(&at(MouseEventKind::ScrollDown, 10, 2), Regions::default()),
        Some(Action::Scroll(WHEEL_ROWS))
    );
}

/// One region's bar does not swallow the other region's rows.
///
/// **The most ordinary screen there is, and it had no gate.** A handful of
/// changed files fit the pinned list, so the list has nothing to scroll and draws
/// no bar; the diff is taller than the pane, so it draws one. Both bars sit on
/// the pane's right edge when both exist, so "am I on a bar" was asked as *is
/// this the bar's column*, and once **either** region drew one the whole right
/// column counted as bar from the top of the body to the bottom.
///
/// What that cost is a list row whose rightmost cell stopped answering. Before
/// the per-region columns landed it seeked a bar that is not drawn, because a
/// bar-less region's track is the whole of it and `along` answered for any row in
/// it; after they landed it did nothing at all, because the gate was still the
/// column alone and no region's `along` matched. A click on a file is neither of
/// those.
///
/// `Region::on_bar` is the column **and** the rows, which is what the comment
/// above the gate had always claimed.
#[test]
fn a_bar_in_one_region_leaves_the_others_rows_clickable() {
    let scrolling_diff = Regions {
        // No bar: three files fit, so there is nothing to scroll.
        list: Region::bare(1, 3, 0, 80, None),
        // A bar: the diff runs past the pane.
        diff: Region::bare(5, 15, 0, 80, Some(79)),
        sheet: None,
    };
    // The list's last row, in the column the diff's bar occupies further down.
    let (column, row) = (79, 3);
    // Guarded from the fixture's own numbers rather than through a private
    // predicate: row 3 is inside the list's rows 1..4 and outside the diff's
    // 5..20, which is what makes this the case at all.
    assert!(
        row >= scrolling_diff.list.top
            && row < scrolling_diff.list.top + scrolling_diff.list.rows
            && row < scrolling_diff.diff.top,
        "the fixture is not the case: row {row} has to be the list's and not \
         the diff's"
    );

    assert_eq!(
        action_for(
            &at(MouseEventKind::Down(MouseButton::Left), column, row),
            scrolling_diff
        ),
        Some(Action::ListRow(row - scrolling_diff.list.top)),
        "a click on the last column of a list row did not select that file, so \
         the diff's bar is swallowing rows it does not own"
    );
    assert_eq!(
        scrolling_diff.hover_at(column, row),
        Some(Hovered::Row(row)),
        "the pointer on the last column of a list row marked nothing"
    );

    // And the diff's own bar still answers, or the fix above traded one swallow
    // for the opposite one.
    assert_eq!(
        scrolling_diff.grab_at(column, 8),
        Some(Grabbed::Diff),
        "a press on the diff's bar, on a diff row, no longer takes hold of it"
    );
}

/// A gesture in one region's columns is not a gesture in the other's.
///
/// **The bar gate's sibling, and the larger half of the same assumption**
/// ([#251](https://github.com/breferrari/vigia/issues/251)). Region *membership*
/// was a row test: `over_list` asked `contains(row)`, and the wheel router and
/// the click arm asked it through that. Sound only while the list sits above the
/// diff, which is the vertical stack this model stops assuming.
///
/// Written against two regions that **share every row and differ in columns**,
/// which the shipped layout does not draw today and
/// [#252](https://github.com/breferrari/vigia/issues/252) will. Under the old
/// model every assertion below would answer for the list, because the list's rows
/// are the diff's rows and the row was all anything asked.
#[test]
fn a_gesture_in_one_regions_columns_is_not_the_others() {
    let rail = beside();
    // A row both regions hold, and a column each of them holds alone.
    let row = 7;
    let (in_rail, in_diff) = (4, 60);

    assert_eq!(
        action_for(&at(MouseEventKind::ScrollDown, in_rail, row), rail),
        Some(Action::ScrollList(1)),
        "a wheel over the rail did not scroll the map"
    );
    assert_eq!(
        action_for(&at(MouseEventKind::ScrollDown, in_diff, row), rail),
        Some(Action::Scroll(WHEEL_ROWS)),
        "a wheel over the diff scrolled the map, because the row is the rail's too"
    );

    let press = MouseEventKind::Down(MouseButton::Left);
    assert_eq!(
        action_for(&at(press, in_rail, row), rail),
        Some(Action::ListRow(row - rail.list.top)),
        "a click in the rail did not send the diff to that file"
    );
    assert_eq!(
        action_for(&at(press, in_diff, row), rail),
        None,
        "a click in the diff acted, and B4 rules the diff's own rows inert"
    );

    // And the hover mark follows the same boundary, or the pointer would light a
    // file in a region it is not over.
    assert_eq!(
        rail.hover_at(in_rail, row),
        Some(Hovered::Row(row)),
        "the pointer in the rail did not mark the row it is on"
    );
    assert_eq!(
        rail.hover_at(in_diff, row),
        None,
        "the pointer in the diff marked a listed file"
    );
}

/// A press on one region's bar column is not a press on the other's.
///
/// **The distinction the pane-wide field could not express.** With one column for
/// both regions, which bar a press belonged to was decided by **row**, through
/// `list.along(row)` and `diff.along(row)`. That is the vertical stack written
/// into the hit-test model, and it is what
/// [#252](https://github.com/breferrari/vigia/issues/252) breaks: beside a rail
/// the two regions share their rows, and only the column tells them apart.
///
/// Written against a fixture whose bars are in **different columns**, which the
/// shipped layout does not draw today and the rail will. That is the point: the
/// model has to be able to say it before the layout can.
#[test]
fn a_press_on_one_regions_bar_is_not_the_others() {
    let side_by_side = beside();
    // Same row in both, which is the whole case: under the old model the row was
    // the only thing distinguishing them and here it distinguishes nothing.
    let row = 5;

    assert_eq!(
        side_by_side.grab_at(29, row),
        Some(Grabbed::List),
        "a press on the rail's own bar column did not take hold of the list"
    );
    assert_eq!(
        side_by_side.grab_at(99, row),
        Some(Grabbed::Diff),
        "a press on the diff's own bar column did not take hold of the diff"
    );
    assert_eq!(
        side_by_side.grab_at(64, row),
        None,
        "a press between the two bars took hold of one of them"
    );

    // **And the seek that follows resolves to the same bar**, which is a separate
    // claim from which bar was grabbed and was not covered until a mutation said
    // so: dropping the column check on the list's seek arm passed the whole suite,
    // because on the stacked layout the two regions never share a row and on the
    // rail nothing was asking.
    //
    // **The variant rather than the position**, because which region a seek
    // belongs to is this gate's claim and where along the track it lands is
    // `dragging_a_bar_reports_where_along_its_own_track`'s. Asserting the scaled
    // figure here would restate that gate and couple this one to `TRACK_SCALE`.
    let drag = MouseEventKind::Drag(MouseButton::Left);
    assert!(
        matches!(
            action_for(&at(drag, 29, row), side_by_side),
            Some(Action::ListTo(_))
        ),
        "a drag on the rail's bar did not seek the map"
    );
    assert!(
        matches!(
            action_for(&at(drag, 99, row), side_by_side),
            Some(Action::DiffTo(_))
        ),
        "a drag on the diff's bar seeked the map, because the row is the rail's too"
    );
}

#[test]
fn dragging_a_bar_reports_where_along_its_own_track() {
    // Both bars are in the same column, so which region the row is in decides
    // which one is being dragged. The fraction is over `TRACK_SCALE` because this
    // module has no frame to ask how many files there are.
    let regions = two_regions();

    for kind in [
        MouseEventKind::Down(MouseButton::Left),
        MouseEventKind::Drag(MouseButton::Left),
    ] {
        // Top of the list's track, and the row below it.
        assert_eq!(
            action_for(&at(kind, 79, 1), regions),
            Some(Action::ListTo(0))
        );
        assert_eq!(
            action_for(&at(kind, 79, 2), regions),
            Some(Action::ListTo(TRACK_SCALE / 2))
        );
        // **And the last row of the track reports the full fraction.** Dividing
        // by the row count instead of by the last row's index makes this
        // `2/3`, so the bottom cell of the bar cannot ask for the end and the
        // view stops one step short. Copilot found it on #71; the gates over the
        // resolver missed it because they passed a fraction in directly rather
        // than going through an event.
        assert_eq!(
            action_for(&at(kind, 79, 3), regions),
            Some(Action::ListTo(TRACK_SCALE))
        );
        // Top of the diff's track, its middle, and its end. **Rows 6, 12 and 18
        // rather than 5, 12 and 19**, because this bar is stepped: its first and
        // last rows are buttons and the track between them is what a drag reads.
        // The three claims are the ones this test always made, re-pointed at the
        // rows the thumb now occupies rather than weakened.
        assert_eq!(
            action_for(&at(kind, 79, 6), regions),
            Some(Action::DiffTo(0))
        );
        assert_eq!(
            action_for(&at(kind, 79, 12), regions),
            Some(Action::DiffTo(TRACK_SCALE / 2))
        );
        assert_eq!(
            action_for(&at(kind, 79, 18), regions),
            Some(Action::DiffTo(TRACK_SCALE))
        );
    }

    // Off the bar's column is not a drag, whatever the row.
    assert_eq!(
        action_for(&at(MouseEventKind::Drag(MouseButton::Left), 40, 2), regions),
        None
    );
    // And a screen with no bars has nothing to drag. Pressed on a row *below*
    // the list, because a press inside it is a jump to that file now and would
    // pass this for the wrong reason.
    assert_eq!(
        action_for(
            &at(MouseEventKind::Down(MouseButton::Left), 79, 12),
            without_bars(regions)
        ),
        None
    );
}

#[test]
fn dragging_the_bar_is_checked_before_the_region_under_it() {
    // The bar's column is inside whichever region drew it, so testing the region
    // first would turn every drag into a wheel. Both gestures on the same cell.
    let regions = two_regions();
    assert_eq!(
        action_for(&at(MouseEventKind::ScrollDown, 79, 2), regions),
        Some(Action::ScrollList(1)),
        "the wheel over the bar should still scroll the region it sits in"
    );
    assert_eq!(
        action_for(&at(MouseEventKind::Drag(MouseButton::Left), 79, 2), regions),
        Some(Action::ListTo(TRACK_SCALE / 2)),
        "a drag on the bar became a wheel"
    );
}

#[test]
fn a_press_on_a_step_button_steps_one_row_in_the_region_it_is_in() {
    // #166's affordance. The list's three rows are below the step floor so its bar
    // has no buttons at all; the diff's fifteen are above it, so rows 5 and 19 are
    // its ends. One step per press, and the direction comes from which end.
    let regions = two_regions();
    let press = MouseEventKind::Down(MouseButton::Left);

    assert_eq!(
        action_for(&at(press, 79, 5), regions),
        Some(Action::Scroll(-1)),
        "the diff's up button did not step up one row"
    );
    assert_eq!(
        action_for(&at(press, 79, 19), regions),
        Some(Action::Scroll(1)),
        "the diff's down button did not step down one row"
    );

    // And the row either side of a button is track, not another button, so the
    // buttons are one row each rather than a zone.
    assert_eq!(
        action_for(&at(press, 79, 6), regions),
        Some(Action::DiffTo(0))
    );
    assert_eq!(
        action_for(&at(press, 79, 18), regions),
        Some(Action::DiffTo(TRACK_SCALE))
    );
}

#[test]
fn a_stepped_list_bar_steps_the_map_and_not_the_diff() {
    // The list's own buttons, on a fixture tall enough to have them. **A separate
    // screen rather than a second assertion on `two_regions`**, because the shared
    // one is deliberately below the floor: what is proved here is that the region
    // decides which action, so a bar drawn over the map moves the map.
    let regions = Regions {
        list: Region {
            top: 1,
            rows: 6,
            left: 0,
            width: 80,
            track: (2, 4),
            bar: Some(79),
        },
        diff: Region {
            top: 8,
            rows: 12,
            left: 0,
            width: 80,
            track: (9, 10),
            bar: Some(79),
        },
        sheet: None,
    };
    let press = MouseEventKind::Down(MouseButton::Left);

    assert_eq!(
        action_for(&at(press, 79, 1), regions),
        Some(Action::ScrollList(-1)),
        "the list's up button did not step the map up"
    );
    assert_eq!(
        action_for(&at(press, 79, 6), regions),
        Some(Action::ScrollList(1)),
        "the list's down button did not step the map down"
    );
    // The list's buttons say nothing about the diff and the diff's say nothing
    // about the map. The bar is one column for both regions, so this is the
    // assertion that stops a press being resolved against the wrong one. Only
    // the diff's rows are swept: the list's two are already pinned to exact
    // actions above, and repeating them as a `matches!` would prove less.
    for row in [8u16, 19] {
        let action = action_for(&at(press, 79, row), regions).expect("a step");
        assert!(
            matches!(action, Action::Scroll(_)),
            "a press on the diff's bar at row {row} produced {action:?}"
        );
    }
}

#[test]
fn the_direction_mark_outlives_its_burst_by_exactly_the_linger() {
    // **The comparison this asserts had no gate at all until it was moved out of
    // the shell.** It lived in `Shell::settle_scroll`, which owns a terminal and
    // three threads, so nothing could drive it: inverting it to `now < until`
    // compiled, passed clippy, and left all 824 tests green while the arrows
    // cleared on the next wake instead of `SCROLL_LINGER` later, which is to say
    // never claimed a direction at all.
    let now = Instant::now();
    let until = now + SCROLL_LINGER;

    assert!(
        !settled(Some(until), now),
        "the mark is spent the instant it is armed, so a scroll never draws its \
         direction"
    );
    assert!(
        !settled(Some(until), until - Duration::from_millis(1)),
        "the mark is spent a millisecond early"
    );

    // The boundary is the ordinary case rather than a corner: the loop is woken
    // *by* this deadline, so `patience` hands back exactly zero and the wake
    // lands on `until` itself.
    assert!(
        settled(Some(until), until),
        "a mark whose deadline is exactly now survives, so the wake that came to \
         retire it retires nothing and the arrows outlive their burst"
    );
    assert!(settled(Some(until), until + SCROLL_LINGER));

    // Nothing armed is nothing to settle, which is the case every idle frame is.
    assert!(!settled(None, now));
}

#[test]
fn nothing_held_means_no_timer_at_all() {
    // **The invariant the whole clock is allowed under.** I1's budget is zero
    // wakeups while idle, and what keeps that true is not care taken elsewhere:
    // it is that `Held::wait` hands back `None` when nothing is held, and `None`
    // is an untimed receive. A version that returned some large timeout instead
    // would look harmless, pass every other gate here, and put this program on a
    // poll loop.
    let now = Instant::now();
    assert_eq!(
        Held::wait(None, now),
        None,
        "with nothing held the loop was given a deadline, which is a timer on an \
         idle monitor"
    );

    // And with something held it is bounded by the step that is due, so the loop
    // still blocks rather than spinning.
    let hold = Held::new(Action::Scroll(1), (79, 5), now);
    let patience = Held::wait(Some(hold), now).expect("a held step has a deadline");
    assert!(
        patience > Duration::ZERO && patience <= STEP_DELAY,
        "a held step asked to wait {patience:?}, which is not inside the delay \
         before it first repeats"
    );
}

#[test]
fn a_press_is_one_step_until_the_delay_has_passed() {
    // A click is a click. The first repeat is not due until `STEP_DELAY`, so a
    // reader who presses and lets go inside it has moved exactly one row, which
    // is what the button meant before it could be held.
    let now = Instant::now();
    let hold = Held::new(Action::Scroll(1), (79, 5), now);

    assert_eq!(hold.fire(now), None, "a press repeated immediately");
    assert_eq!(
        hold.fire(now + STEP_DELAY - Duration::from_millis(1)),
        None,
        "a press repeated before its delay was up"
    );
    assert!(
        hold.fire(now + STEP_DELAY).is_some(),
        "a press never repeated at all"
    );
}

#[test]
fn a_late_repeat_folds_into_one_action_rather_than_a_backlog() {
    // **The performance half, and it is a correctness claim rather than a
    // micro-optimisation.** If a repeat that arrives late applied one step and
    // left the rest owed, a pane that stalled for a second would keep scrolling
    // after the reader let go, working off a queue. Folding the elapsed
    // intervals into one `Scroll(n)` makes the rate a fact about time: the same
    // rows per second on a slow terminal as on a fast one, with coarser
    // granularity, and nothing owed.
    let now = Instant::now();
    let hold = Held::new(Action::Scroll(1), (79, 5), now);

    // Exactly on time is one step.
    let (step, next) = hold.fire(now + STEP_DELAY).expect("a repeat");
    assert_eq!(step, Action::Scroll(1));

    // Four intervals late is five steps in one action, not five actions.
    let (step, _) = next
        .fire(now + STEP_DELAY + STEP_REPEAT * 5)
        .expect("a late repeat");
    assert_eq!(
        step,
        Action::Scroll(5),
        "a repeat four intervals late did not fold, so the backlog would outlive \
         the reader's finger"
    );

    // And the list's buttons fold the same way, in their own units.
    let list = Held::new(Action::ScrollList(-1), (79, 1), now);
    let (step, _) = list
        .fire(now + STEP_DELAY + STEP_REPEAT * 2)
        .expect("a repeat");
    assert_eq!(step, Action::ScrollList(-3));
}

#[test]
fn the_clock_advances_by_whole_intervals_so_the_rate_cannot_drift() {
    // The deadline moves by the steps it just took, from the deadline. Moving it
    // to *now plus one interval* instead would let every late tick push the next
    // one later, so a pane under load scrolls slower and slower with the button
    // still down and nothing says why.
    //
    // **Asserted through the following deadline, not through the action**, and
    // that distinction is the gate. A first version of this test checked only
    // that a late tick produced `Scroll(1)`, which is true of both versions:
    // rescheduling from the wake is invisible in the step it returns and shows up
    // only in when the *next* one is allowed. It passed against exactly the
    // mutation it was written to catch.
    let now = Instant::now();
    let slip = Duration::from_millis(3);
    let hold = Held::new(Action::Scroll(1), (79, 5), now);

    // Fire the first repeat late, but by less than one interval, so it is one
    // step either way and only the bookkeeping differs.
    let (step, next) = hold.fire(now + STEP_DELAY + slip).expect("a repeat");
    assert_eq!(step, Action::Scroll(1), "a slipped tick was counted twice");

    // The second is then due on the grid, `STEP_REPEAT` after the first was, and
    // owes nothing to when the first happened to be serviced.
    assert!(
        next.fire(now + STEP_DELAY + STEP_REPEAT).is_some(),
        "the second repeat was pushed back by the first one's slip, so the rate \
         walks downwards under load"
    );
}

#[test]
fn a_repeat_that_falls_far_behind_never_asks_the_loop_to_spin() {
    // `Held::wait` hands the loop a `recv_timeout`, and a deadline left in the
    // past would make that return instantly, over and over: a busy loop wearing a
    // blocking receive. Folding the elapsed intervals is what keeps the next
    // deadline at or after the moment it was computed, however far behind the
    // loop has fallen.
    let now = Instant::now();
    let hold = Held::new(Action::Scroll(1), (79, 5), now);

    // A whole second late, which is twenty intervals.
    let very_late = now + STEP_DELAY + Duration::from_secs(1);
    let (step, next) = hold.fire(very_late).expect("a repeat");
    assert_eq!(
        step,
        Action::Scroll(21),
        "a second of lateness did not fold into one action"
    );
    assert_eq!(
        next.fire(very_late),
        None,
        "the folded repeat left its own deadline in the past, so the loop would \
         wake instantly and fire again"
    );
    assert!(
        Held::wait(Some(next), very_late).is_some_and(|patience| patience > Duration::ZERO),
        "the loop was asked to wait no time at all, which is a spin"
    );
}

#[test]
fn a_hold_ends_on_release_on_a_key_and_on_a_pointer_that_moved() {
    // The five ways out, and the third and the fifth close holes. `Moved` is
    // motion *with no button down*, so it is positive evidence of a release
    // rather than an absence of evidence, and it is what catches an `Up` that
    // never arrived: without it a lost release leaves the loop stepping until
    // something else happens to wake it.
    let regions = two_regions();
    let hold = Held::new(Action::Scroll(-1), (79, 5), Instant::now());

    for (name, event) in [
        (
            "a release",
            at(MouseEventKind::Up(MouseButton::Left), 79, 5),
        ),
        (
            "a release of another button",
            at(MouseEventKind::Up(MouseButton::Right), 79, 5),
        ),
        ("a pointer move", at(MouseEventKind::Moved, 79, 5)),
        ("a key press", press(KeyCode::Char('j'))),
        ("the quit key", press(KeyCode::Char('q'))),
        // **The fifth way, and it is owed to I1 rather than to symmetry.** The
        // clock a hold owns is licensed on the condition that it may not outlive
        // the gesture that armed it, and a reader who has tabbed away is not
        // holding this button in any sense the repeat should honour. Without it
        // the loop keeps stepping and repainting a pane nobody is looking at, on
        // a timer, which is the state I1's measure exists to protect.
        //
        // It became reachable on Unix with #186, which put `Step::FocusChange`
        // in the takeover so `FocusLost` arrives at all; on Windows the console
        // has always delivered it, so this hole was open there the whole time.
        ("the window losing focus", Event::FocusLost),
    ] {
        assert!(
            hold.ends(&event, regions),
            "{name} did not end the hold, so the button would go on stepping"
        );
    }

    // A twitch inside the same cell keeps it, which is why the press's own
    // position is carried, and leaving the control ends it.
    assert!(
        !hold.ends(&at(MouseEventKind::Drag(MouseButton::Left), 79, 5), regions),
        "a drag that never left the button ended the hold"
    );
    assert!(
        hold.ends(
            &at(MouseEventKind::Drag(MouseButton::Left), 79, 12),
            regions
        ),
        "a drag off the button onto the track did not end the hold"
    );
}

#[test]
fn only_a_step_button_arms_a_hold() {
    // The geometry the loop arms from is the geometry the press is resolved
    // through, so the two cannot disagree about where a button is. Everything
    // else on that column, and everything off it, arms nothing.
    let regions = two_regions();

    assert_eq!(regions.step_at(79, 5), Some(Action::Scroll(-1)));
    assert_eq!(regions.step_at(79, 19), Some(Action::Scroll(1)));
    // The track between them is a seek, not a step.
    assert_eq!(regions.step_at(79, 12), None);
    // The list's bar is below the step floor here, so it has no buttons at all.
    assert_eq!(regions.step_at(79, 1), None);
    // And off the bar's column there is nothing to hold.
    assert_eq!(regions.step_at(40, 5), None);
}

#[test]
fn a_step_button_the_sheet_covers_arms_nothing() {
    // **The unit twin of `sheet.rs`'s sweep, and the producer half of a rule this
    // module already had the decider half of.** `SPEC.md` §11.1 rules that a click
    // landing on the sheet does nothing at all, *"falling through would let a click
    // seek a scrollbar the reader cannot see"*, and `action_for` has honoured it
    // since B12. `Regions::step_at` did not: the loop arms a hold from it directly,
    // outside `action_for`, so a press on a covered button armed a repeat that
    // scrolled a region under the sheet.
    //
    // Measured before the guard landed, over widths 30 to 140 against heights 8 to
    // 40: **85 cells** the sheet covered answered a step, at widths 30, 32, 35 and
    // 38, every one of them a pane at or below I6's own forty columns.
    // `tests/sheet.rs::a_press_under_the_sheet_arms_no_step` is that sweep; this is
    // the same claim where the geometry is spelled rather than laid out, so a
    // failure here names the rule and a failure there names the pane.
    let bare = two_regions();
    // The same cells the gate above proves are buttons, so the contrast below is
    // between two answers for one cell rather than between two cells.
    assert_eq!(bare.step_at(79, 5), Some(Action::Scroll(-1)));
    assert_eq!(bare.step_at(79, 19), Some(Action::Scroll(1)));

    // **Covering the top button and not the bottom one**, so the two assertions
    // below are the same bar at two rows rather than a bar and something else.
    let covered = Regions {
        sheet: Some(Sheet {
            left: 70,
            top: 2,
            width: 10,
            height: 6,
            // Off the button on purpose: the close control is its own surface and
            // `hover_at` still answers `Button` for it, where this answers `None`
            // for every cell of the sheet including that one.
            close: (78, 2),
        }),
        ..bare
    };
    assert!(
        covered.sheet.expect("a sheet").covers(79, 5),
        "the fixture's sheet does not cover the button, so this proves nothing"
    );
    assert_eq!(
        covered.step_at(79, 5),
        None,
        "a press on a step button under the sheet armed a hold, so holding it \
         repeats a scroll on a region the reader cannot see"
    );

    // **The close control is not an exception**, and it is asserted rather than
    // left to the blanket above: it is the one cell of the sheet a click does act
    // on, so a reader of the guard will ask. It closes through `action_for`, which
    // is a different path, and it must arm no hold either.
    assert_eq!(
        covered.step_at(78, 2),
        None,
        "the close control armed a hold, which would repeat a step while the \
         sheet it belongs to is being dismissed"
    );

    // **And the bar's other button still answers**, which is what makes the guard
    // bounded by the sheet rather than switched on by its presence. Without this a
    // `step_at` that returned `None` whenever any sheet existed would pass every
    // assertion above while taking the scrollbar away from a reader who has the
    // sheet open on the other side of the pane.
    assert!(
        !covered.sheet.expect("a sheet").covers(79, 19),
        "the fixture's sheet reaches the lower button, so the case below proves \
         nothing"
    );
    assert_eq!(
        covered.step_at(79, 19),
        Some(Action::Scroll(1)),
        "a step button the sheet does not cover stopped answering, so the guard \
         is on the sheet existing rather than on the cell it covers"
    );
}

#[test]
fn a_track_the_sheet_covers_grabs_nothing() {
    // **The sibling of the gate above, and the call site #298's first draft missed.**
    // `Regions::step_at` and `Regions::grab_at` are the only geometry `run`'s loop
    // asks for directly, outside `action_for`. Guarding one and not the other leaves
    // the class open on the half that costs more: a hold repeats a bounded step,
    // where a grab hands the gesture to `drag_action`, which ignores the column by
    // design, so the next motion relocates a region the sheet is covering to
    // wherever the pointer went.
    //
    // The track is also the bigger target. The sheet is centred on both axes, so
    // wherever it reaches a bar's column it covers the rows *between* the buttons as
    // well, which outnumber the two the buttons occupy.
    let bare = two_regions();
    // The track between the two step buttons, which `only_a_step_button_arms_a_hold`
    // proves is a seek rather than a step.
    assert_eq!(bare.grab_at(79, 12), Some(Grabbed::Diff));

    let covered = Regions {
        sheet: Some(Sheet {
            left: 70,
            top: 10,
            width: 10,
            height: 4,
            close: (78, 10),
        }),
        ..bare
    };
    assert!(
        covered.sheet.expect("a sheet").covers(79, 12),
        "the fixture's sheet does not cover the track, so this proves nothing"
    );
    assert_eq!(
        covered.grab_at(79, 12),
        None,
        "a press on a track under the sheet took hold of the bar, so the next \
         drag moves a region the reader cannot see"
    );

    // **The rectangle's own edges, which nothing else probes.** The two guards rest
    // entirely on `Sheet::covers`, and a mutation of either bound from `<` to `<=`
    // over-refuses by a row or a column with every sweep still green: the sweeps
    // walk `cells_of`, whose range carries the same bound, so they move together and
    // neither notices. Named by round 2's mutation battery as a predicted survivor.
    let box_of = covered.sheet.expect("a sheet");
    assert!(box_of.covers(70, 10), "the first cell the sheet occupies");
    assert!(box_of.covers(79, 13), "the last cell the sheet occupies");
    assert!(
        !box_of.covers(80, 13),
        "one past the last column is not the sheet's"
    );
    assert!(
        !box_of.covers(79, 14),
        "one past the last row is not the sheet's"
    );
    assert!(
        !box_of.covers(69, 10),
        "one before the first column is not the sheet's"
    );
    assert!(
        !box_of.covers(79, 9),
        "one before the first row is not the sheet's"
    );

    // And a track row the sheet does not reach still answers, so the guard is
    // bounded by the sheet rather than switched on by its presence.
    assert!(
        !covered.sheet.expect("a sheet").covers(79, 16),
        "the fixture's sheet reaches row 16, so the case below proves nothing"
    );
    assert_eq!(
        covered.grab_at(79, 16),
        Some(Grabbed::Diff),
        "a track row the sheet does not cover stopped answering, so the guard is \
         on the sheet existing rather than on the cell it covers"
    );

    // **The close control is not an exception here either**, and it is spelled out
    // because it is the one cell of the sheet a click acts on, so a reader of the
    // guard will ask.
    //
    // **What it proves is weaker than its first comment claimed, and saying so is
    // the point.** `Sheet::covers` is a rectangle test that never reads `close`, and
    // the guard calls only `covers`, so this refuses because the cell is *covered*
    // and not because it is the control. No fixture can separate the two: `close` is
    // inside its own sheet's rect by construction. It is kept as the case a reader
    // will look for rather than as a discriminating test, and the sibling gate above
    // has the same shape for the same reason.
    let over_close = Regions {
        sheet: Some(Sheet {
            left: 70,
            top: 10,
            width: 10,
            height: 4,
            close: (79, 12),
        }),
        ..bare
    };
    assert_eq!(
        over_close.grab_at(79, 12),
        None,
        "the close control took hold of the bar underneath it, so a drag from it \
         moves a region the reader cannot see"
    );
}

#[test]
fn repeating_an_action_that_does_not_accumulate_leaves_it_alone() {
    // `Action::repeated` is the seam that makes holding general rather than
    // scrollbar-shaped, so what each action does when held is a ruling and this
    // is where they are held to it. Relative row counts multiply; a page held
    // down is the reader's own key repeat and an absolute move has nothing to
    // accumulate.
    assert_eq!(Action::Scroll(1).repeated(4), Action::Scroll(4));
    assert_eq!(Action::Scroll(-1).repeated(3), Action::Scroll(-3));
    assert_eq!(Action::ScrollList(-1).repeated(2), Action::ScrollList(-2));

    for action in [
        Action::Page(1),
        Action::HalfPage(-1),
        Action::File(1),
        Action::Top,
        Action::Bottom,
        Action::ToggleFollow,
        Action::Redraw,
        Action::Quit,
        Action::ListRow(2),
        Action::ListTo(0),
        Action::DiffTo(0),
    ] {
        assert_eq!(
            action.repeated(5),
            action,
            "{action:?} accumulated when held, which is a decision nobody made"
        );
    }
}

#[test]
fn a_drag_onto_a_step_button_is_inert() {
    // **The ruling, and the reason it is one.** A reader who grabbed the thumb and
    // pulled past the end of the track is over a button, and the honest reading of
    // that is *nothing further*: the last track row already reaches the last
    // window, so the view is where they asked for it to be.
    //
    // Stepping on a drag instead would make a press-and-jiggle on a button walk
    // the view a row per twitch, and clamping to the end would teleport it there.
    // Both need to know a drag *began* on a button, which is state, and this
    // module has none by design.
    let regions = two_regions();
    let drag = MouseEventKind::Drag(MouseButton::Left);

    for row in [5u16, 19] {
        assert_eq!(
            action_for(&at(drag, 79, row), regions),
            None,
            "a drag onto the diff's button at row {row} moved something"
        );
    }
    // And the same cell answers a press, so this is the gesture being told apart
    // rather than the row being dead.
    assert_eq!(
        action_for(&at(MouseEventKind::Down(MouseButton::Left), 79, 5), regions),
        Some(Action::Scroll(-1))
    );
}

#[test]
fn a_step_button_inherits_the_follow_rule_of_the_region_it_is_on() {
    // A button is the region's drag by another gesture, so it has to answer follow
    // mode the same way: moving the map expresses no intent about the diff, and
    // moving the diff is a manual scroll.
    //
    // **Driven through `action_for` rather than asserted about the `Action`
    // variants directly**, and that is the whole gate. A version of this test
    // that read `Action::Scroll(-1).is_manual_scroll()` off the enum would be
    // three existing tests restated, and would stay green with the step buttons
    // deleted outright: what has to be checked is that pressing a button *yields*
    // an action carrying the region's own follow rule, not that the enum still
    // has the rule.
    let regions = Regions {
        list: Region {
            top: 1,
            rows: 6,
            left: 0,
            width: 80,
            track: (2, 4),
            bar: Some(79),
        },
        diff: Region {
            top: 8,
            rows: 12,
            left: 0,
            width: 80,
            track: (9, 10),
            bar: Some(79),
        },
        sheet: None,
    };
    let press = MouseEventKind::Down(MouseButton::Left);

    for (name, row, drag) in [
        ("the list's up button", 1u16, Action::ListTo(0)),
        ("the list's down button", 6, Action::ListTo(0)),
        ("the diff's up button", 8, Action::DiffTo(0)),
        ("the diff's down button", 19, Action::DiffTo(0)),
    ] {
        let step = action_for(&at(press, 79, row), regions).expect("a step");
        assert_eq!(
            step.is_manual_scroll(),
            drag.is_manual_scroll(),
            "{name} and a drag on the same bar disagree about follow mode: \
             {step:?} against {drag:?}"
        );
    }
}

#[test]
fn a_list_drag_leaves_the_diff_alone_and_a_diff_drag_does_not() {
    // The two drags differ in exactly the way the keys do: moving the map
    // expresses no intent about the diff, and moving the diff is a manual scroll
    // like any other, so it disengages follow and hands the map back.
    assert!(!Action::ListTo(0).is_manual_scroll());
    assert!(Action::DiffTo(0).is_manual_scroll());
    // And only one of them needs a height. Both map the track onto travel, but
    // the list's travel is its own row count, which the app holds, where the
    // diff's is the total minus a screenful and the screenful is the caller's to
    // measure. `DiffTo` answered `false` here until 2026-08-02, so `apply`
    // received a height of zero and the bottom of the track fell off the end.
    assert!(!Action::ListTo(0).needs_height());
    assert!(Action::DiffTo(0).needs_height());
}

#[test]
fn a_drag_keeps_the_bar_it_started_on_however_far_the_pointer_goes() {
    // **Reported from use: dragging off the bar's column ended the drag.** It
    // should not. A one-column target is not something a hand stays inside while
    // it moves, and `action_for` asks *what is under the pointer*, which is the
    // right question for a press and the wrong one once a reader is already
    // holding something. So a drag under way is resolved by `drag_action`
    // against the region it began on, and the column is not consulted at all.
    let regions = two_regions();

    // Far off the bar in both directions, and against the left edge.
    for column in [0u16, 40, 78, 79, 120] {
        assert_eq!(
            drag_action(
                &at(MouseEventKind::Drag(MouseButton::Left), column, 12),
                regions,
                Grabbed::Diff
            ),
            Some(Action::DiffTo(TRACK_SCALE / 2)),
            "a drag at column {column} stopped tracking the bar it began on"
        );
    }
}

#[test]
fn a_drag_past_either_end_holds_that_end() {
    // Pulling above the track is the first window and below it is the last,
    // which is what every scrollbar does and what a reader dragging past the end
    // is asking for. Without the clamp those rows resolve to nothing and the
    // view stops wherever the pointer last crossed the track, which reads as the
    // drag having come loose.
    let regions = two_regions();

    for row in [0u16, 3, 5, 6] {
        assert_eq!(
            drag_action(
                &at(MouseEventKind::Drag(MouseButton::Left), 40, row),
                regions,
                Grabbed::Diff
            ),
            Some(Action::DiffTo(0)),
            "a drag at row {row}, above the diff's track, did not hold the top"
        );
    }
    for row in [18u16, 19, 23, 200] {
        assert_eq!(
            drag_action(
                &at(MouseEventKind::Drag(MouseButton::Left), 40, row),
                regions,
                Grabbed::Diff
            ),
            Some(Action::DiffTo(TRACK_SCALE)),
            "a drag at row {row}, below the diff's track, did not hold the bottom"
        );
    }
}

#[test]
fn only_a_press_on_a_track_takes_hold_of_a_bar() {
    // What a press *grabs* is the gesture that continues, so it is the track and
    // never the buttons: a button is a step and is answered on the press itself.
    let regions = two_regions();

    assert_eq!(regions.grab_at(79, 12), Some(Grabbed::Diff));
    assert_eq!(regions.grab_at(79, 2), Some(Grabbed::List));
    // The diff's ends are its step buttons, which step rather than grab.
    assert_eq!(regions.grab_at(79, 5), None);
    assert_eq!(regions.grab_at(79, 19), None);
    // And off the bar's column a press grabs nothing, because a drag has to
    // begin somewhere real even though it may end anywhere.
    assert_eq!(regions.grab_at(40, 12), None);
}

#[test]
fn a_repaint_that_moves_the_bars_retires_the_hover_mark() {
    // **The rule that could not be gated until it moved out of `Shell`.** Round
    // one put it in `Shell::hovered`, which owns a `Session` and can therefore
    // never be reached by a test: mutating the body back to a plain field read
    // passed the entire workspace. It is `hover_repainted` now, and this is what
    // that bought.
    //
    // The mark is a claim about the screen it was resolved against. When a tick
    // moves the bars, every cell it named may belong to something else, and
    // §11.1's clearing ladder has an accepted residual where the pointer is no
    // longer there to say otherwise.
    let before = two_regions();
    let mark = Some(Hovered::Button(79, 5));

    // Same layout, same mark: a paint that changed nothing changes nothing.
    assert_eq!(hover_repainted(mark, before, before), mark);

    // **Any change at all retires it, and the rule deliberately does not try to
    // tell one button from another.** The version that did was a tautology:
    // `hover_at` builds its answer out of its own arguments, so re-validating a
    // cell against the new layout reduces to "is this still a button" and is
    // blind to "is this now a *different* button", which is the whole case.
    let grown = Regions {
        list: Region::bare(1, 5, 0, 80, Some(79)),
        ..before
    };
    let moved_bar = bars_at(before, 60);
    let no_bar = without_bars(before);
    for (name, after) in [
        ("the list grew", grown),
        ("the bar moved column", moved_bar),
        ("the bar went away", no_bar),
        ("everything went away", Regions::default()),
    ] {
        assert_eq!(
            hover_repainted(mark, before, after),
            None,
            "{name} and the mark survived, so it is a claim about a screen that \
             is no longer on show"
        );
    }

    // Nothing is not something: a paint with no mark to carry stays empty
    // whatever the layout did.
    assert_eq!(hover_repainted(None, before, grown), None);
    assert_eq!(hover_repainted(None, before, before), None);
}

#[test]
fn a_grip_ends_on_anything_that_is_not_more_of_the_same_drag() {
    // **The one retirement rule no test could drive until #186 moved it.** It
    // sat inline in `run` from #183, and the pass that argued a rule written
    // inline is a rule with no gate had that counterexample one screen above it.
    //
    // Coarser than `Held::ends` on purpose: a grip is already the answer to
    // *what is this gesture about*, so anything that is not more of the same
    // gesture finishes it, and there are no five cases to tell apart.
    assert!(
        !Grabbed::ends(&at(MouseEventKind::Drag(MouseButton::Left), 79, 12)),
        "a left drag ended the grip it is continuing"
    );

    for event in [
        at(MouseEventKind::Up(MouseButton::Left), 79, 12),
        at(MouseEventKind::Moved, 79, 12),
        at(MouseEventKind::Drag(MouseButton::Right), 79, 12),
        at(MouseEventKind::ScrollDown, 79, 12),
        press(KeyCode::Char('q')),
        Event::FocusLost,
    ] {
        assert!(
            Grabbed::ends(&event),
            "{event:?} did not end a grip, so a drag outlives the gesture that \
             started it"
        );
    }
}

#[test]
fn a_hover_resolves_to_a_button_a_bar_or_a_listed_file() {
    // Every surface a click acts on, and nothing else. The fixture's diff has
    // buttons at rows 5 and 19 (its track is 6..18) and its list has none, which
    // is the asymmetry worth testing over: a bare bar must still answer for its
    // track.
    let regions = two_regions();

    // A button answers as the cell it is drawn on, which is the key
    // `Chrome::pressed` already uses, so the drawer compares one kind of thing.
    assert_eq!(regions.hover_at(79, 5), Some(Hovered::Button(79, 5)));
    assert_eq!(regions.hover_at(79, 19), Some(Hovered::Button(79, 19)));

    // A track answers as **its region**, which is the key `Chrome::gripped`
    // uses, because what a hover on this column means is *this bar* and the
    // thumb is what answers. The track and the thumb are one target: a press
    // anywhere on a track seeks.
    assert_eq!(
        regions.hover_at(79, 12),
        Some(Hovered::Track(Grabbed::Diff)),
        "the diff's"
    );
    assert_eq!(
        regions.hover_at(79, 2),
        Some(Hovered::Track(Grabbed::List)),
        "the list's"
    );

    // The list's bar has no buttons at all in this fixture, so every row of it
    // is track. A resolver that assumed both bars were stepped would answer
    // `Button` here and light a cell that is drawing a thumb.
    assert_eq!(regions.hover_at(79, 1), Some(Hovered::Track(Grabbed::List)));
    assert_eq!(regions.hover_at(79, 3), Some(Hovered::Track(Grabbed::List)));

    // **A listed file, off the bar's column**, which is a surface a click acts
    // on because it puts the diff at that file.
    for row in 1..=3 {
        assert_eq!(
            regions.hover_at(40, row),
            Some(Hovered::Row(row)),
            "row {row} of the list did not answer"
        );
    }

    // **The bar's column wins inside the list's own rows**, which is the
    // ordering this resolver has to get right: the scrollbar is drawn inside the
    // region that owns those rows, so asking the list first would mark a file
    // the reader is pointing past.
    assert_eq!(regions.hover_at(79, 2), Some(Hovered::Track(Grabbed::List)));

    // **The diff's body answers nothing**, which §11.1 rules: it is not
    // clickable and a mark would imply it is.
    for row in 5..20 {
        assert_eq!(
            regions.hover_at(40, row),
            None,
            "row {row} of the diff answered a hover, so the mark reaches a body              nothing there is clickable in"
        );
    }

    // Off every region entirely.
    assert_eq!(regions.hover_at(79, 0), None, "above both regions");
    assert_eq!(regions.hover_at(79, 23), None, "below both regions");
    assert_eq!(regions.hover_at(40, 0), None, "above the list");
    assert_eq!(regions.hover_at(40, 23), None, "below the diff");
}

#[test]
fn a_hover_mark_is_retired_by_its_replacement_and_by_focus_lost() {
    // §11.1's three-mark rule at its third case: a hover's subject *moves*
    // rather than ending, so what retires it is the next observation of where
    // the pointer is. This is the whole of `hover_after`, and it is a free
    // function precisely so this test can exist: the loop that owns the state
    // cannot be driven by a test.
    let regions = two_regions();
    let button = Some(Hovered::Button(79, 5));

    // **Arming, from nothing.** The first case and the easiest to leave
    // untested, because every other assertion here starts from a mark that
    // already exists. Without it, `hover_after` gated on `was.is_some()` would
    // be a mark that can never light at all and the whole feature would be
    // invisible on screen, with every other test in this file still green.
    assert_eq!(
        hover_after(&at(MouseEventKind::Moved, 79, 5), regions, None),
        button,
        "a pointer arriving on a button from nowhere did not arm the mark"
    );

    // **Replacement.** Motion onto another target is the ordinary case and the
    // one that makes the residual rung tolerable: the mark follows the pointer
    // for free, per cell, because `?1003h` is any-event tracking.
    assert_eq!(
        hover_after(&at(MouseEventKind::Moved, 79, 19), regions, button),
        Some(Hovered::Button(79, 19))
    );

    // **Motion onto nothing clears it**, which is the same rule and not a second
    // one: the observation said "not over a target", and that is an answer. Row
    // 12 off the bar is the diff's body, which is the one region with nothing
    // clickable in it.
    assert_eq!(
        hover_after(&at(MouseEventKind::Moved, 40, 12), regions, button),
        None
    );

    // **And motion between two different kinds of target replaces rather than
    // clears**, which is worth asserting separately now that there are three:
    // the same event that leaves a button arrives on a bar or on a file.
    assert_eq!(
        hover_after(&at(MouseEventKind::Moved, 79, 12), regions, button),
        Some(Hovered::Track(Grabbed::Diff)),
        "a pointer moving from a button onto the diff's bar lost the mark"
    );
    assert_eq!(
        hover_after(&at(MouseEventKind::Moved, 40, 2), regions, button),
        Some(Hovered::Row(2)),
        "a pointer moving from a button onto a listed file lost the mark"
    );

    // **Every mouse event is an observation, not just `Moved`.** A press, a
    // release, a drag and the wheel all carry a column and a row, so all of them
    // place the mark. Singling out `Moved` would leave it stale through a whole
    // drag, which is exactly when a reader is looking at the bar.
    for kind in [
        MouseEventKind::Down(MouseButton::Left),
        MouseEventKind::Up(MouseButton::Left),
        MouseEventKind::ScrollDown,
    ] {
        assert_eq!(
            hover_after(&at(kind, 79, 19), regions, button),
            Some(Hovered::Button(79, 19)),
            "{kind:?} did not place the mark, so it can only follow a bare move"
        );
    }

    // **A drag is the exception, and it clears rather than places.** Pulling a
    // grabbed thumb travels over the step button at that end of the track;
    // lighting it would promise a step that releasing there does not perform,
    // because `Grabbed` owns the gesture until the button comes up.
    //
    // **Any button, not just the left one**, which is the mutation that survived
    // the first version of this: `Drag(_)` narrowed to `Drag(MouseButton::Left)`
    // passed the whole suite. A drag is a gesture whichever button is down, and
    // `Held::ends` two functions over already takes that view of a release.
    for held in [MouseButton::Left, MouseButton::Right, MouseButton::Middle] {
        assert_eq!(
            hover_after(&at(MouseEventKind::Drag(held), 79, 19), regions, button),
            None,
            "a drag with {held:?} down lit a step button, promising a step the \
             release will not make"
        );
    }

    // **`FocusLost` clears it**, which is the rung `TAKEOVER` gained a step for.
    // Without `Step::FocusChange` this arm never fires on Unix, and a mark left
    // over a window the reader has tabbed away from is what B10 was declined for.
    assert_eq!(hover_after(&Event::FocusLost, regions, button), None);

    // **A key leaves it alone, and this is deliberately not `Held::ends`'s
    // rule.** That one ends a hold on any key, because a hand at the keyboard is
    // evidence a mouse button is not down. It is no evidence at all about where
    // a pointer is *resting*, so clearing here would blink the mark off under a
    // reader scrolling with `j` while their pointer sits on the bar.
    for event in [
        press(KeyCode::Char('j')),
        press(KeyCode::Char('q')),
        Event::FocusGained,
        Event::Resize(80, 24),
    ] {
        assert_eq!(
            hover_after(&event, regions, button),
            button,
            "{event:?} retired a hover mark, and only a pointer or a lost window \
             can say anything about where a pointer is"
        );
    }
}

#[test]
fn a_drag_answers_only_motion() {
    // A release is not a position, so it falls through to the caller that ends
    // the grip rather than being read as one last seek.
    let regions = two_regions();

    for kind in [
        MouseEventKind::Up(MouseButton::Left),
        MouseEventKind::Moved,
        MouseEventKind::ScrollDown,
        MouseEventKind::Down(MouseButton::Left),
    ] {
        assert_eq!(
            drag_action(&at(kind, 79, 12), regions, Grabbed::Diff),
            None,
            "{kind:?} was read as a drag"
        );
    }
}

#[test]
fn a_direction_mark_expires_where_a_held_button_does_not() {
    // The arrows light while the keys scroll, and a key burst has no release: it
    // simply stops sending. So this is the one mark on the bar that needs an
    // expiry, and `SCROLL_LINGER` is it. Without one the last arrow of a burst
    // would stay lit on an idle tree as a claim about the past.
    //
    // §11.1 states the three marks as one rule and it is the reason this test is
    // about *expiry* rather than about arrows: what decides whether a mark needs
    // a clock is whether the program can observe its subject ending. A hold ends
    // with an `Up`. A burst has no end, only a last member, so it needs this. A
    // hover (§11.2 B10, reversed 2026-08-16) has no end either, but its subject
    // *moves* rather than stopping, so the next motion clears it and it must not
    // be given a clock: one would put the mark out while a reader rests on it.
    //
    // The clock is still bounded by the gesture that armed it: nothing schedules
    // it but a scroll, it fires once, and it clears itself.
    assert!(
        SCROLL_LINGER > STEP_REPEAT,
        "the direction mark expires faster than the repeat that drives it, so a \
         held button would blink"
    );
    assert!(
        SCROLL_LINGER < STEP_DELAY,
        "the direction mark outlives the delay before a button repeats, which \
         makes it a claim about the past rather than about now"
    );
}

#[test]
fn nothing_armed_means_no_deadline_at_all() {
    // **The invariant every clock shares, and the one a source gate cannot see.**
    // `Held::wait` already answers for the repeat; this answers for the repeat,
    // the direction mark and the ageing window together, because deadlines asked
    // separately are that many chances to leave one armed on an idle monitor.
    // `None` here is what makes the loop's receive untimed, which is I1's
    // *0 wakeups while idle* as a structural fact rather than as care taken at
    // three call sites.
    let now = Instant::now();

    assert_eq!(
        patience(None, None, None, now),
        None,
        "with nothing held, nothing lingering and nothing in the window the loop \
         was handed a deadline, which is a timer on an idle monitor"
    );

    // Any one alone arms it, and none is allowed to hide the others.
    let hold = Held::new(Action::Scroll(1), (79, 5), now);
    assert_eq!(patience(Some(hold), None, None, now), Some(STEP_DELAY));
    assert_eq!(
        patience(None, Some(now + SCROLL_LINGER), None, now),
        Some(SCROLL_LINGER)
    );
    assert_eq!(
        patience(None, None, Some(HISTORY_SAMPLE), now),
        Some(HISTORY_SAMPLE),
        "a window with something in it did not ask the loop to wake, so the graph \
         freezes where it is"
    );

    // **The nearest of them**, whichever it is, because the loop has to wake for
    // the first thing due. Taking the wrong one lets the others run late.
    assert_eq!(
        patience(Some(hold), Some(now + SCROLL_LINGER), None, now),
        Some(SCROLL_LINGER),
        "the linger is due first and the loop was told to sleep past it"
    );
    let soon = Held::new(Action::Scroll(1), (79, 5), now - STEP_DELAY + STEP_REPEAT);
    assert_eq!(
        patience(Some(soon), Some(now + SCROLL_LINGER), None, now),
        Some(STEP_REPEAT),
        "the step is due first and the loop was told to sleep past it"
    );

    // **And the ageing clock takes its turn in the same order**, in both
    // directions, because it is the slowest of the three by orders of magnitude
    // and a `min` written the wrong way round would be invisible against the
    // other two: they would simply always win.
    //
    // **Here rather than in a gate of its own**, which is what
    // [#277](https://github.com/breferrari/vigia/issues/277)'s plan named. The
    // property is *the nearest deadline wins*, this test is where that property
    // already lives for the other two clocks, and a second gate would have
    // rebuilt the same fixture to assert the same rule about a third. Recorded
    // rather than done silently, because a promise kept somewhere other than
    // where it was promised is indistinguishable from one dropped unless
    // somebody says so.
    assert_eq!(
        patience(None, Some(now + SCROLL_LINGER), Some(HISTORY_SAMPLE), now),
        Some(SCROLL_LINGER),
        "the linger is due long before the next sample and the loop was told to \
         sleep past it"
    );
    assert_eq!(
        patience(
            None,
            Some(now + HISTORY_SAMPLE * 2),
            Some(HISTORY_SAMPLE),
            now
        ),
        Some(HISTORY_SAMPLE),
        "the next sample is due first and the loop was told to sleep past it, so \
         the window ages a beat late"
    );
}

#[test]
fn an_empty_window_and_nothing_held_means_no_timer_at_all() {
    // **[#243](https://github.com/breferrari/vigia/issues/243)'s half of I1's
    // budget, and it gates the *wiring* rather than the store.** What
    // `History::ages_in` answers is gated in `vigia-core`, which owns the type;
    // what matters here is that its answer reaches the one function deciding
    // whether this program owns a timer, and that an empty window therefore
    // leaves the loop's receive untimed.
    //
    // A first draft of this asserted `ages_in` three times and never called
    // `patience`, so despite its name it gated nothing about the input layer and
    // duplicated the core's own gates. The composition is the whole point: both
    // halves can be right while nothing joins them.
    let mut history = History::new();
    let now = Instant::now();

    assert_eq!(
        patience(None, None, history.ages_in(now), now),
        None,
        "an empty window and nothing held handed the loop a deadline, which is a \
         timer on an idle monitor"
    );

    history.record_sized([("src/a.rs", Some(4_000u64))], now);
    let armed = patience(None, None, history.ages_in(now), now)
        .expect("a window holding a write did not arm the loop, so the graph freezes");
    assert!(
        armed <= HISTORY_SAMPLE,
        "a live window asked the loop to sleep {armed:?}, past the sample it is \
         waiting for"
    );

    // And it stops again, which is the bound the amendment rests on.
    history.record_sized([], now + HISTORY_WINDOW);
    assert_eq!(
        patience(
            None,
            None,
            history.ages_in(now + HISTORY_WINDOW),
            now + HISTORY_WINDOW
        ),
        None,
        "a drained window still had the loop on a clock, so it outlives \
         everything it had to show"
    );
}
#[test]
fn each_bar_answers_only_the_keys_that_move_it() {
    // **The routing behind the arrows, and the half a render gate cannot see.**
    // A test that hands the painter a mark checks the drawing; this checks that
    // the right mark is produced, which is where the 0.5.0 defect actually lived
    // once the drawing was fixed.
    //
    // The two regions move different things: `j`/`k`/`d`/`u`/`Space`/`g`/`G` and
    // the file steps move the diff's viewport, `J`/`K` move the list's window.
    let regions = two_regions();
    let (list, diff) = (Grabbed::List, Grabbed::Diff);

    for (action, want) in [
        (Action::Scroll(1), Some((diff, 1))),
        (Action::Scroll(-1), Some((diff, -1))),
        (Action::Page(1), Some((diff, 1))),
        (Action::HalfPage(-1), Some((diff, -1))),
        (Action::File(1), Some((diff, 1))),
        (Action::Top, Some((diff, -1))),
        (Action::Bottom, Some((diff, 1))),
        // The map's own keys, and the assertion that was missing: these must not
        // reach the diff's bar.
        (Action::ScrollList(1), Some((list, 1))),
        (Action::ScrollList(-1), Some((list, -1))),
        // Neither bar. A jump lands somewhere rather than moving by something,
        // and a drag already lights its own thumb.
        (Action::ListRow(2), None),
        (Action::ListTo(0), None),
        (Action::DiffTo(0), None),
        (Action::ToggleFollow, None),
        (Action::Redraw, None),
        (Action::Quit, None),
        // A step of nothing is not a direction.
        (Action::Scroll(0), None),
    ] {
        assert_eq!(
            scroll_mark(action, regions),
            want,
            "{action:?} lit the wrong bar, or the wrong way, or a bar at all"
        );
    }
}

#[test]
fn a_region_with_no_rows_lights_nothing() {
    // **A mark nobody can see still costs a wake**, which is what this guard is
    // for now and is not what it was for.
    //
    // Until [#254](https://github.com/breferrari/vigia/issues/254) the mark was
    // the region's first row and the danger was a *collision*: with no list on
    // screen the diff starts where the list would have, both reported top 1, and
    // an unguarded `ScrollList` lit the **diff's** arrows for a movement of a map
    // nobody can see. The mark names its region now, so that confusion cannot be
    // spelled at all, and deleting this guard would draw nothing wrong.
    //
    // It stays because the drawing is not the only consumer. `Shell` arms
    // `scrolling_until` on every mark it is handed, so a mark naming a bar that
    // is not drawn buys a `SCROLL_LINGER` timer, a wake, and a repaint that
    // changes no cell. I1 is what that spends, and the `None` below is what does
    // not spend it.
    let regions = Regions {
        list: Region::bare(1, 0, 0, 80, Some(79)),
        diff: Region::bare(1, 20, 0, 80, Some(79)),
        sheet: None,
    };
    assert_eq!(
        regions.list.top, regions.diff.top,
        "the fixture is not the case"
    );

    assert_eq!(
        scroll_mark(Action::ScrollList(1), regions),
        None,
        "scrolling a list that is not drawn armed a linger clock and bought a \
         wake for a mark with no bar to draw it on"
    );
    assert_eq!(
        scroll_mark(Action::Scroll(1), regions),
        Some((Grabbed::Diff, 1))
    );
}

#[test]
fn a_mark_names_its_region_when_both_share_a_first_row() {
    // **The [`beside`] shape, now pointed at the paint marks.** Two tests above
    // already drive it, which is worth stating because an earlier draft of this
    // comment claimed the file drew no such screen: it draws it twice, for the
    // *region geometry* [#251](https://github.com/breferrari/vigia/issues/251)
    // fixed. What no test drove it for is the three marks, which is the whole of
    // [#254](https://github.com/breferrari/vigia/issues/254) and the reason the
    // assumption survived that pass.
    //
    // Every mark that named a region by its top collapses here, and on every
    // layout that ships `list.top < diff.top`, so a top and a region are the same
    // answer and the wrong one cannot be told from the right one.
    let regions = beside();
    assert_eq!(
        (regions.list.top, regions.list.rows),
        (regions.diff.top, regions.diff.rows),
        "the fixture is not the case this test exists for"
    );

    // **The routing half.** `J` moves the map's window and `j` moves the diff's
    // viewport, and under the old encoding both answered `1`.
    assert_eq!(
        scroll_mark(Action::ScrollList(1), regions),
        Some((Grabbed::List, 1)),
        "the map's own key named a region by a row both regions start on"
    );
    assert_eq!(
        scroll_mark(Action::Scroll(1), regions),
        Some((Grabbed::Diff, 1)),
        "the diff's key named a region by a row both regions start on"
    );

    // **The hover half**, one bar column each, which is the coordinate that
    // actually separates them on this layout.
    assert_eq!(
        regions.hover_at(29, 10),
        Some(Hovered::Track(Grabbed::List)),
        "a pointer on the rail's own bar marked the diff's"
    );
    assert_eq!(
        regions.hover_at(99, 10),
        Some(Hovered::Track(Grabbed::Diff)),
        "a pointer on the diff's own bar marked the rail's"
    );
}

#[test]
fn shift_r_and_ctrl_r_are_unbound() {
    // **The pattern this file already applies to `F`, `D`, `U`, `N` and `P`**, and
    // `r` arrived with `SPEC.md` §11.2 B14 without it
    // ([#295](https://github.com/breferrari/vigia/issues/295)). A key map where `g`
    // and `G` mean different things has to say which capitals are deliberate, and
    // a rail toggled by a mis-shifted keystroke is the kind of thing a reader
    // reports as the pane rearranging itself on its own.
    assert_eq!(
        action_for(&press(KeyCode::Char('R')), Regions::default()),
        None,
        "shift-r did something, next to a key map where `g` and `G` differ"
    );
    assert_eq!(
        action_for(
            &with(KeyModifiers::CONTROL, KeyCode::Char('r')),
            Regions::default()
        ),
        None,
        "ctrl-r did something, where the control arm binds only `c` and `d`"
    );
}

// **What covers the arrows beyond the keymap, and why nothing else is written
// here** ([#296](https://github.com/breferrari/vigia/issues/296)). The gate below
// proves `→` resolves to the same `Action` as `n`, and `App::apply` has **one** arm
// for `Action::File`, so what the arrows do to the screen is exactly what the file
// step does and is owned by the gates that already pin it:
//
// - `scroll.rs::n_and_p_step_one_file_and_land_on_its_heading` steps and lands
// - `scroll.rs::the_file_step_stops_at_both_ends` is the inertness at both ends,
//   and it covers a case a from-scratch gate omitted
// - `list.rs::the_window_is_overtaken_when_the_diff_leaves_it` is the map being
//   handed back to the caret from a window a reader had taken over
//
// A 106-line gate driving `Action::File` from a fixture was written first and
// deleted: it never pressed an arrow, so it could not tell one from `n`, and each
// of its five claims already had an owner. **Measured rather than argued**: the
// mutation that stops `App::apply` handing the map back is killed by the `list.rs`
// gate above with the new one removed.

#[test]
fn the_arrows_move_between_files() {
    // **`SPEC.md` §11.2 B15**: vertical keys move inside the diff, horizontal keys
    // move between files, so `↑` `↓` `←` `→` are one complete pair rather than half
    // of one. `←` is `p` and `→` is `n`.
    //
    // Asserted **against the keys they alias** rather than against a literal
    // `Action`, which is what stops the alias drifting from its original: change
    // what `n` means and this goes red rather than pinning the old meaning under a
    // new key.
    assert_eq!(
        action_for(&press(KeyCode::Right), Regions::default()),
        action_for(&press(KeyCode::Char('n')), Regions::default()),
        "`→` does not do what `n` does"
    );
    assert_eq!(
        action_for(&press(KeyCode::Left), Regions::default()),
        action_for(&press(KeyCode::Char('p')), Regions::default()),
        "`←` does not do what `p` does"
    );
    // **No literal pair below, and the first draft had one.** Its stated reason was
    // that two keys both bound to `None` would satisfy the asserts above, which is
    // false in this suite: `n_and_p_are_the_file_step` pins `n` and `p` against
    // `Action::File` literally, so `n` cannot go unbound without that going red.
    // Adding the literal here would also be the exact thing the comment above
    // refuses, pinning today's meaning under a new key.
}

#[test]
fn the_arrows_under_modifiers_do_not_reach_the_list() {
    // **The `SHIFT` block above the plain arms binds `Up` and `Down` only**, and
    // falls through for everything else. So `Shift+←` reaches the plain arm and
    // steps a file, where `Shift+↓` is intercepted and scrolls the pinned list.
    // That asymmetry is real, it is a consequence of the block's own `_ => {}`,
    // and nothing stated it until #296 put keys on the other axis.
    //
    // Pinned in the direction that matters: a shifted horizontal arrow must not
    // become a *list* gesture, because that is the confusion this row exists to
    // end. Whether it steps a file or does nothing is a smaller question; today it
    // steps, and this says so rather than leaving it to be discovered.
    for (code, plain) in [
        (KeyCode::Left, Action::File(-1)),
        (KeyCode::Right, Action::File(1)),
    ] {
        // `Some(plain)` is the whole claim: it is a `File` step, so it is by
        // construction not a `ScrollList`. The first draft asserted both and the
        // second could never fire.
        assert_eq!(
            action_for(&with(KeyModifiers::SHIFT, code), Regions::default()),
            Some(plain),
            "a shifted horizontal arrow does not fall through to the plain arm, so \
             it may have reached the pinned list, which is the confusion #296 \
             exists to end"
        );
        assert_eq!(
            action_for(&with(KeyModifiers::CONTROL, code), Regions::default()),
            None,
            "a control-arrow did something, where the control arm binds only `c` \
             and `d`"
        );
    }
}
