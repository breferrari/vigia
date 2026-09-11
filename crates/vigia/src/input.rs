//! Terminal events to intentions, as a pure function.

use std::time::{Duration, Instant};

use ratatui::crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};

/// Rows a wheel notch moves.
pub const WHEEL_ROWS: isize = 3;

/// How long a step button is held before it begins repeating.
pub const STEP_DELAY: Duration = Duration::from_millis(500);

/// How long between repeats once one has begun.
pub const STEP_REPEAT: Duration = Duration::from_millis(50);

/// A span of rows on the screen, and the part of it a scrollbar's thumb can
/// occupy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Region {
    /// First row of the region, absolute within the pane.
    pub top: u16,
    /// How many rows it has.
    pub rows: u16,
    /// First column of the region, absolute within the pane.
    pub left: u16,
    /// How many columns it has.
    pub width: u16,
    /// First row of its bar's track, and how many rows that is.
    pub track: (u16, u16),
    /// The column this region's bar is drawn in, when it has one.
    pub bar: Option<u16>,
    /// The gutter's first column and its width, before a content row's text; none with no rows.
    pub gutter: (u16, u16),
    /// Columns a row's glyphs span from the gutter's first column, short of the bar; zero with no rows.
    pub text: u16,
}

impl Region {
    /// A region with no scrollbar buttons, whose track is the whole of it.
    pub fn bare(top: u16, rows: u16, left: u16, width: u16, bar: Option<u16>) -> Self {
        Self {
            top,
            rows,
            left,
            width,
            track: (top, rows),
            bar,
            gutter: (0, 0),
            text: 0,
        }
    }

    /// Whether `column`, `row` is a cell of this region's gutter.
    fn on_gutter(self, column: u16, row: u16) -> bool {
        self.contains(row) && Self::within(column, self.gutter)
    }

    /// Whether `column`, `row` is the column a display row's text opens at, which
    /// is the one a note spends on its left side. Measured off the gutter's width.
    fn on_content_origin(self, column: u16, row: u16) -> bool {
        let (left, gutter) = self.gutter;
        self.contains(row) && self.text > gutter && column == left.saturating_add(gutter)
    }

    /// Whether `row` falls inside this region.
    fn contains(self, row: u16) -> bool {
        Self::within(row, (self.top, self.rows))
    }

    /// Whether `column`, `row` is a cell of this region's scrollbar.
    fn on_bar(self, column: u16, row: u16) -> bool {
        self.bar == Some(column) && self.contains(row)
    }

    /// Whether `column`, `row` falls inside this region.
    pub fn covers(self, column: u16, row: u16) -> bool {
        self.contains(row)
            && self.width > 0
            && column >= self.left
            && column < self.left.saturating_add(self.width)
    }

    /// The same test against a bare `(top, rows)` span, for the track.
    fn within(row: u16, span: (u16, u16)) -> bool {
        let (top, rows) = span;
        // Saturating, matching [`Region::covers`] beside it.
        rows > 0 && row >= top && row < top.saturating_add(rows)
    }

    /// `-1` on this region's leading step button, `1` on its trailing one, and
    /// `None` both between them and outside the region.
    fn button(self, row: u16) -> Option<isize> {
        if !self.contains(row) {
            return None;
        }
        let (track_top, track_rows) = self.track;
        if row < track_top {
            Some(-1)
        } else if row >= track_top + track_rows {
            Some(1)
        } else {
            None
        }
    }

    /// `row` pulled into this region, for a drag that has left it.
    fn clamped_row(self, row: u16) -> u16 {
        row.clamp(
            self.top,
            self.top.saturating_add(self.rows.saturating_sub(1)),
        )
    }

    /// How far down this bar's track `row` sits, as a fraction over
    /// [`TRACK_SCALE`], or `None` when it is not on the track.
    fn along(self, row: u16) -> Option<u32> {
        let (top, rows) = self.track;
        if !Self::within(row, self.track) {
            return None;
        }
        // Divided by the last row's index, not by the row count.
        let travel = u32::from(rows - 1);
        if travel == 0 {
            return Some(0);
        }
        Some((u32::from(row - top) * TRACK_SCALE) / travel)
    }

    /// The same, with a row outside the track pulled to whichever end it is past.
    fn along_clamped(self, row: u16) -> u32 {
        let (top, rows) = self.track;
        if rows == 0 {
            return 0;
        }
        let last = top + rows - 1;
        self.along(row.clamp(top, last)).unwrap_or(0)
    }
}

/// Where the screen's regions are, so a pointer can be told what it is over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Regions {
    /// The pinned file list. Zero rows means there is no region and every
    /// gesture belongs to the diff.
    pub list: Region,
    /// The diff region.
    pub diff: Region,
    /// The gestures sheet, when it is drawn.
    pub sheet: Option<Sheet>,
    /// The config menu, when it is drawn. Never beside the sheet: B22 draws one
    /// overlay at a time.
    pub menu: Option<Sheet>,
    /// The list of places the pane can stand, when it is drawn. Never beside
    /// either of the others, for the same rule.
    pub positions: Option<Sheet>,
    /// The header's position token, which a click opens the list from.
    pub position: Option<Token>,
}

