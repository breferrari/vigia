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
    /// First row of its bar's **track**, and how many rows that is.
    pub track: (u16, u16),
    /// The column **this region's** bar is drawn in, when it has one.
    pub bar: Option<u16>,
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
        }
    }

    /// Whether `row` falls inside this region.
    fn contains(self, row: u16) -> bool {
        Self::within(row, (self.top, self.rows))
    }

    /// Whether `column`, `row` is a cell of **this region's** scrollbar.
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
        // Saturating, matching [`Region::covers`] beside it. Every live `Region`
        // comes from `Body::areas`, whose own arithmetic saturates and is bounded
        // by a real terminal, so nothing reaches the overflow; the fields are
        // `pub` and the two tests should not disagree at the edge of the type.
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

    /// How far down this bar's track `row` sits, as a fraction over
    /// [`TRACK_SCALE`], or `None` when it is not on the track.
    fn along(self, row: u16) -> Option<u32> {
        let (top, rows) = self.track;
        if !Self::within(row, self.track) {
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

    /// The step a pointer at `column`, `row` is over, whatever it is doing there.
    pub fn step_at(self, column: u16, row: u16) -> Option<Action> {
        // **Before the columns, because the sheet is drawn over them.** The order
        // is [`Regions::hover_at`]'s own and for the same reason: the sheet
        // swallows what lands on it rather than passing it down, so a cell it
        // covers is not a button however the bars are laid out underneath.
        if self.sheet.is_some_and(|sheet| sheet.covers(column, row)) {
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

    /// The bar a press at `column`, `row` takes hold of, or `None` off them.
    pub fn grab_at(self, column: u16, row: u16) -> Option<Grabbed> {
        // **The sheet first, for [`Regions::step_at`]'s reason and in the same
        // words**: it is drawn over the bars, and it swallows what lands on it
        // rather than passing it down.
        if self.sheet.is_some_and(|sheet| sheet.covers(column, row)) {
            return None;
        }
        if self.list.bar == Some(column) && self.list.along(row).is_some() {
            return Some(Grabbed::List);
        }
        (self.diff.bar == Some(column) && self.diff.along(row).is_some()).then_some(Grabbed::Diff)
    }

    /// What a pointer at `column`, `row` is **over**, for the mark `SPEC.md`
    /// §11.2 B10 adopts.
    pub fn hover_at(self, column: u16, row: u16) -> Option<Hovered> {
        // **The sheet first, because it is drawn over everything.** Its close
        // control is the only thing on it a click acts on, and the rest of the
        // sheet swallows gestures rather than passing them down, so a pointer
        // resting anywhere on it must not mark a bar or a listed file underneath.
        if let Some(sheet) = self.sheet {
            if sheet.covers(column, row) {
                return ((column, row) == sheet.close).then_some(Hovered::Button(column, row));
            }
        }
        // **The bar's column first, for [`Regions::grab_at`]'s reason one
        // function up**: the scrollbar is drawn *inside* whichever region owns
        // those rows, so asking the list first would answer `Row` for a pointer
        // resting on the bar and mark a file the reader is not pointing at.
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
        // A listed file, which is a surface a click acts on: it puts the diff at
        // that file. The diff's own rows are deliberately absent, because
        // nothing there is clickable and a mark would imply it is.
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
}

/// The mark after `event`, given the one before it.
pub fn hover_after(event: &Event, regions: Regions, was: Option<Hovered>) -> Option<Hovered> {
    match event {
        // **A drag is not a hover, and it is the one mouse event that does not
        // re-resolve.** A reader pulling a grabbed thumb travels over the step
        // button at that end of the track, and lighting it would promise a step
        // that releasing there will not perform: `Grabbed` owns the gesture
        // until the button comes up, so a press on the button is not what the
        // release means. This is [`Grabbed`]'s own doctrine one mark over, that
        // a gesture outlives the target it began on.
        Event::Mouse(mouse) if matches!(mouse.kind, MouseEventKind::Drag(_)) => None,
        Event::Mouse(mouse) => regions.hover_at(mouse.column, mouse.row),
        Event::FocusLost => None,
        _ => was,
    }
}

/// The mark after a paint, given the layout before it and the layout it drew.
pub fn hover_repainted(was: Option<Hovered>, before: Regions, after: Regions) -> Option<Hovered> {
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

/// What a drag already under way makes of `event`, **ignoring the column**.
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
        | Action::ToggleMasthead
        | Action::ToggleRail
        | Action::ToggleSingle
        | Action::ToggleStaged
        | Action::ToggleWrap
        | Action::ToggleSheet
        | Action::CloseSheet
        | Action::Escape
        | Action::Redraw
        | Action::Quit => return None,
    };
    (way != 0 && whose.region(regions).rows > 0).then_some((whose, way))
}

/// Whether a scroll's direction mark has outlived its burst.
pub fn settled(linger: Option<Instant>, now: Instant) -> bool {
    linger.is_some_and(|until| now >= until)
}

/// How long the loop may block before some clock here has to act.
pub fn patience(
    held: Option<Held>,
    linger: Option<Instant>,
    ageing: Option<Duration>,
    now: Instant,
) -> Option<Duration> {
    let step = Held::wait(held, now);
    let mark = linger.map(|until| until.saturating_duration_since(now));
    // Folded rather than matched pairwise: a `match` over three options is eight
    // arms, and the arm that returns `None` where one of them was `Some` is the
    // one that quietly stops a clock nobody notices has stopped.
    [step, mark, ageing].into_iter().flatten().min()
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
            // **A window that lost focus has ended the gesture**, and this arm is
            // owed to I1 rather than to tidiness. The clock a hold owns is
            // licensed on three conditions, and the second is that it *may not
            // outlive the gesture that armed it*: a reader who has tabbed away is
            // not holding this button in any sense the repeat should honour, and
            // without this the loop keeps stepping and repainting a pane nobody
            // is looking at, on a timer, which is the state I1's measure exists
            // to protect.
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
    /// Move the **pinned file list's** window by this many rows, negative for up.
    ScrollList(isize),
    /// Move the viewport by whole screens, negative for up.
    Page(isize),
    /// Move the viewport by half screens, negative for up.
    HalfPage(isize),
    /// Move the viewport by this many **changed files**, negative for back.
    File(isize),
    /// Go to the first changed file.
    Top,
    /// Go to the last changed file.
    Bottom,
    /// Engage follow mode, or disengage it.
    ToggleFollow,
    /// Draw the masthead, or stop drawing it.
    ToggleMasthead,
    /// Put the pinned list beside the diff as a left rail, or back above it.
    ToggleRail,
    /// Pin the diff to the file the viewport is inside, or unpin it.
    ToggleSingle,
    /// Show the staged run beside the unstaged one, or stop showing it.
    ToggleStaged,
    /// Wrap a content line too wide for the pane onto the row below, or clip it.
    ToggleWrap,
    /// Draw the gestures sheet, advance it a page, or stop drawing it.
    ToggleSheet,
    /// Stop drawing the gestures sheet, whatever page it is on.
    CloseSheet,
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
            | Self::ToggleMasthead
            | Self::ToggleRail
            | Self::ToggleSingle
            | Self::ToggleStaged
            | Self::ToggleWrap
            | Self::ToggleSheet
            | Self::CloseSheet
            | Self::Escape
            | Self::Redraw
            | Self::Quit => self,
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
            // A resize moves no viewport and expresses no intent, and a pane
            // beside an agent is resized constantly, so treating it as a
            // scroll would disengage follow mode for free. `ToggleFollow` is
            // the reader asking for the opposite of disengaging, and quitting
            // has nothing left to disengage from.
            Self::Quit
            | Self::Escape
            | Self::Redraw
            | Self::ToggleFollow
            // Showing or hiding the masthead resizes the diff's region and does
            // not move the reader inside it, which is a resize by another name
            // and the same answer §11.1 gives one: a resize expresses no intent
            // about what the diff should show.
            // A rail moves the map to the other side of the pane and the diff
            // keeps the row it was on, which is `ToggleMasthead`'s own answer one
            // region over: a resize expresses no intent about what the diff shows.
            | Self::ToggleRail
            | Self::ToggleMasthead
            // **And a pin is the one of the three that can move the viewport,
            // and still expresses no intent about where it should be.** B16 asks
            // for a *subject*, not a position: a screen straddling two files
            // comes to rest on the pinned file's last screenful because that is
            // the nearest legal answer to the position the reader already had,
            // which is the same resolution a diff shrinking under them gets. (The
            // arm that anchors on the way in is what makes that hold from a
            // position a *drag* placed as well as one a scroll did; see
            // `App::apply`.)
            // Calling it a manual scroll would disengage follow for a reader who
            // asked to see one file, which is the pairing the ruling is most
            // useful in: follow chooses the file, the pin keeps the diff on it.
            | Self::ToggleSingle
            | Self::ToggleStaged
            | Self::ToggleWrap
            // And the sheet moves nothing at all: it composites over rows that
            // are already drawn, so it does not even resize a region. B12.
            | Self::ToggleSheet
            | Self::CloseSheet
            | Self::ScrollList(_) => false,
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
    pub fn needs_height(self) -> bool {
        match self {
            // A page steps by a screenful and a half page by half of one, and a
            // drag on the diff's bar maps the track onto everything *but* the
            // last screenful, so all three need to know how tall one is.
            // `ListTo` does not: the list's travel is a question about the *list*
            // rather than about the diff's height, and `App` answers it from the
            // changed set and its own row count. It stopped being the row count
            // outright in [#313](https://github.com/breferrari/vigia/issues/313),
            // where a grouped list gained separators and the travel became
            // `view::last_top`; what has not changed is that this arm needs no
            // height passed to it.
            Self::Page(_) | Self::HalfPage(_) | Self::DiffTo(_) | Self::Bottom => true,
            // `File` steps a file index and lands on a heading, so it is
            // measured in files and never in rows: no height can change where it
            // arrives. `Top` is a heading under a pin too, which is why it stays
            // here and `Bottom` does not.
            Self::Scroll(_) | Self::File(_) | Self::Top | Self::ScrollList(_) => false,
            Self::ListTo(_) | Self::ListRow(_) => false,
            // A toggle changes the region's height; it does not need to be
            // told one to decide what it means.
            Self::Quit
            | Self::Escape
            | Self::Redraw
            | Self::ToggleFollow
            | Self::ToggleMasthead
            | Self::ToggleRail
            | Self::ToggleSingle
            | Self::ToggleStaged
            | Self::ToggleWrap
            | Self::ToggleSheet
            | Self::CloseSheet => false,
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

/// **It takes the key and nothing else, and that is what makes "not a mode"
/// structural rather than a claim.** `SPEC.md` §11.2 B4 refuses a navigable list,
/// B12 reconciles the gestures sheet with it by ruling that no key changes meaning
/// while the sheet is up, and B14 inherits the same for the left rail. None of
/// those needs a gate: this function is handed no shell state, so a key whose
/// meaning depended on one could not be written here without changing the
/// signature, and that is a compile error rather than a red test.
fn key_action(key: &KeyEvent) -> Option<Action> {
    // Windows reports press *and* release; Unix terminals report press only.
    // Acting on both would double every keystroke on one platform and not the
    // other, which is the kind of bug that only ever reproduces for one person.
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
        KeyCode::Char('q') => Some(Action::Quit),
        KeyCode::Esc => Some(Action::Escape),
        KeyCode::Down | KeyCode::Char('j') => Some(Action::Scroll(1)),
        KeyCode::Up | KeyCode::Char('k') => Some(Action::Scroll(-1)),
        // Shift is the modifier because the alternatives are all taken or
        // unreliable: `Ctrl-J` is LF, `Ctrl-C` and `Ctrl-D` already quit, and
        // Alt is intercepted by terminal emulators and by macOS Option. `G`
        // below has already taught a reader that case is load bearing here.
        KeyCode::Char('J') => Some(Action::ScrollList(1)),
        KeyCode::Char('K') => Some(Action::ScrollList(-1)),
        KeyCode::PageDown | KeyCode::Char(' ') => Some(Action::Page(1)),
        KeyCode::PageUp => Some(Action::Page(-1)),
        // `less`'s own half-page pair, and the shell already claims `less +F`
        // semantics one row down, so the precedent is internal as well as
        // cultural. **`Ctrl-D` and `Ctrl-U` are refused rather than overlooked**:
        // `Ctrl-D` quits, four rows up, and rebinding a way out to a scroll is
        // the surprise this map has avoided everywhere else. Plain letters or
        // nothing.
        KeyCode::Char('d') => Some(Action::HalfPage(1)),
        KeyCode::Char('u') => Some(Action::HalfPage(-1)),
        KeyCode::Home | KeyCode::Char('g') => Some(Action::Top),
        KeyCode::End | KeyCode::Char('G') => Some(Action::Bottom),
        // The file granularity the map was missing. Both letters were free: no
        // search exists to claim `n`, and `less` readers carry the next-file
        // reflex from `:n`/`:p` already. `N` and `P` are unbound for the reason
        // `D`, `U` and `F` are, one row above a pair where `g`/`G` have already
        // taught that case is load bearing here.
        // **Aliases in the letter's own arm**, which is the shape `Down |
        // Char('j')` above already has and what stops the two directions drifting
        // apart. Why the arrows and what they spend is `Action::File`'s docblock,
        // not repeated here.
        KeyCode::Right | KeyCode::Char('n') => Some(Action::File(1)),
        KeyCode::Left | KeyCode::Char('p') => Some(Action::File(-1)),
        // **The digits address the drawn window, and there are six of them
        // because `render::LIST_SETTLED` is six.** The digits address the rows
        // **every** pane drawing a list has, so `3` means the same thing at
        // every height, where a key live only above some pane height would be
        // the intermittent affordance `SPEC.md` §11.1 refuses one region over.
        // The rows a taller pane adds are reached with `J`/`K`, `n`/`p` and the
        // pointer.
        KeyCode::Char(digit @ '1'..='6') => Some(Action::ListRow(row_of(digit))),
        // Lower case only, and `G` above is why. `g`/`G` already mean two
        // different things here, so a reader has been taught that shift
        // matters, and folding case would hand `F` a meaning nobody asked for
        // next to a key where case is load bearing.
        KeyCode::Char('f') => Some(Action::ToggleFollow),
        // `m` for masthead, and it was free. Reported from use: *"can we add a
        // shortcut to hide and display this thing at the top? I see it is not
        // always needed"*, which is the honest read of an element that costs
        // four rows of the thing the tool exists to show.
        KeyCode::Char('m') => Some(Action::ToggleMasthead),
        KeyCode::Char('r') => Some(Action::ToggleRail),
        // `s` for single, unbound and in the same lowercase family as `f`, `m`
        // and `r`: the keys that change what the body is made of rather than
        // where in it the reader is. B16.
        KeyCode::Char('s') => Some(Action::ToggleSingle),
        KeyCode::Char('a') => Some(Action::ToggleStaged),
        // **`w`, and it is the reflex rather than what was free.** `ov` binds
        // `[w]`, `[W]` to a character-based wrap toggle, `bat` spells the
        // opposite state `-S` / `--chop-long-lines`, and `less` toggles the same
        // state with `-S`. It was also free, which is the weaker half of the
        // case: `SPEC.md` §11.2 B19.
        KeyCode::Char('w') => Some(Action::ToggleWrap),
        // **`?` and nothing else**, which is `SPEC.md` §11.2's B12: `btop`,
        // `bottom` and `rtop` all open help on it, it was unbound here, and `h`
        // is refused because it is a vi motion everywhere else on a pane with no
        // horizontal scroll. `Esc` is refused too, and that one is a fact about
        // *this* keymap rather than about the convention: `Esc` is Quit four rows
        // up, so teaching it to dismiss would put *dismiss this* one keystroke
        // from *end the program*.
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
    // **The sheet is checked before everything, because it is drawn over
    // everything.** `SPEC.md` §11.2's B12: the close control dismisses, and any
    // other event landing on the sheet does nothing at all. Falling through would
    // let a click seek a scrollbar the reader cannot see and a wheel scroll a
    // diff the sheet is covering, which is the one way an overlay that moves no
    // content could still move content.
    if let Some(sheet) = regions.sheet {
        if sheet.covers(mouse.column, mouse.row) {
            return matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
                .then_some(())
                .filter(|()| (mouse.column, mouse.row) == sheet.close)
                .map(|()| Action::CloseSheet);
        }
    }

    // **The bar is checked before the region it sits in.** A press on the
    // scrollbar column is a gesture about position, and the same column is inside
    // whichever region drew it, so testing the region first would turn every drag
    // into a wheel.
    // **A cell of a bar, not merely its column.** Asking the column alone let one
    // region's bar swallow the other's rows, which `Region::on_bar` records.
    let on_bar = regions.list.on_bar(mouse.column, mouse.row)
        || regions.diff.on_bar(mouse.column, mouse.row);
    if on_bar
        && matches!(
            mouse.kind,
            MouseEventKind::Down(MouseButton::Left) | MouseEventKind::Drag(MouseButton::Left)
        )
    {
        // **A step button answers a press and not a drag**, and that asymmetry is
        // a ruling rather than an omission. A reader who grabbed the thumb and
        // pulled past the end of the track is over a button, and the honest
        // reading of that gesture is *nothing further*: the last track row
        // already reaches the last window, so the view is where they asked for it
        // to be. Stepping instead would make a press-and-jiggle on the top button
        // walk the view up a row per twitch, and clamping to the end would
        // teleport it there; both need to know a drag *began* on a button, which
        // is state, and this module has none by design.
        // **Through `step_at`, which knows whose bar the column is**, since
        // [#251](https://github.com/breferrari/vigia/issues/251). This asked
        // `Regions::step`, which walked both regions' buttons by row alone: with
        // the two bars sharing a column that is the same answer, and beside a rail
        // it is a press on the diff's bar stepping the map.
        if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
            && let Some(step) = regions.step_at(mouse.column, mouse.row)
        {
            return Some(step);
        }
        // **The track, not the region**, so a stepped bar seeks from the rows its
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
        // **The wheel scrolls whatever it is over.** A reader hovering the map
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
        // **A click on a listed file sends the diff to it.** The row is reported
        // as an offset into the window; the app owns where the window is. Only
        // the list, because the diff below is already showing what it is showing
        // and a click on it would have nothing to mean.
        MouseEventKind::Down(MouseButton::Left) if regions.over_list(mouse.column, mouse.row) => {
            Some(Action::ListRow(mouse.row - regions.list.top))
        }
        // Everything else is deliberately inert. Horizontal wheels exist and
        // lines do not pan: the renderer clips instead, which is what I6 asks
        // for. A click on the diff does nothing, because nothing there is
        // selectable in a monitor and §11.2 B4 keeps it that way, and plain
        // movement is not an event worth a frame.
        _ => None,
    }
}
