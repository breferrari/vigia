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
use vigia::{Action, WHEEL_ROWS, action_for};

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
            action_for(&event),
            Some(Action::Quit),
            "{event:?} did not quit, so a reader pressing it would be stuck in the \
             alternate screen"
        );
    }
}

#[test]
fn arrows_and_their_vi_equivalents_agree() {
    for event in [press(KeyCode::Down), press(KeyCode::Char('j'))] {
        assert_eq!(action_for(&event), Some(Action::Scroll(1)), "{event:?}");
    }
    for event in [press(KeyCode::Up), press(KeyCode::Char('k'))] {
        assert_eq!(action_for(&event), Some(Action::Scroll(-1)), "{event:?}");
    }
    for event in [press(KeyCode::PageDown), press(KeyCode::Char(' '))] {
        assert_eq!(action_for(&event), Some(Action::Page(1)), "{event:?}");
    }
    assert_eq!(action_for(&press(KeyCode::PageUp)), Some(Action::Page(-1)));
    for event in [press(KeyCode::Home), press(KeyCode::Char('g'))] {
        assert_eq!(action_for(&event), Some(Action::Top), "{event:?}");
    }
    for event in [press(KeyCode::End), press(KeyCode::Char('G'))] {
        assert_eq!(action_for(&event), Some(Action::Bottom), "{event:?}");
    }
}

#[test]
fn the_wheel_scrolls_both_ways_by_the_same_amount() {
    // Symmetry is the claim. A wheel that moves three rows down and one up feels
    // broken in a way that is hard to name and easy to ship.
    assert_eq!(
        action_for(&wheel(MouseEventKind::ScrollDown)),
        Some(Action::Scroll(WHEEL_ROWS))
    );
    assert_eq!(
        action_for(&wheel(MouseEventKind::ScrollUp)),
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
        let action = action_for(&event);
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
            action_for(&event),
            None,
            "{event:?} became an action, so the shell redraws for it"
        );
    }
}

#[test]
fn a_resize_redraws_without_moving_anything() {
    // A resize changes what fits without changing where the reader is, so it is
    // the one event that has to cost a frame and must not cost a scroll.
    assert_eq!(action_for(&Event::Resize(40, 12)), Some(Action::Redraw));
}