/// Where the header's position token is, so a pointer can be told it is over one.
///
/// A span on one row rather than a box, because the header is one row and a fact
/// inside it moves along the ladder as the pane narrows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    /// Left column, inclusive.
    pub left: u16,
    /// The row it is drawn on.
    pub row: u16,
    /// Columns it occupies, the glyph included.
    pub width: u16,
}

impl Token {
    /// Whether this cell is the token's.
    #[must_use]
    pub fn covers(self, column: u16, row: u16) -> bool {
        row == self.row && column >= self.left && column < self.left.saturating_add(self.width)
    }
}

/// Where the gestures sheet is, so a pointer can be told it is over one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sheet {
    /// Left column, inclusive.
    pub left: u16,
    /// Top row, inclusive.
    pub top: u16,
    /// Columns the sheet occupies.
    pub width: u16,
    /// Rows the sheet occupies.
    pub height: u16,
    /// The close control's own cell.
    pub close: (u16, u16),
}

impl Sheet {
    /// Whether this cell is the sheet's, control included.
    pub fn covers(self, column: u16, row: u16) -> bool {
        column >= self.left
            && column < self.left.saturating_add(self.width)
            && row >= self.top
            && row < self.top.saturating_add(self.height)
    }
}

impl Regions {
    /// Whether `column`, `row` is inside the pinned list.
    fn over_list(self, column: u16, row: u16) -> bool {
        self.list.covers(column, row)
    }

    /// Whether either overlay is drawn over this cell. One predicate, asked by every
    /// guard below: two would let a later one cover the sheet and miss the menu, which
    /// is what a pointer finds pressing a step button an overlay is drawn over.
    fn covered(self, column: u16, row: u16) -> bool {
        self.sheet
            .into_iter()
            .chain(self.menu)
            .chain(self.positions)
            .any(|over| over.covers(column, row))
    }

    /// The step a pointer at `column`, `row` is over, whatever it is doing there.
    pub fn step_at(self, column: u16, row: u16) -> Option<Action> {
        // Before the columns, because an overlay is drawn over them.
        if self.covered(column, row) {
            return None;
        }
        if self.list.bar == Some(column)
            && let Some(rows) = self.list.button(row)
        {
            return Some(Action::ScrollList(rows));
        }
        if self.diff.bar == Some(column) {
            return self.diff.button(row).map(Action::Scroll);
        }
        None
    }

    /// Whether a press at `column`, `row` begins a selection: a row of the diff
    /// itself, off its bar, and not under the sheet drawn over it.
    fn selectable(self, column: u16, row: u16) -> bool {
        // Their presence and not their boxes: a wash beside one would take `Esc`
        // off the frontmost thing, which §11.1 gives to the overlay.
        self.sheet.is_none()
            && self.menu.is_none()
            && self.positions.is_none()
            && self.diff.covers(column, row)
            && !self.diff.on_bar(column, row)
    }

    /// The bar a press at `column`, `row` takes hold of, or `None` off them.
    pub fn grab_at(self, column: u16, row: u16) -> Option<Grabbed> {
        // The overlay first, for [`Regions::step_at`]'s reason: it is drawn over the
        // bars and swallows what lands on it rather than passing it down.
        if self.covered(column, row) {
            return None;
        }
        if self.list.bar == Some(column) && self.list.along(row).is_some() {
            return Some(Grabbed::List);
        }
        (self.diff.bar == Some(column) && self.diff.along(row).is_some()).then_some(Grabbed::Diff)
    }

    /// The screen row a pointer at `column`, `row` rests on when it is on the
    /// diff's gutter, B21's target. Which rows are content rows is the screen's to say.
    pub fn gutter_at(self, column: u16, row: u16) -> Option<u16> {
        if self.covered(column, row) {
            return None;
        }
        self.diff.on_gutter(column, row).then_some(row)
    }

    /// The screen row a pointer at `column`, `row` rests on when it is on a note's
    /// left side. Which rows are note rows is the screen's to say.
    pub fn note_edge_at(self, column: u16, row: u16) -> Option<u16> {
        if self.covered(column, row) {
            return None;
        }
        self.diff.on_content_origin(column, row).then_some(row)
    }

