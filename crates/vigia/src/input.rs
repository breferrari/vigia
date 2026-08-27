//! Terminal events to intentions, as a pure function.
//!
//! Nothing here touches the screen or the repository, which is what makes the
//! whole key map a table test. The loop in [`crate::run`] does the acting; this
//! module only decides what was asked for.

use std::time::{Duration, Instant};

use ratatui::crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};

/// Rows a wheel notch moves.
///
/// Three is what a terminal sends per physical notch when it reports lines
/// rather than pixels, so matching it makes one notch feel like one notch.
pub const WHEEL_ROWS: isize = 3;

/// How long a step button is held before it begins repeating.
///
/// **Taken from the toolkits rather than chosen.** Qt's `QScrollBar` presses a
/// control with `initialDelay = 500` and then runs its repeat timer at `50`;
/// GTK exposes the same pair as `gtk-timeout-initial` and `gtk-timeout-repeat`,
/// the latter defaulting to 50ms. Both landed on the same shape and nearly the
/// same numbers, so a reader arriving from any desktop scrollbar finds the
/// cadence they already have in their hands. Guessing here would have been a
/// number nobody could check.
///
/// The delay is what keeps a single click a single step: without it every press
/// would risk a second row before the finger lifted.
pub const STEP_DELAY: Duration = Duration::from_millis(500);

/// How long between repeats once one has begun.
///
/// See [`STEP_DELAY`] for where the pair comes from. Twenty steps a second,
/// which is a readable scroll rather than a blur.
pub const STEP_REPEAT: Duration = Duration::from_millis(50);

/// A span of rows on the screen, and the part of it a scrollbar's thumb can
/// occupy.
///
/// **One type rather than two parallel tuples, so the pairing cannot be got
/// wrong.** The region and its track are only ever meaningful together: a track
/// belonging to the other region compiles perfectly as a second tuple argument
/// and resolves a press against rows the thumb is not on, which is a bug with no
/// symptom on screen. Carrying them in one value makes that unrepresentable
/// rather than merely untested.
///
/// `rows` of zero means there is no region at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Region {
    /// First row of the region, absolute within the pane.
    pub top: u16,
    /// How many rows it has.
    pub rows: u16,
    /// First column of the region, absolute within the pane.
    ///
    /// **A region has columns since
    /// [#251](https://github.com/breferrari/vigia/issues/251)**, and until then
    /// membership was a row test: `over_list` asked `contains(row)` and the
    /// wheel router and the click arm asked it too. That is sound only while
    /// the list sits *above* the diff, which is the vertical stack this type
    /// exists to stop assuming. Beside a rail both regions hold every row of
    /// the body and only the column says which one a pointer is in.
    ///
    /// Bare `u16`s rather than a `Rect`, matching [`Sheet`] one type down and for
    /// its stated reason: everything in this module is absolute within the pane,
    /// and the module deliberately carries no layout types.
    pub left: u16,
    /// How many columns it has.
    pub width: u16,
    /// First row of its bar's **track**, and how many rows that is.
    ///
    /// **Passed rather than derived, which is this module's whole rule.** A
    /// stepped bar spends its first and last row on step buttons, so the track is
    /// not the region; working that out here would mean knowing the height ladder
    /// that decides it, and a second copy of a threshold is how the pointer comes
    /// to seek from a row the thumb is not on. Equal to `(top, rows)` wherever
    /// the bar has no buttons, so a caller that does not care may ask
    /// unconditionally.
    pub track: (u16, u16),
    /// The column **this region's** bar is drawn in, when it has one.
    ///
    /// **Per region since
    /// [#251](https://github.com/breferrari/vigia/issues/251)**, where it was
    /// one `Option<u16>` on [`Regions`] documented as *"the column both
    /// scrollbars are drawn in"*. That was true only because both regions span
    /// the pane and their bars land on the same right edge, and every reader
    /// then told the two apart **by row**, which is the vertical stack written
    /// into the hit-test model. Beside a rail the two regions share their rows
    /// and differ in their columns, so the row can no longer answer it and the
    /// column already can.
    ///
    /// Paired with `top`, `rows` and `track` here for the reason `track` gives
    /// one field up: a `Region` holding one region's rows beside another's bar is
    /// a bug with no symptom on screen, so the pairing is closed by construction
    /// in `Bar::region` rather than left to a gate.
    pub bar: Option<u16>,
}

impl Region {
    /// A region with no scrollbar buttons, whose track is the whole of it.
    ///
    /// The shape every region had before there were step buttons, and the one a
    /// caller wants whenever the bar is bare or absent.
    ///
    /// **`bar` is the column, not whether there are buttons**, and the two are
    /// different questions since
    /// [#251](https://github.com/breferrari/vigia/issues/251) gave a region its
    /// own bar column. `bare` says this bar has no buttons; `None` here says it
    /// has no bar at all. A fixture that took the first to mean the second would
    /// describe a region a pointer can never grab, which is a screen the renderer
    /// does not draw.
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
    ///
    /// One predicate rather than the three near-copies this module grew: the
    /// same half-open range test decided *is the pointer over the list*, *is it
    /// on a track* and *is it on a button*, and a change to what "inside" means
    /// wanted three edits with gates behind only two of them.
    fn contains(self, row: u16) -> bool {
        Self::within(row, (self.top, self.rows))
    }

    /// Whether `column`, `row` is a cell of **this region's** scrollbar.
    ///
    /// **The column and the rows together**, which is what "on the bar" has
    /// always meant in prose and did not mean in code. `mouse_action`'s gate and
    /// [`Regions::hover_at`]'s asked the column alone, and both bars sat on the
    /// pane's right edge, so once *either* region drew one the whole right column
    /// counted as bar from the top of the body to the bottom. On the most
    /// ordinary screen there is, a handful of files beside a diff too tall for the
    /// pane, that is a list with no bar of its own whose rightmost column stopped
    /// answering: before this it seeked a bar that is not drawn, and after the
    /// per-region columns landed it did nothing at all. Neither is what a click on
    /// a file should do.
    ///
    /// Not used by [`Regions::step_at`] or [`Regions::grab_at`], and that is not
    /// an oversight: each of those pairs the column test with `button` or `along`,
    /// which already answer `None` off their own region's rows, so asking here
    /// would be the containment test twice.
    fn on_bar(self, column: u16, row: u16) -> bool {
        self.bar == Some(column) && self.contains(row)
    }

