//! The pane's own settings, drawn where a reader can see them.

use ratatui::crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};

use crate::input::{Action, Sheet};
use crate::pictures::Choice;

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
    /// The update check. No key, and read at the next launch.
    Update,
    /// How pictures are drawn. No key, read at the next launch, and the one row
    /// that cycles through words rather than flipping.
    Pictures,
    /// Write what the reader flips back into the file. No key: the menu's own row
    /// and nothing else, which is what keeps it a thing the reader asked for.
    Persist,
}

/// Every toggle, in the order the menu draws them.
///
/// The order is the reader's drawing: what the body is made of first, then what
/// a listed path carries. `b` is absent because where the pane stands is a
/// state rather than a setting, which is `SPEC.md` §11.2 B22's rule.
pub const SETTINGS: [Setting; 12] = [
    Setting::Follow,
    Setting::Staged,
    Setting::Rail,
    Setting::Single,
    Setting::Overview,
    Setting::Wrap,
    Setting::Icons,
    Setting::Links,
    Setting::Pictures,
    Setting::Notes,
    Setting::Update,
    Setting::Persist,
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

/// Columns the pictures row's state cell takes, which is its widest word. Its own
/// rather than [`STATE_WIDTH`], so the other rows keep their narrow cell.
pub const CHOICE_WIDTH: usize = {
    let mut widest = 0;
    let mut at = 0;
    while at < Choice::ALL.len() {
        let len = Choice::ALL[at].word().len();
        if len > widest {
            widest = len;
        }
        at += 1;
    }
    widest
};

/// What a row read only at launch says beside its name when the environment
/// overrides the file.
pub const ENV: &str = "env";

/// What the description box says for a row read only at launch once it has been
/// flipped since launch and written.
pub const RESTART: &str = "restart to apply";

/// And once it has been flipped and not written.
pub const UNSAVED: &str = "not saved: turn on Save settings";

/// And once it has been flipped while the environment overrides it.
pub const OVERRIDDEN: &str = "the environment sets this, so a restart keeps its value";

impl Setting {
    /// What the row spells.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Follow => "Follow changes",
            Self::Rail => "Left rail",
            Self::Single => "Single file",
            Self::Overview => "File list only",
            Self::Staged => "Staged changes",
            Self::Wrap => "Wrap lines",
            Self::Notes => "Notes",
            Self::Icons => "File icons",
            Self::Links => "Path links",
            Self::Update => "Check for updates",
            Self::Pictures => "Picture protocol",
            Self::Persist => "Save settings",
        }
    }

    /// One line on what it does, for the box under the rows.
    #[must_use]
    pub const fn description(self) -> &'static str {
        match self {
            Self::Follow => "Move the view to each change as it lands.",
            Self::Staged => "Show what is staged beside what is not.",
            Self::Rail => "The pinned file list beside the diff, from 139 columns.",
            Self::Single => "Show one file's diff at a time.",
            Self::Overview => "Show the file list alone, with no diff under it.",
            Self::Wrap => "Continue a long line on the row below instead of cutting it.",
            Self::Icons => "A file-type icon before each path. Needs a Nerd Font.",
            Self::Links => "Make each path a link that opens the file.",
            Self::Pictures => "How a changed image is drawn. auto asks the terminal.",
            Self::Notes => "Draw your notes to the agent under the lines they are on.",
            Self::Update => "At startup, look for a newer vigia and say so in the footer.",
            Self::Persist => "Write every change made here into the config file.",
        }
    }

    /// The word for where it stands in the shipped pane.
    #[must_use]
    pub fn shipped(self) -> &'static str {
        self.word(Settings::shipped())
    }

    /// The word the state cell spells: the environment's own where it sets one,
    /// the file's otherwise.
    #[must_use]
    pub fn effective(self, settings: Settings) -> &'static str {
        self.env(settings).unwrap_or_else(|| self.word(settings))
    }

    /// Whether what is in effect differs from the shipped pane, which the row marks.
    /// Not `follow`, which scrolling turns off without the reader changing a setting.
    #[must_use]
    pub fn changed(self, settings: Settings) -> bool {
        let effective = match self.effective(settings) {
            // `VIGIA_UPDATE=auto` is the check on.
            "auto" if self == Self::Update => ON,
            word => word,
        };
        self != Self::Follow && effective != self.shipped()
    }

    /// The variable that overrides it, for a row read only at launch.
    #[must_use]
    pub const fn variable(self) -> Option<&'static str> {
        match self {
            Self::Update => Some("VIGIA_UPDATE"),
            Self::Pictures => Some("VIGIA_PICTURES"),
            _ => None,
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
            Self::Update => Action::ToggleUpdate,
            Self::Pictures => Action::CyclePictures,
            Self::Persist => Action::TogglePersist,
        }
    }

    /// The word for where it stands right now: [`ON`] or [`OFF`], or the
    /// pictures choice.
    #[must_use]
    pub const fn word(self, settings: Settings) -> &'static str {
        let on = match self {
            Self::Follow => settings.follow,
            Self::Rail => settings.rail,
            Self::Single => settings.single,
            Self::Overview => settings.overview,
            Self::Staged => settings.staged,
            Self::Wrap => settings.wrap,
            Self::Notes => settings.notes,
            Self::Icons => settings.icons,
            Self::Links => settings.links,
            Self::Update => settings.update,
            Self::Pictures => return settings.pictures.word(),
            Self::Persist => settings.persist,
        };
        if on { ON } else { OFF }
    }

    /// Columns its state cell takes.
    #[must_use]
    pub const fn width(self) -> usize {
        match self {
            Self::Pictures => CHOICE_WIDTH,
            _ => STATE_WIDTH,
        }
    }

    /// Whether the pane reads it only at launch, so a flip changes the file and
    /// not the pane.
    #[must_use]
    pub const fn at_launch(self) -> bool {
        matches!(self, Self::Update | Self::Pictures)
    }

    /// The value the environment sets over the file, for a row read only at
    /// launch, or `None` where it sets nothing.
    #[must_use]
    pub const fn env(self, settings: Settings) -> Option<&'static str> {
        match self {
            Self::Update => match settings.env.update {
                Some(true) => Some("auto"),
                Some(false) => Some(OFF),
                None => None,
            },
            Self::Pictures => match settings.env.pictures {
                Some(choice) => Some(choice.word()),
                None => None,
            },
            _ => None,
        }
    }
}