    /// What a pointer at `column`, `row` is over, for the mark `SPEC.md`
    /// §11.2 B10 adopts.
    pub fn hover_at(self, column: u16, row: u16) -> Option<Hovered> {
        // Whichever overlay is up first, because one is drawn over everything.
        if let Some(list) = self.positions
            && list.covers(column, row)
        {
            if (column, row) == list.close {
                return Some(Hovered::Button(column, row));
            }
            return crate::positions::row_at(list, row).map(|_| Hovered::PositionRow(row));
        }
        if let Some(menu) = self.menu
            && menu.covers(column, row)
        {
            if (column, row) == menu.close {
                return Some(Hovered::Button(column, row));
            }
            return crate::menu::row_at(menu, row).map(|_| Hovered::MenuRow(row));
        }
        // The token is on the header, which no region covers, so it is asked
        // before the bars rather than after them.
        if let Some(token) = self.position
            && token.covers(column, row)
        {
            return Some(Hovered::Position);
        }
        if let Some(sheet) = self.sheet
            && sheet.covers(column, row)
        {
            return ((column, row) == sheet.close).then_some(Hovered::Button(column, row));
        }
        // The bar's column first, for [`Regions::grab_at`]'s reason: the scrollbar
        // is drawn *inside* whichever region owns those rows, so asking the list
        // first would mark a file under a pointer resting on the bar.
        let (on_list_bar, on_diff_bar) =
            (self.list.on_bar(column, row), self.diff.on_bar(column, row));
        if on_list_bar || on_diff_bar {
            if self.step_at(column, row).is_some() {
                return Some(Hovered::Button(column, row));
            }
            if on_list_bar && self.list.along(row).is_some() {
                return Some(Hovered::Track(Grabbed::List));
            }
            return (on_diff_bar && self.diff.along(row).is_some())
                .then_some(Hovered::Track(Grabbed::Diff));
        }
        // A content row's gutter, where a click writes a note, then a listed file,
        // which a click sends the diff to. The diff's content columns stay absent
        // although a press there begins a selection: a drag says where it is going
        // as it goes, so a mark before it would be the second thing saying so.
        if let Some(row) = self.gutter_at(column, row) {
            return Some(Hovered::Gutter(row));
        }
        // Then the column a note spends on its left side, where the gutter ends.
        if let Some(row) = self.note_edge_at(column, row) {
            return Some(Hovered::NoteEdge(row));
        }
        self.over_list(column, row).then_some(Hovered::Row(row))
    }
}

/// What the pointer is doing this frame, as one value.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Pointing {
    /// The cell a step button is being held down on.
    pub pressed: Option<(u16, u16)>,
    /// Which region's bar is being dragged.
    pub gripped: Option<Grabbed>,
    /// What the pointer is resting on, when it is on something a click acts on.
    pub hovered: Option<Hovered>,
    /// Which bar the keys are scrolling, and which way.
    pub scrolling: Option<(Grabbed, isize)>,
    /// The rows a drag has selected, when any are.
    pub selected: Option<Selection>,
}

/// Rows of the diff a drag is washing, which the button coming up sends. Screen
/// rows and not row indices: the span is re-resolved against every frame, so no
/// stored text can disagree with the wash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    /// The screen row the press landed on.
    anchor: u16,
    /// The screen row the drag has reached.
    head: u16,
}

impl Selection {
    fn at(row: u16) -> Self {
        Self {
            anchor: row,
            head: row,
        }
    }

    /// The rows it covers, topmost first and inclusive, so a drag up covers what
    /// the same drag down would.
    pub fn rows(self) -> (u16, u16) {
        (self.anchor.min(self.head), self.anchor.max(self.head))
    }

    /// The same span as offsets from `top`, which the collected rows are indexed by.
    pub fn offsets(self, top: u16) -> (usize, usize) {
        let (from, to) = self.rows();
        (
            usize::from(from.saturating_sub(top)),
            usize::from(to.saturating_sub(top)),
        )
    }
}

/// Whether `event` is the button coming up, which is a drag ending and the moment
/// what it washed reaches the clipboard.
fn ends_a_drag(event: &Event) -> bool {
    matches!(
        event,
        Event::Mouse(mouse) if mouse.kind == MouseEventKind::Up(MouseButton::Left)
    )
}

/// The selection after `event`, and the one `event` ended.
///
/// The second is handed back rather than left for a caller to read off its own
/// field, which this call has already overwritten: the send and the retire would
/// otherwise be two statements whose order decides whether anything is copied at
/// all. Only the button coming up ends one, so only that returns a second span.
pub fn selection_after(
    event: &Event,
    regions: Regions,
    was: Option<Selection>,
) -> (Option<Selection>, Option<Selection>) {
    if ends_a_drag(event) {
        return (None, was);
    }
    let Event::Mouse(mouse) = event else {
        // Focus clears no wash: this pane sits beside one a reader types into, and
        // clicking there must not discard a selection they were about to send.
        return (was, None);
    };
    let standing = match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => regions
            .selectable(mouse.column, mouse.row)
            .then(|| Selection::at(mouse.row)),
        // Clamped, so a drag out of the region reaches its edge rather than the
        // chrome above or below it.
        MouseEventKind::Drag(MouseButton::Left) => was.map(|had| Selection {
            head: regions.diff.clamped_row(mouse.row),
            ..had
        }),
        _ => was,
    };
    (standing, None)
}

/// What the pointer is resting on, when it is on something a click acts on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hovered {
    /// A one-cell control, by the cell it is drawn on.
    Button(u16, u16),
    /// A bar, by the region it belongs to.
    Track(Grabbed),
    /// A listed file, by the screen row it is drawn on.
    Row(u16),
    /// A config menu row, by the screen row. Its own variant rather than
    /// [`Hovered::Row`]'s: one for both would mark a listed file under a pointer
    /// resting on the overlay drawn over it.
    MenuRow(u16),
    /// A row of the position list, by the screen row, for [`Hovered::MenuRow`]'s
    /// reason.
    PositionRow(u16),
    /// The header's position token. It carries no cell, because the one
    /// derivation that says where the token is drawn is what marks it.
    Position,
    /// The diff's gutter, by the screen row; drawn only where that row is a line.
    Gutter(u16),
    /// A note's left side, by the screen row; drawn only on a row of a note's own.
    NoteEdge(u16),
}