    /// Whether `column`, `row` falls inside this region.
    ///
    /// **What membership means since
    /// [#251](https://github.com/breferrari/vigia/issues/251)**, where every
    /// caller asked [`Region::contains`] and bounded nothing horizontally.
    /// Spelled the way [`Sheet::covers`] already spells it, because a region and
    /// the sheet over it are the same kind of question asked of the same pane.
    ///
    /// [`Region::contains`] stays for the callers that genuinely mean a row: a
    /// bar's track runs down one column and `along` maps a row on it to an
    /// offset, and by then the column has already been matched.
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
    ///
    /// Written against the **track's** ends rather than a floor of its own, so
    /// the two cannot come apart: a row above the track is the up button and a
    /// row below it is the down one, whatever the ladder decided the track would
    /// be. A bar with no buttons has a track equal to its region, so it can never
    /// answer here.
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
    ///
    /// **The track and never the region**, which are the same span only while a
    /// bar has no step buttons.
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

    /// The same, with a row outside the track pulled to whichever end it is past.
    ///
    /// **What a drag already in progress uses**, and the difference from
    /// [`Region::along`] is the whole of it: `along` answers *is the pointer on
    /// this track*, which is the right question for deciding what a fresh press
    /// means, and the wrong one once a reader is already dragging. A hand does
    /// not stay inside a one-column target while it moves, and a scrollbar that
    /// let go the moment the pointer wandered would be unusable for the gesture
    /// it exists for.
    ///
    /// Above the track is the first window and below it is the last, which is
    /// what every scrollbar does and what a reader dragging past the end is
    /// asking for.
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
///
/// **The one thing this module knows about layout, and it is passed in rather
/// than derived.** Everything else here is a pure function of a key code, which
/// is what makes the whole map a table test; a mouse gesture cannot be, because
/// "scroll the thing under the pointer" is a question about the screen. Handing
/// it the numbers keeps the decision here and the arithmetic testable.
///
/// Rows are absolute within the pane. `Default` is a screen with no region and
/// no bars, which is what a caller that has not laid out yet should say.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Regions {
    /// The pinned file list. Zero rows means there is no region and every
    /// gesture belongs to the diff.
    pub list: Region,
    /// The diff region.
    pub diff: Region,
    /// The gestures sheet, when it is drawn.
    ///
    /// **The only region here that is *over* the others rather than beside
    /// them**, so it is tested first and it swallows what lands on it. `SPEC.md`
    /// §11.2's B12 rules that a click on the close control dismisses and a click
    /// inside the sheet that is not on the control does nothing; the wheel is
    /// swallowed for the same reason, because scrolling a diff a reader cannot
    /// see is worse than doing nothing.
    pub sheet: Option<Sheet>,
}