impl Settings {
    /// The shipped pane, which is what [`Setting::changed`] measures against.
    #[must_use]
    pub fn shipped() -> Self {
        let config = crate::Config::default();
        Self {
            follow: config.follow,
            rail: config.rail,
            single: config.single,
            overview: config.overview,
            staged: config.staged,
            wrap: config.wrap,
            notes: config.notes,
            icons: config.icons,
            links: config.links,
            update: config.update,
            pictures: config.pictures,
            env: Overrides::default(),
            persist: config.persist,
        }
    }

    /// How many settings the row marks as changed.
    #[must_use]
    pub fn changed(self) -> usize {
        SETTINGS
            .into_iter()
            .filter(|setting| setting.changed(self))
            .count()
    }
}

/// Which rows read only at launch moved since launch.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Flipped {
    /// `Check for updates`.
    pub update: bool,
    /// `Picture protocol`.
    pub pictures: bool,
}

/// A titled group of rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    /// What the pane shows changing.
    Changes,
    /// How the body is laid out.
    Layout,
    /// What a listed path carries.
    Look,
    /// How a changed image is drawn.
    Pictures,
    /// What the agent's side of the pane draws.
    Agent,
    /// The program itself.
    App,
}

impl Group {
    /// What its header spells.
    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::Changes => "What changes",
            Self::Layout => "Layout",
            Self::Look => "Look",
            Self::Pictures => "Pictures",
            Self::Agent => "Agent",
            Self::App => "App",
        }
    }
}

/// What the box under the rows says for the row the caret is on.
#[must_use]
pub fn description(row: Row, settings: Settings, flipped: Flipped) -> &'static str {
    match row {
        Row::Toggle(setting) => {
            let moved = match setting {
                Setting::Update => flipped.update,
                Setting::Pictures => flipped.pictures,
                _ => false,
            };
            match (moved, setting.env(settings).is_some(), settings.persist) {
                (true, true, _) => OVERRIDDEN,
                (true, false, true) => RESTART,
                (true, false, false) => UNSAVED,
                (false, _, _) => setting.description(),
            }
        }
        Row::Reset => "Put every setting back to how vigia ships.",
        Row::Header(_) | Row::Gap => "",
    }
}

/// Every text the box can say, which is what its height is measured against.
pub fn descriptions() -> impl Iterator<Item = &'static str> {
    SETTINGS.into_iter().map(Setting::description).chain([
        description(Row::Reset, Settings::default(), Flipped::default()),
        RESTART,
        UNSAVED,
        OVERRIDDEN,
    ])
}