/// The mark after `event`, given the one before it.
pub fn hover_after(event: &Event, regions: Regions, was: Option<Hovered>) -> Option<Hovered> {
    match event {
        // A drag is not a hover: it is the one mouse event that does not re-resolve.
        Event::Mouse(mouse) if matches!(mouse.kind, MouseEventKind::Drag(_)) => None,
        Event::Mouse(mouse) => regions.hover_at(mouse.column, mouse.row),
        Event::FocusLost => None,
        _ => was,
    }
}

/// A screen-anchored mark after a paint: it survives only where nothing moved.
pub fn repainted<T>(was: Option<T>, before: Regions, after: Regions) -> Option<T> {
    (before == after).then_some(was).flatten()
}

/// Which of the two scrollable regions a mark belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grabbed {
    /// The pinned list's bar.
    List,
    /// The diff's bar.
    Diff,
}

impl Grabbed {
    /// Whether `event` ends the grip.
    pub fn ends(event: &Event) -> bool {
        !matches!(
            event,
            Event::Mouse(mouse) if matches!(mouse.kind, MouseEventKind::Drag(MouseButton::Left))
        )
    }

    /// The region this drag is moving.
    fn region(self, regions: Regions) -> Region {
        match self {
            Self::List => regions.list,
            Self::Diff => regions.diff,
        }
    }
}

/// What a drag already under way makes of `event`, ignoring the column.
pub fn drag_action(event: &Event, regions: Regions, on: Grabbed) -> Option<Action> {
    let Event::Mouse(mouse) = event else {
        return None;
    };
    if !matches!(mouse.kind, MouseEventKind::Drag(MouseButton::Left)) {
        return None;
    }
    let at = on.region(regions).along_clamped(mouse.row);
    Some(match on {
        Grabbed::List => Action::ListTo(at),
        Grabbed::Diff => Action::DiffTo(at),
    })
}

/// Which bar an action scrolls, and which way, or `None` where it scrolls
/// neither.
pub fn scroll_mark(action: Action, regions: Regions) -> Option<(Grabbed, isize)> {
    let (whose, way) = match action {
        Action::Scroll(by) | Action::Page(by) | Action::HalfPage(by) | Action::File(by) => {
            (Grabbed::Diff, by.signum())
        }
        Action::Top => (Grabbed::Diff, -1),
        Action::Bottom => (Grabbed::Diff, 1),
        Action::ScrollList(by) => (Grabbed::List, by.signum()),
        // A jump to a listed file moves the diff to somewhere rather than by
        // something, and a drag on either bar already lights its own thumb.
        Action::ListRow(_)
        | Action::ListTo(_)
        | Action::DiffTo(_)
        | Action::ToggleFollow
        | Action::ToggleRail
        | Action::ToggleSingle
        | Action::ToggleOverview
        | Action::ToggleStaged
        | Action::ToggleWrap
        | Action::ToggleNotes
        | Action::ToggleStanding
        | Action::ToggleIcons
        | Action::ToggleLinks
        | Action::TogglePersist
        | Action::MenuReset
        | Action::ToggleSheet
        | Action::CloseSheet
        | Action::ToggleMenu
        | Action::CloseMenu
        | Action::MenuMove(_)
        | Action::MenuFlip
        | Action::MenuRow(_)
        | Action::TogglePositions
        | Action::ClosePositions
        | Action::PositionsMove(_)
        | Action::PositionsPick
        | Action::PositionsRow(_)
        | Action::Escape
        | Action::Redraw
        | Action::Quit => return None,
    };
    (way != 0 && whose.region(regions).rows > 0).then_some((whose, way))
}

/// Whether a deadline a caller is holding has come due; nothing held is nothing due.
pub fn settled(held: Option<Instant>, now: Instant) -> bool {
    held.is_some_and(|until| now >= until)
}

/// Every deadline the loop owns. Named: they fold to a minimum, so a value in the
/// wrong slot is invisible to a test that arms one clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct Deadlines {
    /// A held button's next repeat.
    pub held: Option<Held>,
    /// When the scroll direction's mark stops being true.
    pub linger: Option<Instant>,
    /// When the footer's notice stops being true.
    pub notice: Option<Instant>,
    /// When the churn window next has to age.
    pub ageing: Option<Duration>,
    /// When the last height kept waiting inside the settle margin settles.
    pub settling: Option<Duration>,
    /// When the fade on an arriving change is done, if one is still running.
    pub arriving: Option<Instant>,
    /// When the next note that is leaving has its rows dropped.
    pub departing: Option<Instant>,
    /// When the note box Esc sent away has its rows dropped.
    pub closing: Option<Instant>,
}

/// How long the loop may block before some clock here has to act.
pub fn patience(due: Deadlines, now: Instant) -> Option<Duration> {
    let since = |until: Instant| until.saturating_duration_since(now);
    // Folded rather than matched: an arm returning `None` where one was `Some` quietly stops a clock.
    [
        Held::wait(due.held, now),
        due.linger.map(since),
        due.notice.map(since),
        due.ageing,
        due.settling,
        due.arriving.map(since),
        due.departing.map(since),
        due.closing.map(since),
    ]
    .into_iter()
    .flatten()
    .min()
}

