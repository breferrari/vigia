//! The places the pane can stand, drawn as a list behind the header's token.

use ratatui::crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use vigia_core::Landmark;

use crate::input::Sheet;

/// One drawn row of the list.
///
/// Not a constant table, which is where this parts from the config menu: two rows
/// are named places and the rest are however much history the reader has scrolled
/// through, so every method below takes the count rather than reading a length off
/// an array.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    /// The live pane, which is the working tree against the index.
    Current,
    /// The branch point, named by the branch rather than by an id.
    Point,
    /// A commit, by its index into the history walked so far.
    Commit(usize),
}

/// What the two named rows draw beside themselves.
///
/// A run's facts, or `None` where nothing has measured them. The row the pane is
/// standing on always has them, because the frame underneath it is that run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Facts {
    /// Changes the run holds.
    pub files: usize,
    /// Lines its working-tree side adds.
    pub added: u32,
    /// Lines its other side loses.
    pub removed: u32,
}

/// The history the shell has walked, and what the named rows say.
///
/// Held by the shell rather than rebuilt per frame: the walk is a repository
/// question, and re-asking it on every paint is what [`Worktree::commits_from`]
/// exists to make unnecessary.
///
/// [`Worktree::commits_from`]: vigia_core::Worktree::commits_from
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Places {
    /// The commits walked so far, newest first.
    pub commits: Vec<Landmark>,
    /// Whether there is history behind the last of them.
    pub more: bool,
    /// What the live pane holds.
    pub current: Option<Facts>,
    /// What everything since the branch point holds, and what to call it. `None`
    /// on a branch with no point to measure from, where the row is not drawn.
    pub point: Option<(String, Option<Facts>)>,
}

impl Places {
    /// Every row the list draws, top to bottom.
    ///
    /// Rebuilt per frame rather than stored, because the rows are a projection of
    /// what has been walked and a stored copy is a second thing to keep in step.
    #[must_use]
    pub fn rows(&self) -> Vec<Row> {
        let named = std::iter::once(Row::Current).chain(self.point.is_some().then_some(Row::Point));
        named
            .chain((0..self.commits.len()).map(Row::Commit))
            .collect()
    }

    /// How many rows there are, without building them.
    #[must_use]
    pub fn len(&self) -> usize {
        1 + usize::from(self.point.is_some()) + self.commits.len()
    }

    /// Whether there is nothing to draw, which no repository with a commit in it
    /// produces: `current` is always a row.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The standing `row` names, or `None` for the branch point, which only the
    /// repository can resolve.
    #[must_use]
    pub fn commit_at(&self, row: Row) -> Option<&Landmark> {
        match row {
            Row::Commit(at) => self.commits.get(at),
            Row::Current | Row::Point => None,
        }
    }
}

/// Whether the caret has come close enough to the end of the walk to extend it.
///
/// One row of margin rather than one page: the caret moves a row at a time, so a row
/// in hand is all it takes for the box never to stall, and a page of margin would
/// make the first page trigger the second before the reader had scrolled at all,
/// which is the laziness this exists to keep.
#[must_use]
pub fn wants_more(caret: Caret, places: &Places) -> bool {
    places.more && caret.at + 2 >= places.len()
}

/// What the title bar spells, which is the reading rather than a name for the box.
///
/// The list documents what choosing a row will do and spends no row saying it.
/// There is one reading today, and this is where a second would be spelled.
#[must_use]
pub fn title() -> &'static str {
    vigia_core::Standing::SINCE
}

/// Where the reader is inside the list.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Caret {
    /// The row it is on.
    pub at: usize,
    /// The first row the window shows.
    pub top: usize,
}

impl Caret {
    /// The window's first row, clamped so the caret is always drawn.
    ///
    /// Resolved on demand for the config menu's reason: how many rows fit is a
    /// property of the pane, and the pane resizes without anybody pressing
    /// anything.
    #[must_use]
    pub fn window(self, rows: usize, of: usize) -> usize {
        if rows == 0 || rows >= of {
            return 0;
        }
        let top = self.top.min(of - rows);
        if self.at < top {
            self.at
        } else if self.at >= top + rows {
            self.at + 1 - rows
        } else {
            top
        }
    }

    /// Where `by` rows from here lands. Every row is selectable, so this is a
    /// clamped step rather than the menu's hunt over rows a caret may not sit on.
    ///
    /// The ends clamp rather than wrap, which is the menu's ruling: a list that
    /// jumps from its last row to its first moves the eye further than the key
    /// asked it to.
    #[must_use]
    pub fn stepped(self, by: isize, of: usize) -> usize {
        let last = of.saturating_sub(1);
        if by >= 0 {
            self.at.saturating_add(by.unsigned_abs()).min(last)
        } else {
            self.at.saturating_sub(by.unsigned_abs())
        }
    }
}

