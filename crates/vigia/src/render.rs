//! Drawing a [`View`] into a buffer, and nothing else.

use std::sync::LazyLock;
use std::time::Duration;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span as TextSpan;
use vigia_core::{Class, HISTORY_BUCKETS, LineKind, Origin, Recency, SPARK_GROUPS, Span};

use crate::glyphs::Glyphs;
use crate::input::{Grabbed, Hovered, Region, Regions, Sheet};
use crate::theme::Theme;
use crate::view::{FileEntry, HEAT_BUCKETS, HeatBucket, ListRow, Row, Scale, View};

/// Columns a tab advances to the next multiple of.
const TAB_STOP: usize = 4;

/// Characters a column may cost before the walk gives up on the row.
const CHARS_PER_COLUMN: usize = 4;

/// Stands in for a character that cannot be drawn.
const UNPRINTABLE: char = '·';

/// Shown where a path had to lose its head to fit.
const ELIDED: char = '…';

/// Shown where anything else ran past the right edge.
const CONTINUES: &str = "›";

/// What a wrapped content line's continuation draws in the sigil column.
const WRAPPED: char = '↳';

/// The footer's left-hand side when there is nothing wrong, widest rung first.
const HINT_RUNGS: [&str; 4] = [
    "q quit · f follow · ? keys",
    "f follow · ? keys",
    "f follow",
    "",
];

/// The rung whose fit decides whether the footer takes a second line.
const HINT_BASELINE: usize = 0;

/// What joins two hints.
pub const HINT_SEPARATOR: &str = " · ";

/// How many buckets a sparkline may show, widest rung first.
const SPARK_RUNGS: [usize; SPARK_GROUPS.len() + 1] = {
    let mut rungs = [0; SPARK_GROUPS.len() + 1];
    let mut at = 0;
    while at < SPARK_GROUPS.len() {
        rungs[at] = HISTORY_BUCKETS / SPARK_GROUPS[at];
        at += 1;
    }
    rungs
};

/// Where [`SPARK_RUNGS`] keeps the rung that draws no sparkline at all.
const SPARK_NONE: usize = SPARK_GROUPS.len();

// Derived rather than written out and then asserted equal, which is the shape this
// replaced.
const _: () = {
    let mut at = 1;
    while at < SPARK_GROUPS.len() {
        assert!(
            SPARK_GROUPS[at] > SPARK_GROUPS[at - 1],
            "the sparkline groupings are not strictly ascending, so the rung \
             ladder derived from them is not widest-first"
        );
        at += 1;
    }
};

// The divisor property is still asserted, because it is what [`spark_of`] rests on and
// it is worth stating where the reliance is rather than one crate over.
const _: () = {
    let mut rung = 0;
    while rung < SPARK_RUNGS.len() {
        assert!(
            SPARK_RUNGS[rung] == 0 || HISTORY_BUCKETS % SPARK_RUNGS[rung] == 0,
            "a sparkline rung does not divide the source resolution, so its \
             newest column would cover less time than the rest"
        );
        rung += 1;
    }
};

/// Columns `buckets` of sparkline occupy at this rung.
const fn spark_cells(buckets: usize, glyphs: Glyphs) -> usize {
    buckets.div_ceil(glyphs.density())
}

// The rounding is asserted rather than only documented, because nothing that runs can
// reach it.
const _: () = {
    assert!(
        spark_cells(7, Glyphs::Braille) == 4,
        "an odd window rounds up"
    );
    assert!(
        spark_cells(1, Glyphs::Braille) == 1,
        "one bucket keeps a cell"
    );
    assert!(
        spark_cells(0, Glyphs::Braille) == 0,
        "no buckets take no cells"
    );
    assert!(
        spark_cells(8, Glyphs::Block) == 8,
        "the floor is the identity"
    );
};

/// The pulse, widest rung first.
const PULSE_RUNGS: [&str; 2] = ["●", ""];

/// One slice of a file, whatever it holds.
const HEAT_SLICE: char = '■';

/// What separates the pinned file list from the diff under it.
const RULE: char = '─';

/// The filled part of a scrollbar: where in the whole you are looking.
const BAR_THUMB: char = '█';

/// The unfilled part, which is drawn rather than left blank.
const BAR_TRACK: char = '│';

/// The step button at the top of a bar, and the one at the bottom.
const STEP_UP: char = '▲';
const STEP_DOWN: char = '▼';

/// Rows a stepped bar spends on buttons: one at each end.
const STEP_ROWS: u16 = 2;

/// The shortest track that can still express more than one position.
const MIN_TRACK: u16 = 2;

/// The shortest region whose bar carries step buttons.
const STEP_FLOOR: u16 = STEP_ROWS + MIN_TRACK;

/// What marks the row for the file the diff is currently inside.
const CARET: &str = "▸";

/// The weight that row's path takes on top of whatever recency gave it.
const CURRENT_WEIGHT: Modifier = Modifier::BOLD;

/// How many slices the heat strip may show, widest rung first.
const HEAT_RUNGS: [usize; 4] = [HEAT_BUCKETS, HEAT_BUCKETS / 2, HEAT_BUCKETS / 4, 0];

// Asserted rather than documented, because a rung that does not divide the source is
// silent.
const _: () = {
    let mut rung = 0;
    while rung < HEAT_RUNGS.len() {
        assert!(
            HEAT_RUNGS[rung] == 0 || HEAT_BUCKETS % HEAT_RUNGS[rung] == 0,
            "a heat rung does not divide the source resolution, so its last \
             slice would cover fewer lines than the rest"
        );
        rung += 1;
    }
};

/// Columns a path keeps before any glance element is allowed to exist.
const MIN_PATH_WIDTH: usize = 12;

/// Columns the kind letter and its gap take at the head of every file row.
const KIND_WIDTH: usize = 2;

/// The narrowest a file row can be and still name its own file.
const ROW_FLOOR: usize = KIND_WIDTH + MIN_PATH_WIDTH;

/// What stands between a content row's sigil and the line itself.
const SIGIL_GAP: &str = " ";

/// Columns a content row spends on the sigil and the gap after it.
const SIGIL_WIDTH: usize = 1 + SIGIL_GAP.len();

/// Columns before a content row's first character of line, gutter included.
pub(crate) const fn line_origin(gutter: usize) -> usize {
    if gutter == 0 {
        SIGIL_WIDTH
    } else {
        gutter + 1 + SIGIL_WIDTH
    }
}

/// Rows the worktree churn band takes when it is drawn at all.
const GRAPH_ROWS: usize = 2;

/// Rows the band leaves blank below itself.
const GRAPH_AIR: usize = 1;

/// Rows of air the body opens with, under the header.
const LEAD_ROWS: usize = 1;

/// Rows the stacked layout spends before the band can have any: the narrowest
/// pinned list and the rule under it.
const LIST_FLOOR_ROWS: usize = 1 + 1;

/// Diff rows the band may not take the pane below.
const GRAPH_KEEP: usize = 10;

/// The narrowest band that can draw every sample it holds.
const GRAPH_FLOOR: usize = 8;

/// Whether a pane this wide can carry the band at all.
const fn band_fits(pane: u16) -> bool {
    planning_width(pane, pane, 0) as usize >= GRAPH_FLOOR
}

/// The smallest body a second footer line may leave behind.
const MIN_BODY: u16 = 2;

/// The deepest pinned file list drawn below the rung above it.
pub const LIST_SETTLED: usize = 6;

/// Rows of pane the list is owed one row of map for, above [`LIST_SETTLED`].
const LIST_SHARE: usize = 4;

/// Rows of list a pane this tall is generous enough to afford.
const fn deep_of(height: u16) -> usize {
    height as usize / LIST_SHARE
}

/// Rows the pinned file list may take on a pane this tall, before the rule.
const fn list_cap(height: u16) -> usize {
    let deep = deep_of(height);
    if deep > LIST_SETTLED {
        deep
    } else {
        LIST_SETTLED
    }
}

/// Columns the caret glyph itself occupies.
const CARET_WIDTH: usize = 1;

/// Columns the caret takes off the pinned list's own row.
const fn caret_gutter(pane: u16) -> usize {
    CARET_WIDTH.saturating_sub(inset_of(pane) as usize)
}

/// Whether a pane this wide can afford the caret at all.
const fn affords_caret(available: u16, pane: u16) -> bool {
    planning_width(available, pane, caret_gutter(pane) as u16) as usize >= ROW_FLOOR
}

/// Columns a scrollbar costs the region it is drawn beside.
const BAR_WIDTH: usize = reserved(1);

/// The narrowest region that can afford a scrollbar.
const BAR_FLOOR: usize = BAR_WIDTH + ROW_FLOOR;

/// Whether a pane of `width` can afford a scrollbar at all.
const fn affords_bar(width: u16) -> bool {
    width as usize >= BAR_FLOOR
}

/// The pane's whole margin, both sides counted together: blank columns it keeps
/// between its own edge and any glyph. Widest pane first. `SPEC.md` §11.1.
const MARGIN_RUNGS: [(u16, u16); 4] = [(80, 4), (79, 3), (44, 2), (43, 1)];

/// The margin a pane this wide takes, both sides together.
const fn margin_of(pane: u16) -> u16 {
    let mut rung = 0;
    while rung < MARGIN_RUNGS.len() {
        let (from, cells) = MARGIN_RUNGS[rung];
        if pane >= from {
            return cells;
        }
        rung += 1;
    }
    // Under every rung, which is where I6's forty-column pane lands.
    0
}

/// The blank columns the pane keeps on its left and on its right.
const fn margins_of(pane: u16) -> (u16, u16) {
    let total = margin_of(pane);
    (total.div_ceil(2), total / 2)
}

/// The column a pane this wide begins drawing text at.
const fn inset_of(pane: u16) -> u16 {
    margins_of(pane).0
}

/// The pane width from which the pinned list may become a left rail beside
/// the diff rather than a strip above it. `SPEC.md` §11.2 B14.
const RAIL_FROM: u16 = 134;

/// Path columns the rail keeps beside a settled glance cluster.
const RAIL_PATH: usize = MIN_PATH_WIDTH * 2;

/// The narrowest rail worth drawing.
const RAIL_FLOOR: u16 =
    (BAR_WIDTH + inset_of(RAIL_FROM) as usize + KIND_WIDTH + RAIL_PATH + SETTLED_CELLS) as u16;

/// The share of a wide pane the rail takes, above [`RAIL_FLOOR`].
const RAIL_SHARE: u16 = 3;

/// Whether a pane this wide draws the list as a rail beside the diff.
const fn affords_rail(pane: u16) -> bool {
    pane >= RAIL_FROM
}

/// Columns the rail takes on a pane it is drawn on.
const fn rail_of(pane: u16) -> u16 {
    let share = pane / RAIL_SHARE;
    if share > RAIL_FLOOR {
        share
    } else {
        RAIL_FLOOR
    }
}

// What the rail's floor promises, asserted where nothing that runs can reach it.
const _: () = {
    assert!(
        planning_width(RAIL_FLOOR, RAIL_FROM, 0) as usize == KIND_WIDTH + RAIL_PATH + SETTLED_CELLS,
        "the narrowest rail is exactly a kind letter, a path and the settled \
         glance cluster, which is the sum RAIL_FLOOR is written as"
    );
    assert!(
        rail_of(RAIL_FROM) == RAIL_FLOOR,
        "the share overtakes the floor at the width the rail arrives at, so the \
         floor is not what the first rail is"
    );
    // The diff keeps the settled cluster on its own headings at the narrowest pane a
    // rail is drawn on, which is the other half of what makes the arrival width cost no
    // rung.
    assert!(
        planning_width(RAIL_FROM - RAIL_FLOOR, RAIL_FROM, 0) as usize
            >= SETTLED_CELLS + MIN_PATH_WIDTH,
        "the first rail leaves the diff too little for the glance cluster its \
         headings drew one column of pane ago"
    );
    // The caret's affordance reads the region and not the pane.
    assert!(
        !affords_caret(16, 200),
        "a region too narrow for a row still licensed the caret because the pane \
         it sits on is wide"
    );
    assert!(
        affords_caret(200, 200),
        "a pane that is its own region stopped affording the caret"
    );
};

/// Columns the frame time's number gets, whatever it says.
const FRAME_NUMBER: usize = 5;

/// What follows the number, so a bare duration is not left saying what it timed.
const FRAME_LABEL: &str = " frame";

/// What a frame time occupies once it is drawn at all.
const FRAME_CELL: usize = FRAME_NUMBER + FRAME_LABEL.len();

/// Columns the memory readout gets, whatever it says.
const MEMORY_CELL: usize = 6;

/// What separates two facts drawn beside each other on the status bar.
const CELL_GAP: &str = "  ";

/// Shown on the footer while the viewport is moving itself.
const FOLLOWING: &str = "follow ▶";

/// The marker inside [`FOLLOWING`], which is drawn green where the word beside
/// it stays dim.
const FOLLOW_MARK: char = '▶';

/// What joins two facts drawn on one line.
const FACT_SEPARATOR: &str = " · ";

/// What the body says when there is no diff at all.
const NOTHING_CHANGED: &str = "no unstaged changes";

/// The same, for a pane showing both runs.
const NOTHING_ANYWHERE: &str = "no staged or unstaged changes";

/// The narrowest the text column may get before line numbers are dropped.
const MIN_TEXT_WIDTH: usize = 24;

/// What the monitor is doing, which is the mockup's `watching` and the set that
/// word implies.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Mode {
    /// The watch is live, so the screen follows the working tree.
    #[default]
    Watching,
    /// The watch never armed, or it ended, so this is a still picture.
    Lost,
}

impl Mode {
    /// The word the header draws, on the right, alone.
    pub fn word(self) -> &'static str {
        match self {
            Self::Watching => "watching",
            Self::Lost => "not watching",
        }
    }
}

/// What the chrome says that the view itself does not know.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Chrome {
    /// Name of the working tree being watched.
    pub worktree: String,
    /// The branch the empty state names, when there is one.
    pub branch: Option<String>,
    /// How many files the staged run holds, or `None` when it is not drawn.
    pub staged: Option<usize>,
    /// How many changes the run that is not drawn holds.
    pub elsewhere: usize,
    /// Whether the watch is still live.
    pub mode: Mode,
    /// The cell a step button is being held down on, when one is.
    pub pressed: Option<(u16, u16)>,
    /// Which region's bar is being dragged, when one is.
    pub gripped: Option<Grabbed>,
    /// What the pointer is resting on, when it is on something a click acts on.
    pub hovered: Option<Hovered>,
    /// Which bar is being scrolled and which way, when one is.
    pub scrolling: Option<(Grabbed, isize)>,
    /// Something the reader should see instead of the key hints.
    pub notice: Option<String>,
    /// Whether the viewport is moving itself to what just changed.
    pub following: bool,
    /// Whether the masthead is drawn at all, which `m` toggles.
    pub masthead: bool,
    /// Whether the reader has asked for the pinned list beside the diff, which `r`
    /// toggles.
    pub rail: bool,
    /// Whether the gestures sheet is drawn over the pane, which `?` toggles.
    pub sheet: Option<usize>,
    /// Whether listed paths carry a file-type icon, from the config file's
    /// `icons` key.
    pub icons: bool,
    /// Whether listed paths are OSC 8 hyperlinks, from the `links` key, on by
    /// default.
    pub links: bool,
    /// The worktree's absolute path, for the links' `file://` targets.
    pub root: String,
    /// What recent frames cost, which `SPEC.md` §5.1 rules is their p99.
    pub frame: Option<Duration>,
    /// Resident set size in bytes, as of the last change.
    pub memory: Option<u64>,
}

/// `N/M`, or nothing at all when there is no diff to be positioned within.
fn position_of(file: usize, files: usize) -> String {
    if files == 0 {
        String::new()
    } else {
        // Saturating because [`View`] has public fields and [`render`] is public,
        // so nothing stops a caller handing over a position past the end of its
        // own file list. [`View::collect`] always clamps, but the type does not.
        format!("{}/{files}", file.saturating_add(1))
    }
}

/// The state's ladder, widest rung first.
fn state_rungs(following: bool, position: &str) -> Vec<String> {
    let mut rungs = Vec::with_capacity(3);
    match (following, position.is_empty()) {
        // `follow ▶ ` with nothing after it would read as a truncation rather
        // than a state, so a clean worktree gets the marker on its own.
        (true, false) => {
            rungs.push(format!("{FOLLOWING}  {position}"));
            rungs.push(FOLLOWING.to_owned());
        }
        (true, true) => rungs.push(FOLLOWING.to_owned()),
        (false, false) => rungs.push(position.to_owned()),
        (false, true) => {}
    }
    rungs.push(String::new());
    rungs
}

/// What a frame cost, in [`FRAME_CELL`] columns exactly.
fn frame_cell(cost: Duration) -> String {
    let micros = cost.as_micros();
    let number = if micros < 9_950 {
        format!("{:.1}ms", micros as f64 / 1000.0)
    } else if micros < 999_500 {
        format!("{}ms", (micros as f64 / 1000.0).round() as u64)
    } else {
        ">1s".to_owned()
    };
    fixed_width(
        format!("{number:>FRAME_NUMBER$}{FRAME_LABEL}"),
        FRAME_CELL,
        "frame",
    )
}

/// Hand back a status-bar cell, having checked it is the width it claims.
fn fixed_width(cell: String, columns: usize, what: &str) -> String {
    debug_assert_eq!(
        width_of(&cell),
        columns,
        "the {what} cell is fixed width, and {cell:?} is not {columns} columns"
    );
    cell
}

/// Resident set size, in [`MEMORY_CELL`] columns exactly.
fn memory_cell(bytes: u64) -> String {
    const MIB: u64 = 1024 * 1024;
    let mib = bytes / MIB;
    // A gibibyte is more than forty times what I3 measures this process at, so past it
    // the sigil says the only thing worth saying: something is very wrong, and the
    // figure is not the interesting part.
    let token = if mib > 999 {
        ">1GiB".to_owned()
    } else {
        format!("{mib}MiB")
    };
    fixed_width(format!("{token:>MEMORY_CELL$}"), MEMORY_CELL, "memory")
}

/// The diagnostics ladder, widest rung first.
fn diagnostic_rungs(frame: Option<Duration>, memory: Option<u64>) -> Vec<String> {
    let mut rungs = Vec::with_capacity(3);
    match (frame.map(frame_cell), memory.map(memory_cell)) {
        (Some(frame), Some(memory)) => {
            rungs.push(format!("{frame}{CELL_GAP}{memory}"));
            rungs.push(frame);
        }
        (Some(frame), None) => rungs.push(frame),
        // Memory without a frame time is the first paint on every platform, and it
        // draws nothing rather than the memory cell alone.
        (None, _) => {}
    }
    rungs.push(String::new());
    rungs
}

/// `N changed`, or nothing at all when there is no diff to count.
fn count_of(files: usize, staged: Option<usize>) -> String {
    let changed = match files {
        0 => String::new(),
        n => format!("{n} changed"),
    };
    // The staged total is a second fact and is owed whenever the run is on, including
    // at zero.
    match (changed.is_empty(), staged) {
        (false, Some(staged)) => format!("{changed}{FACT_SEPARATOR}{staged} staged"),
        _ => changed,
    }
}

