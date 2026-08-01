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
use vigia::{Action, Regions, TRACK_SCALE, WHEEL_ROWS, action_for};

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
    assert!(Action::Bottom.is_manual_scroll());

    // And it is not measured in screens, so the loop never pays for a terminal
    // size to apply one.
    assert!(!Action::ScrollList(1).needs_height());
}

/// A screen with a pinned list on rows 1..4, a diff on 5..20, and bars at 79.
fn two_regions() -> Regions {
    Regions {
        list: (1, 3),
        diff: (5, 15),
        bar: Some(79),
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
            Some(Action::ListTo(TRACK_SCALE / 3))
        );
        // Top of the diff's track, and its middle.
        assert_eq!(
            action_for(&at(kind, 79, 5), regions),
            Some(Action::DiffTo(0))
        );
        assert_eq!(
            action_for(&at(kind, 79, 12), regions),
            Some(Action::DiffTo((7 * TRACK_SCALE) / 15))
        );
    }

    // Off the bar's column is not a drag, whatever the row.
    assert_eq!(
        action_for(&at(MouseEventKind::Drag(MouseButton::Left), 40, 2), regions),
        None
    );
    // And a screen with no bars has nothing to drag.
    assert_eq!(
        action_for(
            &at(MouseEventKind::Down(MouseButton::Left), 79, 2),
            Regions {
                bar: None,
                ..regions
            }
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
        Some(Action::ListTo(TRACK_SCALE / 3)),
        "a drag on the bar became a wheel"
    );
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