/// `text` broken at spaces into lines of at most `width` columns. A word wider
/// than a line is split, so every line fits.
#[must_use]
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    if width == 0 {
        return lines;
    }
    for word in text.split_whitespace() {
        let mut word = word;
        loop {
            let used = line.chars().count();
            let len = word.chars().count();
            let gap = usize::from(used > 0);
            if used + gap + len <= width {
                if gap == 1 {
                    line.push(' ');
                }
                line.push_str(word);
                break;
            }
            if used > 0 {
                lines.push(std::mem::take(&mut line));
                continue;
            }
            let cut = word
                .char_indices()
                .nth(width)
                .map_or(word.len(), |(at, _)| at);
            lines.push(word[..cut].to_owned());
            word = &word[cut..];
            if word.is_empty() {
                break;
            }
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// What the environment sets over the file, for the rows that say so.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Overrides {
    /// `VIGIA_UPDATE`, where it is set.
    pub update: Option<bool>,
    /// `VIGIA_PICTURES`, where it is set.
    pub pictures: Option<Choice>,
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
    /// Whether the file asks for the update check at the next launch.
    pub update: bool,
    /// How the file asks for pictures to be drawn at the next launch.
    pub pictures: Choice,
    /// What the environment sets over the file. No flip moves it.
    pub env: Overrides,
    /// Whether a flip is written back into the reader's own file.
    pub persist: bool,
}

/// One drawn row of the menu, in the order it draws them.
///
/// Not [`SETTINGS`], because headers, the air between groups and the reset are
/// not settings, and only the reset of those takes the caret.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    /// A group's title. Drawn dim and skipped.
    Header(Group),
    /// A toggle, with its state beside it.
    Toggle(Setting),
    /// Air. Drawn blank and skipped.
    Gap,
    /// The one row that throws something away, drawn as the act it is and with no
    /// state cell.
    Reset,
}

/// Every drawn row, top to bottom.
pub const ROWS: [Row; 20] = [
    Row::Header(Group::Changes),
    Row::Toggle(Setting::Follow),
    Row::Toggle(Setting::Staged),
    Row::Header(Group::Layout),
    Row::Toggle(Setting::Rail),
    Row::Toggle(Setting::Single),
    Row::Toggle(Setting::Overview),
    Row::Toggle(Setting::Wrap),
    Row::Header(Group::Look),
    Row::Toggle(Setting::Icons),
    Row::Toggle(Setting::Links),
    Row::Header(Group::Pictures),
    Row::Toggle(Setting::Pictures),
    Row::Header(Group::Agent),
    Row::Toggle(Setting::Notes),
    Row::Header(Group::App),
    Row::Toggle(Setting::Update),
    Row::Gap,
    Row::Toggle(Setting::Persist),
    Row::Reset,
];

/// What the reset row spells.
pub const RESET: &str = "Reset to defaults";

impl Row {
    /// Whether the caret may land here.
    #[must_use]
    pub const fn selectable(self) -> bool {
        matches!(self, Self::Toggle(_) | Self::Reset)
    }
}

/// Where the reader is inside the menu, which is all that survives between frames.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Caret {
    /// The row it is on, as an index into [`ROWS`].
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
    /// Which launch-only rows moved since launch.
    pub flipped: Flipped,
}

/// The first row the caret may land on, where it opens.
pub const FIRST: usize = {
    let mut at = 0;
    while !ROWS[at].selectable() {
        at += 1;
    }
    at
};

impl Caret {
    /// The window's first row, clamped so the caret is always drawn.
    ///
    /// Resolved on demand rather than when the caret moves, because how many rows
    /// fit is a property of the pane and the pane resizes without anybody pressing
    /// anything.
    #[must_use]
    pub fn window(self, rows: usize) -> usize {
        if rows == 0 || rows >= ROWS.len() {
            return 0;
        }
        let top = self.top.min(ROWS.len() - rows);
        if self.at < top {
            // The group's header comes into view with its first row.
            let headed = self.at > 0 && matches!(ROWS[self.at - 1], Row::Header(_));
            self.at - usize::from(headed && rows > 1)
        } else if self.at >= top + rows {
            self.at + 1 - rows
        } else {
            top
        }
    }
}

impl Caret {
    /// Where `by` rows from here lands, skipping the rows a caret may not sit on.
    ///
    /// A step is a step over *selectable* rows, so the rule and the air around it
    /// cost the reader no keystroke, and the ends clamp rather than wrap: a list
    /// that jumps from its last row to its first moves the eye further than the key
    /// asked to.
    #[must_use]
    pub fn stepped(self, by: isize) -> usize {
        let mut at = self.at;
        for _ in 0..by.unsigned_abs() {
            let next = (at..ROWS.len())
                .skip(1)
                .chain(std::iter::empty())
                .find(|row| ROWS[*row].selectable());
            let back = (0..at).rev().find(|row| ROWS[*row].selectable());
            match if by > 0 { next } else { back } {
                Some(landed) => at = landed,
                None => break,
            }
        }
        at
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
            over.row_at(mouse.row)
                .map_or(MenuRoute::Inert, MenuRoute::Row)
        }
        MouseEventKind::Down(_) if !inside => MenuRoute::Close,
        // The wheel passes through, so the diff still scrolls behind it, which is
        // what B21's box already does and what stops the overlay feeling modal
        // in a way it is not.
        _ => MenuRoute::Through,
    }
}