/// What a held mouse button is repeating, and when its next step is due.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Held {
    /// One step in the direction pressed: [`Action::Scroll`] for the diff's
    /// buttons, [`Action::ScrollList`] for the list's.
    step: Action,
    /// The column and row the press landed on, so a drag off the button can be
    /// told from a twitch on it.
    at: (u16, u16),
    /// When the next repeat falls due. [`STEP_DELAY`] after the press, then
    /// [`STEP_REPEAT`] apart.
    due: Instant,
}

impl Held {
    /// Arm a repeat from a press that produced `step` at `at`.
    pub fn new(step: Action, at: (u16, u16), now: Instant) -> Self {
        Self {
            step,
            at,
            due: now + STEP_DELAY,
        }
    }

    /// The cell this hold began on, so the paint can draw that button pressed.
    pub fn at(self) -> (u16, u16) {
        self.at
    }

    /// How long the loop may block before this has to act, or `None` when
    /// nothing is held.
    pub fn wait(held: Option<Self>, now: Instant) -> Option<Duration> {
        held.map(|held| held.due.saturating_duration_since(now))
    }

    /// The step now due, scaled by how many intervals have actually elapsed, and
    /// the state to carry forward.
    pub fn fire(self, now: Instant) -> Option<(Action, Self)> {
        if now < self.due {
            return None;
        }
        let late = now.saturating_duration_since(self.due);
        // The one now due, plus any whole intervals that passed while something
        // else held the loop. `as_nanos` on both sides keeps this integer.
        let extra = (late.as_nanos() / STEP_REPEAT.as_nanos().max(1)) as isize;
        let steps = 1 + extra;
        Some((
            self.step.repeated(steps),
            Self {
                due: self.due + STEP_REPEAT * (steps as u32),
                ..self
            },
        ))
    }

    /// Whether `event` ends the hold.
    pub fn ends(self, event: &Event, regions: Regions) -> bool {
        match event {
            Event::Key(key) if key.kind != KeyEventKind::Release => true,
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::Up(_) | MouseEventKind::Moved => true,
                MouseEventKind::Drag(_) => {
                    (mouse.column, mouse.row) != self.at
                        && regions.step_at(mouse.column, mouse.row) != Some(self.step)
                }
                _ => false,
            },
            // A window that lost focus has ended the gesture, and this arm is owed to
            // I1 rather than to tidiness.
            Event::FocusLost => true,
            _ => false,
        }
    }
}

/// Resolution a drag reports its position at.
pub const TRACK_SCALE: u32 = 1 << 16;

/// What the reader asked the shell to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Leave.
    Quit,
    /// Move the viewport by this many rows, negative for up.
    Scroll(isize),
    /// Move the pinned file list's window by this many rows, negative for up.
    ScrollList(isize),
    /// Move the viewport by whole screens, negative for up.
    Page(isize),
    /// Move the viewport by half screens, negative for up.
    HalfPage(isize),
    /// Move the viewport by this many changed files, negative for back.
    File(isize),
    /// Go to the first changed file.
    Top,
    /// Go to the last changed file.
    Bottom,
    /// Engage follow mode, or disengage it.
    ToggleFollow,
    /// Put the pinned list beside the diff as a left rail, or back above it.
    ToggleRail,
    /// Pin the diff to the file the viewport is inside, or unpin it.
    ToggleSingle,
    /// Draw the file list alone with no diff under it, or put the diff back.
    ToggleOverview,
    /// Show the staged run beside the unstaged one, or stop showing it.
    ToggleStaged,
    /// Wrap a content line too wide for the pane onto the row below, or clip it.
    ToggleWrap,
    /// Draw the rows of the reader's notes under their lines, or only the marks.
    ToggleNotes,
    /// Stand at the branch point, or come back.
    ToggleStanding,
    /// Draw a file-type icon before every listed path, or stop. No key: the menu
    /// and the file are the two ways to it.
    ToggleIcons,
    /// Wrap every listed path in an OSC 8 hyperlink, or stop. No key either.
    ToggleLinks,
    /// Write what the reader flips back into their config file, or stop. B22.
    TogglePersist,
    /// Put every toggle back where the shipped pane has it.
    MenuReset,
    /// Draw the gestures sheet, advance it a page, or stop drawing it.
    ToggleSheet,
    /// Stop drawing the gestures sheet, whatever page it is on.
    CloseSheet,
    /// Draw the config menu, or stop drawing it.
    ToggleMenu,
    /// Stop drawing it, whatever the caret is on.
    CloseMenu,
    /// Move its caret by this many rows, negative for up.
    MenuMove(isize),
    /// Flip the row its caret is on.
    MenuFlip,
    /// Flip the row this many rows down its drawn window.
    MenuRow(u16),
    /// Draw the list of places the pane can stand, or stop drawing it. `B`, and
    /// the header's position token under a click.
    TogglePositions,
    /// Stop drawing it, wherever the caret is.
    ClosePositions,
    /// Move its caret by this many rows, negative for up.
    PositionsMove(isize),
    /// Stand where its caret is.
    PositionsPick,
    /// Stand where the row this many rows down its drawn window names.
    PositionsRow(u16),
    /// Leave the frontmost thing: the gestures sheet if one is up, and the
    /// program if none is. `Esc`.
    Escape,
    /// Put the pinned list's window at this fraction of the changed set.
    ListTo(u32),
    /// Put the diff at the file this many rows down the pinned list.
    ListRow(u16),
    /// Put the diff at this fraction of its total height.
    DiffTo(u32),
    /// Draw again with no state change, which is what a resize needs.
    Redraw,
}