/// Where the gestures sheet is, so a pointer can be told it is over one.
///
/// Absolute within the pane, like every other row and column in this module.
/// Carried as its own type rather than as a pair of `Option`s on [`Regions`],
/// because the rect and the close control inside it are one fact and two fields
/// could disagree.
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
    ///
    /// **A cell rather than a row since
    /// [#251](https://github.com/breferrari/vigia/issues/251)**, for
    /// [`Region::covers`]'s reason: a row alone answers this only while the list
    /// sits above the diff, and every caller of it here is a gesture that has a
    /// column too.
    fn over_list(self, column: u16, row: u16) -> bool {
        self.list.covers(column, row)
    }

    /// The step a pointer at `column`, `row` is over, whatever it is doing there.
    ///
    /// **Geometry alone, with no event kind in it**, which is what lets three
    /// callers ask different questions of the same cell: [`action_for`] asks
    /// *what does this press mean*, the loop asks *is the pointer still on the
    /// button it is repeating*, and [`Regions::hover_at`] asks *is this a
    /// surface a press would act on at all*. A second copy of this arithmetic
    /// is how they would come to disagree about where a button is.
    ///
    /// **The sheet guard is here rather than at the loop's arming site**, for
    /// that same reason. `SPEC.md` §11.1 rules that a gesture landing on the
    /// sheet does nothing, and the arming site honoured it through neither of
    /// the two callers that did, so a press on a step button the sheet covered
    /// armed a [`Held`] and repeated into a region the reader could not see.
    /// It reaches **85** covered cells, at the four widths narrow enough for a
    /// centred sheet to touch a bar's column. [`Regions::grab_at`] is the loop's
    /// other direct question and carries the same guard.
    /// [`Regions::hover_at`]'s own sheet branch stays, because it answers
    /// [`Hovered::Button`] for the close control where this answers `None` for
    /// every cell: a mark is not a press.
    ///
    /// **`Regions` is a pre-batch snapshot, so this guard has a hole in it.**
    /// `Shell::regions` is written once per painted frame, so a `?` and a press
    /// that coalesce into one batch test the press against a sheet that was not
    /// yet open. It bypasses [`action_for`]'s guard alike and is tracked as
    /// [#307](https://github.com/breferrari/vigia/issues/307).
    ///
    /// **What is airtight needs neither guard.** The close control answers a
    /// press with [`Action::CloseSheet`], so the sheet is gone before the next
    /// paint and no frame draws that cell pressed. `Painter::sheet` accordingly
    /// does not ask.
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
    ///
    /// **The track and not the buttons.** A press on a step button is a step and
    /// is already answered by [`Regions::step_at`]; what this reports is the
    /// gesture that *continues*, so the loop knows to keep routing motion to this
    /// region however far the pointer travels afterwards.
    ///
    /// **The sheet guard is [`Regions::step_at`]'s and arrived with it**. These
    /// two are the only geometry the loop asks `Regions` for directly, outside
    /// [`action_for`], and guarding one and not the other is how the class
    /// recurs rather than closes, leaving the unguarded method holding the
    /// identical shape.
    ///
    /// **It is the worse of the two, which is why it is not merely symmetry.** A
    /// hold repeats a bounded step every [`STEP_REPEAT`]; a grab hands the whole
    /// gesture to [`drag_action`], which ignores the column by design so a reader
    /// pulling a one-column bar does not lose it, so the next motion relocates a
    /// region the sheet is covering to wherever the pointer went. The sheet is
    /// centred on both axes, so wherever it reaches a bar's column it covers the
    /// **track** rows between the buttons as well, which are more cells than the
    /// buttons themselves.
    ///
    /// A grab that began before `?` was pressed is already ended by
    /// [`Grabbed::ends`], which ends on any key, so this closes the remaining half
    /// rather than half of a half.
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
    ///
    /// **Defined as *wherever a press would step*, which is the ruling rather
    /// than an optimisation.** §11.1 licenses the mark on "the surfaces a click
    /// already acts on", so this asks [`Regions::step_at`] the question it
    /// already answers. Spelling the column guard and the two `Region::button`
    /// calls again would be a second copy of where a button is.
    ///
    /// **Three answers, and the order is the whole of it.** A step button, then
    /// the bar it sits on, then a listed file. The column is tested before the
    /// list because the scrollbar is drawn *inside* whichever region owns those
    /// rows, so asking the list first would mark a file the reader is pointing
    /// past. **The diff's rows answer nothing**, which is the one region §11.1
    /// keeps unmarked: nothing there is clickable and a mark would imply it is.
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
        //
        // **Either region's bar, asked as one branch**, so a pointer on a bar
        // column never falls through to the list below. That is the whole of the
        // precedence above, and splitting it into two branches would let a cell
        // that is one region's bar but nobody's track answer `Row`.
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
///
/// **Four facts that always travel together**, and bundling them is the same
/// ruling `View::take_file`'s `Changed` records one crate over: they are decided
/// once per turn by the loop, every one of them is `None` on almost every frame,
/// and passed separately they take `App::chrome` past the argument count clippy is
/// willing to read — which is a real signal here rather than a lint to silence,
/// since the alternative is seven `None`s at a call site where transposing two of
/// them compiles.
///
/// `Default` is the common case: nothing pressed, nothing dragged, nothing hovered
/// and nothing scrolling.
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
///
/// **An enum with one variant, on purpose.** [`Hovered::Button`] carries a cell
/// the way `Chrome::pressed` does, so the drawer compares one kind of thing;
/// what is not here yet is a variant for the thumb and one for a list row, both
/// of which are surfaces a click acts on and both of which are waiting on the
/// same ruling about what mark a region with no spare rung gets. A bare
/// `Option<(u16, u16)>` would say the mark is always a cell, which is the thing
/// that is about to stop being true.
///
/// **This is view state and never an [`Action`]**, which is §11.1's ruling and
/// the reason `action_for` never learns about it: the mark says where a pointer
/// is, it is not a thing a reader asked for, and B4 stands because no key means
/// anything different while it is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hovered {
    /// A one-cell control, by the cell it is drawn on.
    ///
    /// **The scrollbars' step buttons and the sheet's close control**, which are
    /// the same thing from a pointer's side: one cell, a glyph in the chrome's own
    /// weight, and a click that acts immediately. Named for the shape rather than
    /// for the scrollbar since B12 gave the pane its second one.
    Button(u16, u16),
    /// A bar, by the region it belongs to.
    ///
    /// **The track and the thumb are one target**, because a press anywhere on a
    /// track seeks: the surface a click acts on is the whole column, and the
    /// thumb is what answers because it is what would move.
    ///
    /// **Carried as the region rather than as the region's first row**, which is
    /// what this was until [#254](https://github.com/breferrari/vigia/issues/254).
    /// A row is an identity only while the two regions are stacked and their tops
    /// differ. [#252](https://github.com/breferrari/vigia/issues/252) puts the
    /// list beside the diff, where both start on the same row and a comparison
    /// against one top starts matching for both bars at once. [`Grabbed`] is the
    /// name for *which of the two bars* that the shell is already holding, so the
    /// mark carries that and the drawer is told which bar it is drawing instead
    /// of inferring it from the `Rect` it was handed.
    Track(Grabbed),
    /// A listed file, by the screen row it is drawn on.
    ///
    /// A row rather than an index into the list, for the reason a button is a
    /// cell: the drawer is handed a `Rect` per row and knows its own `y`, where
    /// an index would make it re-derive which entry it was given.
    Row(u16),
}

/// The mark after `event`, given the one before it.
///
/// **A free function for the reason [`Held::ends`] is one**: the loop that owns
/// this state cannot be driven by a test, so a rule written inline in `run` is a
/// rule with no gate.
///
/// §11.1's rule is that a mark is retired by whatever the program can still
/// observe about its subject, and a hover's subject **moves** rather than
/// ending. Four cases, the last two being the ones that are not obvious:
///
/// - **Any mouse event that is not a drag re-resolves.** Motion, press, release
///   and the wheel all carry a column and a row, so all of them place the mark
///   and one landing on no target clears it.
/// - **A drag clears it, and is the one mouse event that does not re-resolve.**
///   A reader pulling a grabbed thumb travels over the step button at that end
///   of the track, and lighting it would promise a step that releasing there
///   will not perform, because [`Grabbed`] owns the gesture until the button
///   comes up. Nothing is left stuck dark: the `Up` that ends the drag carries a
///   position and re-arms the mark.
/// - **[`Event::FocusLost`] clears it.** The window is gone, so the pointer's
///   last known position has stopped being a claim about anything. Without
///   `TAKEOVER`'s focus step this arm never fires on Unix.
/// - **Everything else leaves it alone, and keys above all.** Deliberately not
///   [`Held::ends`]'s rule, which ends a hold on any key because a hand that has
///   reached the keyboard is not holding a mouse button. That is evidence about
///   a **button** and none at all about where a pointer rests; clearing here
///   would flicker the mark off under a reader scrolling with `j`.
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
///
/// **A mark outlives a frame; it does not outlive a relayout.** [`hover_after`]
/// resolves against the geometry on screen, so the mark is only a claim about
/// *that* screen. When a tick moves the bars, every cell it named may now belong
/// to something else.
///
/// **Any change to the layout retires the mark**, and the next motion re-arms it
/// against current geometry. Two narrower rules look sufficient and are not.
/// *Keeping the cell and trusting it* fails in exactly the residual case: a list
/// of six with its down button on row 6, the pointer resting and leaving, two
/// files reverted, and row 6 is now the diff's up button, lit without ever
/// having been hovered. *Re-validating the cell against the new layout* is a
/// **tautology** — `Regions::hover_at` builds its answer out of its own
/// arguments, so `hover_at(c, r) == Some(mark)` reduces to
/// `hover_at(c, r).is_some()`, which catches *this is no longer a button* and is
/// blind to *this is now a different button*.
///
/// The cost is a mark dropped while it was still correct, and that is the
/// conservative direction: the mark says *the pointer is here*, and after a
/// relayout nothing in this process knows whether it still is.
///
/// Called from the paint rather than from the next frame's chrome, so the
/// staleness cannot outlive the frame that caused it. On an idle tree there is
/// no next frame: I1 means a wrong mark left for "one frame" could sit lit until
/// somebody writes to the worktree.
pub fn hover_repainted(was: Option<Hovered>, before: Regions, after: Regions) -> Option<Hovered> {
    (before == after).then_some(was).flatten()
}

