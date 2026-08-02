//! Terminal events to intentions, as a pure function.
//!
//! Nothing here touches the screen or the repository, which is what makes the
//! whole key map a table test. The loop in [`crate::run`] does the acting; this
//! module only decides what was asked for.

use ratatui::crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};

/// Rows a wheel notch moves.
///
/// Three is what a terminal sends per physical notch when it reports lines
/// rather than pixels, so matching it makes one notch feel like one notch.
pub const WHEEL_ROWS: isize = 3;

/// Where the screen's regions are, so a pointer can be told what it is over.
///
/// **The one thing this module knows about layout, and it is passed in rather
/// than derived.** Everything else here is a pure function of a key code, which
/// is what makes the whole map a table test; a mouse gesture cannot be, because
/// "scroll the thing under the pointer" is a question about the screen. Handing
/// it the three numbers keeps the decision here and the arithmetic testable.
///
/// Rows are absolute within the pane. `Default` is a screen with no region and
/// no bars, which is what a caller that has not laid out yet should say.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Regions {
    /// First row of the pinned list, and how many rows it has. Zero rows means
    /// there is no region and every gesture belongs to the diff.
    pub list: (u16, u16),
    /// First row of the diff region, and how many rows it has.
    pub diff: (u16, u16),
    /// The column both scrollbars are drawn in, when either is.
    pub bar: Option<u16>,
}

impl Regions {
    /// Whether `row` is inside the pinned list.
    fn over_list(self, row: u16) -> bool {
        let (top, rows) = self.list;
        rows > 0 && row >= top && row < top + rows
    }

    /// How far down a region's track `row` sits, as a fraction over
    /// [`TRACK_SCALE`], or `None` when it is not on that track.
    fn along(self, row: u16, region: (u16, u16)) -> Option<u32> {
        let (top, rows) = region;
        if rows == 0 || row < top || row >= top + rows {
            return None;
        }
        // **Divided by the last row's index, not by the row count.** Over `rows`
        // the last row yields `(rows - 1) / rows`, which is short of the full
        // fraction by one row's worth and therefore can never ask for the end:
        // the pointer sits on the bottom cell of the track and the view stops a
        // step early. That is the same defect as mapping a track onto the whole
        // instead of onto its travel, arriving one layer down, and the gates for
        // that one missed it because they called the resolver with a fraction
        // rather than going through a real event.
        //
        // A one-row track has no second position to express, and `regions` never
        // publishes a bar for one, so it reports the top rather than dividing by
        // zero.
        let travel = u32::from(rows - 1);
        if travel == 0 {
            return Some(0);
        }
        Some((u32::from(row - top) * TRACK_SCALE) / travel)
    }
}

/// Resolution a drag reports its position at.
///
/// A fraction rather than a row, because the caller converts it against a count
/// this module does not have: how many files there are is the frame's business.
/// Scaled rather than floating point, for the reason the rest of this crate is:
/// the same input has to produce the same answer on every target.
pub const TRACK_SCALE: u32 = 1 << 16;

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
    /// Put the pinned list's window at this fraction of the changed set.
    ///
    /// From dragging or clicking the list's own scrollbar. A fraction over
    /// [`TRACK_SCALE`] rather than a file index, because the index needs the file
    /// count and this module does not have one.
    ListTo(u32),
    /// Put the diff at the file this many rows down the pinned list.
    ///
    /// From a click on a list row. An **offset into the window**, not a file
    /// index, because the window's position is the app's state and this module
    /// does not have it.
    ///
    /// A click is the one gesture a reader will try without being told, and it
    /// is still not selection: nothing is remembered, no row becomes special,
    /// and the next event is interpreted exactly as it would have been. That is
    /// what `SPEC.md` §11.2 B4 refuses, and it is untouched. The same argument
    /// already licensed dragging a scrollbar.
    ListRow(u16),
    /// Put the diff at this fraction of its total height.
    ///
    /// From dragging or clicking the diff's scrollbar. **A row, once the caller
    /// resolves it**, not a file: I4 was narrowed on 2026-08-01 precisely so the
    /// diff's total row count could be counted rather than approximated, and a
    /// gesture performed on a row-exact readout has to land as precisely as the
    /// readout claims.
    DiffTo(u32),
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
            // Dragging the **list's** bar moves the map and not the diff, so it
            // is `ScrollList` by another input device. Dragging the **diff's**
            // moves the viewport and is a manual scroll like any other.
            Self::ListTo(_) => false,
            // A click on a row moves the diff, so it is a manual scroll for the
            // same reason a drag on the diff's bar is.
            Self::DiffTo(_) | Self::ListRow(_) => true,
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
            // A page steps by a screenful, and a drag on the diff's bar maps the
            // track onto everything *but* the last screenful, so both need to
            // know how tall one is. `ListTo` does not: the list's travel is its
            // own row count, which the app already holds.
            Self::Page(_) | Self::DiffTo(_) => true,
            Self::Scroll(_) | Self::Top | Self::Bottom | Self::ScrollList(_) => false,
            Self::ListTo(_) | Self::ListRow(_) => false,
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
pub fn action_for(event: &Event, regions: Regions) -> Option<Action> {
    match event {
        Event::Key(key) => key_action(key),
        Event::Mouse(mouse) => mouse_action(mouse, regions),
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
        match key.code {
            KeyCode::Down => return Some(Action::ScrollList(1)),
            KeyCode::Up => return Some(Action::ScrollList(-1)),
            _ => {}
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

fn mouse_action(mouse: &MouseEvent, regions: Regions) -> Option<Action> {
    // **The bar is checked before the region it sits in.** A press on the
    // scrollbar column is a gesture about position, and the same column is inside
    // whichever region drew it, so testing the region first would turn every drag
    // into a wheel.
    let on_bar = regions.bar == Some(mouse.column);
    if on_bar
        && matches!(
            mouse.kind,
            MouseEventKind::Down(MouseButton::Left) | MouseEventKind::Drag(MouseButton::Left)
        )
    {
        if let Some(at) = regions.along(mouse.row, regions.list) {
            return Some(Action::ListTo(at));
        }
        if let Some(at) = regions.along(mouse.row, regions.diff) {
            return Some(Action::DiffTo(at));
        }
        return None;
    }

    match mouse.kind {
        // **The wheel scrolls whatever it is over**, which is the one place this
        // shell reads a pointer's position rather than only its kind. A reader
        // hovering the map and turning the wheel means the map; `SPEC.md` §2
        // makes `btop` the reference and that is what `btop` does.
        MouseEventKind::ScrollDown if regions.over_list(mouse.row) => Some(Action::ScrollList(1)),
        MouseEventKind::ScrollUp if regions.over_list(mouse.row) => Some(Action::ScrollList(-1)),
        MouseEventKind::ScrollDown => Some(Action::Scroll(WHEEL_ROWS)),
        MouseEventKind::ScrollUp => Some(Action::Scroll(-WHEEL_ROWS)),
        // **A click on a listed file sends the diff to it.** The row is reported
        // as an offset into the window; the app owns where the window is. Only
        // the list, because the diff below is already showing what it is showing
        // and a click on it would have nothing to mean.
        MouseEventKind::Down(MouseButton::Left) if regions.over_list(mouse.row) => {
            Some(Action::ListRow(mouse.row - regions.list.0))
        }
        // Everything else is deliberately inert. Horizontal wheels exist and
        // lines do not pan: the renderer clips instead, which is what I6 asks
        // for. A click on the diff does nothing, because nothing there is
        // selectable in a monitor and §11.2 B4 keeps it that way, and plain
        // movement is not an event worth a frame.
        _ => None,
    }
}