/// The header's left-hand side, widest rung first.
///
/// `vigia · 3 changed`, then the worktree name alone. Both rungs are facts about
/// the tree, which is what puts them on one side (`SPEC.md` §11.1).
///
/// The count is the rung that drops and the name is the token that marks its
/// edge. The count goes first because the name is the one header fact a reader
/// cannot recover from the body, and because B3's empty state leans on it.
///
/// This ladder does not end in an empty rung, unlike every other one here, so it
/// needs [`widest_fitting_or_last`] rather than [`widest_fitting`]. A worktree
/// that draws no name gets the count and no separator: a separator is owed only
/// where both facts exist.
///
/// A name is *drawn* rather than merely non-empty or non-zero-width. `len` is not
/// width, width is not visibility, and what unicode-width reports is not what the
/// buffer agrees to write — `ratatui` drops a grapheme containing a control before
/// it reaches a cell, and zero-width, whitespace and control names are all legal
/// directory names. `U+2800` and `U+115F` escape deliberately: they draw a real
/// glyph that happens to be blank, and whether a font inks something is not a
/// question this process can ask.
fn header_left(
    worktree: &str,
    branch: Option<&str>,
    files: usize,
    staged: Option<usize>,
) -> Vec<String> {
    let mut rungs = Vec::with_capacity(4);
    let count = count_of(files, staged);

    // The branch is drawn always, rather than on the empty state alone.
    let named = branch.map(str::trim).filter(|branch| !branch.is_empty());

    // A separator is owed only between two facts that are both there, which
    // is [`FACT_SEPARATOR`]'s rule and the reason the name is measured rather
    // than tested for emptiness: a worktree
    // called `a zero-width space` is a non-empty string that draws nothing, and joining it
    // would head the pane with a leading separator.
    let visible = worktree.trim().replace(|c: char| c.is_control(), "");
    let name = (width_of(&visible) != 0).then_some(worktree);
    let join = |facts: [Option<&str>; 3]| {
        facts
            .into_iter()
            .flatten()
            .filter(|fact| !fact.is_empty())
            .collect::<Vec<_>>()
            .join(FACT_SEPARATOR)
    };

    // Widest first, dropping one fact per rung in the order §11.1 rules: the
    // count goes before the branch because the list below repeats it, and the
    // branch before the name because B3's empty state leans on the name to say
    // which repository this is.
    if !count.is_empty() {
        rungs.push(join([name, named, Some(&count)]));
    }
    // The staged total is the first thing a narrowing header gives up, one rung above
    // the count it rides on.
    if staged.is_some() && files > 0 {
        rungs.push(join([name, named, Some(&count_of(files, None))]));
    }
    if named.is_some() {
        rungs.push(join([name, named, None]));
    }
    rungs.push(worktree.to_owned());
    rungs
}

/// The one body line a worktree with no changes gets.
fn empty_state_with(staged: Option<usize>, elsewhere: usize) -> String {
    match (staged, elsewhere) {
        // The run is on and there is nothing in either. One line, both named.
        (Some(_), _) => NOTHING_ANYWHERE.to_owned(),
        // The run is off and the index has work in it: say where the work went.
        (None, n) if n > 0 => format!("{NOTHING_CHANGED}{FACT_SEPARATOR}{n} staged"),
        // A genuinely clean tree, which is B3's own line unchanged.
        (None, _) => NOTHING_CHANGED.to_owned(),
    }
}

/// One file heading's parts, gathered so [`Painter::file_row`] takes a shape
/// rather than seven positional arguments that a caller could transpose.
struct Heading<'r> {
    /// Which run this row is in, or `None` where no gutter column exists.
    origin: Option<Origin>,
    kind: char,
    path: &'r str,
    from: Option<&'r str>,
    churn: Option<(u32, u32)>,
    spark: &'r [u32; HISTORY_BUCKETS],
    recency: Recency,
    /// Whether the newest burst named this file, which is what carries the `●`.
    newest: bool,
    heat: &'r [HeatBucket; HEAT_BUCKETS],
}

impl<'r> Heading<'r> {
    /// Borrow a heading from the entry either region holds.
    fn of(entry: &'r FileEntry, grouped: bool) -> Self {
        Self {
            origin: grouped.then_some(entry.origin),
            kind: entry.kind,
            path: &entry.path,
            from: entry.from.as_deref(),
            churn: entry.churn,
            spark: &entry.spark,
            recency: entry.recency,
            newest: entry.newest,
            heat: &entry.heat,
        }
    }
}

/// Whether a region showing `span` of `of` has anywhere to scroll.
fn scrollable(span: u64, of: u64) -> bool {
    of != 0 && span < of
}

/// What a region's scrollbar is, before anything is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Bar {
    /// No column at all: the region shows everything it holds, or the pane
    /// cannot afford one.
    None,
    /// Track and thumb, from the region's first row to its last.
    Bare,
    /// A step button at each end, with the track between them.
    Stepped,
}

impl Bar {
    /// Whether a column is taken for this at all.
    fn drawn(self) -> bool {
        !matches!(self, Self::None)
    }

    /// The `(top, rows)` of a region that carry track rather than a button.
    fn track(self, top: u16, rows: u16) -> (u16, u16) {
        match self {
            Self::Stepped => (top.saturating_add(1), rows.saturating_sub(STEP_ROWS)),
            Self::None | Self::Bare => (top, rows),
        }
    }

    /// The whole region a pointer is told about: its rows, and the part of them
    /// this bar leaves as track.
    fn region(self, at: Rect) -> Region {
        Region {
            top: at.y,
            rows: at.height,
            left: at.x,
            width: at.width,
            track: self.track(at.y, at.height),
            // The rect's own right edge, which is where `Painter::scrollbar` draws: it
            // takes the region before `with_bar` narrows it and draws down the right of
            // what it was given.
            bar: self.drawn().then(|| bar_column(at)),
        }
    }
}

/// The column a region's scrollbar is drawn in.
const fn bar_column(rect: Rect) -> u16 {
    rect.x.saturating_add(rect.width).saturating_sub(1)
}

/// Decide a region's bar from what it holds and what the pane can afford.
fn bar_for(wide: bool, rows: u16, span: u64, of: u64) -> Bar {
    if !(wide && rows >= MIN_TRACK && scrollable(span, of)) {
        return Bar::None;
    }
    if rows >= STEP_FLOOR {
        Bar::Stepped
    } else {
        Bar::Bare
    }
}

/// The width a region's glance columns are planned against.
const fn planning_width(available: u16, pane: u16, caret: u16) -> u16 {
    available
        .saturating_sub(BAR_WIDTH as u16)
        .saturating_sub(inset_of(pane))
        .saturating_sub(caret)
}

/// Columns something of `width` costs on the right-hand side of a row.
const fn reserved(width: usize) -> usize {
    if width == 0 { 0 } else { width + 1 }
}

/// Narrow `right` past a slot `width` columns wide, drawn or not.
fn past(right: &mut Rect, width: usize) {
    right.width = right.width.saturating_sub(reserved(width) as u16);
}

/// Whether this file has any heat strip to draw at all.
fn has_heat(buckets: &[HeatBucket; HEAT_BUCKETS]) -> bool {
    buckets.iter().any(|bucket| bucket.total() > 0.0)
}

/// Columns one half of the counts cell occupies, whatever that half says.
const COUNT_CELL: usize = 5;

/// Every shape a file row's right-hand side may take, widest first.
const ROW_LAYOUTS: [Columns; 9] = [
    Columns::new(COUNT_CELL, PULSE_RUNGS[0], HEAT_RUNGS[0], SPARK_RUNGS[0]),
    Columns::new(COUNT_CELL, PULSE_RUNGS[0], HEAT_RUNGS[0], SPARK_RUNGS[1]),
    SETTLED,
    Columns::new(COUNT_CELL, PULSE_RUNGS[0], HEAT_RUNGS[1], SPARK_RUNGS[2]),
    Columns::new(COUNT_CELL, PULSE_RUNGS[0], HEAT_RUNGS[2], SPARK_RUNGS[2]),
    Columns::new(
        COUNT_CELL,
        PULSE_RUNGS[0],
        HEAT_RUNGS[2],
        SPARK_RUNGS[SPARK_NONE],
    ),
    Columns::new(
        COUNT_CELL,
        PULSE_RUNGS[0],
        HEAT_RUNGS[3],
        SPARK_RUNGS[SPARK_NONE],
    ),
    Columns::new(
        COUNT_CELL,
        PULSE_RUNGS[1],
        HEAT_RUNGS[3],
        SPARK_RUNGS[SPARK_NONE],
    ),
    Columns::NOTHING,
];

/// The widest layout below the rung above it.
const SETTLED: Columns = Columns::new(COUNT_CELL, PULSE_RUNGS[0], HEAT_RUNGS[1], SPARK_RUNGS[1]);

/// [`SETTLED`]'s own width, at the glyph rung where it is widest.
const SETTLED_CELLS: usize = reserved(counts_width(COUNT_CELL))
    + reserved(1)
    + reserved(HEAT_RUNGS[1])
    + reserved(spark_cells(SPARK_RUNGS[1], Glyphs::Block));

/// The share of a row the glance elements may take, above the settled ladder.
const GLANCE_NUMER: usize = 2;
/// The denominator of [`GLANCE_NUMER`]'s share.
const GLANCE_DENOM: usize = 5;

/// Columns the glance elements may spend on a row this wide, above the settled
/// ladder.
const fn generous_of(width: u16) -> usize {
    width as usize * GLANCE_NUMER / GLANCE_DENOM
}

/// Columns a whole counts cell of `cell`-wide halves occupies, its space included.
const fn counts_width(cell: usize) -> usize {
    if cell == 0 { 0 } else { cell * 2 + 1 }
}

/// One half of a counts cell, right-aligned by its caller into [`COUNT_CELL`].
fn churn_of(sigil: char, lines: u32) -> String {
    let room = COUNT_CELL.saturating_sub(1);
    for (unit, per) in [
        ("", 1u32),
        ("k", 1_000),
        ("M", 1_000_000),
        ("G", 1_000_000_000),
    ] {
        let scaled = lines / per;
        if scaled == 0 && per > 1 {
            // A unit this value is smaller than. Taking it would draw `0k` for
            // 250, which is not an abbreviation but a wrong number.
            continue;
        }
        let text = format!("{scaled}{unit}");
        if text.chars().count() <= room {
            return format!("{sigil}{text}");
        }
    }
    // Unreachable at [`COUNT_CELL`]. Drawn rather than panicked, because a
    // monitor that dies on a number is worse than one that rounds it.
    format!("{sigil}9")
}

/// One half of a file row's counts cell: what it says, and the ink it says it
/// in.
struct Half {
    /// `+42`, `-7`, or nothing where there is no line diff to count.
    text: String,
    /// [`Theme::added`], [`Theme::removed`], or [`Theme::chrome_dim`] where this
    /// half has nothing to say.
    ink: Style,
}

/// The two halves of a file row's counts cell, or empty strings when there is no
/// line diff to count.
fn counts_of(churn: Option<(u32, u32)>, theme: &Theme) -> (Half, Half) {
    let half = |sigil: char, lines: u32, ink: Style| Half {
        text: churn_of(sigil, lines),
        ink: if lines == 0 { theme.chrome_dim } else { ink },
    };
    // Beside `half` rather than inside the arm, and the two are the same rule reached
    // by two routes: a half says nothing when its own count is zero, and both halves
    // say nothing when there is no line diff behind them.
    let empty = || Half {
        text: String::new(),
        ink: theme.chrome_dim,
    };
    match churn {
        Some((added, removed)) => (
            half('+', added, theme.added),
            half('-', removed, theme.removed),
        ),
        None => (empty(), empty()),
    }
}

/// Where each glance element sits on every file row of one region.
#[derive(Clone, Copy)]
struct Columns {
    /// Columns each half of the counts cell occupies, or zero where the pair
    /// does not fit at all. The two halves stand or fall together, because `+42`
    /// with no `-7` beside it reads as a total rather than as half a pair.
    cell: usize,
    /// The pulse rung reserved on every row, or empty when none fits.
    pulse: &'static str,
    /// Heat buckets drawn on every row.
    heat: usize,
    /// Sparkline buckets drawn on every row.
    spark: usize,
}

impl Columns {
    /// A row with no room for anything but its path.
    const NOTHING: Self = Self::new(0, "", 0, 0);

    const fn new(cell: usize, pulse: &'static str, heat: usize, spark: usize) -> Self {
        Self {
            cell,
            pulse,
            heat,
            spark,
        }
    }

    /// The widest layout a region `width` columns wide both fits and deserves.
    fn plan(width: u16, glyphs: Glyphs) -> Self {
        // Named rather than shadowed, because the docblock above calls them two
        // different questions and the code said `budget` twice.
        let survival = usize::from(width).saturating_sub(ROW_FLOOR);
        // Floored at the settled ladder rather than applied to it, which is what
        // makes every boundary that shipped stay exactly where it was.
        let generous = generous_of(width).max(SETTLED.width(glyphs));
        let budget = survival.min(generous);
        ROW_LAYOUTS
            .iter()
            .copied()
            .find(|layout| layout.width(glyphs) <= budget)
            .unwrap_or(Self::NOTHING)
    }

    /// Columns this layout needs, every gap included.
    fn width(&self, glyphs: Glyphs) -> usize {
        reserved(counts_width(self.cell))
            + reserved(width_of(self.pulse))
            + reserved(self.heat)
            + reserved(spark_cells(self.spark, glyphs))
    }
}

/// What one drawn slice of the heat strip means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Heat {
    /// Nothing changed in this slice. Drawn as the track rather than skipped,
    /// because a gap would make the strip's own length ambiguous.
    Cool,
    /// Additions only.
    Added(Band),
    /// Removals only.
    Removed(Band),
    /// Both, which `SPEC.md` §5.1 rules yellow: every alternative paints a
    /// mixed slice as pure, and separating addition from removal by position is
    /// the strip's whole job.
    Mixed(Band),
}

/// How busy one thing is, against whatever the caller measures it against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Band {
    /// Below a third of the busiest slice.
    Low,
    /// A third or more of it.
    Warm,
    /// Two thirds or more of it.
    Hot,
}

impl Band {
    /// Which band `total` falls in, against the `busiest` the caller chose.
    fn of(total: f32, busiest: f32) -> Self {
        if busiest == 0.0 {
            return Self::Low;
        }
        if total * 3.0 >= busiest * 2.0 {
            Self::Hot
        } else if total * 3.0 >= busiest {
            Self::Warm
        } else {
            Self::Low
        }
    }

    /// Integer variant, for sparkline and band scales that are counts.
    fn of_count(total: u32, busiest: u32) -> Self {
        if busiest == 0 {
            return Self::Low;
        }
        let (total, busiest) = (u64::from(total), u64::from(busiest));
        if total * 3 >= busiest * 2 {
            Self::Hot
        } else if total * 3 >= busiest {
            Self::Warm
        } else {
            Self::Low
        }
    }
}

/// Re-project a heat map onto `width` slices and classify each one.
fn heat_at(buckets: &[HeatBucket; HEAT_BUCKETS], width: usize) -> Vec<Heat> {
    if width == 0 || !has_heat(buckets) {
        return Vec::new();
    }

    let group = HEAT_BUCKETS / width;
    let summed: Vec<HeatBucket> = buckets
        .chunks(group)
        .map(|chunk| {
            chunk
                .iter()
                .fold(HeatBucket::default(), |sum, bucket| HeatBucket {
                    added: sum.added + bucket.added,
                    removed: sum.removed + bucket.removed,
                })
        })
        .collect();

    let busiest = summed.iter().map(|b| b.total()).fold(0.0_f32, f32::max);
    summed
        .iter()
        .map(|bucket| {
            let band = Band::of(bucket.total(), busiest);
            match (bucket.added > 0.0, bucket.removed > 0.0) {
                (false, false) => Heat::Cool,
                (true, false) => Heat::Added(band),
                (false, true) => Heat::Removed(band),
                (true, true) => Heat::Mixed(band),
            }
        })
        .collect()
}

/// One bucket of a drawn sparkline: what it says, before how it looks.
#[derive(Clone, Copy)]
enum Bucket {
    /// Nothing was written in this cell's slice of the window.
    Empty,
    /// Written: the glyph [`Glyphs::glyph`] spelled it with, and its stop of the
    /// ramp.
    Written(char, Band, u8),
}

impl Bucket {
    /// What this bucket draws and what it is drawn in, together.
    fn drawn(self, theme: &Theme, glyphs: Glyphs, ramp: Option<&[Color; 8]>) -> (char, Style) {
        match self {
            Self::Empty => (glyphs.glyph(0, 0), theme.spark_track),
            // With a ramp, the stop picks the ink and the band's style keeps
            // any modifier a theme set; without one, the three stops draw what
            // they always drew; see the ladder note on
            // [`Theme::spark_ramp`].
            Self::Written(glyph, band, stop) => {
                let mut style = theme.spark_at(band);
                if let Some(ramp) = ramp {
                    style = style.fg(ramp[usize::from(stop).min(7)]);
                }
                (glyph, style)
            }
        }
    }
}

/// Which level of `levels` a count reaches, against the busiest count on screen.
fn level_to(total: u32, scale: u32, levels: usize) -> usize {
    if scale == 0 || total == 0 || levels == 0 {
        return 0;
    }
    let scaled = (total as u64 * levels as u64).div_ceil(scale as u64) as usize;
    scaled.clamp(1, levels)
}

/// A path's window, re-projected onto `rung` buckets and packed into the cells
/// those buckets are drawn in.
fn spark_of(
    buckets: &[u32; HISTORY_BUCKETS],
    rung: usize,
    scale: Scale,
    glyphs: Glyphs,
) -> [Bucket; HISTORY_BUCKETS] {
    let mut drawn = [Bucket::Empty; HISTORY_BUCKETS];
    // Nothing anywhere on screen has been written, so every bucket is empty and the
    // division below has no denominator.
    if rung == 0 {
        return drawn;
    }
    // Total for a rung wider than the window, and that is a release hazard rather than
    // tidiness.
    let group = HISTORY_BUCKETS / rung;
    if group == 0 {
        return drawn;
    }
    let yardstick = scale.at(group);
    if yardstick == 0 {
        return drawn;
    }
    // Summed into the rung first, then packed into cells, which is two projections
    // rather than one and they answer different questions: the first is how much of the
    // window one drawn bucket covers, the second is how many buckets one terminal cell
    // can hold.
    let mut summed = [0u32; HISTORY_BUCKETS];
    for (at, chunk) in buckets.chunks(group).enumerate() {
        summed[at] = chunk.iter().copied().fold(0, u32::saturating_add);
    }
    // One loop at every rung, and the density is the only thing that moves.
    for (cell, pair) in drawn
        .iter_mut()
        .zip(summed[..rung].chunks(glyphs.density()))
    {
        let (left, right) = (pair[0], pair.get(1).copied().unwrap_or(0));
        // The busier of the pair decides both, and at the block rung the pair is one
        // bucket so this is that bucket.
        let busiest = left.max(right);
        if busiest == 0 {
            continue;
        }
        // Through [`level_to`], which is where the rounding rule lives.
        // Written twice, the rule that keeps one write from drawing as empty
        // could move at one rung and not the other.
        let level = |count: u32| level_to(count, yardstick, glyphs.levels());
        // Against the same `scale` the heights are scaled from, which is `scale_of`'s
        // figure over every bucket on screen rather than over this file.
        let band = Band::of_count(busiest, yardstick);
        // The ramp stop, from the same figure the heights and the band are
        // scaled from, quantised through the same rounding rule so one write
        // never lands on the ramp's floor colour.
        let stop = (level_to(busiest, yardstick, 8).max(1) - 1) as u8;
        *cell = Bucket::Written(glyphs.glyph(level(left), level(right)), band, stop);
    }
    drawn
}