/// Which of the two scrollable regions a mark belongs to.
///
/// **A gesture outlives the target it began on**, which is why this exists and
/// is all it meant at first: a drag. [`Regions`] answers *what is under the
/// pointer now*; once a button is down that question has stopped being the
/// relevant one, and the answer that matters is *what did this drag start on*.
/// Keeping the two apart is what lets the pointer wander off the bar's one
/// column without the drag ending, and [`drag_action`] is where it is spent.
///
/// **It names the region for all three paint marks since
/// [#254](https://github.com/breferrari/vigia/issues/254), and only one of the
/// three is a drag.** [`Hovered::Track`] is a pointer resting on a bar and
/// `Chrome::scrolling` is a key burst with nothing under a finger. Carrying the
/// region's **first row** instead tells the two regions apart only while they
/// are stacked: every shipped layout stacks them and
/// [#252](https://github.com/breferrari/vigia/issues/252) is the one that does
/// not. So this is wider than its own name now, and says *which bar* whatever
/// asked.
///
/// **Renaming it was considered and rejected.** `Bar` is taken by `render::Bar`,
/// the `None`/`Bare`/`Stepped` style enum, and a second two-variant type
/// carrying the same fact would need a conversion at every boundary to buy
/// nothing. A rename is a naming question and this was a defect fix, so the
/// wider docblock carries the width instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grabbed {
    /// The pinned list's bar.
    List,
    /// The diff's bar.
    Diff,
}

impl Grabbed {
    /// Whether `event` ends the grip.
    ///
    /// **A free function's worth of rule, moved out of the loop for
    /// [`Held::ends`]'s reason**: `run` cannot be driven by a test, so a
    /// retirement rule written inline there is a rule with no gate. This one sat
    /// inline from [#183](https://github.com/breferrari/vigia/issues/183) until
    /// [#186](https://github.com/breferrari/vigia/issues/186) added a third mark
    /// and made the omission visible, since the pass that argued the point for
    /// `hover_after` had a counterexample one screen above it.
    ///
    /// **Only a left-drag continues a grip**, so a release, any key, and a
    /// pointer that moved with nothing down all end it. That is deliberately
    /// coarser than [`Held::ends`], which has to distinguish five cases: a grip
    /// is *already* the answer to "what is this gesture about", so anything that
    /// is not more of the same gesture finishes it.
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
///
/// A press on a track is answered by [`action_for`] like any other press. Every
/// motion after it comes here instead, because by then the reader is holding
/// something and the pointer's column has stopped being a question about intent.
/// Dragging out of the one-column target is what a hand does; ending the gesture
/// there was the defect this function exists to remove.
///
/// The row is clamped rather than tested, so pulling above the track holds the
/// first window and pulling below it holds the last. Returns `None` for anything
/// that is not a motion, so a release falls through to the caller that ends the
/// grip.
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
///
/// **The routing the arrows draw from, and it is a fact about the key map rather
/// than about the screen**, which is why it lives here beside the map itself.
/// The two regions move different things and answer different keys: `j`, `k`,
/// `d`, `u`, `Space`, `g` and `G` move the diff's viewport, `J` and `K` move the
/// list's window. An arrow that lit from a bare direction lit the matching one on
/// both bars at once, which is what 0.5.0 shipped.
///
/// The [`Grabbed`] is which bar, which is what a drawer is told rather than left
/// to infer from the `Rect` it was handed. It was the region's first row until
/// [#254](https://github.com/breferrari/vigia/issues/254), for the reason
/// [`Hovered::Track`] records: a top is an identity only while the regions are
/// stacked. A region with no rows still reports nothing, and the guard is why
/// this reads the layout at all rather than the action alone: with no list on
/// screen there is no bar to light, so an unguarded answer would mark a movement
/// of a map nobody can see.
///
/// A free function rather than a method on the shell, for the reason
/// [`patience`] is one: the shell owns a terminal and three threads, and the
/// routing is exactly the part worth driving from a test.
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
///
/// **A free function for `patience`'s reason, one paragraph down: the shell owns
/// a terminal and three threads, so a decision left inside it has no gate.** It
/// had none. `Shell::settle_scroll` carried this comparison and a bool that its
/// only caller discarded, and inverting the comparison to `now < until` left the
/// whole suite green while the arrows never claimed a direction at all: the mark
/// cleared on the next wake instead of `SCROLL_LINGER` later.
///
/// `>=` rather than `>`, so a mark whose deadline is exactly now is spent. The
/// loop is woken *by* that deadline, so the boundary case is the ordinary one
/// here rather than a corner.
pub fn settled(linger: Option<Instant>, now: Instant) -> bool {
    linger.is_some_and(|until| now >= until)
}

/// How long the loop may block before some clock here has to act.
///
/// **`None` is the whole invariant, and it is why every clock is asked through
/// one function.** With nothing held, nothing lingering and nothing left in the
/// history window this returns `None`, the loop's receive is untimed, and I1's
/// *0 wakeups while idle* is a fact about the structure rather than about care
/// taken at three call sites. Deadlines asked separately would be that many
/// chances to leave one armed on an idle monitor, and the loop's own source gate
/// can see which branch is taken but not what fed it.
///
/// A free function rather than a method on the shell so it can be driven by a
/// test: the shell owns a terminal and three threads, and an invariant that can
/// only be exercised by starting the program is an invariant with no gate.
///
/// Where several are armed it is the nearest, because the loop wakes for
/// whichever comes first. Saturating, so a deadline already past asks for zero
/// rather than panicking.
///
/// `ageing` is [`vigia_core::History::ages_in`], which is `None` for a window
/// holding nothing and so cannot arm this on an idle tree. It is passed as a
/// duration rather than as a `History` so this stays a free function over three
/// values, which is what lets a test drive every combination of them.
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
///
/// **A clock, and its bounds are the whole of why it is allowed.** `SPEC.md`
/// §11.1 carries the ruling; the short version is that I1's budget is *zero
/// wakeups while idle*, this timer exists only between a press on a step button
/// and its release, and a held button is not idle. [`Held::wait`] is the seam
/// that keeps that true rather than merely intended: with nothing held it hands
/// back `None`, and the loop blocks on an untimed receive exactly as it did
/// before any of this existed.
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
    ///
    /// **The one function that decides whether this program owns a timer**, and
    /// it is written to be called with `None` so the answer is a value rather
    /// than a control-flow accident. `None` means an untimed receive: no
    /// deadline, no wakeup, no cost. That is the whole of I1's budget, and
    /// `the_loop_waits_untimed_with_nothing_held` is what fails if it stops
    /// being true.
    ///
    /// Saturating, so a deadline already past asks for zero rather than
    /// panicking on the subtraction.
    pub fn wait(held: Option<Self>, now: Instant) -> Option<Duration> {
        held.map(|held| held.due.saturating_duration_since(now))
    }