impl Action {
    /// This action performed `times` over, as a single action where that is
    /// possible.
    pub fn repeated(self, times: isize) -> Self {
        match self {
            // Relative row counts, so `n` steps *is* one action with `n` in it.
            Self::Scroll(by) => Self::Scroll(by.saturating_mul(times)),
            Self::ScrollList(by) => Self::ScrollList(by.saturating_mul(times)),
            // Everything else repeats as itself. Folding a page count into
            // `Page` would be correct arithmetic and the wrong feel, and the
            // absolute moves have nothing to fold at all.
            Self::Page(_)
            | Self::HalfPage(_)
            | Self::File(_)
            | Self::Top
            | Self::Bottom
            | Self::ListRow(_)
            | Self::ListTo(_)
            | Self::DiffTo(_)
            | Self::ToggleFollow
            | Self::ToggleRail
            | Self::ToggleSingle
            | Self::ToggleOverview
            | Self::ToggleStaged
            | Self::ToggleWrap
            | Self::ToggleNotes
            | Self::ToggleStanding
            | Self::ToggleIcons
            | Self::ToggleLinks
            | Self::TogglePersist
            | Self::MenuReset
            | Self::ToggleSheet
            | Self::CloseSheet
            | Self::ToggleMenu
            | Self::CloseMenu
            | Self::MenuFlip
            | Self::MenuRow(_)
            | Self::TogglePositions
            | Self::ClosePositions
            | Self::PositionsPick
            | Self::PositionsRow(_)
            | Self::Escape
            | Self::Redraw
            | Self::Quit => self,
            // A caret steps, so `n` steps is one action with `n` in it.
            Self::MenuMove(by) => Self::MenuMove(by.saturating_mul(times)),
            Self::PositionsMove(by) => Self::PositionsMove(by.saturating_mul(times)),
        }
    }

    /// Whether this is the reader moving the viewport themselves.
    pub fn is_manual_scroll(self) -> bool {
        match self {
            // `File` is here rather than below because it moves the *diff*, and
            // it disengages even at an end where it moves nothing: `Top` at the
            // top and `Bottom` at the last file already do, so on this map
            // follow yields to a reader's intent rather than to whether the
            // arithmetic happened to land somewhere new.
            Self::Scroll(_)
            | Self::Page(_)
            | Self::HalfPage(_)
            | Self::File(_)
            | Self::Top
            | Self::Bottom => true,
            // A resize moves no viewport and expresses no intent, and a pane beside an
            // agent is resized constantly, so treating it as a scroll would disengage
            // follow mode for free.
            Self::Quit
            | Self::Escape
            | Self::Redraw
            | Self::ToggleFollow
            // Moving the list beside the diff resizes the diff's region and does not
            // move the reader inside it, which is a resize by another name and the same
            // answer §11.1 gives one: a resize expresses no intent about what the diff
            // should show.
            | Self::ToggleRail
            // And a pin is the one of the two that can move the viewport, and still
            // expresses no intent about where it should be.
            | Self::ToggleSingle
            | Self::ToggleOverview
            | Self::ToggleStaged
            | Self::ToggleWrap
            // Display rows too, so hiding them moves nothing the bar counts.
            | Self::ToggleNotes
            // Standing elsewhere remakes the body and moves no viewport.
            | Self::ToggleStanding
            // Neither reaches a key, so neither can be a reader moving anything.
            | Self::ToggleIcons
            | Self::ToggleLinks
            | Self::TogglePersist
            | Self::MenuReset
            // And the sheet moves nothing at all: it composites over rows that
            // are already drawn, so it does not even resize a region. B12.
            | Self::ToggleSheet
            | Self::CloseSheet
            // The menu's caret moves inside the box, not inside the diff. B22.
            | Self::ToggleMenu
            | Self::CloseMenu
            | Self::MenuMove(_)
            | Self::MenuFlip
            | Self::MenuRow(_)
            // And the list's caret moves inside its own box, the same way.
            | Self::TogglePositions
            | Self::ClosePositions
            | Self::PositionsMove(_)
            | Self::PositionsPick
            | Self::PositionsRow(_)
            | Self::ScrollList(_) => false,
            // Dragging the list's bar moves the map and not the diff, so it
            // is `ScrollList` by another input device. Dragging the diff's
            // moves the viewport and is a manual scroll like any other.
            Self::ListTo(_) => false,
            // A click on a row moves the diff, so it is a manual scroll for the
            // same reason a drag on the diff's bar is.
            Self::DiffTo(_) | Self::ListRow(_) => true,
        }
    }

