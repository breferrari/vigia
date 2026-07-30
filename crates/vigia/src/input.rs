//! Terminal events to intentions, as a pure function.
//!
//! Nothing here touches the screen or the repository, which is what makes the
//! whole key map a table test. The loop in [`crate::run`] does the acting; this
//! module only decides what was asked for.

use ratatui::crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind,
};

/// Rows a wheel notch moves.
///
/// Three is what a terminal sends per physical notch when it reports lines
/// rather than pixels, so matching it makes one notch feel like one notch.
pub const WHEEL_ROWS: isize = 3;

/// What the reader asked the shell to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Leave.
    Quit,
    /// Move the viewport by this many rows, negative for up.
    Scroll(isize),
    /// Move the viewport by whole screens, negative for up.
    Page(isize),
    /// Go to the first changed file.
    Top,
    /// Go to the last changed file.
    Bottom,
    /// Draw again with no state change, which is what a resize needs.
    Redraw,
}

/// The intention behind one terminal event, or `None` if there was not one.
///
/// Most events mean nothing to a monitor: key releases, plain mouse movement,
/// focus changes, pasted text. Returning `None` for them is not a gap, it is
/// the point. A monitor that redrew on every event the terminal can produce
/// would be a timer with extra steps.
pub fn action_for(event: &Event) -> Option<Action> {
    match event {
        Event::Key(key) => key_action(key),
        Event::Mouse(mouse) => mouse_action(mouse),
        // A resize changes what fits, so the frame has to be rebuilt even
        // though no state moved.
        Event::Resize(_, _) => Some(Action::Redraw),
        Event::FocusGained | Event::FocusLost | Event::Paste(_) => None,
    }
}

fn key_action(key: &KeyEvent) -> Option<Action> {
    // Windows reports press *and* release; Unix terminals report press only.
    // Acting on both would double every keystroke on one platform and not the
    // other, which is the kind of bug that only ever reproduces for one person.
    //
    // Only releases are dropped, not everything that is not a press. A terminal
    // speaking the kitty keyboard protocol reports auto-repeat as its own kind,
    // and a held arrow key has to keep scrolling: that is what holding it means.
    if key.kind == KeyEventKind::Release {
        return None;
    }

    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return match key.code {
            // Ctrl-C is handled here rather than by a signal handler. In raw
            // mode the terminal does not translate it into SIGINT at all, so it
            // arrives as an ordinary key event; I8's "restored on SIGINT" is
            // about the signal a *non-raw* terminal would have sent, and is
            // issue #8's to prove.
            KeyCode::Char('c') | KeyCode::Char('d') => Some(Action::Quit),
            _ => None,
        };
    }

    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => Some(Action::Quit),
        KeyCode::Down | KeyCode::Char('j') => Some(Action::Scroll(1)),
        KeyCode::Up | KeyCode::Char('k') => Some(Action::Scroll(-1)),
        KeyCode::PageDown | KeyCode::Char(' ') => Some(Action::Page(1)),
        KeyCode::PageUp => Some(Action::Page(-1)),
        KeyCode::Home | KeyCode::Char('g') => Some(Action::Top),
        KeyCode::End | KeyCode::Char('G') => Some(Action::Bottom),
        _ => None,
    }
}

fn mouse_action(mouse: &MouseEvent) -> Option<Action> {
    match mouse.kind {
        MouseEventKind::ScrollDown => Some(Action::Scroll(WHEEL_ROWS)),
        MouseEventKind::ScrollUp => Some(Action::Scroll(-WHEEL_ROWS)),
        // Everything else is deliberately inert. Horizontal wheels exist and
        // lines do not pan: the renderer clips instead, which is what I6 asks
        // for. Clicks and drags do nothing because nothing is selectable in a
        // monitor, and plain movement is not an event worth a frame.
        _ => None,
    }
}