/// The widest rung of `ladder` that fits in `room`.
fn widest_fitting<S: AsRef<str>>(ladder: &[S], room: usize) -> &str {
    fitting(ladder, room).unwrap_or("")
}

/// The widest rung that fits, or `None` when none does.
fn fitting<S: AsRef<str>>(ladder: &[S], room: usize) -> Option<&str> {
    ladder
        .iter()
        .map(AsRef::as_ref)
        .find(|rung| width_of(rung) <= room)
}

/// The widest rung of `ladder` that fits, or its last rung when none does.
fn widest_fitting_or_last<S: AsRef<str>>(ladder: &[S], room: usize) -> &str {
    fitting(ladder, room)
        .or_else(|| ladder.last().map(AsRef::as_ref))
        .unwrap_or("")
}

/// What the footer will draw, and how many rows it needs.
struct Footer<'a> {
    /// One, or two when a single line cannot hold both halves. Zero on a screen
    /// with no room for a footer at all.
    rows: u16,
    /// Columns the state may take on its line.
    reserved: usize,
    /// The hints rung, or the notice.
    left: &'a str,
    /// Whether `left` is a notice, which is what decides its colour.
    alert: bool,
    /// Whether a rule is drawn above the footer's text.
    rule: bool,
    /// The frame-time and memory cells, already narrowed to what is left after
    /// the hints and the state have taken theirs.
    diagnostics: String,
}

impl<'a> Footer<'a> {
    /// Rows the footer takes off the pane, its rule included.
    fn height(&self) -> u16 {
        self.rows + u16::from(self.rule)
    }

    /// Decide the footer's shape from the width, the state, and the file count.
    fn plan(area: Rect, chrome: &'a Chrome, files: usize) -> Self {
        // The room the footer's glyphs actually get, which is the pane less its inset
        // on both sides: chrome has no scrollbar reserve standing in for the right-hand
        // half the way a glance row does.
        let (leading, trailing) = margins_of(area.width);
        let width = usize::from(area.width.saturating_sub(leading).saturating_sub(trailing));
        if area.height < 2 {
            return Self {
                rows: 0,
                reserved: 0,
                left: "",
                alert: false,
                rule: false,
                diagnostics: String::new(),
            };
        }

        // The last file's position is the widest this count can produce, since
        // no position numbers higher than the count itself.
        let widest = position_of(files.saturating_sub(1), files);
        let reserved = width_of(widest_fitting(
            &state_rungs(chrome.following, &widest),
            width,
        ));
        // The gap keeps the state from touching the hints, and is only owed when
        // there is a state to keep away from them.
        let taken = if reserved == 0 {
            0
        } else {
            reserved + CELL_GAP.len()
        };

        // A second line is worth taking only if it buys something: there has to be a
        // state to move up to it, and a body still worth showing underneath.
        let grows = width_of(HINT_RUNGS[HINT_BASELINE]) + taken > width
            && reserved > 0
            && area.height >= 3 + MIN_BODY;
        let rows = if grows { 2 } else { 1 };
        // Charged the way the second footer line is, against the same floor: one
        // header, the footer's own rows, the rule, and a body still worth showing under
        // it.
        let rule = area.height >= 1 + rows + 1 + MIN_BODY;

        let room = if grows {
            width
        } else {
            width.saturating_sub(taken)
        };
        // A notice is one token: it takes whatever room the line gives it and
        // marks the cut. The hints are a list, so they drop whole rungs instead.
        let hints = widest_fitting(&HINT_RUNGS, room);
        let (left, alert) = match &chrome.notice {
            Some(notice) => (notice.as_str(), true),
            None => (hints, false),
        };

        // Last, and out of what is left over, which is the whole design.
        let occupied = taken + CELL_GAP.len() + if grows { 0 } else { width_of(hints) };
        let diagnostics = widest_fitting(
            &diagnostic_rungs(chrome.frame, chrome.memory),
            width.saturating_sub(occupied),
        )
        .to_owned();

        Self {
            rows,
            reserved,
            left,
            alert,
            rule,
            diagnostics,
        }
    }
}

/// How the body divides between the regions `SPEC.md` §11.1 rules.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Body {
    /// Blank rows between the header and whatever the body opens with.
    pub lead: usize,
    /// Rows the worktree churn band takes, zero when the pane cannot spare them.
    pub graph: usize,
    /// The blank row under the band, zero whenever the band is.
    pub air: usize,
    /// Rows the pinned file list takes, zero when there is no room for one.
    pub list: usize,
    /// Whether the rule under the list is drawn, which is exactly when there is
    /// a list to put it under.
    pub rule: bool,
    /// Rows left for the diff, which is what [`View::collect`] is asked for.
    pub diff: usize,
    /// Whether the list is a left rail beside the diff rather than a strip
    /// above it.
    pub rail: bool,
    /// Columns the diff's content is laid out against, the scrollbar's charged
    /// whether or not one is drawn.
    pub diff_width: usize,
    /// Pages the gestures sheet takes on this pane, `Some(0)` on a pane too small
    /// to draw one, and `None` when nothing measured it.
    pub sheet_pages: Option<usize>,
}

impl Body {
    /// The layout of a pane with no room for a list: all diff, no rule.
    pub fn diff_only(rows: usize) -> Self {
        Self {
            lead: 0,
            graph: 0,
            air: 0,
            list: 0,
            rule: false,
            diff: rows,
            // Not a rail, whatever the pane is wide enough for. A rail is two regions,
            // and this shape has one.
            diff_width: 0,
            rail: false,
            // Attached by `body_layout`, which is the only caller that has the
            // pane. A `Body` built for a diff walk has no sheet to count.
            sheet_pages: None,
        }
    }

    /// Split a body of `area` minus its header and an already planned footer.
    pub fn split(
        area: Rect,
        footer_rows: u16,
        files: usize,
        list_rows: usize,
        chrome: &Chrome,
    ) -> Self {
        let mut body = Self::split_rows(area, footer_rows, files, list_rows, chrome);
        // Here rather than in each of `split_rows`' four exits, and after them rather
        // than inside, because the width is a function of the *shape* the split chose
        // and of the pane, and both are known once it has chosen.
        body.diff_width = usize::from(planning_width(body.areas(area).diff.width, area.width, 0));
        body
    }

    /// [`Body::split`]'s row arithmetic, which is all of it but the one width.
    fn split_rows(
        area: Rect,
        footer_rows: u16,
        files: usize,
        list_rows: usize,
        chrome: &Chrome,
    ) -> Self {
        let masthead = chrome.masthead;
        let body = usize::from(area.height).saturating_sub(1 + usize::from(footer_rows));

        // The rail is decided before the row clamps, because it removes two of them.
        if chrome.rail && affords_rail(area.width) && files > 0 {
            return Self::beside(body, area.width, list_rows, masthead);
        }

        // The rule costs a row and [`LEAD_ROWS`] costs another, so the diff needs
        // `LEAD_ROWS + MIN_BODY + 1` before the list may have its first.
        let affordable = body.saturating_sub(LEAD_ROWS + usize::from(MIN_BODY) + 1);
        // `list_rows` rather than `files`: a grouped list draws a separator per run,
        // and a region sized from the files alone is short by exactly those rows and
        // drops the tail of the last run.
        let list = list_rows.min(list_cap(area.height)).min(affordable);
        if list == 0 {
            return Self::diff_only(body);
        }
        let after = body - LEAD_ROWS - list - 1;

        // The band is last in the clamp order and that is the ruling.
        let (graph, air) = Self::band_rows_of(masthead, area.width, after);
        Self {
            lead: LEAD_ROWS,
            graph,
            air,
            list,
            rule: true,
            diff: after - graph - air,
            diff_width: 0,
            rail: false,
            // Attached by `body_layout`, which is the only caller with the pane.
            sheet_pages: None,
        }
    }

    /// The rows the band takes out of `after`, drawn and blank, or zero twice.
    fn band_rows_of(masthead: bool, width: u16, after: usize) -> (usize, usize) {
        let framed = GRAPH_ROWS + GRAPH_AIR;
        if masthead && band_fits(width) && after >= framed + GRAPH_KEEP {
            (GRAPH_ROWS, GRAPH_AIR)
        } else {
            (0, 0)
        }
    }

    /// The same body, laid out as a rail beside the diff rather than a strip
    /// above it.
    fn beside(body: usize, width: u16, list_rows: usize, masthead: bool) -> Self {
        // The rail is drawn only where a list would have been drawn at all, and that is
        // [`Body::split`]'s own `affordable` test rather than a floor of this layout's
        // own.
        if body <= LEAD_ROWS + usize::from(MIN_BODY) + 1 {
            return Self::diff_only(body);
        }
        let after = body - LEAD_ROWS;

        // The same question [`Body::split`] asks, asked through the same function and
        // against the rows that layout would have left.
        let (graph, air) =
            Self::band_rows_of(masthead, width, after.saturating_sub(LIST_FLOOR_ROWS));
        let rows = after - graph - air;
        Self {
            lead: LEAD_ROWS,
            graph,
            air,
            // `list_rows` rather than `files`, for the reason [`Body::split`]'s stacked
            // branch takes it: a grouped list draws a separator per run and a region
            // sized from the files alone is short by exactly those rows, so the last
            // run is announced and its tail is not drawn.
            list: list_rows.min(rows),
            // No rule, and it is dissolved rather than declined. §11.2 B11 rules the
            // rule between the regions stays bare, and `Body::split`'s `rule: list > 0`
            // makes the rule and the list coextensive.
            rule: false,
            diff: rows,
            diff_width: 0,
            rail: true,
            // Attached by `body_layout`, which is the only caller with the pane.
            sheet_pages: None,
        }
    }

    /// Every row the band occupies, drawn and blank together.
    pub fn band_rows(&self) -> usize {
        self.graph + self.air
    }

    /// Every row between the header and the list.
    pub fn above_list(&self) -> usize {
        self.lead + self.band_rows()
    }

    /// Every row the body holds, across every region it has.
    pub fn rows(&self) -> usize {
        if self.rail {
            return self.above_list() + self.diff;
        }
        self.above_list() + self.list + usize::from(self.rule) + self.diff
    }

    /// Shrink the list to the rows a view actually carries, giving the rest back
    /// to the diff.
    pub fn clamped_to(self, have: usize) -> Self {
        // Beside a rail there is nothing to give back. The rows the list does not use
        // are in the rail's own column, and the diff is not below them: handing them
        // over would draw the diff twice, once in each region.
        if self.rail {
            if have == 0 {
                // The page count survives the collapse.
                return Self {
                    sheet_pages: self.sheet_pages,
                    ..Self::diff_only(self.rows())
                };
            }
            return Self {
                list: self.list.min(have),
                ..self
            };
        }
        let list = self.list.min(have);
        // The band and the lead blank go with the list, for `Body::split`'s
        // own reason: the three are one region saying what the worktree is doing,
        // and a stale view with no entries draws B3's sentence rather than a graph
        // over blank rows under a blank row.
        let (lead, graph, air) = if list > 0 {
            (self.lead, self.graph, self.air)
        } else {
            (0, 0, 0)
        };
        // The diff takes what is left of the body, rather than a give-back term per
        // field.
        let rule = list > 0;
        Self {
            lead,
            graph,
            air,
            list,
            rule,
            diff: self.rows() - (lead + graph + air + list + usize::from(rule)),
            // Carried, for [`Body::sheet_pages`]' reason two fields down.
            diff_width: self.diff_width,
            rail: false,
            // Carried, unlike the two constructors above. A clamp re-divides
            // the rows this body already has and does not re-measure the pane, so
            // the sheet it could draw is the same sheet.
            sheet_pages: self.sheet_pages,
        }
    }
}

/// Where each part of the body sits inside a pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Areas {
    /// The worktree churn band, empty when the pane cannot spare it.
    pub band: Rect,
    /// The pinned file list, empty when there is no room for one.
    pub list: Rect,
    /// The rule under the list, empty when there is no list to put it under.
    pub rule: Rect,
    /// Everything left for the diff.
    pub diff: Rect,
}

impl Areas {
    /// Whether each region is wide enough for a scrollbar at all.
    fn bars(&self) -> (bool, bool) {
        (affords_bar(self.list.width), affords_bar(self.diff.width))
    }
}

impl Body {
    /// Where each part of this body sits inside `area`.
    pub fn areas(&self, area: Rect) -> Areas {
        // The two shapes are exclusive and every field here is `pub`, so a caller can
        // build a `Body` claiming both.
        debug_assert!(
            !(self.rail && self.rule),
            "a body cannot draw a rule between regions that are side by side"
        );

        // The header's row, then the lead blank.
        let top = area.y.saturating_add(1);
        let band = Rect {
            y: top.saturating_add(self.lead as u16),
            height: self.graph as u16,
            ..area
        };
        let under_band = top.saturating_add(self.above_list() as u16);

        // The rail, where the two regions share a `y` range and differ in `x`.
        if self.rail {
            let columns = rail_of(area.width);
            let list = Rect {
                y: under_band,
                height: self.list as u16,
                width: columns.min(area.width),
                ..area
            };
            let diff = Rect {
                x: area.x.saturating_add(list.width),
                y: under_band,
                width: area.width.saturating_sub(list.width),
                height: self.diff as u16,
            };
            return Areas {
                band,
                list,
                // Zero height *and* zero width, so a consumer that asks either question
                // gets the same answer.
                rule: Rect {
                    x: area.x,
                    y: under_band,
                    width: 0,
                    height: 0,
                },
                diff,
            };
        }

        let list = Rect {
            y: under_band,
            height: self.list as u16,
            ..area
        };
        let rule = Rect {
            y: list.y.saturating_add(list.height),
            height: u16::from(self.rule),
            ..area
        };
        let diff = Rect {
            y: rule.y.saturating_add(rule.height),
            height: self.diff as u16,
            ..area
        };
        Areas {
            band,
            list,
            rule,
            diff,
        }
    }
}

/// Split this area's body between the file list and the diff.
pub fn body_layout(area: Rect, chrome: &Chrome, files: usize, list_rows: usize) -> Body {
    let footer = Footer::plan(area, chrome, files).height();
    let mut body = Body::split(area, footer, files, list_rows, chrome);
    // Attached rather than split, which is [`Body::sheet_pages`]' own docblock: the
    // sheet is not a region and takes no row from one, so it has no place in the split
    // that divides the body between them.
    body.sheet_pages = Some(sheet_pages_of(area, footer, margins_of(area.width)));
    body
}

/// Rows the diff gets, which is what a caller has to ask [`View::collect`]
/// for and what a page-down step is measured in.
pub fn diff_height(area: Rect, chrome: &Chrome, files: usize, list_rows: usize) -> usize {
    body_layout(area, chrome, files, list_rows).diff
}

/// What one paint cost, in the term that decides whether it followed the pane.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PaintStats {
    /// Rows of file content drawn. Headings, hunk headers and notes are not
    /// content and are bounded by the screen on their own.
    pub rows: u64,
    /// Source characters examined to draw them.
    pub examined: u64,
}

impl std::ops::AddAssign for PaintStats {
    /// Sum two paints, so a run of frames is one figure rather than two
    /// hand-summed ones.
    fn add_assign(&mut self, other: Self) {
        let Self { rows, examined } = other;
        self.rows += rows;
        self.examined += examined;
    }
}

/// Where this screen's regions and scrollbars are, for a pointer to be told what
/// it is over.
pub fn regions(area: Rect, chrome: &Chrome, view: &View) -> Regions {
    if area.width == 0 || area.height == 0 {
        return Regions::default();
    }
    let footer = Footer::plan(area, chrome, view.files);
    let body = Body::split(area, footer.height(), view.files, view.list.len(), chrome)
        .clamped_to(view.list.len());
    // The same geometry the painter draws into, asked for by name rather than rebuilt
    // here.
    let areas = body.areas(area);

    // Asked through `bar_for`, which is what `render` asks.
    let (list_bars, diff_bars) = areas.bars();
    let list_bar = bar_for(
        list_bars,
        areas.list.height,
        // A screenful in files, which is what this bar is measured in at both ends.
        view.list_span as u64,
        view.files as u64,
    );
    let diff_bar = bar_for(
        diff_bars,
        areas.diff.height,
        body.diff as u64,
        view.total_rows as u64,
    );

    Regions {
        list: list_bar.region(areas.list),
        diff: diff_bar.region(areas.diff),
        // From the same plan the painter draws, which is what keeps the pointer and the
        // screen one answer: a sheet the reader can see but the pointer cannot would
        // swallow nothing and let a click seek a bar behind it.
        sheet: chrome
            .sheet
            .and_then(|page| sheet_plan(area, footer.height(), margins_of(area.width), page))
            .map(|plan| plan.target()),
    }
}

/// Draw a whole screen: one header line, the body, and one or two footer lines.
pub fn render(
    buf: &mut Buffer,
    area: Rect,
    view: &View,
    theme: &Theme,
    glyphs: Glyphs,
    chrome: &Chrome,
) -> PaintStats {
    // Clip to the buffer on both axes.
    let area = buf.area.intersection(area);
    if area.is_empty() {
        return PaintStats::default();
    }

    // One plan, one split, both read by everything below.
    let footer = Footer::plan(area, chrome, view.files);
    let body = Body::split(area, footer.height(), view.files, view.list.len(), chrome)
        .clamped_to(view.list.len());
    let margins = margins_of(area.width);
    // Planned before anything is painted, and painted last.
    let sheet = chrome
        .sheet
        .and_then(|page| sheet_plan(area, footer.height(), margins, page));

    let mut painter = Painter {
        buf,
        theme,
        // A parameter beside `theme` rather than a field on `chrome`, and for the same
        // reason `theme` is one: both are properties of the terminal, resolved once
        // before the screen was taken and unchanged for the session, where `chrome`
        // carries what *this frame* says.
        glyphs,
        gutter: 0,
        // From the pane, once, before anything is drawn.
        inset: margins.0,
        trailing: margins.1,
        paint: PaintStats::default(),
        pressed: chrome.pressed,
        gripped: chrome.gripped,
        hovered: chrome.hovered,
        scrolling: chrome.scrolling,
        spark_ramp: theme.spark_ramp(),
        covered: sheet.as_ref().map(|plan| plan.area),
        icons: chrome.icons,
        link_root: (chrome.links && !chrome.root.is_empty()).then(|| chrome.root.clone()),
    };

    painter.header(Rect { height: 1, ..area }, view, chrome);

    if footer.rows > 0 {
        painter.footer(area, view, chrome, &footer);
    }

    // The geometry, once, from the same method the pointer reads.
    let areas = body.areas(area);
    // The same pair `regions` reads, so the pointer never seeks a bar the screen
    // declined to draw.
    let (list_bars, diff_bars) = areas.bars();

    if areas.band.height > 0 {
        painter.band(areas.band, view);
    }

    if body.list > 0 {
        let region = areas.list;
        // Counted in files, which is exactly what this region shows.
        let full = region;
        let (region, bar) =
            painter.with_bar(region, list_bars, view.list_span as u64, view.files as u64);
        // Before the content here, and it does not matter which, because a list row
        // carries no wash: the bar's cell and the row's cells never overlap.
        if bar.drawn() {
            painter.scrollbar(
                full,
                Grabbed::List,
                bar,
                view.list_top as u64,
                // A screenful in files, so all three terms of this bar are one unit and
                // its travel is the drag's travel.
                view.list_span as u64,
                view.files as u64,
            );
        }
        painter.list(region, full.width, view, area.width);
    }

    if areas.rule.height > 0 {
        painter.rule(areas.rule);
    }

    if body.diff > 0 {
        let region = areas.diff;
        // Counted in rows, which is what the call below passes: `rows_above` over
        // `total_rows`, with the thumb spanning the screen's own height.
        let full = region;
        // Zero is *nobody measured*, which is a hand-built [`Body`] in a test and not a
        // real pane: [`Body::split`] fills the field for every shape it returns, and a
        // pane whose diff has no columns draws no content to wrap.
        debug_assert!(
            body.diff_width == 0
                || body.diff_width == usize::from(planning_width(full.width, area.width, 0)),
            "the rows were wrapped against {} columns and this region lays out with {}",
            body.diff_width,
            planning_width(full.width, area.width, 0)
        );
        let (region, bar) =
            painter.with_bar(region, diff_bars, body.diff as u64, view.total_rows as u64);
        // The wash spans the region's whole width, the bar's own column
        // included.
        painter.body(
            region,
            full,
            view,
            area,
            &empty_state_with(chrome.staged, chrome.elsewhere),
        );
        if bar.drawn() {
            painter.scrollbar(
                full,
                Grabbed::Diff,
                bar,
                view.rows_above as u64,
                body.diff as u64,
                view.total_rows as u64,
            );
        }
    }

    // Last, over everything, and only if a reader asked.
    if let Some(plan) = sheet {
        painter.sheet(&plan);
    }

    painter.paint
}