    /// Whether applying this needs to know how tall the body is.
    pub fn needs_height(self) -> bool {
        match self {
            // A page steps by a screenful and a half page by half of one, and a drag on
            // the diff's bar maps the track onto everything *but* the last screenful,
            // so all three need to know how tall one is.
            Self::Page(_) | Self::HalfPage(_) | Self::DiffTo(_) | Self::Bottom => true,
            // `File` steps a file index and lands on a heading, so it is measured in
            // files and never in rows: no height can change where it arrives.
            Self::Scroll(_) | Self::File(_) | Self::Top | Self::ScrollList(_) => false,
            Self::ListTo(_) | Self::ListRow(_) => false,
            // A toggle changes the region's height; it does not need to be
            // told one to decide what it means.
            Self::Quit
            | Self::Escape
            | Self::Redraw
            | Self::ToggleFollow
            | Self::ToggleRail
            | Self::ToggleSingle
            | Self::ToggleOverview
            | Self::ToggleStaged
            | Self::ToggleWrap
            | Self::ToggleNotes
            | Self::ToggleStanding
            | Self::ToggleIcons
            | Self::ToggleLinks
            | Self::TogglePersist
            | Self::MenuReset
            | Self::ToggleSheet
            | Self::CloseSheet
            | Self::ToggleMenu
            | Self::CloseMenu
            | Self::MenuMove(_)
            | Self::MenuFlip
            | Self::MenuRow(_)
            | Self::TogglePositions
            | Self::ClosePositions
            | Self::PositionsMove(_)
            | Self::PositionsPick
            | Self::PositionsRow(_) => false,
        }
    }
}

/// The intention behind one terminal event, or `None` if there was not one.
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

/// It takes the key and nothing else, which is what makes "not a mode" structural
/// rather than a claim. The box is the exception, and only because its keys never reach it.
fn key_action(key: &KeyEvent) -> Option<Action> {
    // Windows reports press *and* release; Unix terminals report press only.
    // Acting on both would double every keystroke on one platform and not the
    // other, which is the kind of bug that only ever reproduces for one person.
    if key.kind == KeyEventKind::Release {
        return None;
    }

    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return match key.code {
            // Ctrl-C is handled here rather than by a signal handler.
            KeyCode::Char('c') | KeyCode::Char('d') => Some(Action::Quit),
            _ => None,
        };
    }

    // Before the plain arrow arms, or `Shift-↓` falls through to a diff scroll.
    if key.modifiers.contains(KeyModifiers::SHIFT) {
        match key.code {
            KeyCode::Down => return Some(Action::ScrollList(1)),
            KeyCode::Up => return Some(Action::ScrollList(-1)),
            _ => {}
        }
    }

    match key.code {
        KeyCode::Char('q') => Some(Action::Quit),
        KeyCode::Esc => Some(Action::Escape),
        KeyCode::Down | KeyCode::Char('j') => Some(Action::Scroll(1)),
        KeyCode::Up | KeyCode::Char('k') => Some(Action::Scroll(-1)),
        // Shift is the modifier because the alternatives are all taken or unreliable:
        // `Ctrl-J` is LF, `Ctrl-C` and `Ctrl-D` already quit, and Alt is intercepted by
        // terminal emulators and by macOS Option.
        KeyCode::Char('J') => Some(Action::ScrollList(1)),
        KeyCode::Char('K') => Some(Action::ScrollList(-1)),
        KeyCode::PageDown | KeyCode::Char(' ') => Some(Action::Page(1)),
        KeyCode::PageUp => Some(Action::Page(-1)),
        // `less`'s own half-page pair, and the shell already claims `less +F` semantics
        // one row down, so the precedent is internal as well as cultural.
        KeyCode::Char('d') => Some(Action::HalfPage(1)),
        KeyCode::Char('u') => Some(Action::HalfPage(-1)),
        KeyCode::Home | KeyCode::Char('g') => Some(Action::Top),
        KeyCode::End | KeyCode::Char('G') => Some(Action::Bottom),
        // The file granularity the map was missing. Both letters were free: no
        // search exists to claim `n`, and `less` readers carry the next-file
        // reflex from `:n`/`:p` already. `N` and `P` are unbound for the reason
        // `D`, `U` and `F` are, one row above a pair where `g`/`G` have already
        // taught that case is load bearing here.
        // Aliases in the letter's own arm, which is the shape `Down |
        // Char('j')` above already has and what stops the two directions drifting
        // apart. Why the arrows and what they spend is `Action::File`'s docblock,
        // not repeated here.
        KeyCode::Right | KeyCode::Char('n') => Some(Action::File(1)),
        KeyCode::Left | KeyCode::Char('p') => Some(Action::File(-1)),
        // The digits address the drawn window, and there are six of them because
        // `render::LIST_SETTLED` is six.
        KeyCode::Char(digit @ '1'..='6') => Some(Action::ListRow(row_of(digit))),
        // Lower case only, and `G` above is why.
        KeyCode::Char('f') => Some(Action::ToggleFollow),
        KeyCode::Char('r') => Some(Action::ToggleRail),
        // `s` for single, unbound and in the same lowercase family as `f` and
        // `r`: the keys that change what the body is made of rather than where
        // in it the reader is. B16.
        KeyCode::Char('s') => Some(Action::ToggleSingle),
        // `o` for overview. `l` is refused where `h` is: a vi motion everywhere else.
        KeyCode::Char('o') => Some(Action::ToggleOverview),
        KeyCode::Char('a') => Some(Action::ToggleStaged),
        // `b` for the branch point, which is the one place a key alone reaches.
        KeyCode::Char('b') => Some(Action::ToggleStanding),
        // `B` for the list behind it, which is the same family a rung up: `g`/`G`
        // and `j`/`J` have already taught that case is load bearing here, and the
        // menu's arrival is the precedent for the letter staying where it was.
        KeyCode::Char('B') => Some(Action::TogglePositions),
        // `w`, and it is the reflex rather than what was free. `ov` binds `[w]`, `[W]`
        // to a character-based wrap toggle, `bat` spells the opposite state `-S` /
        // `--chop-long-lines`, and `less` toggles the same state with `-S`.
        KeyCode::Char('w') => Some(Action::ToggleWrap),
        // `c` for the comments the notes are, free below `Ctrl`, in `w`'s family. B21.
        KeyCode::Char('c') => Some(Action::ToggleNotes),
        // `m` for the menu: every accurate noun's initial was taken, so the key
        // carries the mnemonic and the bar carries the destination. B22.
        KeyCode::Char('m') => Some(Action::ToggleMenu),
        // `?` and nothing else, which is `SPEC.md` §11.2's B12: `btop`, `bottom` and
        // `rtop` all open help on it, it was unbound here, and `h` is refused because
        // it is a vi motion everywhere else on a pane with no horizontal scroll.
        KeyCode::Char('?') => Some(Action::ToggleSheet),
        _ => None,
    }
}