/// The list as one frame draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Positions {
    /// Where the reader is inside it.
    pub caret: Caret,
    /// What the rows say.
    pub places: Places,
}

/// What one terminal event means while the list is open.
///
/// The config menu's shape, and deliberately as narrow: four keys change meaning
/// and every other one reaches the map underneath, so a reader who knows the
/// letters keeps them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PositionsRoute {
    /// Move the caret by this many rows, negative for up.
    Move(isize),
    /// Stand where the caret is.
    Pick,
    /// Stand where the row this many rows down the drawn window is.
    Row(u16),
    /// Put the list away and do nothing else with the event.
    Close,
    /// The list's own, and it means nothing: a press on its frame, a key's
    /// release.
    Inert,
    /// Not the list's to answer.
    Through,
}

/// Where `event` goes while the list is drawn over `over`.
///
/// A press outside it closes it, which is the gestures sheet's rule and the config
/// menu's: all three are overlays over cells that are already drawn, and none
/// holds anything a reader could lose by clicking away.
#[must_use]
pub fn positions_route(event: &Event, over: Option<Sheet>) -> PositionsRoute {
    match event {
        Event::Key(key) => key_route(key, over.is_some()),
        Event::Mouse(mouse) => mouse_route(mouse, over),
        _ => PositionsRoute::Through,
    }
}

/// The four keys the mode owns, and everything else passing through.
///
/// `drawn` is what makes this a mode rather than a state, for the config menu's
/// reason: a pane too short for the box keeps the request, and a request that is
/// not on screen may not take the arrows, or a reader presses `B`, sees nothing,
/// and loses scrolling with no way to know why. `Esc` closes either way, so the
/// state cannot be stuck.
fn key_route(key: &KeyEvent, drawn: bool) -> PositionsRoute {
    if key.kind == KeyEventKind::Release {
        return PositionsRoute::Inert;
    }
    // Shift-arrows scroll the pinned list, and that keeps working: the mode owns
    // the bare arrows and not the modified ones.
    if key
        .modifiers
        .intersects(KeyModifiers::SHIFT | KeyModifiers::CONTROL)
    {
        return PositionsRoute::Through;
    }
    match key.code {
        KeyCode::Esc => PositionsRoute::Close,
        _ if !drawn => PositionsRoute::Through,
        KeyCode::Down => PositionsRoute::Move(1),
        KeyCode::Up => PositionsRoute::Move(-1),
        // The menu's pair, for the menu's reason: `Space` chooses a list row in
        // every toolkit there is, and `Enter` is universal for the same act.
        KeyCode::Char(' ') | KeyCode::Enter => PositionsRoute::Pick,
        // `j` and `k` are not the caret's. They scroll the diff behind the list,
        // which is what every other letter does, and taking them would make the
        // mode wider than its own edge says it is.
        _ => PositionsRoute::Through,
    }
}

/// A press inside the box is the box's; anywhere else closes it.
fn mouse_route(mouse: &MouseEvent, over: Option<Sheet>) -> PositionsRoute {
    let Some(over) = over else {
        return PositionsRoute::Through;
    };
    let inside = over.covers(mouse.column, mouse.row);
    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) if inside => {
            if (mouse.column, mouse.row) == over.close {
                return PositionsRoute::Close;
            }
            row_at(over, mouse.row).map_or(PositionsRoute::Inert, PositionsRoute::Row)
        }
        MouseEventKind::Down(_) if !inside => PositionsRoute::Close,
        // The wheel passes through, so the diff still scrolls behind the list,
        // which is what the note box and the menu already do and what stops the
        // overlay feeling modal in a way it is not.
        _ => PositionsRoute::Through,
    }
}

/// Rows the frame and the air inside it cost, top and bottom.
///
/// The config menu's number and for its reason: two border rows and one blank row
/// at each end, the blank being what stops a name touching the edge it is written
/// under.
pub const POSITIONS_FRAME: usize = 4;

/// The drawn row a screen row falls on, counted from the top of the window.
#[must_use]
pub fn row_at(over: Sheet, row: u16) -> Option<u16> {
    let first = over.top.saturating_add(2);
    let rows = usize::from(over.height).saturating_sub(POSITIONS_FRAME);
    let offset = row.checked_sub(first)?;
    (usize::from(offset) < rows).then_some(offset)
}