    /// The step now due, scaled by how many intervals have actually elapsed, and
    /// the state to carry forward.
    ///
    /// **Scaled rather than one step per wake, and that is what makes the rate a
    /// fact about time instead of about paint speed.** `Action::Scroll` already
    /// carries a row count, so a tick that arrives three intervals late applies
    /// `Scroll(3)` in one `apply` and one paint rather than queueing three of
    /// each. A slow terminal therefore scrolls at the same rows per second as a
    /// fast one, with coarser granularity, instead of accumulating a backlog it
    /// works off after the reader lets go.
    ///
    /// Returns `None` before the first repeat is due, so a press that is
    /// released inside [`STEP_DELAY`] is exactly one step.
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
    ///
    /// Five ways, and the third and the fifth are the ones that close a hole
    /// rather than stating the obvious:
    ///
    /// - **A release.** The ordinary case, and any button rather than the left
    ///   one, because a reader who has pressed a second button is done with this
    ///   gesture whichever they lift.
    /// - **Any key.** A hand that has reached the keyboard is not holding a
    ///   mouse button, and `q` in particular must not be answered by a program
    ///   still stepping.
    /// - **A pointer move.** `MouseEventKind::Moved` is motion *with no button
    ///   down*, which is positive evidence of a release rather than an absence
    ///   of evidence. `crossterm`'s bundle sets `?1003h`, so it arrives on the
    ///   first pixel of movement, and this is what catches an `Up` that never
    ///   came: without it a lost release would leave the loop stepping until
    ///   something else happened to wake it.
    /// - **A drag off the button.** A twitch inside the same cell keeps the
    ///   repeat, which is why the press's own position is carried; leaving the
    ///   control ends it. Resuming on re-entry is what a desktop scrollbar does
    ///   and is deliberately not done here, because it needs a suspended state
    ///   this module has nowhere to put.
    /// - **The window losing focus.** The fifth, and the one this list was short
    ///   by until [#186](https://github.com/breferrari/vigia/issues/186). A
    ///   release is not the only way a gesture ends: a reader who tabs away with
    ///   a button down sends no `Up`, and the repeat went on stepping and
    ///   repainting a pane nobody was looking at. It is owed to I1's second
    ///   condition, that a clock may not outlive the gesture that armed it, and
    ///   it was reachable on Windows from the day the repeat shipped, because
    ///   that console delivers the event whether or not anyone asks.
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
            //
            // **It became reachable on Unix with
            // [#186](https://github.com/breferrari/vigia/issues/186)**, which put
            // `Step::FocusChange` in the takeover so `FocusLost` arrives at all;
            // on Windows the console has always delivered it. Of the four marks
            // this shell keeps, three now retire here and the fourth is
            // `scrolling`, which expires on its own clock.
            Event::FocusLost => true,
            _ => false,
        }
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
    /// Move the viewport by half screens, negative for up.
    ///
    /// Its own action rather than a fraction on [`Action::Page`], and the two
    /// steps are the reason: a page moves `height - 1` rows and this moves
    /// `height / 2`, so one is not the other scaled. Half of `Page`'s own step
    /// would also inherit the overlap row, which a half screen does not want and
    /// `App::apply` says why.
    ///
    /// The separate variant is what forces the two exhaustive matches below to
    /// rule on it, which is the whole reason they are written out.
    HalfPage(isize),
    /// Move the viewport by this many **changed files**, negative for back.
    ///
    /// The granularity the map was missing. Rows are `j`/`k`, screens are
    /// `Space`/`d`, the two ends are `g`/`G`, and none of them is the unit the
    /// pinned list draws, the header counts and the reader thinks in.
    ///
    /// A signed step rather than two directional variants, which is what
    /// [`Action::Page`] and [`Action::HalfPage`] already are: one arm in
    /// `App::apply`, one rule, and no way for the two directions to drift apart.
    ///
    /// **`←` and `→` are aliases of `p` and `n` since `SPEC.md` §11.2 B15**:
    /// vertical keys move inside the diff and horizontal keys move between
    /// files, so the four arrows are one set rather than half of one. A reader
    /// reaching for an arrow was getting `Shift+↑`'s list scroll, which is
    /// `Action::ScrollList` doing exactly what §11.1 says and not what was
    /// wanted.
    ///
    /// **The one thing the arrow pair conflicts with is declined rather than
    /// deferred.** §11.1 rules a long line clipped rather than wrapped and names a
    /// horizontal pan as the alternative it did not take, *"a key and a mode this
    /// spec does not name"*.
    /// [#272](https://github.com/breferrari/vigia/issues/272) reopens reading long
    /// lines and asks for `w`, a wrap toggle, which wants no arrows. If a pan is
    /// ever ruled in it takes `h` and `l`, or it takes these back.
    ///
    /// **Not [`Action::ListRow`] by another route.** That one names a row of the
    /// *window* and needs the app's `list_top` to mean anything; this names a
    /// file relative to where the diff already is, and works with no region on
    /// screen at all.
    File(isize),
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
    /// Draw the masthead, or stop drawing it.
    ///
    /// **A reader's own gesture, which is what licenses it to move content.**
    /// `SPEC.md` §11.1 keeps the body split a function of pane height, footer
    /// height and changed-file count, all of which change only when the diff
    /// does, so that no transient thing jogs a reader's diff. A keypress is the
    /// opposite of transient: it is an instruction, and the same ruling already
    /// lets `f` change what the footer says and `J` move the map.
    ///
    /// Its own action rather than a flag, for [`Action::ToggleFollow`]'s reason
    /// one element over: the band is worth four rows of diff on a short pane and
    /// nothing at all on a tall one, and a reader who has decided which is what
    /// this asks.
    ToggleMasthead,
    /// Put the pinned list beside the diff as a left rail, or back above it.
    ///
    /// `SPEC.md` §11.2 **B14**, from `r`.
    /// [#252](https://github.com/breferrari/vigia/issues/252) shipped the rail
    /// arriving on its own at 134 columns, and what that meant is that a reader
    /// who had not asked for a narrower diff got one: at 133 the diff plans
    /// against 129 columns and at 134 against 60. The width is still #252's and
    /// still derived; crossing it is now a gesture.
    ///
    /// **Below 134 it changes nothing and eats nothing**, exactly as
    /// [`Action::ToggleMasthead`] does on a pane that cannot carry the band. The
    /// state is kept either way, so a reader who asked for a rail on a wide pane,
    /// narrowed it and widened it again gets the rail back rather than having to
    /// ask twice.
    ///
    /// It is **not a mode**. No key changes meaning while the rail is up, which is
    /// what keeps it out of B4's refusal the way B12's sheet is kept out of it: the
    /// list is still not navigable, it is merely somewhere else.
    ToggleRail,
    /// Pin the diff to the file the viewport is inside, or unpin it.
    ///
    /// `SPEC.md` §11.2 **B16**, from `s`. While it is on, the diff is the one
    /// file the caret marks: scrolling clamps at that file's two ends instead
    /// of carrying on into the file below, and the scrollbar measures the file
    /// rather than the changeset.
    ///
    /// **It is not a mode**, which is what keeps it out of B4's refusal the way
    /// [`Action::ToggleRail`] and B12's sheet are kept out of it. No key changes
    /// meaning while it is on: `j` and `k` still scroll a row, `Space` still
    /// pages, a digit still jumps. They have one file's worth of rows to move
    /// through, and `n`, `p`, a digit and a click on a listed file are still how
    /// the file changes.
    ///
    /// **`g` and `G` are the case that has to be argued rather than asserted.**
    /// They mean the first and last row of the pinned file, which is the same
    /// meaning they always had — *the ends of what you can scroll to* — over the
    /// subject the pin narrows. `G` reaching a real last **row** is affordable
    /// here and nowhere else: the pinned file has been diffed by definition,
    /// where finding the last row of the whole diff means adding up every file's
    /// height, which is the read I4 forbids and which is why `G` unpinned is the
    /// last file's *heading*.
    ///
    /// **Follow still moves between files while it is on**, because `f` and an
    /// automatic follow are explicit requests to be moved and this is about what
    /// a reader's own scrolling reaches. It is not a manual scroll for the same
    /// reason [`Action::ToggleRail`] is not: it expresses no opinion about where
    /// the viewport should be.
    ToggleSingle,
    /// Show the staged run beside the unstaged one, or stop showing it.
    ///
    /// `SPEC.md` §11.2 **B17**, from `a`. `a` for *all*: with it on the pane
    /// holds everything uncommitted, and the issue's own key note names it for
    /// exactly this case — `i` for *index* was the proposal while the toggle
    /// was expected to swap one comparison for the other, and it adds rather
    /// than swaps.
    ///
    /// **Unlike every other toggle here, this one changes what is *walked* rather
    /// than what is drawn.** `Action::ToggleMasthead`, `Action::ToggleRail` and
    /// `Action::ToggleSingle` all rearrange rows the frame already holds; this
    /// adds a second status walk and a second set of diffs, so it goes through
    /// `vigia_core::Frame::show_staged` rather than living in the shell's own
    /// state alone.
    ///
    /// **It moves the viewport, and that is deliberate.** The other three keep the
    /// reader's row, because the row still names the same file afterwards. Here the
    /// changed set itself grows or shrinks underneath the position, so the row a
    /// reader was on is not the row they would be on; `App::apply` sends the
    /// position to the top, which is the answer `Action::Top` already gives for
    /// *the map you were reading is not the map any more*.
    ToggleStaged,
    /// Wrap a content line too wide for the pane onto the row below, or clip it.
    ///
    /// `SPEC.md` §11.2 **B19**, from `w`. Off on launch.
    ///
    /// **It is not a mode**, for the reason [`Action::ToggleRail`] and
    /// [`Action::ToggleSingle`] are not: no key changes meaning while it is on.
    /// `j` and `k` still scroll a row of the diff, a digit still jumps, `G` still
    /// reaches the end. What moves is how much of the pane a long line is drawn
    /// over, and the diff's own rows are exactly the rows they were, which is why
    /// the scrollbar's thumb does not move when this is pressed.
    ///
    /// **It rearranges rows the frame already holds**, so it is
    /// [`Action::ToggleMasthead`]'s kind rather than [`Action::ToggleStaged`]'s:
    /// nothing is re-walked, nothing is re-read, and the position is kept. **The
    /// position is kept except where the reader is resting on the diff's last
    /// screenful**, where the clamp gives rows back so the last row stays on the
    /// last row; the total the bar is drawn against is unmoved either way.
    ToggleWrap,
    /// Draw the gestures sheet, advance it a page, or stop drawing it.
    ///
    /// `SPEC.md` §11.2's B12 ruling, from `?` or from a click on the sheet's own
    /// close control. **The one action on this map that moves nothing**: the
    /// sheet composites over cells that are already drawn, so unlike
    /// [`Action::ToggleMasthead`] it does not even resize a region, which is the
    /// whole reason the keymap went over the pane rather than into the footer.
    ///
    /// **Three outcomes since §11.2 B13**, and which one it takes is the
    /// receiving state's rather than this variant's: closed opens the first
    /// page, a page with another after it advances, and the last page closes.
    /// On every pane whose sheet is one page, which is every pane the first
    /// three rungs of the ladder serve, that is the toggle it has always been.
    /// The close control still closes from any page, so a reader who wants out
    /// of a six-page sheet is never made to walk it.
    ///
    /// **The name is kept rather than widened to `AdvanceSheet`.** What a reader
    /// presses `?` for is *the sheet*, and one variant is what says the key has
    /// one meaning; splitting it would put the ladder's arithmetic into the
    /// keymap, which is the layer that has no pane to measure it against.
    ///
    /// It is not a mode. **Every *other* key keeps its meaning while the sheet is
    /// up**, so nothing here becomes context-dependent and `Esc` still quits.
    /// That is the sentence B13 was ruled against, and it is why a sheet that
    /// **scrolled** was declined: `j`, `k`, `Space`, `d` and `u` are all spoken
    /// for, and taking one back is the mode B12 refused.
    ToggleSheet,
    /// Stop drawing the gestures sheet, whatever page it is on.
    ///
    /// **From the close control alone**, which is the difference between it and
    /// [`Action::ToggleSheet`] and the reason B13 needs two variants where B12
    /// needed one. `?` means *the sheet*, so on a paged pane it advances; the
    /// control means *close*, and a control that walked the reader through five
    /// more pages before letting them out is not a close control. `SPEC.md` §11.1
    /// says the control dismisses it, and until
    /// [#286](https://github.com/breferrari/vigia/issues/286)'s audit both the
    /// spec and this file said so while the click sent `ToggleSheet` and paged.
    ///
    /// It is the sheet's only pointer escape, and the pointer has no `?`.
    CloseSheet,
    /// Leave the frontmost thing: the gestures sheet if one is up, and the
    /// program if none is. `Esc`.
    ///
    /// **One key, one meaning, and the state is resolved where state lives**.
    /// `SPEC.md` §11.2 B12 rules that no key changes meaning while the sheet is
    /// up, and [`key_action`] is handed no shell state so that the rule is
    /// structural rather than remembered. Both survive: this action *is* one
    /// meaning, the one every terminal program gives `Esc`, and which thing is
    /// frontmost is a question only [`crate::App`] can answer. `q` is untouched
    /// and still quits from anywhere, which is what a reader who wants out of
    /// the program presses.
    ///
    /// It was `Quit` outright until #340, reported from a real pane: a reader
    /// pressed `Esc` to put the help away and the monitor exited. That is the
    /// worst shape a mistake can take here, because the sheet is the surface a
    /// reader opens precisely when they are unsure what a key does.
    Escape,
    /// Put the pinned list's window at this fraction of the changed set.
    ///
    /// From dragging or clicking the list's own scrollbar. A fraction over
    /// [`TRACK_SCALE`] rather than a file index, because the index needs the file
    /// count and this module does not have one.
    ListTo(u32),
    /// Put the diff at the file this many rows down the pinned list.
    ///
    /// From a click on a list row, or from one of the digits `1`-`6`. An
    /// **offset into the window**, not a file index, because the window's
    /// position is the app's state and this module does not have it.
    ///
    /// **One variant for both, because they are one intention.** A digit names
    /// the row it is drawn beside, which is the same sentence a click makes with
    /// a pointer instead of a key: *what you can see is what you can name*. A
    /// second variant would be a second spelling needing the same guard in the
    /// same arm.
    ///
    /// A click is the one gesture a reader will try without being told, and it
    /// is still not selection: nothing is remembered, no row becomes special,
    /// and the next event is interpreted exactly as it would have been. That is
    /// what `SPEC.md` §11.2 B4 refuses, and it is untouched. The same argument
    /// already licensed dragging a scrollbar.
    ///
    /// **An offset past the drawn window is possible from a digit and not from a
    /// click**, since [`Regions::over_list`] bounds a pointer to the region and
    /// nothing bounds a keystroke. `App::apply` is where that is caught, because
    /// how many rows were drawn is the app's state and not this module's.
    ListRow(u16),
    /// Put the diff at this fraction of its total height.
    ///
    /// From dragging or clicking the diff's scrollbar. **A row, once the caller
    /// resolves it**, not a file: I4 is narrowed precisely so the diff's total
    /// row count can be counted rather than approximated, and a
    /// gesture performed on a row-exact readout has to land as precisely as the
    /// readout claims.
    DiffTo(u32),
    /// Draw again with no state change, which is what a resize needs.
    Redraw,
}