/// The list row a digit names, counting from zero where a reader counts from one.
fn row_of(digit: char) -> u16 {
    digit
        .to_digit(10)
        .map_or(0, |rank| rank.saturating_sub(1) as u16)
}

fn mouse_action(mouse: &MouseEvent, regions: Regions) -> Option<Action> {
    // The sheet is checked before everything, because it is drawn over everything.
    // The menu, which is never up at the same time, answers its own events in
    // `menu::menu_route` before this is reached.
    // `SPEC.md` §11.2's B12: the close control dismisses, and any other event landing
    // on the sheet does nothing at all.
    if let Some(sheet) = regions.sheet
        && sheet.covers(mouse.column, mouse.row)
    {
        return matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
            .then_some(())
            .filter(|()| (mouse.column, mouse.row) == sheet.close)
            .map(|()| Action::CloseSheet);
    }

    // The position token, on the header, which no region covers: a press on it opens
    // the list, which is the same act `B` performs and the one the chevron promises.
    // A press rather than a release, for the step buttons' reason.
    if let Some(token) = regions.position
        && token.covers(mouse.column, mouse.row)
    {
        return matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
            .then_some(Action::TogglePositions);
    }

    // The bar is checked before the region it sits in. A press on the scrollbar column
    // is a gesture about position, and the same column is inside whichever region drew
    // it, so testing the region first would turn every drag into a wheel.
    let on_bar = regions.list.on_bar(mouse.column, mouse.row)
        || regions.diff.on_bar(mouse.column, mouse.row);
    if on_bar
        && matches!(
            mouse.kind,
            MouseEventKind::Down(MouseButton::Left) | MouseEventKind::Drag(MouseButton::Left)
        )
    {
        // A step button answers a press and not a drag, and that asymmetry is a ruling
        // rather than an omission.
        if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
            && let Some(step) = regions.step_at(mouse.column, mouse.row)
        {
            return Some(step);
        }
        // The track, not the region, so a stepped bar seeks from the rows its
        // thumb actually occupies. Where there are no buttons the two are the
        // same span and this is what it always was.
        if regions.list.bar == Some(mouse.column)
            && let Some(at) = regions.list.along(mouse.row)
        {
            return Some(Action::ListTo(at));
        }
        if regions.diff.bar == Some(mouse.column)
            && let Some(at) = regions.diff.along(mouse.row)
        {
            return Some(Action::DiffTo(at));
        }
        return None;
    }

    match mouse.kind {
        // The wheel scrolls whatever it is over. A reader hovering the map
        // and turning the wheel means the map; `SPEC.md` §2 makes `btop` the
        // reference and that is what `btop` does.
        MouseEventKind::ScrollDown if regions.over_list(mouse.column, mouse.row) => {
            Some(Action::ScrollList(1))
        }
        MouseEventKind::ScrollUp if regions.over_list(mouse.column, mouse.row) => {
            Some(Action::ScrollList(-1))
        }
        MouseEventKind::ScrollDown => Some(Action::Scroll(WHEEL_ROWS)),
        MouseEventKind::ScrollUp => Some(Action::Scroll(-WHEEL_ROWS)),
        // A click on a listed file sends the diff to it. The row is reported as an
        // offset into the window; the app owns where the window is.
        MouseEventKind::Down(MouseButton::Left) if regions.over_list(mouse.column, mouse.row) => {
            Some(Action::ListRow(mouse.row - regions.list.top))
        }
        // Everything else is deliberately inert. Horizontal wheels exist and lines do
        // not pan: the renderer clips instead, which is what I6 asks for.
        _ => None,
    }
}
