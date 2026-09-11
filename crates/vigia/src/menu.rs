//! The pane's own settings, drawn where a reader can see them.

use ratatui::crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};

use crate::input::{Action, Sheet};

/// One toggle the menu draws.
///
/// Each variant carries both the label and the action that flips it, so the row
/// a reader clicks and the key they could press instead cannot come apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Setting {
    /// `f`.
    Follow,
    /// `r`.
    Rail,
    /// `s`.
    Single,
    /// `o`.
    Overview,
    /// `a`.
    Staged,
    /// `w`.
    Wrap,
    /// `c`.
    Notes,
    /// No key; the config file alone.
    Icons,
    /// No key either.
    Links,
}

/// Every toggle, in the order the menu draws them.
///
/// The order is the reader's drawing: what the body is made of first, then what
/// a listed path carries. `b` is absent because where the pane stands is a
/// state rather than a setting, which is `SPEC.md` §11.2 B22's rule.
pub const SETTINGS: [Setting; 9] = [
    Setting::Follow,
    Setting::Rail,
    Setting::Single,
    Setting::Overview,
    Setting::Staged,
    Setting::Wrap,
    Setting::Notes,
    Setting::Icons,
    Setting::Links,
];

/// What the state cell spells when a setting is on.
pub const ON: &str = "on";

/// And when it is off.
pub const OFF: &str = "off";

/// Columns the state cell always takes, whichever word is in it.
///
/// Fixed rather than measured: `on` and `off` are different widths, and a field
/// that moved under the eye each time a row was flipped would be worse than one
/// that does not.
pub const STATE_WIDTH: usize = 3;

impl Setting {
    /// What the row spells.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Follow => "follow the newest change",
            Self::Rail => "left rail",
            Self::Single => "one file only",
            Self::Overview => "the file list alone",
            Self::Staged => "staged changes",
            Self::Wrap => "wrap long lines",
            Self::Notes => "the note rows",
            Self::Icons => "file icons",
            Self::Links => "path links",
        }
    }

    /// The action that flips it, which is the same action its key sends.
    #[must_use]
    pub const fn action(self) -> Action {
        match self {
            Self::Follow => Action::ToggleFollow,
            Self::Rail => Action::ToggleRail,
            Self::Single => Action::ToggleSingle,
            Self::Overview => Action::ToggleOverview,
            Self::Staged => Action::ToggleStaged,
            Self::Wrap => Action::ToggleWrap,
            Self::Notes => Action::ToggleNotes,
            Self::Icons => Action::ToggleIcons,
            Self::Links => Action::ToggleLinks,
        }
    }

    /// Where it stands right now.
    #[must_use]
    pub const fn of(self, settings: Settings) -> bool {
        match self {
            Self::Follow => settings.follow,
            Self::Rail => settings.rail,
            Self::Single => settings.single,
            Self::Overview => settings.overview,
            Self::Staged => settings.staged,
            Self::Wrap => settings.wrap,
            Self::Notes => settings.notes,
            Self::Icons => settings.icons,
            Self::Links => settings.links,
        }
    }
}

/// Every toggle the pane has, as it stands.
///
/// Not [`crate::Config`], which carries `hide` and has no `follow`: this is what
/// is true of the running pane, where that is what a file asked for.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Settings {
    /// Whether the viewport is moving itself to what just changed.
    pub follow: bool,
    /// Whether the pinned list is beside the diff.
    pub rail: bool,
    /// Whether the diff is pinned to one file.
    pub single: bool,
    /// Whether the file list is drawn alone.
    pub overview: bool,
    /// Whether the staged run is drawn beside the unstaged one.
    pub staged: bool,
    /// Whether a long content line continues on the row below.
    pub wrap: bool,
    /// Whether the reader's notes draw their rows.
    pub notes: bool,
    /// Whether a listed path carries a file-type icon.
    pub icons: bool,
    /// Whether a listed path is an OSC 8 hyperlink.
    pub links: bool,
}

/// Where the reader is inside the menu, which is all that survives between frames.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Caret {
    /// The row it is on, as an index into [`SETTINGS`].
    pub at: usize,
    /// The first row the window shows.
    pub top: usize,
}

/// The menu as one frame draws it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Menu {
    /// Where the reader is inside it.
    pub caret: Caret,
    /// What the rows say.
    pub settings: Settings,
}

