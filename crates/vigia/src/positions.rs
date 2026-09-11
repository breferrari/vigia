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
    /// Rows naming a place rather than a commit, read by both the row count and the
    /// index arithmetic so a third cannot arrive in one and not the other.
    fn named(&self) -> usize {
        1 + usize::from(self.point.is_some())
    }

    /// The row `at` draws, or `None` past the end.
    ///
    /// Arithmetic rather than a built list: the caret, the painter and the pick all ask
    /// for one row, and a `Vec` of every row walked would grow with how far the reader
    /// scrolled rather than with the window.
    #[must_use]
    pub fn row_at(&self, at: usize) -> Option<Row> {
        if at >= self.rows() {
            return None;
        }
        Some(match at {
            0 => Row::Current,
            1 if self.point.is_some() => Row::Point,
            _ => Row::Commit(at - self.named()),
        })
    }

    /// How many rows there are. Never zero: the live pane is always one of them, which
    /// is why this is not spelled `len`, a length carrying an `is_empty` that could only
    /// ever answer no.
    #[must_use]
    pub fn rows(&self) -> usize {
        self.named() + self.commits.len()
    }

    /// Take `page`, freshly walked from HEAD, as the front of this list, and say whether
    /// the rows were replaced rather than grown.
    ///
    /// A commit landing on the branch the list is already on leaves the page reaching
    /// what is held, so only what sits in front of that is new and the reader keeps the
    /// depth they scrolled to. That is the ordinary case here: the pane watches a tree an
    /// agent is committing into, and clearing the rows on every commit would take the
    /// reader's place away several times a minute.
    ///
    /// A checkout reaches nothing held, and then these rows are another branch's.
    /// Replacing them is the only honest answer, because two histories interleaved by
    /// date are a list that is neither, and the caller is told so it can put the caret
    /// back where the pane stands.
    pub fn re_anchor(&mut self, page: vigia_core::Page) -> bool {
        let held = self.commits.first().map(|at| at.id);
        if held == page.commits.first().map(|at| at.id) {
            return false;
        }
        match held.and_then(|id| page.commits.iter().position(|at| at.id == id)) {
            Some(at) => {
                // `more` is a fact about the oldest row held, which gained nothing.
                let mut fresh = page.commits;
                fresh.truncate(at);
                self.commits.splice(0..0, fresh);
                false
            }
            None => {
                self.commits = page.commits;
                self.more = page.more;
                true
            }
        }
    }

    /// Take `page` onto the end of what is already walked.
    ///
    /// The page's own `more` replaces this one's, because what is behind the history is
    /// a fact about the last commit walked and the new page holds it. A commit already
    /// held is dropped rather than appended: a resumed walk skips the tip it was given,
    /// so a repeat means the resume point was wrong and appending it would draw one
    /// commit twice.
    pub fn extend(&mut self, page: vigia_core::Page) {
        for commit in page.commits {
            if !self.commits.iter().any(|held| held.id == commit.id) {
                self.commits.push(commit);
            }
        }
        self.more = page.more;
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

/// The commit to walk on from, where the caret is within a row of the end of what is
/// walked, and `None` while it is not. A page of margin would make the first page ask
/// for the second before the reader had scrolled, which is the laziness this keeps.
///
/// It hands back the commit rather than a bool so the decision and the place it names
/// are one answer; two would let a caret near the end resume from the wrong place.
#[must_use]
pub fn resume_from(caret: Caret, places: &Places) -> Option<&Landmark> {
    if !places.more || caret.at + 2 < places.rows() {
        return None;
    }
    places.commits.last()
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
            over.row_at(mouse.row)
                .map_or(PositionsRoute::Inert, PositionsRoute::Row)
        }
        MouseEventKind::Down(_) if !inside => PositionsRoute::Close,
        // The wheel passes through, so the diff still scrolls behind the list,
        // which is what the note box and the menu already do and what stops the
        // overlay feeling modal in a way it is not.
        _ => PositionsRoute::Through,
    }
}