/// One row of the gestures sheet: how to ask for a thing, and what it does.
struct Gesture {
    /// The keys cell. Aliases live inside it rather than in rows of their own.
    keys: [&'static str; 2],
    /// What the gesture does.
    verb: [&'static str; 2],
}

/// The keyboard half, in the order a reader reads it, which is not the order
/// the ladder drops it in.
const KEYBOARD: [Gesture; 16] = [
    Gesture {
        keys: ["j  k  ↓  ↑", "j  k  ↓  ↑"],
        verb: ["scroll a row", "scroll a row"],
    },
    Gesture {
        keys: ["Space  PgDn  PgUp", "Space  PgDn"],
        verb: ["page", "page"],
    },
    Gesture {
        keys: ["d  u", "d  u"],
        verb: ["half a page", "half a page"],
    },
    Gesture {
        keys: ["g  Home  /  G  End", "g  /  G"],
        verb: ["first / last changed file", "first / last file"],
    },
    Gesture {
        // The arrows at the wide spelling and not at the tight one, and that is
        // measured rather than preferred.
        keys: ["n  →  /  p  ←", "n  /  p"],
        verb: ["next / previous changed file", "next / prev file"],
    },
    Gesture {
        keys: ["1  to  6", "1  to  6"],
        verb: ["jump to that row of the list", "jump to a list row"],
    },
    Gesture {
        keys: ["J  K  Shift+↑  Shift+↓", "J  K"],
        verb: ["scroll the pinned file list", "scroll the list"],
    },
    Gesture {
        keys: ["f", "f"],
        verb: ["follow the newest change", "follow the newest"],
    },
    Gesture {
        keys: ["m", "m"],
        verb: ["show or hide the churn band", "the churn band"],
    },
    Gesture {
        keys: ["r", "r"],
        verb: ["show or hide the left rail", "the left rail"],
    },
    // Both cells sit inside the field maxima this table already had, so no rung's width
    // moves: the wide verb field is 28 on `next / previous changed file` and the tight
    // one is 19 on the mouse group's `a row, held repeats`, where these are 27 and 13.
    Gesture {
        keys: ["s", "s"],
        verb: ["one file, or the whole diff", "one file only"],
    },
    // Both cells sit inside the field maxima this table already had, for the reason
    // B16's row above states: the wide verb field is 28 on `next / previous changed
    // file` and the tight one is 19 on the mouse group's `a row, held repeats`, where
    // these are 26 and 14.
    Gesture {
        keys: ["a", "a"],
        verb: ["show or hide staged changes", "staged changes"],
    },
    Gesture {
        keys: ["w", "w"],
        verb: ["wrap a long line, or clip it", "wrap long lines"],
    },
    // Last of `view`: the one key that spends something outside this program.
    Gesture {
        keys: ["y", "y"],
        verb: ["copy this file's path", "copy the path"],
    },
    Gesture {
        keys: ["?  Esc", "?  Esc"],
        verb: ["this sheet", "this sheet"],
    },
    // `Esc` sits a row up, where it leaves the frontmost thing rather than quitting
    // outright.
    Gesture {
        keys: ["q  Ctrl+C  Ctrl+D", "q"],
        verb: ["quit", "quit"],
    },
];

/// The order the height ladder gives keyboard rows up, first to go, as indices
/// into [`KEYBOARD`].
const DROP_ORDER: [usize; KEYBOARD.len()] = [15, 0, 1, 2, 3, 4, 5, 6, 9, 10, 12, 13, 11, 7, 8, 14];

/// The keyboard rows a rung with `from` dropped still draws, in display order.
fn kept_keyboard(from: usize) -> impl Iterator<Item = &'static Gesture> {
    KEYBOARD
        .iter()
        .enumerate()
        .filter(move |(i, _)| !DROP_ORDER[..from].contains(i))
        .map(|(_, row)| row)
}

/// The mouse half, which is the first gesture the height ladder drops.
const MOUSE: [Gesture; 8] = [
    Gesture {
        keys: ["wheel", "wheel"],
        verb: ["scroll what you point at", "what you point at"],
    },
    Gesture {
        keys: ["drag a scrollbar", "drag a bar"],
        verb: ["move that region", "move that region"],
    },
    Gesture {
        keys: ["click a track", "click a track"],
        verb: ["send that region there", "send it there"],
    },
    Gesture {
        keys: ["click  ▲ ▼", "click  ▲ ▼"],
        verb: ["one row, and repeats held", "a row, held repeats"],
    },
    Gesture {
        keys: ["click a listed file", "click a file"],
        verb: ["jump the diff to it", "jump the diff to it"],
    },
    // The tail is the three rows this table most easily omits, and `README.md`'s Mouse
    // table is the other place each is named; a gate holds the two against each other.
    // The last is the only row here the pane does not answer, since the terminal hands
    // selection back while the modifier is held, and it keeps one spelling at both rungs
    // because a gesture token has to be a substring of each.
    Gesture {
        keys: ["click  ✕", "click  ✕"],
        verb: ["close the sheet", "close the sheet"],
    },
    Gesture {
        keys: ["just point", "just point"],
        verb: ["it marks itself", "it marks itself"],
    },
    Gesture {
        keys: ["Shift+drag", "Shift+drag"],
        verb: ["the terminal selects text", "select text"],
    },
];

/// Which rows a [`Section`] heads.
#[derive(Debug, Clone, Copy)]
enum Rows {
    /// `KEYBOARD[from..to]`.
    Keyboard {
        /// First row of the run.
        from: usize,
        /// One past the last.
        to: usize,
    },
    /// The whole of [`MOUSE`].
    Mouse,
}

impl Rows {
    /// The rows themselves.
    fn rows(self) -> &'static [Gesture] {
        match self {
            Rows::Keyboard { from, to } => &KEYBOARD[from..to],
            Rows::Mouse => &MOUSE,
        }
    }
}

/// One labelled run of the sheet's table, which the roomy rung draws as a section
/// and every other rung draws unlabelled.
#[derive(Debug, Clone, Copy)]
struct Section {
    /// What the heading spells. Plain text on the roomy rung, standing back from
    /// its own rows rather than ruled to the frame.
    label: &'static str,
    /// The rows under it.
    rows: Rows,
}

/// The reader's own sections, in the order the roomy rung's mock draws them.
const SECTIONS: [Section; 5] = [
    Section {
        label: "moving",
        rows: Rows::Keyboard { from: 0, to: 3 },
    },
    Section {
        label: "files",
        rows: Rows::Keyboard { from: 3, to: 7 },
    },
    Section {
        label: "view",
        rows: Rows::Keyboard { from: 7, to: 14 },
    },
    Section {
        label: "mouse",
        rows: Rows::Mouse,
    },
    Section {
        label: "leaving",
        rows: Rows::Keyboard { from: 14, to: 16 },
    },
];

// What [`SECTIONS`]' *shape* promises, asserted where nothing that runs can reach it.
const _: () = {
    let (mut at, mut i, mut mice) = (0usize, 0usize, 0usize);
    while i < SECTIONS.len() {
        match SECTIONS[i].rows {
            Rows::Keyboard { from, to } => {
                assert!(
                    from == at,
                    "the sections leave a gap in KEYBOARD or overlap in it, which \
                     is a row drawn twice or not at all"
                );
                assert!(from < to, "a section names an empty run of KEYBOARD");
                at = to;
            }
            Rows::Mouse => mice += 1,
        }
        i += 1;
    }
    assert!(
        at == KEYBOARD.len(),
        "the sections do not reach the end of KEYBOARD, so the roomy rung draws \
         fewer gestures than the ladder promises"
    );
    assert!(mice == 1, "the mouse group is not named exactly once");

    // [`DROP_ORDER`] is a permutation of the table's rows. A repeated index leaves one
    // row undroppable and gives another up twice, so a rung's row count and the rows it
    // draws come apart.
    let mut seen = [false; KEYBOARD.len()];
    let mut i = 0;
    while i < DROP_ORDER.len() {
        assert!(
            !seen[DROP_ORDER[i]],
            "DROP_ORDER names a row twice, so another row can never be given up"
        );
        seen[DROP_ORDER[i]] = true;
        i += 1;
    }
};

/// Blank columns between the roomy rung's frame and its keys cells.
const ROOMY_INSET: usize = 4;

/// Blank columns between the roomy rung's frame and its section headings.
const ROOMY_HEADING_INSET: usize = 2;

/// Blank columns between a roomy row's keys cell and its verb.
const ROOMY_GAP: usize = 8;

/// Keyboard rows the ladder may never drop: `f`, `m` and `?`.
const SHEET_KEEP: usize = 3;
// It names two things and only one of them is a keep-set.

/// Rows the sheet's frame costs, one border at each end.
const SHEET_FRAME: usize = 2;

/// What the sheet's own title bar spells, corner excluded.
const SHEET_TITLE: &str = "─ gestures ";

/// What the rounded rungs splice into the top border, `┐` and `┌` either side.
const SHEET_SPLICE: &str = " gestures ";

/// What the mouse group's heading spells, spaces included.
const SHEET_MOUSE_LABEL: &str = " mouse ";

/// What the keyboard group's heading spells, and it is drawn only on the
/// two-column rung.
const SHEET_KEYBOARD_LABEL: &str = " keyboard ";

/// The close control, which is the pane's first.
const SHEET_CLOSE: char = '✕';

/// The widest keys cell and the widest verb over a group, at one spelling.
fn fields_of<'a>(rows: impl IntoIterator<Item = &'a Gesture>, level: usize) -> (usize, usize) {
    let (mut keys, mut verb) = (0, 0);
    for row in rows {
        keys = keys.max(width_of(row.keys[level]));
        verb = verb.max(width_of(row.verb[level]));
    }
    (keys, verb)
}

/// The three widths a one-column rung needs: keys field, verb field, and the
/// whole sheet.
fn sheet_fields(level: usize, from: usize, mouse: bool) -> (usize, usize, usize) {
    let (mut keys, mut verb) = fields_of(kept_keyboard(from), level);
    if mouse {
        let (mk, mv) = fields_of(&MOUSE, level);
        keys = keys.max(mk);
        verb = verb.max(mv);
    }
    // Border, space, keys, two of gap, verb, space, border. Floored at the width the
    // title bar needs, so a narrow table cannot draw a truncated heading.
    let total = sheet_floor(sheet_span(keys, verb) - SHEET_GAP + 4);
    (keys, verb, total)
}

/// Columns a group's own block occupies: its keys field, two of gap, its verb
/// field, and the two columns of separation that follow it.
const fn sheet_span(keys: usize, verb: usize) -> usize {
    keys + SHEET_GAP + verb + SHEET_GAP
}

/// Blank columns between a keys cell and its verb, and between one group's block
/// and the next.
const SHEET_GAP: usize = 2;

/// The floor every rung's width takes, so a narrow table cannot draw a truncated
/// heading. Named once because all three rung shapes charge it.
fn sheet_floor(total: usize) -> usize {
    total.max(width_of(SHEET_TITLE) + *SHEET_COUNTER_FLOOR + 6)
}

/// The two-column rung's groups and the whole sheet's width.
fn sheet_beside(level: usize) -> (Group, Group, usize) {
    let (kb_keys, kb_verb) = fields_of(&KEYBOARD, level);
    let (ms_keys, ms_verb) = fields_of(&MOUSE, level);
    // Border and the space inside it, then each group's block in turn, then the
    // space and border that close the row.
    let keyboard = Group {
        at: 1,
        keys: kb_keys,
        verb: kb_verb,
        gap: SHEET_GAP,
    };
    // The heading row is measured too, not only the gesture rows.
    let mouse = Group {
        at: (keyboard.at + sheet_span(kb_keys, kb_verb))
            .max(keyboard.at + width_of(SHEET_KEYBOARD_LABEL)),
        keys: ms_keys,
        verb: ms_verb,
        gap: SHEET_GAP,
    };
    // The mouse group's block, then the space and the border that close the row.
    // `Group::at` points at the space *before* a group's keys, so the span from it
    // is one wider than the span the block itself occupies.
    let total = sheet_floor(mouse.at + sheet_span(ms_keys, ms_verb) + 1);
    (keyboard, mouse, total)
}

/// The roomy rung's one placed group and the whole sheet's width.
fn sheet_roomy() -> (Group, usize) {
    // Over [`SECTIONS`], not over the two tables.
    let (keys, verb) = fields_of(SECTIONS.iter().flat_map(|s| s.rows.rows()), 0);
    let group = Group {
        at: ROOMY_INSET,
        keys,
        verb,
        gap: ROOMY_GAP,
    };
    // Border, the inset, the keys field, the gap, the verb field, the inset again, and
    // the border, summed from the group that was just placed rather than from the
    // constants it was built out of.
    let mut total = group.at + group.keys + group.gap + group.verb + ROOMY_INSET + 2;
    for section in SECTIONS.iter() {
        total = total.max(ROOMY_HEADING_INSET + width_of(section.label) + 2);
    }
    (group, sheet_floor(total))
}

/// Rows the roomy rung draws, frame excluded.
fn sheet_roomy_rows() -> usize {
    // Walked rather than summed as `2 * SECTIONS.len() + KEYBOARD.len() +
    // MOUSE.len()`, for `sheet_roomy`'s reason one measurement over: the painter
    // walks the sections, and a formula agreeing with it by arithmetic is a second
    // copy of the number rather than the same one.
    1 + SECTIONS
        .iter()
        .map(|section| section.rows.rows().len() + 2)
        .sum::<usize>()
}

/// One group of gestures, placed: where its block starts inside the sheet, and
/// the two fields its rows are drawn in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Group {
    /// Columns in from the sheet's left edge where this group's frame-relative
    /// block begins. The keys cells start one further in.
    at: usize,
    /// The keys field's width.
    keys: usize,
    /// The verb field's width.
    verb: usize,
    /// Blank columns between this group's keys cells and its verbs.
    gap: usize,
}

impl Group {
    /// The screen column this group's keys cells start in.
    fn keys_at(self, left: u16) -> u16 {
        left + self.at as u16 + 1
    }

    /// The screen column this group's verb cells start in.
    fn verb_at(self, left: u16) -> u16 {
        self.keys_at(left) + self.keys as u16 + self.gap as u16
    }
}

/// Rows a one-column rung draws, frame excluded.
fn sheet_rows(from: usize, mouse: bool) -> usize {
    column_lines(from, mouse).count()
}