impl Caret {
    /// The window's first row, clamped so the caret is always drawn.
    ///
    /// Resolved on demand rather than when the caret moves, because how many rows
    /// fit is a property of the pane and the pane resizes without anybody pressing
    /// anything.
    #[must_use]
    pub fn window(self, rows: usize) -> usize {
        if rows == 0 || rows >= SETTINGS.len() {
            return 0;
        }
        let top = self.top.min(SETTINGS.len() - rows);
        if self.at < top {
            self.at
        } else if self.at >= top + rows {
            self.at + 1 - rows
        } else {
            top
        }
    }
}

/// What one terminal event means while the menu is open.
///
/// The mode is deliberately narrow. Four keys change meaning and every other one
/// reaches the map underneath, so a reader who knows the letters keeps them and
/// the arrows are a second way in rather than a replacement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuRoute {
    /// Move the caret by this many rows, negative for up.
    Move(isize),
    /// Flip the row the caret is on.
    Flip,
    /// Flip the row this many rows down the drawn window.
    Row(u16),
    /// Put the menu away and do nothing else with the event.
    Close,
    /// The menu's own, and it means nothing: a press on its frame, a key's
    /// release.
    Inert,
    /// Not the menu's to answer.
    Through,
}

/// Where `event` goes while the menu is drawn over `over`.
///
/// A press outside it closes it, which is the gestures sheet's rule rather than
/// the note box's: both are overlays over cells that are already drawn, and
/// neither holds anything a reader could lose by clicking away.
#[must_use]
pub fn menu_route(event: &Event, over: Option<Sheet>) -> MenuRoute {
    match event {
        Event::Key(key) => key_route(key, over.is_some()),
        Event::Mouse(mouse) => mouse_route(mouse, over),
        _ => MenuRoute::Through,
    }
}

/// The four keys the mode owns, and everything else passing through.
///
/// `drawn` is the whole of what makes this a mode rather than a state: a pane too
/// short for the box keeps the request the way `rail on` below 134 columns does,
/// and a request that is not on screen may not take the arrows. Otherwise a reader
/// presses `m`, sees nothing, and loses scrolling with no way to know why. `Esc`
/// is the exception and closes either way, so the state cannot be stuck.
fn key_route(key: &KeyEvent, drawn: bool) -> MenuRoute {
    if key.kind == KeyEventKind::Release {
        return MenuRoute::Inert;
    }
    // Shift-arrows scroll the pinned list, and that keeps working: the mode owns
    // the bare arrows and not the modified ones.
    if key
        .modifiers
        .intersects(KeyModifiers::SHIFT | KeyModifiers::CONTROL)
    {
        return MenuRoute::Through;
    }
    match key.code {
        KeyCode::Esc => MenuRoute::Close,
        _ if !drawn => MenuRoute::Through,
        KeyCode::Down => MenuRoute::Move(1),
        KeyCode::Up => MenuRoute::Move(-1),
        // `Space` toggles a list row in every toolkit there is. `Enter` is
        // universal for the same act and stays bound, but the edge names
        // `Space`, because `Enter` already means *commit* on this pane.
        KeyCode::Char(' ') | KeyCode::Enter => MenuRoute::Flip,
        // `j` and `k` are not the caret's. They scroll the diff behind the menu,
        // which is what every other letter does too, and taking them would make
        // the mode wider than the edge says it is.
        _ => MenuRoute::Through,
    }
}

/// A press inside the box is the box's; anywhere else closes it.
fn mouse_route(mouse: &MouseEvent, over: Option<Sheet>) -> MenuRoute {
    let Some(over) = over else {
        return MenuRoute::Through;
    };
    let inside = over.covers(mouse.column, mouse.row);
    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) if inside => {
            if (mouse.column, mouse.row) == over.close {
                return MenuRoute::Close;
            }
            row_at(over, mouse.row).map_or(MenuRoute::Inert, MenuRoute::Row)
        }
        MouseEventKind::Down(_) if !inside => MenuRoute::Close,
        // The wheel passes through, so the diff still scrolls behind it, which is
        // what B21's box already does and what stops the overlay feeling modal
        // in a way it is not.
        _ => MenuRoute::Through,
    }
}

/// Rows the frame and the air inside it cost, top and bottom.
///
/// Two border rows and one blank row at each end: the blank is what stops a name
/// touching the edge it is written under.
pub const MENU_FRAME: usize = 4;

/// The drawn row a screen row falls on, counted from the top of the window.
#[must_use]
pub fn row_at(over: Sheet, row: u16) -> Option<u16> {
    let first = over.top.saturating_add(2);
    let rows = usize::from(over.height).saturating_sub(MENU_FRAME);
    let offset = row.checked_sub(first)?;
    (usize::from(offset) < rows).then_some(offset)
}
