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
    /// Move the **pinned file list's** window by this many rows, negative for up.
    ///
    /// Its own action rather than a mode on [`Action::Scroll`], because
    /// `SPEC.md` §11.1 keeps the list not navigable: there is no focus to be in
    /// and no selection to move, so a key means one thing whatever the screen is
    /// showing. What moves is a window over a map; the diff does not move at all.
    ScrollList(isize),
    /// Move the viewport by whole screens, negative for up.
    Page(isize),
    /// Go to the first changed file.
    Top,
    /// Go to the last changed file.
    Bottom,
    /// Engage follow mode, or disengage it.
    ///
    /// Its own action rather than a flag on the others, because `SPEC.md`
    /// §11.1 makes re-engaging a jump as well as a state change: `f` moves to
    /// the newest change rather than waiting for the next one.
    ToggleFollow,
    /// Draw again with no state change, which is what a resize needs.
    Redraw,
}

impl Action {
    /// Whether this is the reader moving the viewport themselves.
    ///
    /// `SPEC.md` §11.1 hangs follow mode on this: a manual scroll disengages
    /// it, so that following never fights a reader mid-read.
    ///
    /// Written as an exhaustive match rather than a `matches!` list on
    /// purpose. The two are identical today and differ the moment an action is
    /// added: this one stops compiling and asks, where the list would answer
    /// "does not disengage" on its own and be right about half the time.
    pub fn is_manual_scroll(self) -> bool {
        match self {
            Self::Scroll(_) | Self::Page(_) | Self::Top | Self::Bottom => true,
            // A resize moves no viewport and expresses no intent, and a pane
            // beside an agent is resized constantly, so treating it as a
            // scroll would disengage follow mode for free. `ToggleFollow` is
            // the reader asking for the opposite of disengaging, and quitting
            // has nothing left to disengage from.
            //
            // **`ScrollList` is a ruling rather than an oversight**, and this
            // exhaustive match is what forced it to be made. Follow is a claim
            // about the *diff* viewport; moving a window over the map of it
            // expresses no intent about what the diff should show, exactly as a
            // resize does not. Browsing the changed set while the diff goes on
            // following what an agent is writing is the monitor behaviour, and
            // the two would fight if one disengaged the other. `SPEC.md` §11.1.
            Self::Quit | Self::Redraw | Self::ToggleFollow | Self::ScrollList(_) => false,
        }
    }

    /// Whether applying this needs to know how tall the body is.
    ///
    /// Only [`Action::Page`] does: it is the one action measured in screens
    /// rather than in rows. Everything else is given the height and ignores it.
    ///
    /// This exists because the answer is **expensive**, not because it is
    /// interesting. Deriving the height costs an uncached terminal-size syscall
    /// plus a `Chrome`, and since the shell began draining a whole gesture into
    /// one paint there can be sixty-four actions between two frames. Paying it
    /// per action would put the syscall back on the path the drain took it off.
    ///
    /// Exhaustive rather than a `matches!` list, for the reason
    /// [`Action::is_manual_scroll`] gives: a new action stops this compiling and
    /// asks, where a list would silently answer "no" and be wrong the one time
    /// it mattered.
    pub fn needs_height(self) -> bool {
        match self {
            Self::Page(_) => true,
            Self::Scroll(_) | Self::Top | Self::Bottom | Self::ScrollList(_) => false,
            Self::Quit | Self::Redraw | Self::ToggleFollow => false,
        }
    }
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

    // **Before the plain arrow arms, or `Shift-↓` falls through to a diff
    // scroll.** The letters below would still work, so the defect would be one
    // binding silently doing the other's job on terminals that report modifiers
    // and nothing at all to see on terminals that do not.
    if key.modifiers.contains(KeyModifiers::SHIFT) {
        if let KeyCode::Down | KeyCode::Up = key.code {
            let rows = if key.code == KeyCode::Down { 1 } else { -1 };
            return Some(Action::ScrollList(rows));
        }
    }

    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => Some(Action::Quit),
        KeyCode::Down | KeyCode::Char('j') => Some(Action::Scroll(1)),
        KeyCode::Up | KeyCode::Char('k') => Some(Action::Scroll(-1)),
        // Shift is the modifier because the alternatives are all taken or
        // unreliable: `Ctrl-J` is LF, `Ctrl-C` and `Ctrl-D` already quit, and
        // Alt is intercepted by terminal emulators and by macOS Option. `G`
        // below has already taught a reader that case is load bearing here.
        //
        // A plain letter as well as the arrow, because a terminal that never
        // reports a modified arrow would otherwise have no way in at all.
        KeyCode::Char('J') => Some(Action::ScrollList(1)),
        KeyCode::Char('K') => Some(Action::ScrollList(-1)),
        KeyCode::PageDown | KeyCode::Char(' ') => Some(Action::Page(1)),
        KeyCode::PageUp => Some(Action::Page(-1)),
        KeyCode::Home | KeyCode::Char('g') => Some(Action::Top),
        KeyCode::End | KeyCode::Char('G') => Some(Action::Bottom),
        // Lower case only, and `G` above is why. `g`/`G` already mean two
        // different things here, so a reader has been taught that shift
        // matters, and folding case would hand `F` a meaning nobody asked for
        // next to a key where case is load bearing.
        KeyCode::Char('f') => Some(Action::ToggleFollow),
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