/// One drawn line of a one-column rung.
#[derive(Clone, Copy)]
enum Line {
    /// A gesture.
    Row(&'static Gesture),
    /// The mouse group's heading, which costs a row and is not a gesture.
    Heading(&'static str),
}

/// Every line a one-column rung draws, in the order it draws them.
fn column_lines(from: usize, mouse: bool) -> impl Iterator<Item = Line> {
    kept_keyboard(from).map(Line::Row).chain(
        mouse
            .then(|| {
                std::iter::once(Line::Heading(SHEET_MOUSE_LABEL)).chain(MOUSE.iter().map(Line::Row))
            })
            .into_iter()
            .flatten(),
    )
}

/// Gestures the whole sheet holds, which is what the page counter counts against.
const SHEET_TOTAL: usize = KEYBOARD.len() + MOUSE.len();

/// The page counter the title bar carries: which gestures this page draws, and how
/// many the tables hold.
fn sheet_counter(shown: (usize, usize)) -> String {
    let (first, last) = shown;
    if first == last {
        format!(" {first} of {SHEET_TOTAL} ")
    } else {
        format!(" {first}-{last} of {SHEET_TOTAL} ")
    }
}

/// The widest [`sheet_counter`] can ever be, which every rung's width charges.
static SHEET_COUNTER_FLOOR: LazyLock<usize> = LazyLock::new(|| {
    (1..=SHEET_TOTAL)
        .flat_map(|first| (first..=SHEET_TOTAL).map(move |last| (first, last)))
        .map(|shown| width_of(&sheet_counter(shown)))
        .max()
        .unwrap_or_default()
});

/// The ordinals of the gestures a page draws, or `None` when it draws them all.
fn shown_of(from: usize, mouse: bool, skip: usize, take: usize) -> Option<(usize, usize)> {
    let is_row = |line: &Line| matches!(line, Line::Row(_));
    let before = column_lines(from, mouse).take(skip).filter(is_row).count();
    let count = column_lines(from, mouse)
        .skip(skip)
        .take(take)
        .filter(is_row)
        .count();
    (count < SHEET_TOTAL).then_some((before + 1, before + count))
}

/// Rows the two-column rung draws, frame excluded: the taller column, heading
/// included.
fn sheet_beside_rows() -> usize {
    1 + KEYBOARD.len().max(MOUSE.len())
}

/// Where the gestures sheet goes, or `None` on a pane that cannot hold one.
fn sheet_plan(area: Rect, footer_rows: u16, margins: (u16, u16), page: usize) -> Option<SheetPlan> {
    let body = area.height.saturating_sub(1 + footer_rows);
    let room = area.width.saturating_sub(margins.0 + margins.1);
    let level = usize::from(sheet_fields(0, 0, true).2 > usize::from(room));

    // Rows the body has for a page, which is the whole of the height axis now.
    let capacity = usize::from(body).saturating_sub(SHEET_FRAME);
    // The floor, stated once and early rather than folded into the rung sequence. Below
    // it no rung fits on the height axis at all, and not only the paged ones: the
    // shortest rung above them is the two-column one at sixteen rows.
    if capacity < SHEET_KEEP {
        return None;
    }
    // The row sets, widest first, so a pane with the columns for the mouse group
    // pages it rather than dropping it.
    let sets = std::iter::once((0, true))
        .chain((0..=KEYBOARD.len() - SHEET_KEEP).map(|from| (from, false)));

    // The order is the ruling's: the roomy rung where there is room for it, then
    // every row in one column, then the two-column rung that buys height with
    // width, then the paged rungs, widest row set first.
    let rungs = std::iter::once_with(roomy_fit)
        .chain(std::iter::once_with(move || column_fit(level, 0, true)))
        .chain([0, 1].into_iter().map(beside_fit))
        .chain(sets.map(move |(from, mouse)| paged_fit(level, from, mouse, page, capacity)));
    for fit in rungs {
        let height = fit.rows + SHEET_FRAME;
        if fit.total > usize::from(room) || height > usize::from(body) {
            continue;
        }
        let width = fit.total as u16;
        let height = height as u16;
        let left = area.x + margins.0 + (room - width) / 2;
        let top = area.y + 1 + (body - height) / 2;
        return Some(SheetPlan {
            area: Rect {
                x: left,
                y: top,
                width,
                height,
            },
            level: fit.level,
            shape: fit.shape,
            // Three in from the right edge: `┐`, the space before it, and this.
            close: (left + width - 3, top),
            pages: fit.pages,
        });
    }
    None
}

/// How many pages `?` walks through on this pane before the sheet closes, and
/// zero on a pane that draws none.
fn sheet_pages_of(area: Rect, footer_rows: u16, margins: (u16, u16)) -> usize {
    sheet_plan(area, footer_rows, margins, 0).map_or(0, |plan| plan.pages)
}

/// What one rung would cost and what it would draw, before it is known to fit.
struct Fit {
    /// Which spelling this rung takes.
    level: usize,
    /// The whole sheet's width at this rung.
    total: usize,
    /// Rows it draws, frame excluded.
    rows: usize,
    /// What the drawer will be told to draw.
    shape: Shape,
    /// How many pages `?` walks through before it closes.
    pages: usize,
}

/// One column, every row drawn, headed and with air around it.
fn roomy_fit() -> Fit {
    let (group, total) = sheet_roomy();
    Fit {
        level: 0,
        total,
        rows: sheet_roomy_rows(),
        shape: Shape::Roomy { group },
        pages: 1,
    }
}

/// One column, at a spelling the pane picked, with the mouse group below the
/// keyboard group or dropped, and `from` keyboard rows already gone.
fn column_fit(level: usize, from: usize, mouse: bool) -> Fit {
    // A capacity nothing can exceed is one page by arithmetic, which is the whole of
    // what makes this rung and the paged ones one constructor.
    paged_fit(level, from, mouse, 0, usize::MAX)
}

/// One column, `capacity` rows of it at a time, showing page `page`.
fn paged_fit(level: usize, from: usize, mouse: bool, page: usize, capacity: usize) -> Fit {
    let (keys, verb, total) = sheet_fields(level, from, mouse);
    let lines = sheet_rows(from, mouse);
    let pages = lines.div_ceil(capacity.max(1)).max(1);
    let page = page.min(pages - 1);
    let skip = page * capacity;
    let take = capacity.min(lines - skip);
    Fit {
        level,
        total,
        // The box is the pane's, not this page's.
        rows: capacity.min(lines),
        shape: Shape::Column {
            from,
            mouse,
            group: Group {
                at: 1,
                keys,
                verb,
                gap: SHEET_GAP,
            },
            skip,
            take,
        },
        pages,
    }
}

/// Two columns, keyboard beside mouse, every row drawn.
fn beside_fit(level: usize) -> Fit {
    let (keyboard, mouse, total) = sheet_beside(level);
    Fit {
        level,
        total,
        rows: sheet_beside_rows(),
        shape: Shape::Beside { keyboard, mouse },
        pages: 1,
    }
}

/// Which of the sheet's three shapes a rung draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    /// One column: the first `from` entries of [`DROP_ORDER`] already given up,
    /// the mouse group below the keyboard group or gone.
    Column {
        /// How many entries of [`DROP_ORDER`] have been given up.
        from: usize,
        /// Whether the mouse group is drawn below the keyboard group.
        mouse: bool,
        /// The single column both groups share, so their rows stay a table.
        group: Group,
        /// Lines of [`column_lines`] this page starts after.
        skip: usize,
        /// Lines this page draws, which on the last page is fewer than the box
        /// has rows.
        take: usize,
    },
    /// One column, every row drawn, the sections headed and air around them.
    Roomy {
        /// The single column every section's rows share.
        group: Group,
    },
    /// Two columns, every row drawn, each group placed by the layout.
    Beside {
        /// The keyboard group, on the left.
        keyboard: Group,
        /// The mouse group, on the right.
        mouse: Group,
    },
}

impl Shape {
    /// The ordinals the page counter draws, or `None` when this shape draws every
    /// gesture the tables hold.
    fn shown(self) -> Option<(usize, usize)> {
        match self {
            Self::Column {
                from,
                mouse,
                skip,
                take,
                ..
            } => shown_of(from, mouse, skip, take),
            // Both draw the whole table or are not selected, so there is never
            // anything for a counter to say.
            Self::Roomy { .. } | Self::Beside { .. } => None,
        }
    }
}

/// A laid-out gestures sheet: where it goes and which rung it draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SheetPlan {
    /// The whole sheet, frame included.
    area: Rect,
    /// Which spelling every cell takes: 0 wide, 1 tight.
    level: usize,
    /// Roomy, one column or two, with each group's fields and placement.
    shape: Shape,
    /// The close control's cell.
    close: (u16, u16),
    /// How many pages `?` walks through on this pane before it closes.
    pages: usize,
}

impl SheetPlan {
    /// What a pointer needs to know about it.
    fn target(&self) -> Sheet {
        Sheet {
            left: self.area.x,
            top: self.area.y,
            width: self.area.width,
            height: self.area.height,
            close: self.close,
        }
    }
}

/// A buffer, a palette, and the one measurement the body rows share.
struct Painter<'a> {
    buf: &'a mut Buffer,
    theme: &'a Theme,
    /// Which glyphs the sparkline draws from, from [`render`]'s own parameter.
    glyphs: Glyphs,
    /// Digits reserved for line numbers, or zero when there is no room.
    gutter: usize,
    /// Blank columns the pane keeps on its left and on its right, from
    /// [`margins_of`] and resolved once against the whole pane.
    inset: u16,
    /// The right-hand half of the pair above.
    trailing: u16,
    /// What the content rows have cost so far, returned by [`render`].
    paint: PaintStats,
    /// The cell a step button is being held on, from [`Chrome::pressed`].
    pressed: Option<(u16, u16)>,
    /// Which region's bar is being dragged, from [`Chrome::gripped`].
    gripped: Option<Grabbed>,
    /// What the pointer is resting on, from [`Chrome::hovered`].
    hovered: Option<Hovered>,
    /// Which bar the keys are scrolling and which way, from
    /// [`Chrome::scrolling`].
    scrolling: Option<(Grabbed, isize)>,
    /// [`Theme::spark_ramp`], computed once per paint: `None` below truecolour
    /// and on palettes whose stops are not RGB, which is the whole ladder.
    spark_ramp: Option<[Color; 8]>,
    /// [`Chrome::icons`]: whether a listed path carries its type's glyph.
    icons: bool,
    /// Where the gestures sheet will be composited, so nothing underneath it
    /// claims a column it will draw over.
    covered: Option<Rect>,
    /// [`Chrome::links`] with [`Chrome::root`]: the `file://` prefix every
    /// linked path shares, or `None` where links are off or rootless.
    link_root: Option<String>,
}