impl Action {
    /// This action performed `times` over, as a single action where that is
    /// possible.
    ///
    /// **The seam that makes holding a control a general facility rather than a
    /// scrollbar feature.** [`Held`] repeats whatever it was armed with, and this
    /// is where each action says how it composes with itself. Nothing about the
    /// mechanism knows what a step button is: arm it with any action and the loop
    /// will repeat that action, which is what a future held control gets for
    /// free.
    ///
    /// **Folding rather than queueing is the performance half.** A repeat that
    /// arrives three intervals late applies `Scroll(3)` through one `App::apply`
    /// and one paint, instead of three of each. That makes the scroll rate a fact
    /// about elapsed time rather than about how fast the terminal can draw: a
    /// slow pane moves the same rows per second as a fast one, with coarser
    /// granularity, and can never accumulate a backlog that keeps moving after
    /// the reader lets go.
    ///
    /// Exhaustive rather than a wildcard, for the reason
    /// [`Action::is_manual_scroll`] gives one function down: an action added
    /// later stops this compiling and asks whether holding it means anything,
    /// where a wildcard would silently answer "repeat it unchanged" and be wrong
    /// for anything that is not a relative step. **The ones that return `self`
    /// are rulings, not defaults**: a page or a half page held down is already
    /// the reader's own key repeat, and `Top`, `Bottom` and `File` name a
    /// destination that does not accumulate.
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
            //
            // **`ScrollList` is a ruling rather than an oversight**, and this
            // exhaustive match is what forced it to be made. Follow is a claim
            // about the *diff* viewport; moving a window over the map of it
            // expresses no intent about what the diff should show, exactly as a
            // resize does not. Browsing the changed set while the diff goes on
            // following what an agent is writing is the monitor behaviour, and
            // the two would fight if one disengaged the other. `SPEC.md` §11.1.
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
    ///
    /// **It is *reads it in some reachable state*, not *always reads it*.**
    /// [`Action::Bottom`] is measured in files unpinned, where it lands on a
    /// heading and no height can move it, and in **rows** under a pin, where it
    /// rests the file's last row on the bottom. So it answers `true` and pays a
    /// height it ignores half the time, which is the conservative direction: the
    /// other reading hands it a zero in the state that reads one, and
    /// `saturating_sub(0)` then quietly means *the whole file*. No other action
    /// is state-dependent, which is checked rather than assumed.
    ///
    /// This exists because the answer is **expensive**, not because it is
    /// interesting. Deriving the height costs an uncached terminal-size syscall
    /// plus a `Chrome`, and a drained gesture can be sixty-four actions between
    /// two frames, so paying it per action would put the syscall back on the
    /// path the drain took it off.
    ///
    /// Exhaustive rather than a `matches!` list, for the reason
    /// [`Action::is_manual_scroll`] gives: a new action stops this compiling and
    /// asks, where a list would silently answer "no".
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
            //
            // **`Bottom` joined them with `SPEC.md` §11.2 B16**, and this arm
            // is the whole of what makes that ruling reach a reader. Unpinned
            // it is `jump_to(len - 1)`, which lands on a heading and is
            // measured in files exactly as the comment below says; **pinned it
            // rests the file's last row on the bottom**, and *the bottom* is a
            // height. Left in the arm below, `crate::run` hands it a height of
            // zero, the resting row saturates back to the file's whole span,
            // and every keystroke batched behind `G` in the same wake is
            // swallowed by the walk's own clamp. That is the defect B16 records
            // as fixed, still shipping, with the gate green because a test
            // calls `App::apply` with the height a shell does not pass.
            // `crates/vigia/tests/scroll.rs::only_the_action_that_reads_the_height_is_given_one`
            // is where the two are held together, and it now runs pinned as
            // well.
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

/// **It takes the key and nothing else, and that is what makes "not a mode"
/// structural rather than a claim.** `SPEC.md` §11.2 B4 refuses a navigable list,
/// B12 reconciles the gestures sheet with it by ruling that no key changes meaning
/// while the sheet is up, and B14 inherits the same for the left rail. None of
/// those needs a gate: this function is handed no shell state, so a key whose
/// meaning depended on one could not be written here without changing the
/// signature, and that is a compile error rather than a red test.
///
/// A gate for B14 here is **tautological**: it compares this function's answers
/// either side of a state change it cannot see. The claim is recorded here
/// instead, where the next person to reach for the
/// state is standing. What a rail genuinely moves is the **pointer**'s regions, and
/// `tests/rail.rs::a_pointer_at_the_rails_own_columns_reaches_the_region_it_is_over`
/// is the gate for that.
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
        KeyCode::Char('q') => Some(Action::Quit),
        KeyCode::Esc => Some(Action::Escape),
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
        // `less`'s own half-page pair, and the shell already claims `less +F`
        // semantics one row down, so the precedent is internal as well as
        // cultural. **`Ctrl-D` and `Ctrl-U` are refused rather than overlooked**:
        // `Ctrl-D` quits, four rows up, and rebinding a way out to a scroll is
        // the surprise this map has avoided everywhere else. Plain letters or
        // nothing.
        //
        // Below the CONTROL arm above, which returns, so `Ctrl-D` cannot reach
        // here. `D` and `U` are unbound for the reason `F` is: `g` and `G`
        // already teach that case is load bearing on this map.
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
        //
        // The bound is **restated rather than imported**: everything else in
        // this module is a pure function of a key code, and reaching into the
        // renderer for a layout constant would end that. `tests/input.rs`
        // presses every digit `1..=LIST_SETTLED` and asserts the one after it is
        // unbound, so moving the settled cap goes red here instead of leaving a
        // drawn row unreachable.
        //
        // `0` and `7`-`9` stay unbound rather than becoming out-of-range jumps:
        // an unbound key is no action at all, where a bound one naming a row
        // that is not drawn is the empty-list-space case and disengages follow
        // like any other jump.
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
///
/// Total rather than fallible, and the call site is why: the only one is a
/// `'1'..='6'` match arm, where `to_digit` cannot fail. A monitor has no useful
/// answer to an impossible input beyond the first row, and it certainly has no
/// business panicking on a keystroke, so the one unreachable branch falls there
/// rather than being propagated to a caller that would only have to discard it.
///
/// The narrowing cannot fail either and so is not written as though it might:
/// base ten yields at most nine, which is a `u16` several times over.
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
        //
        // **Each gated on its own region's bar**, for the reason above: the row
        // says where along a track a press landed, and only the column says which
        // track it was.
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
        //
        // It is **not** "the one place this shell reads a pointer's position
        // rather than only its kind": the bar test above reads `mouse.column`,
        // and the click arm below reads
        // `mouse.row` through the same `over_list`. What is true is that reading
        // the position never *remembers* it, which is what §11.2 B4 turns on.
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