impl Painter<'_> {
    /// The columns of `area` a glyph may use.
    fn text_area(&self, area: Rect) -> Rect {
        Rect {
            x: area.x.saturating_add(self.inset),
            width: area
                .width
                .saturating_sub(self.inset)
                .saturating_sub(self.trailing),
            ..area
        }
    }

    /// The same, for a rect a scrollbar may already have narrowed.
    fn region_text(&self, area: Rect, pane: Rect) -> Rect {
        // The two derivations of the margin have to be the same one.
        debug_assert_eq!(
            margins_of(pane.width),
            (self.inset, self.trailing),
            "a region is being drawn against a different pane than the painter was \
             built for, so its margin and the chrome's have come apart"
        );
        let left = area.x.saturating_add(self.inset);
        // Through `Rect::right`, which `ratatui` defines as exactly this saturating
        // add and which this file already calls one region over.
        let stop = area.right().min(pane.right().saturating_sub(self.trailing));
        Rect {
            x: left,
            width: stop.saturating_sub(left),
            ..area
        }
    }

    /// Write `text` at `x`, clipped to `limit` columns, and return the next
    /// column.
    fn put(&mut self, x: u16, y: u16, text: &str, limit: usize, style: Style) -> u16 {
        if limit == 0 {
            return x;
        }
        let (next, _) = self.buf.set_stringn(x, y, text, limit, style);
        next
    }

    /// Write `text` clipped to `limit`, and say so when it did not fit.
    fn put_marked(&mut self, x: u16, y: u16, text: &str, limit: usize, style: Style) {
        if limit == 0 || text.is_empty() {
            return;
        }
        if width_of(text) <= limit {
            self.put(x, y, text, limit, style);
            return;
        }
        // `limit` is always derived from a screen width, so it fits in a `u16`.
        self.put(x, y, text, limit - 1, style);
        self.buf
            .set_stringn(x + limit as u16 - 1, y, CONTINUES, 1, style);
    }

    /// Write a sequence of styled runs under one limit, marking the edge.
    fn put_runs_marked(
        &mut self,
        x: u16,
        y: u16,
        runs: &[(String, Style)],
        clipped: bool,
        limit: usize,
    ) {
        if limit == 0 {
            return;
        }

        let budget = if clipped { limit - 1 } else { limit };

        // The style the mark inherits: whichever run ran out of room, so a
        // clipped comment is marked in the comment's colour rather than in
        // whatever the row started with.
        let mut marked_in = runs.first().map_or(self.theme.context, |(_, style)| *style);
        let end = x + budget as u16;
        let mut at = x;
        for (text, style) in runs {
            if at >= end {
                break;
            }
            marked_in = *style;
            at = self.put(at, y, text, usize::from(end - at), *style);
        }

        if clipped {
            self.buf
                .set_stringn(x + limit as u16 - 1, y, CONTINUES, 1, marked_in);
        }
    }

    /// Write `text` so that it ends at the right edge of `area`.
    fn put_right(&mut self, area: Rect, text: &str, style: Style) -> usize {
        let width = width_of(text);
        if width == 0 || width > usize::from(area.width) {
            return 0;
        }
        self.buf.set_stringn(
            area.x + area.width - width as u16,
            area.y,
            text,
            width,
            style,
        );
        // The gap keeps the right-hand text from touching whatever is drawn from
        // the left, which at forty columns happens constantly.
        width + 1
    }

    /// One line of chrome: a ladder on the left, one token on the right, and the
    /// right-hand side wins the space.
    fn status_line<S: AsRef<str>>(
        &mut self,
        area: Rect,
        left: &[S],
        style: Style,
        right: &str,
        right_style: Style,
    ) {
        // The wash takes the whole row and the text takes the inset one,
        // which is §5.3's furniture rule and the reason these two lines address
        // different rectangles. See [`Painter::text_area`].
        self.buf.set_style(area, self.theme.chrome_dim);
        let text = self.text_area(area);
        let taken = self.put_right(text, right, right_style);
        let room = usize::from(text.width).saturating_sub(taken);
        let rung = widest_fitting_or_last(left, room);
        self.put_marked(text.x, text.y, rung, room, style);
    }

    /// Put `label` as one OSC 8 hyperlink to `root/path`, tui-link's shape.
    fn put_linked(&mut self, x: u16, y: u16, label: &str, root: &str, path: &str, ink: Style) {
        let width = width_of(label) as u16;
        if width == 0 {
            return;
        }
        // A claim that reaches under the sheet is not made at all, because the differ
        // would then skip the sheet's own cells across it and the row would show
        // through the overlay.
        if let Some(sheet) = self.covered {
            let rows = sheet.y..sheet.y.saturating_add(sheet.height);
            let reaches = x.saturating_add(width) > sheet.x && x < sheet.right();
            if rows.contains(&y) && reaches {
                self.put(x, y, label, usize::from(width), ink);
                return;
            }
        }
        let mut uri = String::with_capacity(root.len() + path.len() + 8);
        uri.push_str("file://");
        for half in [root, "/", path] {
            for byte in half.bytes() {
                match byte {
                    b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                        uri.push(byte as char);
                    }
                    other => {
                        uri.push('%');
                        uri.push_str(&format!("{other:02X}"));
                    }
                }
            }
        }
        let wrapped = format!("\x1b]8;;{uri}\x1b\\{label}\x1b]8;;\x1b\\");
        // The shadow: what the terminal will actually be showing in the columns
        // the cell below claims. Styled like the label so a later `set_style`
        // over the row cannot make the record disagree with the paint.
        for (offset, grapheme) in label.chars().skip(1).enumerate() {
            let at = x + 1 + offset as u16;
            if at >= x + width {
                break;
            }
            if let Some(cell) = self.buf.cell_mut((at, y)) {
                cell.reset();
                cell.set_char(grapheme);
                cell.set_style(ink);
            }
        }
        if let Some(cell) = self.buf.cell_mut((x, y)) {
            cell.set_symbol(&wrapped);
            cell.set_style(ink);
            cell.diff_option = ratatui::buffer::CellDiffOption::ForcedWidth(
                std::num::NonZeroU16::new(width).expect("width is checked nonzero above"),
            );
        }
    }

    fn header(&mut self, area: Rect, view: &View, chrome: &Chrome) {
        // The worktree name leads the left, which is the one place the layout departs
        // from `assets/preview.svg` on purpose: a title bar reading `vigia` spends six
        // of forty columns telling the reader which program they started, and what they
        // cannot tell by looking is which *tree*.
        let right = chrome.mode.word();
        // A dead watch has to be visible, not merely present. Drawn in the header's dim
        // grey, `not watching` is a word a reader has to go looking for, and a monitor
        // whose failure state looks exactly like its working one has failed twice.
        let right_style = match chrome.mode {
            Mode::Watching => self.theme.chrome_dim,
            Mode::Lost => self.theme.alert,
        };
        // One style across both facts on the left, and that is a ruling. Drawing the
        // count in the mode word's dim grey gives one clause two weights, telling the
        // reader in colour that these are separate claims.
        let rungs = header_left(
            &chrome.worktree,
            chrome.branch.as_deref(),
            view.files,
            chrome.staged,
        );
        self.status_line(area, &rungs, self.theme.chrome, right, right_style);
    }

    /// The footer, on the bottom one or two rows of `area`.
    fn footer(&mut self, area: Rect, view: &View, chrome: &Chrome, footer: &Footer<'_>) {
        let position = position_of(view.top.file, view.files);
        // Clamped to what was reserved, not to the width.
        let rungs = state_rungs(chrome.following, &position);
        let state = widest_fitting(&rungs, footer.reserved);
        // One string rather than two placements, because `status_line` puts a single
        // right-hand token and lets the left lose characters to it.
        let right = match (footer.diagnostics.as_str(), state) {
            ("", state) => state.to_owned(),
            (diagnostics, "") => diagnostics.to_owned(),
            (diagnostics, state) => format!("{diagnostics}{CELL_GAP}{state}"),
        };

        let style = if footer.alert {
            self.theme.alert
        } else {
            self.theme.chrome_dim
        };
        let bottom = Rect {
            y: area.y + area.height - 1,
            height: 1,
            ..area
        };

        // Above the footer's own rows, and full bleed like the one over the diff.
        if footer.rule {
            self.rule(Rect {
                y: bottom.y - footer.rows,
                height: 1,
                ..area
            });
        }

        // Where `put_right` will place that string, and how much of its head the
        // readouts occupy. Computed from the same two strings it is drawn from,
        // so the tint below cannot address a column the text does not.
        let text = self.text_area(bottom);
        let placed = text.x + text.width - width_of(&right).min(usize::from(text.width)) as u16;
        let readouts = width_of(&footer.diagnostics);

        if footer.rows == 2 {
            // State above, hints below. The hints keep the bottom row they had
            // at eighty columns, so narrowing a pane moves the new line in
            // rather than moving the old one out from under the reader.
            let upper = Rect {
                y: bottom.y - 1,
                ..bottom
            };
            self.status_line(
                upper,
                &[""],
                self.theme.chrome_dim,
                &right,
                self.theme.chrome_dim,
            );
            // The inset row for the reason `placed` uses one: the walk is bounded
            // by its `row`'s right edge, and the text's edge is not the pane's.
            self.tint_readouts(Rect { y: upper.y, ..text }, placed, readouts);
            self.status_line(bottom, &[footer.left], style, "", self.theme.chrome_dim);
        } else {
            self.status_line(bottom, &[footer.left], style, &right, self.theme.chrome_dim);
            self.tint_readouts(text, placed, readouts);
        }
    }

    /// Give the footer's right-hand side the three colours the picture draws.
    fn tint_readouts(&mut self, row: Rect, at: u16, readouts: usize) {
        // A measurement and its unit: a run opening with a digit or the
        // over-magnitude sigil, carried through the letters that name the unit.
        // The label `frame` opens with a letter and so is never picked up.
        if row.y >= self.buf.area.bottom() {
            return;
        }
        let edge = row.x.saturating_add(row.width).min(self.buf.area.right());
        let end = at.saturating_add(readouts as u16).min(edge);
        let opening = |c: char| c.is_ascii_digit() || c == '>';
        let carrying = |c: char| c.is_ascii_digit() || c == '.' || c.is_ascii_alphabetic();
        let mut x = at.min(edge);
        while x < end {
            let head = self.buf[(x, row.y)].symbol().chars().next();
            if !head.is_some_and(opening) {
                x += 1;
                continue;
            }
            self.buf[(x, row.y)].set_style(self.theme.chrome);
            x += 1;
            while x < end
                && self.buf[(x, row.y)]
                    .symbol()
                    .chars()
                    .next()
                    .is_some_and(carrying)
            {
                self.buf[(x, row.y)].set_style(self.theme.chrome);
                x += 1;
            }
        }

        // Bounded to the state's own columns, which start where the diagnostics end.
        let mut glyph = [0u8; 4];
        let glyph: &str = FOLLOW_MARK.encode_utf8(&mut glyph);
        for x in end.min(edge)..edge {
            if self.buf[(x, row.y)].symbol() == glyph {
                self.buf[(x, row.y)].set_style(self.theme.added);
                return;
            }
        }
    }

    /// Draw the worktree churn band, `SPEC.md` §11.1's masthead.
    fn band(&mut self, area: Rect, view: &View) {
        // The precondition, checked rather than assumed: `Body::split` owns
        // this decision and hands down zero rows when it says no, so reaching
        // here on a pane too narrow would mean the two had come apart.
        debug_assert!(
            band_fits(area.width),
            "the band was given {} columns, under the floor the layout applies",
            area.width
        );
        if !band_fits(area.width) || area.height == 0 {
            return;
        }
        let left_edge = area.x.saturating_add(self.inset);
        let width = usize::from(planning_width(area.width, area.width, 0));

        // One value per sub-column, and a zero draws the baseline, which is the settled
        // answer to this exact signal
        // ([#232](https://github.com/breferrari/vigia/issues/232)).
        let rung = self.glyphs;
        let density = rung.density();
        // The pane can ask for more sub-columns than the window holds samples, and then
        // values repeat rather than run out.
        let slots = width * density;
        // The level, not the events. `assets/preview.svg` draws this as a wave and a
        // write is a point event, so the raw series is zero almost everywhere and an
        // area chart of it is a spike train.
        let series = view.worktree_churn.levels(slots);

        // `Churn::scale_at`, the same rule the sparkline divides by. It lived here
        // while the band was the only element that had it, which is exactly how the
        // sparkline was left dividing by a maximum over the same byte samples.
        let scale = view.worktree_churn.scale_at(slots);
        // No data, no axis, which is what keeps the reported defect fixed while the
        // axis exists at all.
        if scale == 0 {
            return;
        }
        let rows = usize::from(area.height);
        // Levels one row carries. The block ramp gives eight, a 2x4 cell gives
        // its dot rows less the baseline, and [`Glyphs::glyph`] already spells
        // both, floor included: level zero is the axis rather than nothing.
        let levels = rung.levels();
        // The ramp's top row index, invariant across every cell and row of
        // this band rather than recomputed inside both loops.
        let top = rows.saturating_sub(1).max(1);

        for cell in 0..width {
            // A dense cell carries two sub-columns, left older than right, which is
            // `Glyphs::glyph`'s own order and the sparkline's. At the block rung the
            // density is one and the right half is never read.
            let at = |sub: usize| series[cell * density + sub];
            let (older, newer) = (at(0), at(density - 1));
            let full = |total: u32| level_to(total, scale, rows * levels);
            let (left, right) = (full(older), full(newer));
            // Against the same denominator the heights are scaled from, so
            // colour and shape say one thing at one scale.
            let band = Band::of_count(older.max(newer), scale);
            for row in 0..rows {
                // Drawn bottom up, so `row` counts from the baseline and the
                // buffer's `y` counts down from the top.
                let y = area.y + (rows - 1 - row) as u16;
                let fill = |level: usize| level.saturating_sub(row * levels).min(levels);
                let (low, high) = (fill(left), fill(right));
                // Sky above the bar is left alone; the baseline row is not. A graph's
                // empty upper rows are background, and painting them would draw a solid
                // block the height of the band on every column.
                if row > 0 && low == 0 && high == 0 {
                    continue;
                }
                let glyph = rung.glyph(low, high);
                let x = left_edge.saturating_add(cell as u16);
                // Multi-row graphs colour per row against the vertical axis, btop's own
                // rule: the baseline draws the quiet stop and the top row the hot one,
                // one style per row, so the graph reads hotter as it climbs while
                // costing one lookup.
                let mut ink = self.theme.spark_at(band);
                if let Some(ramp) = self.spark_ramp.as_ref() {
                    ink = ink.fg(ramp[(row * 7 / top).min(7)]);
                }
                self.bar_cell(x, y, glyph, ink);
            }
        }
    }

    /// Draw the pinned file list, `SPEC.md` §11.1's upper region.
    fn list(&mut self, area: Rect, available: u16, view: &View, pane: u16) {
        // Against this region's full width rather than the rect it was handed: `area`
        // has already lost the bar's columns when one is drawn, and deciding from it
        // would make the caret's presence depend on whether the list happens to be
        // scrollable.
        let caret = affords_caret(available, pane);
        let gutter = if caret { caret_gutter(pane) as u16 } else { 0 };

        // The caret sits on the pane's own leading column, and the row starts after
        // whatever margin is left over.
        let left = area.x;

        // From the pane, less the caret's gutter and less a scrollbar column whether or
        // not one was taken, for [`affords_caret`]'s reason one element out.
        let shown = usize::from(area.height);
        let inner = planning_width(available, pane, gutter);
        let columns = Columns::plan(inner, self.glyphs);
        // Hoisted beside the width it goes with, because it is a property of the region
        // and not of any row.
        let origin_x = area.x.saturating_add(self.inset).saturating_add(gutter);

        // The file index is walked rather than added to the offset: with run separators
        // in the window a drawn row and a file are not the same ordinal, so `list_top +
        // offset` names the wrong file from the first separator onwards — and it names
        // it *silently*, putting the caret on a neighbour.
        let mut file = view.list_top;
        for (offset, row) in view.list.iter().take(shown).enumerate() {
            let y = area.y + offset as u16;
            let entry = match row {
                ListRow::Group { origin, count } => {
                    self.group_row(
                        Rect {
                            y,
                            height: 1,
                            x: origin_x,
                            width: inner,
                        },
                        *origin,
                        *count,
                    );
                    continue;
                }
                ListRow::File(entry) => entry,
            };
            let at = file;
            file += 1;
            // Saturating, because `list_top` is not bounded by the file count: a pane
            // too short for a region hands the reader's request back untouched, so
            // `View::collect` can legitimately report `usize::MAX` here.
            let current = caret && at == view.top.file;
            if current {
                self.put(left, y, CARET, CARET_WIDTH, self.theme.pulse);
            }
            self.file_row(
                Rect {
                    y,
                    height: 1,
                    // `area.x` is this region's leading column: `with_bar` narrows the
                    // *width* on the right without moving the origin, so it is the
                    // origin `render` handed down.
                    x: origin_x,
                    width: inner,
                },
                &Heading::of(entry, view.grouped),
                view.scale,
                &columns,
                current,
                // The one region hover answers on, and a literal for the same
                // reason `current` is one at the other call site: a mark confined
                // by a parameter cannot reach a region that was never meant to
                // carry it, where one confined by geometry is only ever as safe as
                // the layout that happens to be drawn.
                true,
            );
        }
    }

    /// One run's separator: `──  staged  2 ─────────`.
    fn group_row(&mut self, area: Rect, origin: Origin, count: usize) {
        let room = usize::from(area.width);
        let ink = match origin {
            Origin::Staged => self.theme.staged,
            Origin::Unstaged => self.theme.chrome,
        };
        let word = origin.label();
        if room < width_of(word) {
            return;
        }

        // Widest first, and every rung is built rather than measured twice: the
        // leading rule is what places the word, so the two cannot disagree about
        // where it starts.
        let lead = "\u{2500}\u{2500}  ";
        let tally = format!("  {count} ");
        let (lead, tally) = if width_of(lead) + width_of(word) + width_of(&tally) <= room {
            (lead, tally)
        } else if width_of(lead) + width_of(word) < room {
            (lead, String::new())
        } else {
            ("", String::new())
        };

        let mut at = area.x;
        if !lead.is_empty() {
            at = self.put(at, area.y, lead, room, self.theme.chrome_dim);
        }
        let used = usize::from(at - area.x);
        at = self.put(at, area.y, word, room - used, ink);
        let used = usize::from(at - area.x);
        if !tally.is_empty() {
            at = self.put(at, area.y, &tally, room - used, self.theme.chrome_dim);
        }

        // The trailing rule takes whatever is left, which is what makes the row
        // read as a rule with a word on it rather than as a word with two dashes
        // in front. Nothing is drawn where nothing is left.
        let rest = room - usize::from(at - area.x);
        if rest > 0 {
            self.rule(Rect {
                x: at,
                width: rest as u16,
                height: 1,
                ..area
            });
        }
    }

    /// Decide this region's scrollbar, and hand back the room left for content
    /// along with the shape decided.
    fn with_bar(&mut self, region: Rect, wide: bool, span: u64, of: u64) -> (Rect, Bar) {
        let bar = bar_for(wide, region.height, span, of);
        if !bar.drawn() {
            return (region, bar);
        }
        (
            Rect {
                width: region.width.saturating_sub(BAR_WIDTH as u16),
                ..region
            },
            bar,
        )
    }

    /// Draw a one-column scrollbar down the right of `area`.
    fn scrollbar(&mut self, area: Rect, whose: Grabbed, bar: Bar, at: u64, span: u64, of: u64) {
        // Width and height guarded here as well as by the caller. `render` only calls
        // this above `BAR_FLOOR` and only through `bar_for`, so a zero width cannot
        // reach it today, and the subtractions below would underflow if one ever did.
        if area.height == 0 || area.width == 0 || !scrollable(span, of) {
            return;
        }

        // The track, not the region.
        let (track_top, track_rows) = bar.track(area.y, area.height);
        let rows = u64::from(track_rows);
        if rows == 0 {
            return;
        }

        let thumb = ((span * rows) / of).max(1).min(rows);
        // The scroll's travel mapped onto the track's travel, not the position mapped
        // onto the whole track.
        let travel = of - span;
        let start = (at.min(travel) * (rows - thumb)) / travel;
        // Through [`bar_column`], so the column a pointer is told about and the
        // column this paints in are one formula rather than two that agree.
        let x = bar_column(area);

        // Lit while the reader is dragging this bar, which is the same reading the step
        // buttons already carry one block down: bright means *you are doing this now*.
        // The thumb is the thing being moved, so it is the thing that answers.
        let dragging = self.gripped == Some(whose);
        let hovering = self.hovered == Some(Hovered::Track(whose));
        let thumb_style = if dragging {
            self.theme.bar_active
        } else if hovering {
            self.theme.bar_hover
        } else {
            self.theme.bar
        };
        for row in 0..rows {
            let filled = row >= start && row < start + thumb;
            let (glyph, style) = if filled {
                (BAR_THUMB, thumb_style)
            } else {
                (BAR_TRACK, self.theme.bar_track)
            };
            self.bar_cell(x, track_top + row as u16, glyph, style);
        }

        // The buttons last, and after the track rather than around it.
        if matches!(bar, Bar::Stepped) {
            let bottom = area.y + area.height - 1;
            for (y, glyph, way) in [(area.y, STEP_UP, -1isize), (bottom, STEP_DOWN, 1)] {
                // Three ways an arrow lights, and they are one state rather than three:
                // the reader is holding *this* button, the reader is dragging *this*
                // bar, or the keys are moving this region *that* way.
                let held = self.pressed == Some((x, y));
                // This bar's own scroll, not any scroll.
                let keyed = self
                    .scrolling
                    .is_some_and(|(on, by)| on == whose && by.signum() == way)
                    && self.gripped != Some(whose);
                // Three rungs, and a press beats a hover. `bar_track` at rest, `bar`
                // under a pointer, `bar_active` while a gesture is on it.
                let hovered = self.hovered == Some(Hovered::Button(x, y));
                let style = if held || keyed {
                    self.theme.bar_active
                } else if hovered {
                    self.theme.bar_hover
                } else {
                    self.theme.bar_track
                };
                self.bar_cell(x, y, glyph, style);
            }
        }
    }

    /// Write one cell of a scrollbar's column.
    fn bar_cell(&mut self, x: u16, y: u16, glyph: char, style: Style) {
        if let Some(cell) = self.buf.cell_mut((x, y)) {
            // The band's background is kept; the bar owns everything else.
            let behind = cell.bg;
            cell.set_symbol(glyph.encode_utf8(&mut [0u8; 4]));
            cell.fg = style.fg.unwrap_or(cell.fg);
            cell.bg = style.bg.unwrap_or(behind);
            cell.modifier = style.add_modifier;
        }
    }

    /// Draw the rule that separates the two regions.
    fn rule(&mut self, area: Rect) {
        // Cell by cell for the reason [`Painter::scrollbar`] gives. The allocation a
        // built string costs is the small half: measured at two hundred columns it was
        // 5% of the row, and segmenting two hundred graphemes was the rest.
        let style = self.theme.chrome_dim;
        let mut glyph = [0u8; 4];
        let glyph = RULE.encode_utf8(&mut glyph);
        for x in area.x..area.x + area.width {
            // Clipped, for the reason the heat strip gives.
            let Some(cell) = self.buf.cell_mut((x, area.y)) else {
                continue;
            };
            cell.set_symbol(glyph).set_style(style);
        }
    }

    /// Draw the gestures sheet over whatever the regions already drew.
    fn sheet(&mut self, plan: &SheetPlan) {
        let area = plan.area;
        let frame = self.theme.chrome_dim;
        let lit = self.theme.chrome;
        let width = usize::from(area.width);

        for y in area.y..area.y.saturating_add(area.height) {
            for x in area.x..area.x.saturating_add(area.width) {
                // Clipped rather than assumed, for the reason the heat strip is:
                // any area is legal here, including one the buffer has shrunk
                // under.
                if let Some(cell) = self.buf.cell_mut((x, y)) {
                    // Reset first, then style.
                    cell.reset();
                    cell.set_symbol(" ").set_style(frame);
                }
            }
        }

        // The title bar, with the close control's cell left blank and written after it
        // in its own weight: a control is not part of the frame it sits in, which is
        // the step buttons' rule one element over.
        let counter = plan.shape.shown().map(sheet_counter).unwrap_or_default();
        // The corners follow the glyph rung, and the splice goes with them (`SPEC.md`
        // §11.2 B18).
        let rounded = !matches!(self.glyphs, Glyphs::Block);
        let mut top = String::with_capacity(width * 3);
        if rounded {
            top.push('╭');
            top.push('┐');
            top.push_str(SHEET_SPLICE);
            top.push_str(&counter);
            top.push('┌');
            // Corner, splice brackets, the title, the counter, then the three
            // cells the control sits in and the closing corner: the same
            // sixteen fixed cells the square spelling costs, which is what
            // keeps every width rung where it was.
            let fixed = 3 + width_of(SHEET_SPLICE) + width_of(&counter) + 4;
            for _ in 0..width.saturating_sub(fixed) {
                top.push(RULE);
            }
            top.push_str("   ╮");
        } else {
            top.push('┌');
            top.push_str(SHEET_TITLE);
            top.push_str(&counter);
            // Corner, title, counter, dashes, then the three cells the control
            // sits in and the closing corner. The rule cannot go negative
            // because `sheet_floor` charges `sheet_counter_floor` on every rung.
            for _ in 0..width.saturating_sub(width_of(SHEET_TITLE) + width_of(&counter) + 5) {
                top.push(RULE);
            }
            top.push_str("   ┐");
        }
        self.put(area.x, area.y, &top, width, frame);
        if rounded {
            // The spliced word in the chrome's own weight, over the border's
            // reserved gap: the splice is what makes the title read as a label
            // on the box rather than a break in it.
            self.put(
                area.x + 2,
                area.y,
                SHEET_SPLICE,
                width_of(SHEET_SPLICE),
                lit,
            );
        }
        // The control takes a hover rung, which is what says it is clickable. B10's
        // ladder, minus its top rung: chrome at rest and [`Theme::bar_hover`] under the
        // pointer.
        let hovered = self.hovered == Some(Hovered::Button(plan.close.0, plan.close.1));
        let control = if hovered { self.theme.bar_hover } else { lit };
        self.put(
            plan.close.0,
            plan.close.1,
            &SHEET_CLOSE.to_string(),
            1,
            control,
        );

        match plan.shape {
            Shape::Roomy { group } => self.sheet_roomy(plan, group),
            Shape::Column {
                from,
                mouse,
                group,
                skip,
                take,
            } => self.sheet_column(plan, from, mouse, group, skip, take),
            Shape::Beside { keyboard, mouse } => self.sheet_beside(plan, keyboard, mouse),
        }

        let mut bottom = String::with_capacity(width * 3);
        bottom.push(if rounded { '╰' } else { '└' });
        for _ in 0..width.saturating_sub(2) {
            bottom.push(RULE);
        }
        bottom.push(if rounded { '╯' } else { '┘' });
        self.put(area.x, area.y + area.height - 1, &bottom, width, frame);
    }

    /// One column: the keyboard rows a rung kept, then the mouse group under its
    /// own heading or gone.
    fn sheet_column(
        &mut self,
        plan: &SheetPlan,
        from: usize,
        mouse: bool,
        group: Group,
        skip: usize,
        take: usize,
    ) {
        let area = plan.area;
        let width = usize::from(area.width);

        // The pipes first, over every interior row, which is the roomy rung's own shape
        // one rung over: the last page draws fewer lines than its box has rows, and a
        // frame open down its tail is not a box.
        self.sheet_pipes_over(area, area.y + 1);
        // The plan's slice, not one recomputed here. `skip` and `take` name
        // the page, so drawing anything else is drawing a different page from
        // the one that was asked for.
        let lines = column_lines(from, mouse).skip(skip).take(take);
        for (y, line) in (area.y + 1..).zip(lines) {
            match line {
                Line::Row(row) => self.sheet_row(y, row, plan.level, group, area.x),
                // The group's own rule, which is the same shape the title bar has
                // and for the same reason: a heading inside a table is furniture,
                // so it runs to the frame rather than standing back from it.
                Line::Heading(label) => self.sheet_heading(area.x, y, width, label),
            }
        }
    }

    /// Every section headed, with air around it, which is the rung a pane with
    /// room to spare buys.
    fn sheet_roomy(&mut self, plan: &SheetPlan, group: Group) {
        let area = plan.area;

        self.sheet_pipes_over(area, area.y + 1);

        // One blank row under the title bar, then each section: its heading, its rows,
        // and the blank row that separates it from the next.
        let mut y = area.y + 2;
        for section in SECTIONS.iter() {
            // Capped against the frame rather than against the label's own width, which
            // is `sheet_heading`'s `width - 3` one heading over: a label written at its
            // own width has nothing to clip it, so a long one would run through the
            // right pipe instead of being cut.
            self.put(
                area.x + ROOMY_HEADING_INSET as u16 + 1,
                y,
                section.label,
                usize::from(area.width).saturating_sub(ROOMY_HEADING_INSET + 2),
                self.theme.chrome_dim,
            );
            y += 1;
            for row in section.rows.rows() {
                self.sheet_row(y, row, plan.level, group, area.x);
                y += 1;
            }
            y += 1;
        }
    }

    /// The two groups side by side, which is the rung a wide pane buys.
    fn sheet_beside(&mut self, plan: &SheetPlan, keyboard: Group, mouse: Group) {
        let area = plan.area;
        let width = usize::from(area.width);

        // One rule carrying two labels, not two headings butted together.
        self.sheet_heading(area.x, area.y + 1, width, SHEET_KEYBOARD_LABEL);
        self.put(
            mouse.keys_at(area.x) - 1,
            area.y + 1,
            SHEET_MOUSE_LABEL,
            width_of(SHEET_MOUSE_LABEL),
            self.theme.chrome_dim,
        );

        // From the row below the heading, which drew its own pipes as part of the
        // rule that carries the labels.
        self.sheet_pipes_over(area, area.y + 2);
        for (group, rows) in [(keyboard, &KEYBOARD[..]), (mouse, &MOUSE[..])] {
            for (n, row) in rows.iter().enumerate() {
                self.sheet_row(area.y + 2 + n as u16, row, plan.level, group, area.x);
            }
        }
    }

    /// A group's heading: its name, then rule to the frame.
    fn sheet_heading(&mut self, x: u16, y: u16, width: usize, label: &str) {
        // Cell by cell through [`Painter::rule`] rather than a built string, which is
        // that method's own measurement reused: writing cells is six times cheaper than
        // either fix to the allocation a built rule costs.
        let style = self.theme.chrome_dim;
        self.put(x, y, "│", 1, style);
        let after = self.put(x + 1, y, label, width.saturating_sub(3), style);
        let right = x + width as u16 - 1;
        self.rule(Rect {
            x: after,
            y,
            width: right.saturating_sub(after).saturating_sub(1),
            height: 1,
        });
        // The space is written rather than left to the blank pass that ran before
        // this, so a heading is correct on its own terms: this is called from two
        // places and neither should have to know what painted the cells first.
        self.put(right - 1, y, " ", 1, style);
        self.put(right, y, "│", 1, style);
    }

    /// The frame down both edges of every interior row from `first` down.
    fn sheet_pipes_over(&mut self, area: Rect, first: u16) {
        for y in first..area.y + area.height - 1 {
            self.sheet_pipes(area, y);
        }
    }

    /// The frame down both edges of one row.
    fn sheet_pipes(&mut self, area: Rect, y: u16) {
        self.put(area.x, y, "│", 1, self.theme.chrome_dim);
        self.put(area.x + area.width - 1, y, "│", 1, self.theme.chrome_dim);
    }

    /// One row of a group: the keys cell lit, the verb dim.
    fn sheet_row(&mut self, y: u16, row: &Gesture, level: usize, group: Group, left: u16) {
        self.put(
            group.keys_at(left),
            y,
            row.keys[level],
            group.keys,
            self.theme.chrome,
        );
        self.put(
            group.verb_at(left),
            y,
            row.verb[level],
            group.verb,
            self.theme.chrome_dim,
        );
    }

    /// Draw the body: the masthead, the pinned list, the rule and the diff.
    fn body(&mut self, area: Rect, full: Rect, view: &View, pane: Rect, empty: &str) {
        // Two rects, because this region draws both roles.
        let washed = full.width;
        let glyphs = self.region_text(area, pane);
        if view.files == 0 {
            self.put_marked(
                glyphs.x,
                glyphs.y,
                empty,
                usize::from(glyphs.width),
                self.theme.chrome_dim,
            );
            return;
        }

        // The stream's own width rather than the list's: the two regions are different
        // widths, and `SPEC.md` §11.1 rules they need not align glyph for glyph.
        let shown = usize::from(area.height);
        let inner = planning_width(full.width, pane.width, 0);
        let columns = Columns::plan(inner, self.glyphs);

        // The gutter comes from the same width, and that is the fixed-slot ruling one
        // element over.
        self.gutter = view
            .gutter
            .unwrap_or_else(|| gutter_width(&view.rows, usize::from(inner)));
        // And the two are allowed to differ, which is the ruling rather than a gap.
        for (offset, row) in view.rows.iter().take(shown).enumerate() {
            let y = area.y + offset as u16;
            match row {
                // Given the planning width rather than the region's, for
                // [`Painter::list`]'s reason: the elements are placed from the
                // right edge, so the edge has to be a fact about the pane too.
                Row::File(entry) => self.file_row(
                    Rect {
                        y,
                        x: glyphs.x,
                        width: inner,
                        ..area
                    },
                    &Heading::of(entry, view.grouped),
                    view.scale,
                    &columns,
                    // A literal, which is the whole reason this is a parameter. No
                    // heading in the stream is *the* file the diff is inside; every one
                    // of them is a file the diff contains.
                    false,
                    // And the same literal for the hover mark, which is the
                    // sentence above coming true: beside a rail the two regions
                    // *do* share a `y`, and the mark that was confined by
                    // geometry reached this row.
                    false,
                ),
                Row::Hunk {
                    old_start,
                    old_lines,
                    new_start,
                    new_lines,
                } => {
                    // Named `header` rather than `text`, because the `Row::Line`
                    // arm below binds a `text` field of its own and two `text`
                    // bindings one arm apart, meaning different things, is how the
                    // wrong one gets read.
                    let header = format!(
                        "@@ -{} +{} @@",
                        span(*old_start, *old_lines),
                        span(*new_start, *new_lines)
                    );
                    // Marked rather than clipped, and this is the row where it
                    // matters most: `@@ -258,7 +25` is not a shortened header,
                    // it is a header naming a different line.
                    self.put_marked(
                        glyphs.x,
                        y,
                        &header,
                        usize::from(glyphs.width),
                        self.theme.hunk,
                    );
                }
                // Empty on purpose, and the arm exists to say so. A gap is the blank
                // that closes a file's block, and an unwritten row is already blank: it
                // is what every row below a short diff has always been.
                Row::Gap => {}
                Row::Note(note) => {
                    let drawn = format!("  {note}");
                    self.put_marked(
                        glyphs.x,
                        y,
                        &drawn,
                        usize::from(glyphs.width),
                        self.theme.note,
                    );
                }
                Row::Line {
                    kind,
                    number,
                    text,
                    spans,
                    emph,
                } => {
                    // One row tall, explicitly.
                    self.line_row(
                        Rect {
                            y,
                            height: 1,
                            width: washed,
                            ..area
                        },
                        Rect {
                            y,
                            height: 1,
                            x: glyphs.x,
                            width: glyphs.width,
                        },
                        *kind,
                        Some(*number),
                        text,
                        spans,
                        emph,
                        0,
                    );
                }
                // The same drawer, told it has no number, which is what keeps the wash,
                // the left bar, the two-tone gutter, the word patch and the degradation
                // ladder one rule rather than two.
                Row::Wrap {
                    kind,
                    text,
                    spans,
                    emph,
                    indent,
                } => {
                    self.line_row(
                        Rect {
                            y,
                            height: 1,
                            width: washed,
                            ..area
                        },
                        Rect {
                            y,
                            height: 1,
                            x: glyphs.x,
                            width: glyphs.width,
                        },
                        *kind,
                        None,
                        text,
                        spans,
                        emph,
                        *indent,
                    );
                }
            }
        }
    }

    /// `M src/engine/watch.rs                ● ████████████ __▁▂▆▄▆█   +42    -7`
    fn file_row(
        &mut self,
        area: Rect,
        heading: &Heading<'_>,
        scale: Scale,
        columns: &Columns,
        current: bool,
        hoverable: bool,
    ) {
        let mut right = area;

        // Every slot is subtracted whether or not this row fills it, which is the whole
        // of the fixed-slot ruling: otherwise a row without a sparkline lets its
        // neighbours' elements slide right into the space, and a row with a
        // two-column-narrower counts cell moves everything outside it.
        if columns.cell > 0 {
            // Each half right-anchored in its own sub-column, so the digits change
            // under a reader without moving anything beside them, and an eye running
            // down the additions of three files compares them.
            let (added, removed) = counts_of(heading.churn, self.theme);
            let end = right.x + right.width;
            let field = |width: usize, from_right: usize| Rect {
                x: end.saturating_sub(from_right as u16),
                width: width as u16,
                ..right
            };
            self.put_right(
                field(columns.cell, counts_width(columns.cell)),
                &added.text,
                added.ink,
            );
            self.put_right(
                field(columns.cell, columns.cell),
                &removed.text,
                removed.ink,
            );
        }
        past(&mut right, counts_width(columns.cell));

        // Drawn right to left, so each block knows where the one outside it
        // ended. The strip drawn is the whole window at every rung, rather
        // than the tail of one with the oldest on the left.
        let slot = spark_cells(columns.spark, self.glyphs);
        if columns.spark > 0 {
            // Cell by cell rather than as one string, for the reason the heat strip
            // below gives and one more that the strip does not have: the style differs
            // *per cell* now, so a single styled write could not draw this row anyway.
            debug_assert!(
                SPARK_RUNGS.contains(&columns.spark),
                "a layout asked for {} sparkline buckets, which is not a rung of \
                 {SPARK_RUNGS:?}, so its yardstick would be one set for another \
                 width",
                columns.spark
            );
            // Counted in cells rather than buckets, which is the one thing the glyph
            // rung changes here.
            let strip = spark_of(heading.spark, columns.spark, scale, self.glyphs);
            // The cells `spark_of` actually filled.
            let filled = slot.min(spark_cells(HISTORY_BUCKETS, self.glyphs));
            let take = filled.min(right.width as usize);
            let x = right.x + right.width.saturating_sub(take as u16);
            for (offset, bucket) in strip[filled - take..filled].iter().enumerate() {
                // Both out of the one value, which is [`Bucket`]'s whole reason.
                let (glyph, style) =
                    bucket.drawn(self.theme, self.glyphs, self.spark_ramp.as_ref());
                // `set_char` rather than an `encode_utf8` into a local buffer,
                // which is what `Cell::set_char` does internally: the heat strip
                // below can hoist its encode out of the loop because every slice
                // is the same glyph, and this cannot, so the hand-rolled form
                // here would be the library's own three lines written again.
                if let Some(cell) = self.buf.cell_mut((x + offset as u16, right.y)) {
                    cell.set_char(glyph).set_style(style);
                }
            }
        }
        // `slot` here and `take` above, deliberately.
        past(&mut right, slot);

        // Unguarded, because `heat_at` opens by returning nothing for a zero
        // width, so an outer `if` would be the same precondition twice.
        let heat = heat_at(heading.heat, columns.heat);
        if !heat.is_empty() {
            // Cell by cell rather than as one string: every slice is the same
            // glyph and only the style differs, which is the whole design.
            let mut glyph = [0u8; 4];
            let glyph = HEAT_SLICE.encode_utf8(&mut glyph);
            let x = right.x + right.width - heat.len() as u16;
            for (offset, slice) in heat.iter().enumerate() {
                // `cell_mut` rather than `Index`.
                if let Some(cell) = self.buf.cell_mut((x + offset as u16, right.y)) {
                    cell.set_symbol(glyph).set_style(self.theme.heat(*slice));
                }
            }
        }
        past(&mut right, columns.heat);
        // Into its reserved slot like everything else, so a file starting or
        // stopping to pulse moves nothing.
        if heading.newest && !columns.pulse.is_empty() {
            let width = width_of(columns.pulse) as u16;
            self.put_right(
                Rect {
                    x: right.x + right.width - width,
                    width,
                    ..right
                },
                columns.pulse,
                self.theme.pulse,
            );
        }
        past(&mut right, width_of(columns.pulse));

        let mut room = usize::from(right.width);
        let at = area.x;

        // The kind letter carries the staged mark rather than a column beside it
        // carrying one.
        let ink = match heading.origin {
            Some(Origin::Staged) => self.theme.staged,
            _ => self.theme.kind,
        };
        let letter = format!("{} ", heading.kind);
        let x = self.put(at, area.y, &letter, room, ink);
        room = room.saturating_sub(usize::from(x - at));

        // Which file it *was* is the whole content of a rename, so it is part of
        // the label rather than something to reveal on a keypress.
        let full = heading
            .from
            .map(|from| format!("{} ← {from}", heading.path));
        let label = match &full {
            Some(pair) if width_of(pair) <= room => pair.as_str(),
            _ => heading.path,
        };
        // Two marks on one path, and they are resolved on two channels rather than as a
        // priority order. The pointer chooses the colour and the underline; the caret
        // adds the weight.
        let ink = if hoverable && self.hovered == Some(Hovered::Row(area.y)) {
            self.theme.path_hover
        } else {
            self.theme.recency(heading.recency)
        };
        let ink = if current {
            ink.add_modifier(CURRENT_WEIGHT)
        } else {
            ink
        };
        // The type's glyph, in the row's own ink so it dims with its file, and only
        // where the reader turned it on: off is byte-identical output, which the gate
        // holds as a buffer comparison.
        let x = if self.icons {
            let mark = format!("{} ", crate::icons::icon_of(heading.path));
            let next = self.put(x, area.y, &mark, room, ink);
            room = room.saturating_sub(usize::from(next - x));
            next
        } else {
            x
        };
        let drawn = elide_head(label, room);
        match self.link_root.clone() {
            Some(root) => self.put_linked(x, area.y, &drawn, &root, heading.path, ink),
            None => {
                self.put(x, area.y, &drawn, room, ink);
            }
        }
    }

    /// Walk a line into styled runs, stopping at the pane's edge, and say
    /// whether anything was left over.
    fn content_runs(
        &mut self,
        runs: &mut Vec<(String, Style)>,
        text: &str,
        spans: &[Span],
        content: usize,
        emphasis: Option<(Color, &[std::ops::Range<u32>])>,
    ) -> bool {
        let mut column = 0usize;
        let mut at = 0usize;
        for span in spans {
            let end = (at + span.len).min(text.len());
            let Some(piece) = text.get(at..end) else {
                // A span boundary that is not a character boundary. It should not
                // happen and it must not panic, because the alternative to one
                // uncoloured line is a monitor that dies on a file.
                break;
            };
            let start = at;
            at = end;
            if piece.is_empty() {
                continue;
            }
            if self.push_split(
                runs,
                text,
                start..end,
                span.class,
                &mut column,
                content,
                emphasis,
            ) {
                return true;
            }
        }
        if at < text.len() {
            // Whatever the spans did not reach, which is the whole line when
            // there are none: an unrecognised file type, or a row a test built
            // by hand.
            return self.push_split(
                runs,
                text,
                at..text.len(),
                Class::Plain,
                &mut column,
                content,
                emphasis,
            );
        }
        false
    }

    /// Push one classified range, cut wherever a word-emphasis range crosses
    /// it, so the hotter background lands on exactly the bytes the pair diff
    /// marked and nothing downstream ever learns emphasis exists.
    #[allow(clippy::too_many_arguments)]
    fn push_split(
        &mut self,
        runs: &mut Vec<(String, Style)>,
        text: &str,
        range: std::ops::Range<usize>,
        class: Class,
        column: &mut usize,
        content: usize,
        emphasis: Option<(Color, &[std::ops::Range<u32>])>,
    ) -> bool {
        let plain = |painter: &mut Self, runs: &mut Vec<(String, Style)>, column: &mut usize| {
            let piece = text.get(range.start..range.end).unwrap_or_default();
            painter.push_run(runs, piece, class, column, content, None)
        };
        // The colour and the ranges are one value: a patch with no ranges is
        // not a patch, so the pairing is in the type rather than in a filter
        // every caller has to remember.
        let Some((word, emph)) = emphasis.filter(|(_, emph)| !emph.is_empty()) else {
            return plain(self, runs, column);
        };
        let mut at = range.start;
        for span in emph {
            let (from, to) = (span.start as usize, span.end as usize);
            if to <= at {
                continue;
            }
            if from >= range.end {
                break;
            }
            let from = from.max(at);
            let to = to.min(range.end);
            let (Some(before), Some(inside)) = (text.get(at..from), text.get(from..to)) else {
                return plain(self, runs, column);
            };
            if !before.is_empty() && self.push_run(runs, before, class, column, content, None) {
                return true;
            }
            if self.push_run(runs, inside, class, column, content, Some(word)) {
                return true;
            }
            at = to;
        }
        if at < range.end {
            let piece = text.get(at..range.end).unwrap_or_default();
            return self.push_run(runs, piece, class, column, content, None);
        }
        false
    }

    /// Add one run to a row, and say whether the pane cut it short.
    fn push_run(
        &mut self,
        runs: &mut Vec<(String, Style)>,
        piece: &str,
        class: Class,
        column: &mut usize,
        content: usize,
        word: Option<Color>,
    ) -> bool {
        if *column >= content {
            // Room ran out on an earlier run and this one has something to say,
            // so the row continues and nothing more of it is walked.
            return true;
        }
        let printed = printable(piece, column, content);
        self.paint.examined += printed.examined;
        let mut style = self.theme.class(class);
        if let Some(bg) = word {
            // The word patch: a background over the run, syntax foreground
            // untouched, so it composes with the row wash exactly as the wash
            // composes with the pane.
            style = style.bg(bg);
        }
        runs.push((printed.text, style));
        printed.clipped
    }

    /// `  128 +    let value = 1;`
    #[allow(clippy::too_many_arguments)]
    fn line_row(
        &mut self,
        area: Rect,
        glyphs: Rect,
        kind: LineKind,
        number: Option<u32>,
        text: &str,
        spans: &[Span],
        emph: &[std::ops::Range<u32>],
        indent: usize,
    ) {
        let (diff, sigil) = match kind {
            LineKind::Added => (self.theme.added, '+'),
            LineKind::Removed => (self.theme.removed, '-'),
            LineKind::Context => (self.theme.context, ' '),
        };
        // `None` is a continuation, and it changes exactly two cells: the sigil becomes
        // [`WRAPPED`] and the gutter goes blank.
        let sigil = if number.is_some() { sigil } else { WRAPPED };

        let (wash, bar) = match kind {
            LineKind::Added => self.theme.row(true),
            LineKind::Removed => self.theme.row(false),
            LineKind::Context => (Style::new(), Style::new()),
        };
        let sigil_style = diff;
        if wash.bg.is_some() {
            self.buf.set_style(area, wash);
        }

        // §5.1's left bar, and it costs no column. The pane's leading cell is blank
        // margin that the wash above has already bled under, so setting its background
        // spends nothing that was carrying content.
        if bar.bg.is_some() && self.inset > 0 {
            if let Some(cell) = self.buf.cell_mut((area.x, area.y)) {
                cell.set_style(bar);
            }
        }

        // The wash above took the whole row and the glyphs take the inset one, which is
        // the half of `SPEC.md` §5.3 that makes the inset design rather than padding: a
        // band the content sits *on* reads as a band, and a band that stopped where the
        // text stopped would read as a highlight someone misaligned.
        let mut x = glyphs.x;
        let mut room = usize::from(glyphs.width);
        if self.gutter > 0 {
            let gutter = self.gutter;
            // A continuation has no number, and the blank where one would be is `bat
            // --style=numbers`' own signal that this row is not a new line.
            let numbered = match number {
                Some(number) => format!("{number:>gutter$} "),
                None => " ".repeat(gutter + 1),
            };
            // crush's two-tone gutter (`SPEC.md` §11.2 B18): on a changed row the
            // number cells take a tone one step off the wash, so the gutter reads as a
            // column with no border spent.
            let tone = match kind {
                LineKind::Added => self.theme.added_gutter,
                LineKind::Removed => self.theme.removed_gutter,
                LineKind::Context => Style::new(),
            };
            // `patch` on an unset style is the identity, so the context row
            // and the palettes that draw no tone need no arm of their own.
            x = self.put(x, area.y, &numbered, room, self.theme.gutter.patch(tone));
            room = room.saturating_sub(gutter + 1);
        }

        // Capped by the pane as well as by the span count, because the walk now stops
        // at the edge: a minified line of three hundred spans in an eighty-column pane
        // pushes a handful of runs, and reserving for all three hundred is fourteen
        // kilobytes a row of churn.
        let mut runs = Vec::with_capacity((spans.len() + 3).min(room + 2));
        runs.push((sigil.to_string(), sigil_style));

        // The gap `assets/preview.svg` has drawn since before any of this existed.
        runs.push((SIGIL_GAP.to_owned(), diff));

        // Tab stops are counted from the start of the line's own content, not from the
        // left edge of the screen.
        let content = room.saturating_sub(SIGIL_WIDTH);
        // The agreement with [`gutter_width`], asserted rather than left to be read.
        debug_assert_eq!(
            content,
            usize::from(glyphs.width).saturating_sub(line_origin(self.gutter)),
            "the content bound and the gutter's affordability rule disagree \
             about what a row spends before its first character"
        );
        // The word patch's colour, when this row is a paired side.
        let emphasis = match kind {
            LineKind::Added => self.theme.added_word.bg,
            LineKind::Removed => self.theme.removed_word.bg,
            LineKind::Context => None,
        }
        .map(|word| (word, emph));
        // Neovim's `'breakindent'`, paid out of the tail's own budget.
        let indent = indent.min(content);
        if indent > 0 {
            runs.push((" ".repeat(indent), Style::new()));
        }
        let clipped = self.content_runs(&mut runs, text, spans, content - indent, emphasis);
        self.paint.rows += 1;

        // Content is the one thing that can neither break nor elide: wrapping it would
        // move every line below it, and no part of a line is its identifying part the
        // way a path's tail is. So it says it continues and nothing more.
        self.put_runs_marked(x, area.y, &runs, clipped, room);
    }
}

fn width_of(text: &str) -> usize {
    TextSpan::raw(text).width()
}

/// One side of a hunk header, in git's own shorthand.
fn span(start: u32, lines: u32) -> String {
    if lines == 1 {
        format!("{start}")
    } else {
        format!("{start},{lines}")
    }
}

/// Digits to reserve for line numbers, or zero to draw none.
pub(crate) fn gutter_width(rows: &[Row], width: usize) -> usize {
    let largest = rows
        .iter()
        .filter_map(|row| match row {
            Row::Line { number, .. } => Some(*number),
            // A continuation carries no number, named rather than swept up by the arm
            // below. It adds no protection today and is not pretending to: both arms
            // answer `None` and deleting this one changes nothing.
            Row::Wrap { .. } => None,
            _ => None,
        })
        .max()
        .unwrap_or(0);

    let digits = largest.max(1).ilog10() as usize + 1;
    // Through [`line_origin`] rather than a literal: `digits + 2` is exact while the
    // sigil stands alone and a column behind the moment it gets its gap, so the gutter
    // survives on 23 columns of text where `MIN_TEXT_WIDTH` rules 24.
    if width.saturating_sub(line_origin(digits)) >= MIN_TEXT_WIDTH {
        digits
    } else {
        0
    }
}

/// Columns a content row has for its text, once the gutter and the sigil are paid.
pub(crate) fn content_width(gutter: usize, width: usize) -> usize {
    width.saturating_sub(line_origin(gutter))
}

/// Keep the tail of `text`, marking the loss, when it will not fit.
fn elide_head(text: &str, room: usize) -> String {
    if width_of(text) <= room {
        return text.to_owned();
    }
    if room == 0 {
        return String::new();
    }

    // Walked backwards, accumulating width, rather than forwards testing each
    // suffix. The forward form re-measures the whole tail once per candidate,
    // which is quadratic in the path length and runs once per visible file row.
    let budget = room - 1;
    let mut start = text.len();
    let mut kept_width = 0usize;
    for (i, c) in text.char_indices().rev() {
        let next = kept_width + width_of(&text[i..i + c.len_utf8()]);
        if next > budget {
            break;
        }
        kept_width = next;
        start = i;
    }

    let mut kept = String::with_capacity(text.len() - start + ELIDED.len_utf8());
    kept.push(ELIDED);
    kept.push_str(&text[start..]);
    kept
}

/// One run of a line, made safe for the screen and stopped at the pane's edge.
struct Printed {
    text: String,
    /// Source characters examined to produce it, for [`PaintStats::examined`].
    examined: u64,
    /// Whether the source had more to give than the room allowed.
    clipped: bool,
    /// Byte offset after the last character that left the walk inside `room`,
    /// or the source's length where the source ran out first.
    at: usize,
    /// Columns the walk reached, so a caller can tell *the room ran out* from
    /// *the character bound ran out*.
    column: usize,
}

/// Make one line of file content safe to write into terminal cells.
fn printable(text: &str, column: &mut usize, room: usize) -> Printed {
    // Sized from what will be kept rather than from what was offered. Four bytes
    // a column is the widest UTF-8 encoding, and a tab can expand past the end
    // by at most one stop.
    let out = String::with_capacity(
        text.len()
            .min(room.saturating_mul(4).saturating_add(TAB_STOP)),
    );
    walk_printable(text, column, room, Some(out))
}

/// Where a line has to break to fit `room` columns, or `None` when it fits.
pub(crate) fn split_at(text: &str, room: usize) -> Option<usize> {
    let mut column = 0usize;
    let walked = walk_printable(text, &mut column, room, None);
    // `column >= room` as well as `clipped`.
    (walked.clipped && walked.column >= room && walked.at > 0 && walked.at < text.len())
        .then_some(walked.at)
}

/// Every byte offset a line breaks at, in order, to fit `room` columns a row.
pub(crate) fn breaks_of(text: &str, room: usize, limit: usize) -> Vec<usize> {
    let mut cuts = Vec::new();
    let mut at = match split_at(text, room) {
        Some(at) => at,
        None => return cuts,
    };
    // The continuation stands in by the line's own indent, so every row after the
    // first has that much less to give. `indent_of` caps it at half the content,
    // so this cannot reach zero and the walk cannot fail to advance.
    let tail = room.saturating_sub(indent_of(text, room)).max(1);
    while cuts.len() < limit {
        cuts.push(at);
        match split_at(&text[at..], tail) {
            // `+ at` because the split is measured from the tail's own start, and
            // every offset this returns is into the whole line.
            Some(next) => at += next,
            None => break,
        }
    }
    cuts
}

/// Columns a wrapped line's continuation stands in by, so a block keeps its shape.
pub(crate) fn indent_of(text: &str, content: usize) -> usize {
    let mut column = 0usize;
    for c in text.chars() {
        match c {
            '\t' => column += TAB_STOP - (column % TAB_STOP),
            ' ' => column += 1,
            _ => break,
        }
    }
    column.min(content / 2)
}

/// Write `times` copies of `c` into the walk's output, where there is one.
fn emit(out: &mut Option<String>, c: char, times: usize) {
    if let Some(out) = out.as_mut() {
        out.extend(std::iter::repeat_n(c, times));
    }
}

/// [`printable`] and [`split_at`] as one walk, with the string made optional.
fn walk_printable(text: &str, column: &mut usize, room: usize, mut out: Option<String>) -> Printed {
    // `None` is [`split_at`] asking where the break falls, and it must stay `None` all
    // the way down rather than becoming an empty `String`: an empty one allocates the
    // moment anything is pushed into it, and this runs once per drawn content row per
    // frame.
    let walk = room
        .saturating_mul(CHARS_PER_COLUMN)
        .saturating_add(TAB_STOP) as u64;
    let mut examined = 0u64;
    // Where the grapheme being measured began, and what it has cost so far.
    let mut cluster = 0usize;
    let mut cluster_width = 0usize;
    // Where the walk still fitted. Advanced at the end of each character's
    // arm, so a character that took the row past `room` leaves this on the byte
    // before it. See [`Printed::at`].
    let mut fitted = 0usize;
    for (i, c) in text.char_indices() {
        if *column >= room || examined >= walk {
            // Stopped with source left over, which is the caller's signal to
            // mark the row and stop asking the rest of the spans for anything.
            return Printed {
                text: out.unwrap_or_default(),
                examined,
                clipped: true,
                at: fitted,
                column: *column,
            };
        }
        examined += 1;
        match c {
            // The emoji presentation selector is dropped, not drawn.
            '\u{fe0f}' => {}
            '\t' => {
                let stop = TAB_STOP - (*column % TAB_STOP);
                emit(&mut out, ' ', stop);
                *column += stop;
            }
            c if c.is_control() => {
                emit(&mut out, UNPRINTABLE, 1);
                *column += 1;
            }
            c if c.is_ascii() => {
                emit(&mut out, c, 1);
                *column += 1;
                (cluster, cluster_width) = (i, 1);
            }
            c => {
                emit(&mut out, c, 1);
                // Only the non-ASCII tail pays for a width lookup, which keeps
                // the common line off the measuring path entirely.
                let end = i + c.len_utf8();
                let width = width_of(&text[i..end]);
                if width == 0 && cluster_width > 0 {
                    // A zero-width character joins what came before it and can
                    // change what the pair is worth, so the pair is what gets
                    // measured, and only the difference is charged.
                    let joined = width_of(&text[cluster..end]);
                    *column += joined.saturating_sub(cluster_width);
                    cluster_width = joined;
                } else {
                    *column += width;
                    (cluster, cluster_width) = (i, width);
                }
            }
        }
        if *column <= room {
            fitted = i + c.len_utf8();
        }
    }
    Printed {
        text: out.unwrap_or_default(),
        at: text.len(),
        column: *column,
        // The source ran out rather than the room, so the only way this row
        // still overflows is a two-column glyph that straddled the last cell.
        clipped: *column > room,
        examined,
    }
}

#[cfg(test)]
mod tests {
    //! What [`Painter::scrollbar`] draws when two regions start on one row.

    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::style::Color;

    use super::*;

    /// A cell's style reduced to what the bar's rungs actually differ in.
    fn weight(style: Style) -> (Option<Color>, Modifier) {
        (style.fg, style.add_modifier)
    }

    /// Paint both regions' bars into one buffer, side by side over one row range.
    fn rail(
        gripped: Option<Grabbed>,
        hovered: Option<Hovered>,
        scrolling: Option<(Grabbed, isize)>,
    ) -> Buffer {
        let mut buf = Buffer::empty(Rect::new(0, 0, 80, 24));
        let theme = Theme::default();
        let (list, diff) = (Rect::new(0, 1, 30, 20), Rect::new(30, 1, 50, 20));
        assert_eq!(
            (list.y, list.height),
            (diff.y, diff.height),
            "the fixture is not the case this module exists for"
        );

        let mut painter = Painter {
            buf: &mut buf,
            theme: &theme,
            glyphs: Glyphs::default(),
            gutter: 0,
            inset: 0,
            trailing: 0,
            paint: PaintStats::default(),
            pressed: None,
            gripped,
            hovered,
            scrolling,
            spark_ramp: None,
            covered: None,
            icons: false,
            link_root: None,
        };
        // Scrollable by a wide margin, so both bars draw a thumb well short of
        // their track and the arrows exist to be read.
        painter.scrollbar(list, Grabbed::List, Bar::Stepped, 0, 10, 100);
        painter.scrollbar(diff, Grabbed::Diff, Bar::Stepped, 0, 10, 100);
        buf
    }

    #[test]
    fn only_the_scrolled_regions_arrows_light_when_both_start_on_one_row() {
        // The assertion that would have caught 0.5.0, on the layout that brings the
        // defect back.
        let theme = Theme::default();
        let buf = rail(None, None, Some((Grabbed::Diff, -1)));
        let at = |x: u16, y: u16| weight(buf[(x, y)].style());

        assert_eq!(
            at(79, 1),
            weight(theme.bar_active),
            "the scrolled region's own up arrow did not light"
        );
        assert_eq!(
            at(29, 1),
            weight(theme.bar_track),
            "scrolling the diff lit the *list's* up arrow, because both bars \
             start on one row"
        );
        // The direction half, so a mark that lit its whole bar would still fail.
        assert_eq!(
            at(79, 20),
            weight(theme.bar_track),
            "the arrow the scroll moves away from lit"
        );
    }

    #[test]
    fn only_the_gripped_regions_thumb_lights_when_both_start_on_one_row() {
        // The drag mark, on the same fixture. `Chrome::gripped` carried its
        // region from the start and was correct throughout, and it was correct
        // *because* the tops differed rather than because it said which region.
        let theme = Theme::default();
        let buf = rail(Some(Grabbed::Diff), None, None);
        let thumbs = |x: u16| {
            (2..20)
                .filter(|y| buf[(x, *y)].symbol() == BAR_THUMB.to_string())
                .map(|y| weight(buf[(x, y)].style()))
                .collect::<Vec<(Option<Color>, Modifier)>>()
        };

        let (list, diff) = (thumbs(29), thumbs(79));
        assert!(
            !list.is_empty() && !diff.is_empty(),
            "the fixture drew no thumb on one of the bars"
        );
        assert!(
            diff.iter().all(|f| *f == weight(theme.bar_active)),
            "the gripped region's thumb did not light"
        );
        assert!(
            list.iter().all(|f| *f == weight(theme.bar)),
            "gripping the diff lit the *list's* thumb, because both bars start \
             on one row"
        );
    }

    #[test]
    fn only_the_hovered_regions_thumb_lights_when_both_start_on_one_row() {
        // Added because a mutation survived, which is the only reason worth adding a
        // test for.
        let theme = Theme::default();
        let buf = rail(None, Some(Hovered::Track(Grabbed::Diff)), None);
        let thumbs = |x: u16| {
            (2..20)
                .filter(|y| buf[(x, *y)].symbol() == BAR_THUMB.to_string())
                .map(|y| weight(buf[(x, y)].style()))
                .collect::<Vec<(Option<Color>, Modifier)>>()
        };

        let (list, diff) = (thumbs(29), thumbs(79));
        assert!(
            !list.is_empty() && !diff.is_empty(),
            "the fixture drew no thumb on one of the bars"
        );
        assert!(
            diff.iter().all(|f| *f == weight(theme.bar_hover)),
            "the hovered region's thumb did not take the hover rung"
        );
        assert!(
            list.iter().all(|f| *f == weight(theme.bar)),
            "hovering the diff lit the list's thumb, both bars starting on one row"
        );
    }

    /// The rail's floor is the sum it is written as, checked against a
    /// computed glance cluster rather than against its own expansion.
    #[test]
    fn the_rails_floor_is_the_settled_cluster_and_a_path() {
        assert_eq!(
            SETTLED_CELLS,
            SETTLED.width(Glyphs::Block),
            "the settled cluster's width is written out as {SETTLED_CELLS} and \
             computes to {}, so RAIL_FLOOR is a floor for a cluster the ladder no \
             longer draws",
            SETTLED.width(Glyphs::Block)
        );

        // The block rung is the widest, so a floor safe there is safe at every
        // rung. Asserted rather than argued, because it is the half that makes
        // the sum above the right one to build the floor from.
        for glyphs in [Glyphs::Block, Glyphs::Braille, Glyphs::Octant] {
            assert!(
                SETTLED.width(glyphs) <= SETTLED_CELLS,
                "{glyphs:?} draws the settled cluster in {} columns, over the \
                 {SETTLED_CELLS} the rail's floor reserves for it",
                SETTLED.width(glyphs)
            );
        }

        assert_eq!(
            RAIL_FLOOR as usize,
            BAR_WIDTH
                + inset_of(RAIL_FROM) as usize
                + KIND_WIDTH
                + RAIL_PATH
                + SETTLED.width(Glyphs::Block),
            "the rail's floor is no longer a bar's reserve, the pane's inset, a \
             kind letter, {RAIL_PATH} columns of path and the settled cluster"
        );

        // And what the floor is *for*: a rail at it plans a row with room for the
        // cluster and the path, with the kind letter's own cell on top.
        assert_eq!(
            planning_width(RAIL_FLOOR, RAIL_FROM, 0) as usize,
            KIND_WIDTH + RAIL_PATH + SETTLED.width(Glyphs::Block),
            "the narrowest rail no longer plans a row of exactly what its floor \
             was built from"
        );
    }
}

#[cfg(test)]
mod sheet_tables {
    //! The gestures sheet's two orders, which no gate above this level can reach.

    use super::*;

    #[test]
    fn the_last_rows_to_go_are_the_unguessable_three() {
        // §11.1: the unguessable outlives the reflexive.
        let kept: Vec<&str> = DROP_ORDER[DROP_ORDER.len() - SHEET_KEEP..]
            .iter()
            .map(|&i| KEYBOARD[i].keys[0])
            .collect();
        assert_eq!(
            kept,
            vec!["f", "m", "?  Esc"],
            "the rows the ladder keeps longest are not the three §11.1 names"
        );
    }

    #[test]
    fn the_first_row_to_go_is_the_one_the_hint_bar_already_names() {
        // The other end of the same rule, and the one an identity `DROP_ORDER`
        // would break without touching the keep-set: `q` is on the hint bar at
        // every rung, so it is the row a sheet can most afford to lose.
        assert_eq!(
            KEYBOARD[DROP_ORDER[0]].keys[0], "q  Ctrl+C  Ctrl+D",
            "the first row the ladder gives up is not `q`"
        );
    }

    #[test]
    fn the_drop_order_is_not_the_display_order() {
        // The gate on the separation itself. Every assertion above is also satisfiable
        // by re-conflating the two orders *and* reordering the table back, which is a
        // plausible tidy-up: one array is simpler than two.
        assert_ne!(
            DROP_ORDER,
            std::array::from_fn::<usize, { KEYBOARD.len() }, _>(|i| i),
            "DROP_ORDER has become the display order, so the two are one order \
             again and reordering KEYBOARD silently re-ranks what the height \
             ladder keeps"
        );
    }

    #[test]
    fn the_roomy_rungs_reach_over_the_mouse_group_is_slack_rather_than_a_rung() {
        // A third branch nothing can currently make fire, found by mutation
        // rather than by reading, and recorded here for the same reason
        // `the_two_guards_no_rung_reaches_are_still_the_right_size` records the
        // other two.
        let (kb_keys, kb_verb) = fields_of(&KEYBOARD, 0);
        let (ms_keys, ms_verb) = fields_of(&MOUSE, 0);
        assert!(
            ms_keys < kb_keys && ms_verb < kb_verb,
            "the mouse group has caught the keyboard group at the wide spelling \
             ({ms_keys}/{ms_verb} against {kb_keys}/{kb_verb}), so the roomy \
             rung's reach over both tables is now a rung rather than slack and \
             wants a gate on a drawn pane"
        );

        // The heading row's own two terms are slack in the same way, and a mutation run
        // found both: deleting the heading term from `sheet_roomy`'s width, and
        // widening the label's clip in the drawer to the whole sheet, each change
        // nothing a pane can show.
        let (keys, verb) = fields_of(SECTIONS.iter().flat_map(|s| s.rows.rows()), 0);
        let block = ROOMY_INSET + keys + ROOMY_GAP + verb + ROOMY_INSET + 2;
        for section in SECTIONS.iter() {
            let heading = ROOMY_HEADING_INSET + width_of(section.label) + 2;
            assert!(
                heading < block,
                "the section label {:?} needs {heading} columns against the \
                 table's {block}, so the roomy rung's heading term and its \
                 label clip have stopped being slack and are now what keeps a \
                 heading inside the frame",
                section.label
            );
        }
    }

    #[test]
    fn no_section_label_hides_inside_a_cell_or_another_label() {
        // `tests/sheet.rs` decides *which rung a pane took* by looking for a label with
        // `contains`, so a label that is a substring of a cell would report a plain
        // rung as a roomy one and every additivity assertion built on that reading
        // would be vacuous.
        for section in SECTIONS.iter() {
            for row in KEYBOARD.iter().chain(MOUSE.iter()) {
                for cell in row.keys.iter().chain(row.verb.iter()) {
                    assert!(
                        !cell.contains(section.label),
                        "the label {:?} hides inside the cell {cell:?}, so a pane \
                         drawing that cell reads as one that took the roomy rung",
                        section.label
                    );
                }
            }
        }
        for (i, a) in SECTIONS.iter().enumerate() {
            for (j, b) in SECTIONS.iter().enumerate() {
                assert!(
                    i == j || !b.label.contains(a.label),
                    "the label {:?} hides inside the label {:?}",
                    a.label,
                    b.label
                );
            }
        }
    }
    #[test]
    fn the_rows_given_up_before_the_keep_set_are_the_rail_then_the_pin() {
        // Addressed by the cell it draws, not by its index. `y` is among the toggles
        // rather than the keep-set: it costs less to give up than an unguessable one.
        const EXPECTED: [&str; 5] = ["r", "s", "w", "y", "a"];
        let outside: Vec<&str> = DROP_ORDER[DROP_ORDER.len() - SHEET_KEEP - EXPECTED.len()..]
            .iter()
            .take(EXPECTED.len())
            .map(|&row| KEYBOARD[row].keys[0])
            .collect();
        assert_eq!(
            outside, EXPECTED,
            "the rows given up before the keep-set are {outside:?} rather than the \
             rail, then the pin, then the wrap, then the yank, then the staged run, \
             so a pane at the floor is spending it on a gesture that could have \
             fired there"
        );
    }

    #[test]
    fn the_whole_table_in_one_column_fits_i6s_forty_columns() {
        // The arithmetic behind `SPEC.md` §11.2 B13's promise, asserted where a copy
        // edit will trip over it.
        let (keys, verb, total) = sheet_fields(1, 0, true);
        assert_eq!(
            (keys, verb),
            (13, 19),
            "the tight fields are not what the ruling measured"
        );
        assert!(
            total <= 40,
            "the whole table in one column is {total} columns at the tight \
             spelling, and a pane at I6's forty has forty to give, so the mouse \
             group is unreachable there however tall the pane is"
        );
        assert_eq!(
            total, 38,
            "the tight one-column sheet is not the 38 §11.1 states"
        );
        assert_eq!(
            margin_of(40),
            0,
            "a forty column pane has stopped having forty columns of room"
        );
    }
}
