//! Drawing a [`View`] into a buffer, and nothing else.
//!
//! Pure: same view, same theme, same area, same cells. That is what makes the
//! snapshot suite worth anything, and it is why the frame path, the scroll
//! arithmetic and the terminal all live somewhere else.
//!
//! **No borders, no boxes.** `btop` frames everything, and `btop` has the whole
//! screen. Half a laptop screen beside an agent is the case I6 names, and a box
//! spends two of forty columns and two of twenty-four rows on decoration. Every
//! cell here goes to the diff or to the chrome: one line at the top, and one at
//! the bottom that takes a second only when forty columns cannot hold it.
//!
//! Content is written cell by cell rather than through the widget set. A
//! `Paragraph` would wrap, and wrapping is the one thing a monitor must not do:
//! a wrapped diff line moves every line below it, so the shape of the screen
//! stops meaning anything. Lines are clipped instead.
//!
//! ## I6, which is the whole of the layout
//!
//! `SPEC.md` §11.1 states the rule this module implements: **a thing made of
//! items breaks, a thing made of characters marks its edge, and content is
//! neither.**
//!
//! The hint bar is the only thing on screen made of items, so it is the only
//! thing that breaks — onto [`Footer`]'s second line, and then by dropping whole
//! rungs of [`HINT_RUNGS`]. Everything else is one token and says which end it
//! lost: [`ELIDED`] on the left for a file path, whose tail names the file, and
//! [`CONTINUES`] on the right for everything else, content included.

use std::time::Duration;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Span as TextSpan;
use vigia_core::{Class, HISTORY_BUCKETS, LineKind, Recency, Span};

use crate::input::Regions;
use crate::theme::Theme;
use crate::view::{FileEntry, HEAT_BUCKETS, HeatBucket, Row, View};

/// Columns a tab advances to the next multiple of.
///
/// Four, and not configurable: `SPEC.md` names no setting for it, and a monitor
/// that has to be told how to draw a tab has already lost. Expanding matters
/// more than the number does, because a raw `\t` written into a terminal cell
/// renders as nothing and silently misaligns everything after it.
const TAB_STOP: usize = 4;

/// Characters a column may cost before the walk gives up on the row.
///
/// The second half of [`printable`]'s bound, and it exists because the first
/// half cannot hold on its own: a bound written in **columns** is defeated by a
/// character that occupies none. Combining marks, zero-width joiners, variation
/// selectors and `U+200B` all measure zero, so a run of them leaves `column`
/// where it was and the walk runs to the end of the line however long it is.
/// That is the unbounded shape the bound was added to remove, and it is ordinary
/// content rather than an attack: decomposed Unicode, emoji built from joiners,
/// and text pasted out of a web page all reach it.
///
/// Four, because a grapheme the pane can actually show is a base character plus
/// a handful of marks, and `ratatui` measures a grapheme's width as its base's.
/// Decomposed text at three characters a column still finishes on the column
/// bound; past four the row is degenerate, and what the walk drops there is
/// invisible anyway, since `Buffer::set_stringn` filters out zero-width symbols
/// before writing a cell.
const CHARS_PER_COLUMN: usize = 4;

/// Stands in for a character that cannot be drawn.
///
/// Chosen for reach rather than beauty: U+00B7 is in Latin-1 and in CP437, so it
/// survives the legacy Windows console that `SPEC.md` §10 leaves open, and it
/// does not read as file content the way `.` or `?` would.
const UNPRINTABLE: char = '·';

/// Shown where a path had to lose its head to fit.
const ELIDED: char = '…';

/// Shown where anything else ran past the right edge.
///
/// The two marks are two directions and never overlap: [`ELIDED`] on the left
/// says the beginning is gone, this one on the right says it continues. A
/// reader never has to work out which end they are missing.
const CONTINUES: &str = "›";

/// The footer's left-hand side when there is nothing wrong, widest rung first.
///
/// **Each rung is the one above it minus whole hints**, and nothing is reworded
/// on the way down: a bar that shortened `jk scroll` to `jk scr` is the
/// truncated-to-useless shape I6 forbids, and one that invented a shorter
/// wording would teach a second dialect at exactly the width where the reader
/// has least to go on. `tests/legibility.rs` gates both properties over the
/// rungs it observes by rendering, so this table cannot drift from what ships.
///
/// The drop order is `SPEC.md` §11.1's ruling. `JK files` goes first, then
/// `jk scroll`, and `f follow` is last standing: `q` and `jk` are pager reflexes
/// and four keys reach quit, while `f` is the one nobody would guess and the
/// only one that restores a state a reader can lose without noticing. It only
/// fires below twenty-nine columns, because above that [`Footer`] gives the bar
/// a line of its own rather than shortening it.
///
/// **`JK files` is here rather than left undiscoverable**, and it goes first
/// rather than last for the reason `f` goes last: the pinned list is drawn on
/// every screen wide and tall enough to have one, so a reader can see that the
/// region exists without being told, and it slides on its own besides. `f`
/// restores a state whose *absence* is invisible, which is a different and worse
/// thing to have to guess.
///
/// It is a **bonus rung**, and [`HINT_BASELINE`] is what makes that true rather
/// than a hope: adding it made the widest bar forty columns, which is exactly the
/// width I6 is named for, and the footer immediately took a second line there
/// against a gate that had asserted otherwise since I6 landed. One body row spent
/// on advice at the pane's worst width, for every reader, including the ones who
/// never press `J`.
const HINT_RUNGS: [&str; 5] = [
    "q quit · f follow · jk scroll · JK files",
    "q quit · f follow · jk scroll",
    "q quit · f follow",
    "f follow",
    "",
];

/// The rung whose fit decides whether the footer takes a second line.
///
/// **Not rung zero, and that is the whole point of it being named.** The footer
/// grows when the bar cannot sit beside the state on one line, and measuring that
/// against the *widest* rung would let any hint added above this one change the
/// footer's height at widths where the old bar fitted perfectly well. Rungs above
/// this are drawn where there happens to be room and are never worth a row.
///
/// Everything from here down is what `SPEC.md` §11.1 rules a reader is owed at
/// forty columns; above it is what a wider pane can afford.
const HINT_BASELINE: usize = 1;

/// What joins two hints.
///
/// Exported because `tests/legibility.rs` splits the rendered bar on it to check
/// that every hint on screen is a whole one. A test that restated the separator
/// as its own literal would be a second implementation of the parse, agreeing
/// with itself while disagreeing with the screen. The ladder is deliberately
/// **not** exported for the same reason inverted: a test comparing the rung
/// table against itself proves nothing, so the rungs are observed by rendering.
pub const HINT_SEPARATOR: &str = " · ";

/// A churn bucket's height, emptiest first.
///
/// The eighth-blocks every sparkline in every terminal is drawn from. They are
/// outside CP437, like the `▶` the footer has carried since I5, so the legacy
/// Windows console `SPEC.md` §10 leaves open degrades on both together rather
/// than on this alone.
const SPARK_RAMP: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// A bucket nothing happened in.
///
/// A space rather than the lowest block. `▁` would say "a little happened",
/// which is a different claim from "nothing did", and over eight buckets the
/// difference is what tells a settling file from a busy one.
const SPARK_EMPTY: char = ' ';

/// How many buckets a sparkline may show, widest rung first.
///
/// A sparkline is **a thing made of items**, so `SPEC.md` §11.1 makes it break
/// rather than mark an edge: it drops whole buckets, oldest first, and never
/// draws a partial one. Halving keeps the remaining strip readable as a shape,
/// where shaving one bucket at a time would leave widths where the picture is
/// neither the full window nor an obvious fraction of it.
const SPARK_RUNGS: [usize; 3] = [HISTORY_BUCKETS, HISTORY_BUCKETS / 2, 0];

/// The pulse, widest rung first.
///
/// `SPEC.md` §5.1 draws this as a persisting label with a dot rather than a
/// flash. The mark survives narrowing on its own, for the reason `f follow` is
/// the last hint standing: it is the one signal on the row that cannot be
/// recovered from anything else on screen, and one column is what it costs.
const PULSE_RUNGS: [&str; 3] = ["● just changed", "●", ""];

/// One slice of a file, whatever it holds.
///
/// A solid block, not a ramp. The sparkline two columns away already encodes
/// magnitude as height, and a second magnitude encoding beside it would be a
/// second dialect for one fact. Here the block is a position and the **colour**
/// is the meaning, which is exactly what `assets/preview.svg` draws: twelve
/// rects of equal height differing only in fill.
const HEAT_BLOCK: char = '█';

/// What separates the pinned file list from the diff under it.
///
/// `assets/preview.svg` draws a one-pixel line at `y=178`, and a terminal's
/// nearest honest equivalent is a row of box-drawing horizontals. A blank row
/// was the alternative and is worse: it reads as the diff having nothing at the
/// top rather than as two regions, which is the ambiguity the rule exists to
/// remove.
const RULE: char = '─';

/// The filled part of a scrollbar: where in the whole you are looking.
const BAR_THUMB: char = '█';

/// The unfilled part, which is drawn rather than left blank.
///
/// A bar with no track is a mark floating in space, and a reader cannot tell a
/// short thumb near the top from a long one without the extent it sits in. The
/// half-block is narrower than the thumb on purpose: the track is context and
/// the thumb is the reading.
const BAR_TRACK: char = '▕';

/// What marks the row for the file the diff is currently inside.
///
/// **Not a cursor**, and the glyph is chosen to say so. `SPEC.md` §11.2 B4 keeps
/// the list not navigable, so nothing is selected and nothing moves on a
/// keypress that is not a scroll; this points at where the diff already is. A
/// filled block or an inverted row would read as a selection, which is the
/// reviewer-class affordance the ruling refuses.
const CARET: char = '▸';

/// How many slices the heat strip may show, widest rung first.
///
/// **A projection re-projects; it does not drop items**, and that is the third
/// case of `SPEC.md` §11.1's rule rather than an instance of the first. The hint
/// bar and the sparkline are lists, so dropping an item shows less. A heat strip
/// that dropped its last six buckets would show the first half of the file
/// *drawn as the whole of it*, and a reader would read an untouched tail. So a
/// narrower rung sums adjacent buckets and classifies the sums: less resolution,
/// still the whole file.
///
/// Halves, so the sum is exact and every drawn bucket covers the same span.
const HEAT_RUNGS: [usize; 3] = [HEAT_BUCKETS, HEAT_BUCKETS / 2, 0];

/// Columns a path keeps before any glance element is allowed to exist.
///
/// A heading whose path has been elided past this is a row that has stopped
/// naming its own file, which is exactly the "truncated to useless" shape I6
/// forbids. Twelve leaves `…engine/watch.rs` legible at forty columns, where the
/// counters and the pulse together already want twenty.
const MIN_PATH_WIDTH: usize = 12;

/// Columns the kind letter and its gap take at the head of every file row.
///
/// **Named because three places need it and two of them used to guess.** It is
/// the `2` in [`Painter::file_row`]'s own floor, and both [`CARET_FLOOR`] and
/// [`BAR_FLOOR`] are defined as "a glance element on top of what that row will
/// already refuse to go below". Before this had a name one floor wrote it as a
/// bare literal and the other borrowed [`CARET_WIDTH`], which is a different
/// quantity that happens to equal the same number, so widening the caret gutter
/// would silently have moved the kind letter's allowance too.
const KIND_WIDTH: usize = 2;

/// The narrowest a file row can be and still name its own file.
///
/// What [`Painter::file_row`] refuses to go below, and therefore what every
/// floor built on top of it starts from.
const ROW_FLOOR: usize = KIND_WIDTH + MIN_PATH_WIDTH;

/// The smallest body a second footer line may leave behind.
///
/// Two rows, because that is the shortest thing that still reads as a diff: a
/// file heading and one line under it. Below that the footer would be buying
/// legibility with the content it exists to make legible.
const MIN_BODY: u16 = 2;

/// Rows the pinned file list may take, before the rule under it.
///
/// **A cap rather than a height**, which is the difference between this and a
/// fixed region: three changed files draw three rows, matching
/// `assets/preview.svg` exactly, and a formatter touching two hundred draws six
/// and scrolls. `SPEC.md` §11.1 rules the height a function of pane height and
/// changed-file count alone, which is the same pair `Footer::plan` already takes
/// and for the same reason: both change only when the diff does, so neither can
/// jog a reader's diff.
///
/// Six, because it is the largest block that still reads as a glance rather than
/// as a list to be searched, and because on the 24-row pane this tool is built
/// for it leaves fourteen rows of diff after the header, the rule and a one-line
/// footer.
pub const LIST_ROWS: usize = 6;

/// Columns the caret column costs the pinned list, gap included.
///
/// The list is **indented** by this rather than the caret being found a column
/// somewhere: the two regions are separated by a rule and do not have to align
/// glyph for glyph, and inseting here is what lets [`Painter::file_row`] stay one
/// drawer that knows nothing about which region called it.
const CARET_WIDTH: usize = 2;

/// The narrowest **pane** that can afford the caret column.
///
/// Below it the caret is dropped and the list draws full width. It is a glance
/// element like any other, and [`ROW_FLOOR`] outranks every glance element: a row
/// that spent the file's name on a marker pointing at it would be naming nothing.
///
/// **It counts [`BAR_WIDTH`] even on a screen with no bar, and that is the
/// ruling.** The two ladders otherwise collide: [`Painter::list`] measures the
/// width it is handed, `render` has already taken the bar's columns off it, and
/// whether a bar exists depends on whether the list is *scrollable* — which is a
/// fact about the changed-file count, not about the pane. With both floors at
/// sixteen, a seventh changed file made the caret vanish at sixteen and seventeen
/// columns with nothing about the pane having moved, which is exactly the
/// "reads as the current file changing" failure the ladder's own gate exists to
/// prevent. Paying for the bar unconditionally is what makes the caret's presence
/// a function of pane width alone.
///
/// The cost is the caret at two widths that would otherwise have room for it,
/// both far below the forty columns I6 is named for.
const CARET_FLOOR: usize = CARET_WIDTH + BAR_WIDTH + ROW_FLOOR;

/// Columns a scrollbar costs the region it is drawn beside.
///
/// **Two, and the second one is the gap.** The bar itself is one column, drawn
/// in the last. The column before it is left empty for the same reason
/// [`reserved`] leaves one everywhere else on the right-hand side: a full-block
/// thumb against a row that ends in `+6 -6` reads as `-6█`, and a reader
/// checking a count should not have to decide whether the block is part of it.
/// Seen by rendering fifty files rather than by reading the code, which is what
/// `the_region_at_fifty_files` exists for.
///
/// Written as [`reserved`] rather than as `2`, because that function is where
/// this repo already keeps the right-hand gap rule and its own doc says why: *a
/// `+ 1` remembered in two of three is a row that overwrites its own path at one
/// width in twenty.* A bar is one column with that gap in front of it.
const BAR_WIDTH: usize = reserved(1);

/// The narrowest region that can afford a scrollbar.
///
/// Same reasoning as [`CARET_FLOOR`], and now the same expression: a bar is a
/// glance element and [`ROW_FLOOR`] outranks every one of them. The parallel
/// between the two floors reads off the source rather than being asserted here.
const BAR_FLOOR: usize = BAR_WIDTH + ROW_FLOOR;

/// Columns the frame time's number gets, whatever it says.
///
/// **A fixed field, and it is the whole of why the readout is safe to draw.**
/// The value changes every frame by construction, so a cell sized to its own
/// text would be eleven columns one frame and ten the next, and everything to
/// its left would shuffle sideways while a reader was reading it. Right-aligned
/// into a constant width, the digits change and nothing moves.
///
/// Five, because that is the widest any branch of [`frame_cell`] produces, and
/// the branches are chosen to make it so rather than the other way round.
const FRAME_NUMBER: usize = 5;

/// What follows the number, so a bare duration is not left saying what it timed.
///
/// The mockup's own word. Dropped as a whole with the cell rather than shortened
/// to `f`: `SPEC.md` §11.1's rule is that a thing made of characters marks its
/// edge and a thing made of items breaks, and one word is neither, so it goes
/// entire or not at all. Same treatment the header's mode word gets.
const FRAME_LABEL: &str = " frame";

/// What a frame time occupies once it is drawn at all.
const FRAME_CELL: usize = FRAME_NUMBER + FRAME_LABEL.len();

/// Columns the memory readout gets, whatever it says.
///
/// Six, for [`FRAME_NUMBER`]'s reason: `19MiB` and `999MiB` are different widths
/// and the same fact, and only one of them is allowed to decide where the cell
/// to its left ends. The unit rides inside the field rather than beside it,
/// unlike the frame time, because `MiB` already says what the number is.
///
/// **Six rather than seven, and the column saved is the point.** Four digits
/// would fit `1024MiB`, and a process that reached a gibibyte has breached I3's
/// budget by more than an order of magnitude, at which point the exact figure
/// tells a reader nothing that `>1GiB` does not. Sized to the range the readout
/// is actually for — I3 measures this process at 19 to 27 MiB — the field spends
/// its columns on the numbers that occur, and a column is worth arguing about on
/// a footer I6 has to fit into forty of them.
const MEMORY_CELL: usize = 6;

/// What separates two facts drawn beside each other on the status bar.
///
/// Two spaces rather than [`FACT_SEPARATOR`]'s middle dot, and that is a
/// distinction rather than an inconsistency: the dot joins two facts about
/// **one** subject, and these are separate readouts that happen to share a line.
/// The mockup draws the dot; the shipped footer already used two spaces for
/// `follow ▶  N/M` before this existed, and one status bar with two join styles
/// on it would be the second dialect §11.1 keeps rejecting.
///
/// This cell used to give `watching · 3 files` as its example of one subject,
/// which was the wrong example for the right rule and is exactly what
/// [#67](https://github.com/breferrari/vigia/issues/67) found: those were two
/// subjects joined by a separator that promises one. The rule is unchanged and
/// the header now draws a pair that keeps it.
const CELL_GAP: &str = "  ";

/// Shown on the footer while the viewport is moving itself.
///
/// The mockup's own words. It sits with the position rather than with the
/// hints because it is **state**, not advice, and a notice replaces the hints:
/// a reader being told a file could not be read still needs to know whether
/// what they are looking at is live.
const FOLLOWING: &str = "follow ▶";

/// What joins two facts drawn on one line.
///
/// Twice on screen: the header's worktree name and its changed-file count, and
/// the empty state's "nothing changed" and the branch it did not change on. The
/// mockup's own character, and the same one the hint bar uses, because two
/// separators would be two dialects for one idea.
///
/// **What it joins has to be two facts about one subject**, which is a
/// constraint on the caller rather than on this string.
/// [#67](https://github.com/breferrari/vigia/issues/67) is what happens when it
/// is not: the header joined a fact about the tree to a fact about `vigia`, and
/// a separator that promises one subject made English supply one.
///
/// Deliberately **not** [`HINT_SEPARATOR`] itself, which is exported so
/// `tests/legibility.rs` can split the *hint bar* on it. Sharing the constant
/// would let a change to how hints are joined silently reshape the header, and
/// these are two independent choices that happen to agree today.
const FACT_SEPARATOR: &str = " · ";

/// What the body says when there is no diff at all.
///
/// **Not `working tree clean`**, which is what this used to say and which is
/// wrong rather than merely plain. That is git's phrase, and git means
/// index-against-HEAD as well as tree-against-index; this diff is only the
/// second, so a worktree with every change staged draws nothing here and was
/// being told it was clean while `git status` said the opposite. `SPEC.md` §11.1
/// rules the wording. Untracked files are included in the claim, since an
/// untracked file is an unstaged one too.
const NOTHING_CHANGED: &str = "no unstaged changes";

/// The narrowest the text column may get before line numbers are dropped.
///
/// Below this the gutter costs more than it explains, which is the shape of
/// "truncated to useless" that I6 forbids. At forty columns with four-digit line
/// numbers the text still gets thirty-four, so the gutter survives the case the
/// invariant is actually about.
const MIN_TEXT_WIDTH: usize = 24;

/// What the monitor is doing, which is the mockup's `watching` and the set that
/// word implies.
///
/// **Two, and I1 is the reason rather than minimalism.** `SPEC.md` §5.1 read
/// `watching` as implying at least a settling state and an idle one, and it
/// implies neither: both are *durations*, and this shell wakes only when a file
/// changes. [`vigia_core::Watcher::next_tick`] blocks until a burst has settled,
/// so the shell is never awake **during** settling; "idle" would need a wake that
/// says nothing happened, which is the timer I1 forbids. Either word could come
/// into existence and then never leave, which is the frozen clock §11.1 already
/// rejected for the pulse.
///
/// So what is left is the only distinction the shell can actually make: whether
/// what is on screen is still following the tree.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Mode {
    /// The watch is live, so the screen follows the working tree.
    ///
    /// The default, and that is a ruling rather than an accident of ordering.
    /// The watch is armed *after* first paint, deliberately, so it does not
    /// observe the shell's own setup reads. A third word for those microseconds
    /// would flicker on every launch to describe a state that always resolves the
    /// same way within one wake, and a genuine arming failure arrives as its own
    /// wake and corrects this.
    #[default]
    Watching,
    /// The watch never armed, or it ended, so this is a still picture.
    Lost,
}

impl Mode {
    /// The word the header draws, on the right, alone.
    ///
    /// `not watching` rather than `stalled` or `still`: it is the mockup's own
    /// word negated, so a reader who has learned one has learned both. `stalled`
    /// reads as temporary when this is not, and `still` means both "motionless"
    /// and "continuing".
    ///
    /// **This is the last thing on the row to give way**, which is why it holds
    /// the side [`Painter::status_line`] places first. The changed-file count
    /// beside it summarises a body that is on screen and can be recovered by
    /// counting; whether the pane is still live is recoverable from nowhere at
    /// all. So the count is what yields when the two cannot both fit, and that
    /// ordering matters most at exactly the widths where the body has nothing in
    /// it to count, which is the empty state this word exists for.
    ///
    /// **It is never cut**, which is stricter than the marking rule the rest of
    /// the header follows: `wat›` is a state a reader cannot read, and unlike a
    /// path it has no half that identifies it. That is delivered by
    /// [`Painter::put_right`], which drops a token whole rather than truncating
    /// it, rather than by a ladder of its own. It used to need one, because this
    /// side carried the count too and had a real choice to make between
    /// `watching · 3 files` and `watching`; moving the count to the left
    /// ([#67](https://github.com/breferrari/vigia/issues/67)) left a ladder with
    /// one rung wrapped around a mechanism that was already doing the work.
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
    ///
    /// `None` covers two different things on purpose, because the empty state
    /// draws them identically: a detached HEAD, which names no branch, and a
    /// frame that has a diff to show and therefore never asked. The second is
    /// what keeps I4 true. Reading `.git/HEAD` on a frame that will not draw the
    /// answer is exactly the shape I4 forbids, so the shell asks only when the
    /// diff is empty. See [`crate::branch_for`].
    pub branch: Option<String>,
    /// Whether the watch is still live.
    ///
    /// Durable, which is why it is here rather than riding [`Chrome::notice`]. A
    /// watch that ends puts the word on the header and its error on the footer:
    /// the header says the diff has stopped being live, the notice says which
    /// failure did it. Before they were split, the durable half rode the notice
    /// alone and survived only because the tick that clears a notice can never
    /// arrive again once the watch is gone.
    pub mode: Mode,
    /// Something the reader should see instead of the key hints.
    ///
    /// A monitor survives a failed frame rather than exiting, so this is where a
    /// missing blob mid-`git gc` or an unreadable file goes. It replaces the
    /// hints because a reader who has just been told something is wrong does not
    /// need reminding that `q` quits.
    pub notice: Option<String>,
    /// Whether the viewport is moving itself to what just changed.
    ///
    /// Drawn, because I5 is otherwise invisible: a view that has not moved
    /// because nothing changed and one that has not moved because following
    /// was switched off look identical, and the reader's next action differs
    /// completely between them.
    pub following: bool,
    /// What recent frames cost, which `SPEC.md` §5.1 rules is their p99.
    ///
    /// `None` on the very first paint, when no frame has completed to have a
    /// percentile of. That is a real state rather than a placeholder, and
    /// [`Footer::plan`] is written so its arrival on the second paint cannot
    /// move a row: see [`Footer::diagnostics`].
    pub frame: Option<Duration>,
    /// Resident set size in bytes, as of the last change.
    ///
    /// **As of the last change, not as of now**, and that is ruled rather than
    /// tolerated. This shell wakes only on a filesystem event, so a pane left
    /// open on an idle tree keeps showing whatever was true when the last write
    /// landed; refreshing it needs a wake I1 forbids inventing. It is the same
    /// contract the diff beside it already has. `SPEC.md` §5.1 carries the
    /// argument, including why the pulse's escape from that wall does not
    /// transfer here.
    ///
    /// `None` where reading it is not cheap enough to do per frame, which is no
    /// tier-1 target today. See [`crate::memory`].
    pub memory: Option<u64>,
}

/// `N/M`, or nothing at all when there is no diff to be positioned within.
///
/// One function rather than two because it is built twice per frame from
/// different inputs: [`Footer::plan`] wants the widest position a file count can
/// produce, and the draw wants the real one. Written twice, the two would
/// eventually disagree about a format the layout is measured against.
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
///
/// `follow ▶  N/M`, then the marker alone, then nothing. The position goes
/// before the marker because the header already carries the file count, so
/// `N/M` is the half a reader can reconstruct; whether the view is still live is
/// not recoverable from anywhere else on the screen.
///
/// Always ends in an empty rung, which is what makes [`widest_fitting`] total.
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
///
/// Three branches, and the boundaries between them are chosen so the number can
/// never exceed [`FRAME_NUMBER`]. Rounding is what makes that non-obvious:
/// `{:.1}` of 9.96ms is `10.0ms`, which is six columns, so the one-decimal
/// branch has to end *below* where rounding would carry rather than at a round
/// number.
///
/// Past a second the value gives way to a sigil rather than to more digits.
/// `>1s` is the honest thing to draw there: a frame at that magnitude has
/// already failed every budget in `SPEC.md` §3, and knowing whether it was 1.4
/// or 1.9 seconds tells a reader nothing the sigil does not, while a sixth
/// column would move the footer under their eye.
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
///
/// **The rule both cells live by, named once rather than twice.** A cell whose
/// width follows its value moves everything to its left as the value changes,
/// which over a number that changes every frame is a status bar that will not
/// hold still. The padding above is what makes it true; this is what says so.
///
/// Belt to the gates' braces, and not a substitute for them: `tests/render.rs`
/// proves the width by *rendering* across each formatter's boundary values,
/// which is the only proof that covers what a reader sees. This catches the same
/// mistake one layer earlier and on every debug run, which is most of them.
fn fixed_width(cell: String, columns: usize, what: &str) -> String {
    debug_assert_eq!(
        width_of(&cell),
        columns,
        "the {what} cell is fixed width, and {cell:?} is not {columns} columns"
    );
    cell
}

/// Resident set size, in [`MEMORY_CELL`] columns exactly.
///
/// **Mebibytes, where `assets/preview.svg` drew `11MB`**, and the departure is
/// deliberate enough to be argued in `SPEC.md` §5.1 and corrected in the
/// picture. I3's soak is the only other place this quantity is ever quoted and
/// it is MiB throughout, so drawing `MB` here would put two units on one number
/// and leave a reader comparing the screen against a soak report reading the
/// 4.9% difference as drift.
///
/// Whole mebibytes, no decimal. A tenth of a mebibyte is below what a glance can
/// use and below what the number is stable to between two reads of the same
/// idle process.
fn memory_cell(bytes: u64) -> String {
    const MIB: u64 = 1024 * 1024;
    let mib = bytes / MIB;
    // A gibibyte is more than forty times what I3 measures this process at, so
    // past it the sigil says the only thing worth saying: something is very
    // wrong, and the figure is not the interesting part. Drawn rather than
    // clamped, because a clamped number looks exact. Symmetric with the frame
    // cell's `>1s`, which gives up on precision at its own useless magnitude.
    let token = if mib > 999 {
        ">1GiB".to_owned()
    } else {
        format!("{mib}MiB")
    };
    fixed_width(format!("{token:>MEMORY_CELL$}"), MEMORY_CELL, "memory")
}

/// The diagnostics ladder, widest rung first.
///
/// `0.8ms frame  19MiB`, then the frame time alone, then nothing. These are the
/// two cells that describe **`vigia` itself** rather than the worktree, which is
/// what puts them below both the hints and the state in `SPEC.md` §11.1's drop
/// order: the hints are how a reader operates the tool and the state is what the
/// tree is doing, and a narrow pane owes a reader those before it owes them
/// instrumentation.
///
/// Memory drops before frame time for the same kind of reason one rung down. The
/// frame cell reports a budget a reader can act on when it moves; the memory
/// cell reports a claim that barely moves at all, and when it does the answer is
/// a soak rather than a glance.
///
/// Either cell may be absent before any narrowing happens: the frame time has
/// nothing to report on the first paint, and the memory readout has nothing to
/// report on a platform with no cheap read. Both cases fall out of the same
/// ladder rather than needing a branch, which is why this takes `Option`s.
///
/// Always ends in an empty rung, which is what makes [`widest_fitting`] total.
fn diagnostic_rungs(frame: Option<Duration>, memory: Option<u64>) -> Vec<String> {
    let mut rungs = Vec::with_capacity(3);
    match (frame.map(frame_cell), memory.map(memory_cell)) {
        (Some(frame), Some(memory)) => {
            rungs.push(format!("{frame}{CELL_GAP}{memory}"));
            rungs.push(frame);
        }
        (Some(frame), None) => rungs.push(frame),
        // Memory without a frame time is the first paint on every platform, and
        // it draws nothing rather than the memory cell alone. A lone readout on
        // an otherwise bare status bar reads as the important one, and this is
        // the cell the ladder drops *first* everywhere else on screen; saying
        // two opposite things about the same number at two moments is worse than
        // waiting one frame for the pair.
        (None, _) => {}
    }
    rungs.push(String::new());
    rungs
}

/// `N changed`, or nothing at all when there is no diff to count.
///
/// Zero is nothing rather than `0 changed`, the same way [`position_of`] is
/// nothing when there is no diff to be positioned within. `0 changed` spends
/// columns restating what the empty state below says in words.
///
/// **`changed` rather than `files`, and that is the load-bearing half of
/// [#67](https://github.com/breferrari/vigia/issues/67) rather than a rewording
/// that came with it.** Beside the mode word this said `3 files`, and
/// `watching · 3 files` reads as *"watching 3 files"*: a participle with an
/// object, naming a curated set that does not exist, since this watches the
/// whole worktree minus gitignore and the number is what changed inside it.
/// Moving the count next to the worktree name defuses that, but `vigia · 3 files`
/// would be a **worse** claim than the one it replaced, because the repository
/// has more than three files in it. `changed` is what makes the count a fact
/// about the tree rather than a description of it.
///
/// One rule where there used to be two: `changed` is a participle with no plural
/// to inflect, so `1 changed` and `3 changed` need no singular case.
fn count_of(files: usize) -> String {
    if files == 0 {
        String::new()
    } else {
        format!("{files} changed")
    }
}

/// The header's left-hand side, widest rung first.
///
/// `vigia · 3 changed`, then the worktree name alone.
///
/// **Both rungs are facts about the tree**, which is what puts them on one side.
/// `SPEC.md` §11.1 lays the footer out by subject — advice, then what the tree is
/// doing, then what `vigia` itself is doing — and the header has the same three
/// subjects available. It used to seat a tree-fact next to the self-fact, and
/// that adjacency is what let English fuse them.
///
/// **The count is the rung that drops and the name is the token that marks its
/// edge**, which is §11.1's rule applied inside one clause: a thing made of items
/// breaks, a thing made of characters marks its edge. The count goes first
/// because the name is the one header fact a reader cannot recover by looking at
/// the body, and because B3's empty state leans on it to say which repository
/// this is.
///
/// Deliberately **does not** end in an empty rung, unlike every other ladder
/// here, so it needs [`widest_fitting_or_last`] rather than [`widest_fitting`].
/// Its last rung is a token to be marked, not a rung to be dropped.
fn header_left(worktree: &str, files: usize) -> Vec<String> {
    let mut rungs = Vec::with_capacity(2);
    let count = count_of(files);
    if !count.is_empty() {
        rungs.push(format!("{worktree}{FACT_SEPARATOR}{count}"));
    }
    rungs.push(worktree.to_owned());
    rungs
}

/// The one body line a worktree with no changes gets.
///
/// This is B3, ruled into `SPEC.md` §11.1 with its number left behind in §11.2,
/// and it carries two of that ruling's four facts.
/// The other two are the header's: which repository, from the worktree name, and
/// that it is watching, from the mode word. So the empty state costs one row
/// rather than four, and the mode word is what makes the fourth fact sayable in
/// none at all.
///
/// **The branch is orientation, not the comparison.** Nothing about it changes
/// what is diffed. It is named because two agents on two worktrees of one
/// repository are otherwise identical on screen, which is the multi-worktree case
/// `SPEC.md` §4 defers rather than rejects.
///
/// A detached HEAD drops it rather than inventing one: `HEAD@abc123` would put a
/// commit id in a monitor that shows no commits.
fn empty_state(branch: Option<&str>) -> String {
    match branch {
        Some(branch) => format!("{NOTHING_CHANGED}{FACT_SEPARATOR}{branch}"),
        None => NOTHING_CHANGED.to_owned(),
    }
}

/// One file heading's parts, gathered so [`Painter::file_row`] takes a shape
/// rather than seven positional arguments that a caller could transpose.
///
/// Borrowed from a [`FileEntry`], which both regions supply: the pinned list
/// hands one per visible file and the stream hands one per [`Row::File`]. There
/// is deliberately **no** field saying which region asked. The caret marking the
/// file the diff is inside is a fact about the screen rather than about the
/// file, so [`Painter::list`] draws it and insets the area it passes here; this
/// type stays what a *file* looks like, and [`Painter::file_row`] stays one
/// drawer with one degradation ladder to gate.
struct Heading<'r> {
    kind: char,
    path: &'r str,
    from: Option<&'r str>,
    churn: Option<(u32, u32)>,
    spark: &'r [u16; HISTORY_BUCKETS],
    recency: Recency,
    heat: &'r [HeatBucket; HEAT_BUCKETS],
}

impl<'r> Heading<'r> {
    /// Borrow a heading from the entry either region holds.
    fn of(entry: &'r FileEntry) -> Self {
        Self {
            kind: entry.kind,
            path: &entry.path,
            from: entry.from.as_deref(),
            churn: entry.churn,
            spark: &entry.spark,
            recency: entry.recency,
            heat: &entry.heat,
        }
    }
}

/// Whether a region showing `span` of `of` has anywhere to scroll.
///
/// Asked **before** the column is taken as well as inside
/// [`Painter::scrollbar`], and the two must agree: a region that gave up a column
/// for a bar the drawer then declined to draw would be a column of blank taken
/// off every path on screen, for nothing. Every snapshot in `tests/render.rs`
/// caught exactly that when this was one check instead of two.
fn scrollable(span: u64, of: u64) -> bool {
    of != 0 && span < of
}

/// Columns something of `width` costs on the right-hand side of a row.
///
/// One more than it measures, because [`Painter::put_right`] leaves a gap so the
/// right-hand text never touches what is drawn from the left. Written once
/// rather than as a `+ 1` at each call site: the two places that reserve space
/// and the one that draws it have to agree, and a `+ 1` remembered in two of
/// three is a row that overwrites its own path at one width in twenty.
const fn reserved(width: usize) -> usize {
    if width == 0 { 0 } else { width + 1 }
}

/// What one drawn slice of the heat strip means.
///
/// Public because [`Theme::heat`] resolves it, the same way [`Theme::class`]
/// resolves a syntax class: the shell decides which distinctions are worth a
/// colour here and the theme decides which colour each gets, so
/// [#11](https://github.com/breferrari/vigia/issues/11) can repaint all of this
/// without touching the projection.
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

/// How busy one slice is, against the busiest slice of **its own file**.
///
/// Three, because `assets/preview.svg` ramps its additions across three greens
/// and the strip is the one element whose intensity the picture actually
/// specifies. It used to be two: sixteen foreground-only colours hold a normal and
/// a bright of each hue and no third stop, so the ramp was as wide as the palette
/// could draw rather than as wide as the picture asked for.
/// [#11](https://github.com/breferrari/vigia/issues/11) closed that, and the
/// asymmetry it leaves is honest: at [`Depth::Ansi16`](crate::Depth::Ansi16) the
/// `ansi` palette still spends two, and says so in its own fields rather than
/// leaving the ladder to collapse them by accident.
///
/// Ordered, so a comparison reads the way the ramp does.
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
    /// Which band `total` falls in, against this file's `busiest` slice.
    ///
    /// Compared by cross-multiplication rather than by dividing, so an awkward
    /// `busiest` cannot round a genuinely hot slice down. Widened to `u64` first
    /// because the multiplication is what would overflow, not the counts: a slice
    /// is a sum of `u16` pairs and a large file's busiest slice times three does
    /// not fit the type the counts arrive in.
    fn of(total: u32, busiest: u32) -> Self {
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
///
/// `width` is a rung of [`HEAT_RUNGS`], so it divides [`HEAT_BUCKETS`] exactly
/// and each drawn slice is the **sum** of the same number of source slices. That
/// is what makes the narrower rung a lower resolution of the whole file rather
/// than a prefix of it.
///
/// `heavy` is measured against the busiest slice **of this file**, and the
/// asymmetry with the sparkline is deliberate. A sparkline is compared *down* a
/// file list, so it shares one scale across the screen; a heat strip is read
/// *across* one row to find where in that file the work is, so its own busiest
/// slice is the only meaningful denominator. `SPEC.md` §11.1 carries both.
///
/// Empty when `width` is zero, and when nothing changed anywhere: a strip of
/// pure track says "this file is in the diff and I cannot tell you where", which
/// is worse than saying nothing and costs twelve columns to say it.
fn heat_at(buckets: &[HeatBucket; HEAT_BUCKETS], width: usize) -> Vec<Heat> {
    if width == 0 || buckets.iter().all(|bucket| bucket.total() == 0) {
        return Vec::new();
    }

    let group = HEAT_BUCKETS / width;
    let summed: Vec<HeatBucket> = buckets
        .chunks(group)
        .map(|chunk| HeatBucket {
            added: chunk.iter().map(|b| b.added).sum(),
            removed: chunk.iter().map(|b| b.removed).sum(),
        })
        .collect();

    let busiest = summed.iter().map(|b| b.total()).max().unwrap_or(0);
    summed
        .iter()
        .map(|bucket| {
            let band = Band::of(bucket.total(), busiest);
            match (bucket.added > 0, bucket.removed > 0) {
                (false, false) => Heat::Cool,
                (true, false) => Heat::Added(band),
                (false, true) => Heat::Removed(band),
                (true, true) => Heat::Mixed(band),
            }
        })
        .collect()
}

/// A path's buckets as glyphs, or `None` when it has no churn to draw.
///
/// `peak` is the busiest bucket **anywhere on the screen**, so two rows drawn
/// side by side can be compared by height. A bucket with anything in it is never
/// blank: it takes the lowest block, because "one write" and "no writes" are the
/// distinction the strip exists to make and rounding the first down to nothing
/// would erase it.
fn spark_of(buckets: &[u16; HISTORY_BUCKETS], peak: u16) -> Option<[char; HISTORY_BUCKETS]> {
    if peak == 0 || buckets.iter().all(|&count| count == 0) {
        return None;
    }
    let mut glyphs = [SPARK_EMPTY; HISTORY_BUCKETS];
    for (glyph, &count) in glyphs.iter_mut().zip(buckets.iter()) {
        if count == 0 {
            continue;
        }
        let scaled = (usize::from(count) * SPARK_RAMP.len()).div_ceil(usize::from(peak));
        *glyph = SPARK_RAMP[scaled.clamp(1, SPARK_RAMP.len()) - 1];
    }
    Some(glyphs)
}

/// The widest rung of `ladder` that fits in `room`.
///
/// Ladders are written widest first, so this is the first that fits. Every
/// ladder ends in an empty rung, so the fallback is unreachable rather than a
/// silent default.
fn widest_fitting<S: AsRef<str>>(ladder: &[S], room: usize) -> &str {
    fitting(ladder, room).unwrap_or("")
}

/// The widest rung that fits, or `None` when none does.
///
/// The one place this file decides what *fits* means, which is why it exists
/// rather than the two pickers each carrying the test. They differ only in what
/// they do when nothing fits, and a predicate written twice is a predicate that
/// can be narrowed once: a reserved column for the mark, or a floor under the
/// room, would otherwise be added to one picker and silently not the other.
fn fitting<S: AsRef<str>>(ladder: &[S], room: usize) -> Option<&str> {
    ladder
        .iter()
        .map(AsRef::as_ref)
        .find(|rung| width_of(rung) <= room)
}

/// The widest rung of `ladder` that fits, or its **last** rung when none does.
///
/// [`widest_fitting`]'s sibling, for a ladder whose final rung is a *token*
/// rather than nothing, and the pair of them is `SPEC.md` §11.1's two halves:
/// a thing made of items breaks, a thing made of characters marks its edge. The
/// name of a function is what tells a reader which of the two a call site is
/// asking for, which is why these are two names over one predicate rather than
/// one function with a flag.
///
/// **It has two callers and the doc used to credit one**, which is worth stating
/// because the missing one is what makes the `or_else` arm look like scaffolding.
/// The header's left ends in the worktree name, which marks its edge instead of
/// being dropped, so falling through to the empty string would delete the one
/// fact on the row a reader cannot recover by looking at the body. **And the
/// footer's left is a notice**, passed as a single-rung ladder, where this is the
/// only thing standing between an over-long notice and a blank row. Delete the
/// arm as one-caller scaffolding and notices vanish silently at narrow widths.
///
/// So the two are not interchangeable, and the difference is which failure they
/// produce. Used on a ladder that ends in nothing this returns that empty rung,
/// which is [`widest_fitting`]'s own answer; used the other way round, a token
/// too long for its room would vanish rather than be marked.
///
/// `unwrap_or` is reachable only for an empty `ladder`, which no caller passes.
/// It is what makes this total rather than a case anyone has to think about.
fn widest_fitting_or_last<S: AsRef<str>>(ladder: &[S], room: usize) -> &str {
    fitting(ladder, room)
        .or_else(|| ladder.last().map(AsRef::as_ref))
        .unwrap_or("")
}

/// What the footer will draw, and how many rows it needs.
///
/// Planned rather than drawn, because two callers need the answer before there
/// is anything to draw: [`diff_height`] has to know how many rows are left for
/// the body, and [`render`] has to put the body somewhere that does not collide
/// with it. Both go through here with the same inputs, so the row budget and the
/// layout are one computation and cannot drift apart.
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
    /// The frame-time and memory cells, already narrowed to what is left after
    /// the hints and the state have taken theirs.
    ///
    /// Owned where its three siblings are borrowed or copied, because these are
    /// formatted numbers with nowhere to live in the [`Chrome`]. Bounded by
    /// [`FRAME_CELL`] plus [`MEMORY_CELL`] plus a gap, so I3 never sees it.
    diagnostics: String,
}

impl<'a> Footer<'a> {
    /// Decide the footer's shape from the width, the state, and the file count.
    ///
    /// **From the file count, never the scroll position.** `{files}/{files}` is
    /// the widest position that count can produce, so reserving it means the
    /// footer cannot gain or lose a row while a reader scrolls from file 9 to
    /// file 10. A layout that reflowed under scrolling would be worse than one
    /// that is occasionally a column meaner than it had to be, and the meanness
    /// only shows below seventeen columns.
    fn plan(area: Rect, chrome: &'a Chrome, files: usize) -> Self {
        let width = usize::from(area.width);
        if area.height < 2 {
            return Self {
                rows: 0,
                reserved: 0,
                left: "",
                alert: false,
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
        let taken = if reserved == 0 { 0 } else { reserved + 1 };

        // A second line is worth taking only if it buys something: there has to
        // be a state to move up to it, and a body still worth showing
        // underneath. One header, two footer rows and `MIN_BODY` is the shortest
        // screen where both hold.
        //
        // **Measured against the hints, never against a notice.** A notice is
        // transient — a file that vanished between being named and being read,
        // a repository mid-`git gc` — so letting it decide the height would jog
        // the reader's diff down a row and back every time one flickered. That
        // is the same thing I5 ruled out for a terminal resize: a monitor does
        // not move content for something that expresses no intent. It also means
        // this height is a function of width, follow state and file count alone,
        // so a caller that sampled the chrome before a notice was raised still
        // gets the answer the renderer will use.
        // Measured against [`HINT_BASELINE`] rather than the widest rung, so a
        // hint added above it can never buy itself a row.
        let grows = width_of(HINT_RUNGS[HINT_BASELINE]) + taken > width
            && reserved > 0
            && area.height >= 3 + MIN_BODY;
        let rows = if grows { 2 } else { 1 };

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

        // **Last, and out of what is left over, which is the whole design.**
        // Every number above was computed exactly as it was before the readouts
        // existed, so `rows` is still a function of width, follow state and file
        // count alone. Two things would otherwise move a row under a reader for
        // no reason they could see: the frame cell does not exist on the first
        // paint, and the memory cell does not exist on a platform with no cheap
        // read. Both would be a footer that grew once, at startup or per
        // platform, which is the jog `SPEC.md` §11.1 already forbids a notice
        // from causing.
        //
        // Measured against the **hints** even when a notice is showing, for that
        // same rule's sake: a notice is transient and its length varies, so
        // letting it decide would make the readouts blink. A notice long enough
        // to collide simply marks its own cut, which is what `put_marked`
        // already does for it.
        //
        // `grows` means the hints are on the row below, so only the state is
        // beside the diagnostics; otherwise both are, and the hints take theirs
        // first because advice outranks instrumentation at every width.
        //
        // The gap is part of what has to be cleared rather than a second
        // subtraction, because the cells always sit beside something. `taken`
        // carries a further column that a grown footer does not strictly need,
        // its hints being on the other row. Tightening that was tried and
        // **reverted**: the column it buys is not observable by any fixture in
        // `tests/legibility.rs`, and behaviour no test can see is behaviour this
        // repo does not ship. Same meanness `reserved` already accepts, two
        // rungs up, for the same reason.
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
            diagnostics,
        }
    }
}

/// How the body divides between the two regions `SPEC.md` §11.1 rules.
///
/// The rows between the header and the footer are a pinned file list, a rule,
/// and the scrolling diff. All three numbers come from one function because they
/// have to agree: a caller that derived the diff's height by subtracting its own
/// idea of the list's would be a second layout rule, and the two would disagree
/// on exactly the pane heights where the region is giving way.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Body {
    /// Rows the pinned file list takes, zero when there is no room for one.
    pub list: usize,
    /// Whether the rule under the list is drawn, which is exactly when there is
    /// a list to put it under.
    pub rule: bool,
    /// Rows left for the diff, which is what [`View::collect`] is asked for.
    pub diff: usize,
}

impl Body {
    /// The layout of a pane with no room for a list: all diff, no rule.
    ///
    /// A real state rather than a test convenience. [`body_layout`] returns
    /// exactly this shape on a pane too short for a region and on a worktree
    /// with nothing changed, which are the two screens where the map has nothing
    /// to be a map of or nowhere to be one.
    ///
    /// It is also what a test about the **diff walk** should ask for. A gate on
    /// how `View::collect` crosses files is not a gate on the region above it,
    /// and giving it one would couple its row arithmetic to a cap it is not
    /// about. The gates that *are* about the shipped screen's cost take
    /// [`body_layout`] instead, so the region is inside them.
    pub fn diff_only(rows: usize) -> Self {
        Self {
            list: 0,
            rule: false,
            diff: rows,
        }
    }

    /// Split a body of `area` minus its header and an **already planned** footer.
    ///
    /// Takes `footer_rows` rather than planning the footer itself, which is what
    /// lets [`render`] compute the split without paying for a second
    /// [`Footer::plan`]: that function builds two ladders of `String` and several
    /// `format!`s, and a paint needs the plan anyway to draw the footer.
    /// [`body_layout`] is the wrapper for callers that have no plan in hand.
    ///
    /// **The list is what gives way**, and the order of the three clamps is the
    /// ruling. It wants one row per changed file; it may not exceed [`LIST_ROWS`];
    /// and it may not take the diff below [`MIN_BODY`], counting the rule it
    /// costs. A monitor whose diff has been squeezed out by the map of the diff
    /// has stopped being one, which is the same argument `Footer::plan` makes one
    /// region down about its second line.
    ///
    /// **Nothing here reads the notice**, deliberately. §11.1 forbids a transient
    /// thing from moving content, and a region that appeared and vanished as
    /// files were named and read would jog the reader's diff exactly the way a
    /// growing footer would. The inputs are pane height, footer height and
    /// changed-file count, all of which change only when the diff does.
    ///
    /// Saturating rather than clamped so a one-row terminal asks for nothing
    /// instead of underflowing.
    pub fn split(area: Rect, footer_rows: u16, files: usize) -> Self {
        let body = usize::from(area.height).saturating_sub(1 + usize::from(footer_rows));

        // The rule costs a row too, so the diff needs `MIN_BODY + 1` before the
        // list may have its first. No changed files is B3's empty state, which
        // reaches the same answer through `files.min(..)` rather than through a
        // branch of its own: one line of prose in the diff region, no list, and
        // no rule over nothing, because a rule above an absent region is chrome
        // announcing it.
        let affordable = body.saturating_sub(usize::from(MIN_BODY) + 1);
        let list = files.min(LIST_ROWS).min(affordable);
        if list == 0 {
            return Self::diff_only(body);
        }
        Self {
            list,
            rule: true,
            diff: body - list - 1,
        }
    }

    /// Shrink the list to the rows a view actually carries, giving the rest back
    /// to the diff.
    ///
    /// **The one reconciliation the draw site genuinely owns.** [`body_layout`]
    /// answers what the *pane* affords, which is all a caller sizing a request
    /// can know; only the view that came back knows how many entries it holds. On
    /// the shipped path the two are equal by construction, because the caller
    /// sizes its request from the same function and [`View::take_list`] fills
    /// exactly that many rows whenever the files exist. Where they differ is a
    /// stale view redrawn after a failed collect, and a region sized for files it
    /// does not hold would draw blank rows under a rule, announcing a list that
    /// is not there.
    ///
    /// The rows come back to the diff rather than being dropped, so the regions
    /// still sum to the body and no row is left unpainted between the rule and
    /// the diff.
    pub fn clamped_to(self, have: usize) -> Self {
        let list = self.list.min(have);
        Self {
            list,
            rule: list > 0,
            diff: self.diff + (self.list - list) + usize::from(self.rule && list == 0),
        }
    }
}

/// Split this area's body between the file list and the diff.
///
/// One line goes to the header and one or two to the footer, so this needs the
/// same inputs the footer is planned from: `files` is
/// [`vigia_core::Frame::files`]'s length, which a caller knows before collecting
/// anything and which equals [`View::files`] afterwards.
///
/// Plans the footer itself, which is what a caller sizing a *request* needs and
/// what a caller about to *paint* already has. [`render`] takes
/// [`Body::split`] directly with the plan it made anyway; everything else comes
/// through here. See [`Body::split`] for the ruling the arithmetic encodes.
pub fn body_layout(area: Rect, chrome: &Chrome, files: usize) -> Body {
    Body::split(area, Footer::plan(area, chrome, files).rows, files)
}

/// Rows the **diff** gets, which is what a caller has to ask [`View::collect`]
/// for and what a page-down step is measured in.
///
/// This was `body_height` until §11.1 made the body two regions, at which point
/// the name became a claim the function no longer honours: the body is
/// `list + rule + diff` and this is the last term. Renamed rather than kept,
/// because every caller of it wants the diff's height specifically — the number
/// goes to `View::collect` and to `Action::Page`, neither of which has anything
/// to say about the file list — and a name that quietly means one region is
/// worse than one that says which.
pub fn diff_height(area: Rect, chrome: &Chrome, files: usize) -> usize {
    body_layout(area, chrome, files).diff
}

/// What one paint cost, in the term that decides whether it followed the pane.
///
/// The renderer's counterpart to [`vigia_core::FrameStats`] and
/// [`vigia_core::HighlightStats`], and it exists for the reason those do: I4's
/// shape is *"cost follows the window"*, and a claim about cost that nothing
/// counts is a claim nothing can gate. `SPEC.md` §7's structural tier is exact
/// counters rather than a wall clock precisely so a regression is caught on a
/// hosted runner too.
///
/// The pair is deliberate. [`Self::examined`] alone says how much work a frame
/// did and not whether that was a lot, and the bound it is checked against is
/// [`Self::rows`] times the pane's width — derived from the run, because a
/// constant would be a bound no input could approach.
///
/// **Per paint, where its two siblings are cumulative**, and the difference is
/// forced rather than chosen: they hang off an object that lives across frames
/// and this comes back from a free function with nothing to accumulate onto. So
/// the comparable number over a run is a *sum*, and [`AddAssign`] is how a caller
/// takes it: summing field by field is what silently drops the field added next.
/// There is deliberately no `paint_delta` beside `support::delta` for the same
/// reason inverted, since there are no two readings here to subtract.
///
/// [`AddAssign`]: std::ops::AddAssign
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PaintStats {
    /// Rows of file content drawn. Headings, hunk headers and notes are not
    /// content and are bounded by the screen on their own.
    pub rows: u64,
    /// Source characters examined to draw them.
    ///
    /// Counted where the walk happens rather than derived from what was
    /// produced, and that is the whole instrument: the two are equal once the
    /// walk is bounded, so a counter over the *output* would be true by
    /// construction and could never fail.
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
///
/// **The same arithmetic [`render`] uses, called by both**, rather than a second
/// derivation the input path could drift from. A wheel that scrolled the region
/// beside the one under the pointer is worse than a wheel that does nothing, and
/// the only way to be sure is for one function to answer.
pub fn regions(area: Rect, chrome: &Chrome, view: &View) -> Regions {
    if area.width == 0 || area.height == 0 {
        return Regions::default();
    }
    let footer = Footer::plan(area, chrome, view.files);
    let body = Body::split(area, footer.rows, view.files).clamped_to(view.list.len());
    let wide = usize::from(area.width) >= BAR_FLOOR;

    let list_top = area.y + 1;
    let diff_top = list_top + body.list as u16 + u16::from(body.rule);

    // A bar column only where a bar is actually drawn, and only where it can
    // express more than one position: a one-row track is full at every window,
    // so it says nothing and would still swallow a click.
    let list_bar = wide && body.list > 1 && scrollable(body.list as u64, view.files as u64);
    let diff_bar = wide && body.diff > 1 && scrollable(body.diff as u64, view.total_rows as u64);

    Regions {
        list: (list_top, body.list as u16),
        diff: (diff_top, body.diff as u16),
        bar: (list_bar || diff_bar).then(|| area.x + area.width - 1),
    }
}

/// Draw a whole screen: one header line, the body, and one or two footer lines.
///
/// Any area is legal, including one too short for a body and one column wide. A
/// monitor that panics when a pane is dragged narrow is worse than one that
/// draws something cramped.
pub fn render(
    buf: &mut Buffer,
    area: Rect,
    view: &View,
    theme: &Theme,
    chrome: &Chrome,
) -> PaintStats {
    if area.width == 0 || area.height == 0 {
        return PaintStats::default();
    }

    // **One plan, one split, both read by everything below.** `Footer::plan` is
    // not cheap — it builds two ladders of `String` and several `format!`s — and
    // `body_layout` plans the footer again to reach the same answer, so asking
    // for both separately costs a second plan on every paint.
    //
    // Planned from `view.files`, which on the path that matters is the same
    // number the caller passed: `View::collect` copies it straight off the frame
    // and changes nothing. Where they differ is a caller redrawing a *stale*
    // view, and `clamped_to` below is what makes that case draw honestly rather
    // than announcing files the view does not hold.
    let footer = Footer::plan(area, chrome, view.files);
    let body = Body::split(area, footer.rows, view.files).clamped_to(view.list.len());

    let mut painter = Painter {
        buf,
        theme,
        gutter: 0,
        paint: PaintStats::default(),
    };

    painter.header(Rect { height: 1, ..area }, view, chrome);

    if footer.rows > 0 {
        painter.footer(area, view, chrome, &footer);
    }

    // Wide enough for a bar at all. Each region then decides separately whether
    // it has anywhere to scroll, because a list of three files and a diff of
    // thirty thousand rows are different questions.
    let bars = usize::from(area.width) >= BAR_FLOOR;
    let mut y = area.y + 1;

    if body.list > 0 {
        let region = Rect {
            y,
            height: body.list as u16,
            ..area
        };
        // Counted in **files**, which is exactly what this region shows.
        let region = painter.with_bar(
            region,
            bars,
            view.list_top as u64,
            body.list as u64,
            view.files as u64,
        );
        painter.list(region, view, area.width);
        y += body.list as u16;
    }

    if body.rule {
        painter.rule(Rect {
            y,
            height: 1,
            ..area
        });
        y += 1;
    }

    if body.diff > 0 {
        let region = Rect {
            y,
            height: body.diff as u16,
            ..area
        };
        // **Counted in rows**, which is what the call below passes: `rows_above`
        // over `total_rows`, with the thumb spanning the screen's own height.
        // Two superseded rulings are recorded under this one, because both were
        // reported from use and the second replaced the first within the hour.
        //
        // **The earlier ruling made the *whole* depend on the current file**, as
        // `files * current_span`, and it was wrong in three visible ways at once:
        // the bar vanished when the current file was shorter than the pane (a
        // binary file at the top of a seventeen-file tree drew none at all), it
        // ballooned when the current file was long, and it never reached the
        // bottom when the trailing files were smaller than the one being read —
        // scrolling to the very last line of the very last file left the thumb at
        // two thirds. Reported from use, which is the fourth time this repo has
        // been corrected by someone running it rather than by a gate.
        //
        // **Rows, exactly, because counting them turned out to be affordable.**
        // The thumb spans the screen's rows over the diff's total rows and sits
        // at the rows above it, which is what every other scrollbar means and
        // what a reader expects.
        //
        // It was ruled the other way first, on the argument that a total needs
        // every changed file diffed and I4 forbids that. The argument was right
        // about the cost of *diffing* and wrong that a total needs it: counting a
        // file's height needs its hunk boundaries and none of its text, and a
        // `FileDiff` allocates a `String` per drawn line. Measured over a hundred
        // files of five hundred rewritten lines, totalling through full diffs is
        // **442.71ms** and counting is **8.76ms**, against `git diff --numstat`'s
        // 46ms for the same work. `SPEC.md` §3's I4 carries the rewording that
        // admits the walk, and `what_a_row_exact_scrollbar_would_cost` is the
        // measurement it rests on.
        //
        // Zero total means the caller did not ask for one, which is a pane too
        // short for a bar, and `with_bar` draws nothing when told a whole of
        // zero.
        let region = painter.with_bar(
            region,
            bars,
            view.rows_above as u64,
            u64::from(body.diff as u16),
            view.total_rows as u64,
        );
        painter.body(region, view, chrome);
    }

    painter.paint
}

/// A buffer, a palette, and the one measurement the body rows share.
struct Painter<'a> {
    buf: &'a mut Buffer,
    theme: &'a Theme,
    /// Digits reserved for line numbers, or zero when there is no room.
    gutter: usize,
    /// What the content rows have cost so far, returned by [`render`].
    paint: PaintStats,
}

impl Painter<'_> {
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
    ///
    /// This is I6's rule for everything that is one token rather than a list:
    /// the worktree name, a notice, a hunk header, a note, the empty-state line
    /// and a line of file content. None of them can drop an item the way the
    /// hint bar can, and none has an identifying half the way a path does, so
    /// the honest thing is to fill the room and mark the edge.
    ///
    /// The mark gets a **reserved** column rather than overwriting the last one,
    /// and the reason is not the one it looks like. Overwriting cannot corrupt
    /// the row: `ratatui` refuses to write into the continuation cell a
    /// two-column glyph covers. What it does instead is **drop the mark**, so a
    /// row filled to its last column by a wide glyph is drawn as one that simply
    /// ends. Reserving the column is what guarantees the mark always lands.
    /// `tests/legibility.rs` gates it, and a plain ASCII line never reaches the
    /// case because it ends on a one-column character.
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

    /// Write a sequence of styled runs under **one** limit, marking the edge.
    ///
    /// The many-run form of [`Painter::put_marked`], and it has to be one call
    /// rather than a `put_marked` per run. The limit belongs to the row, but the
    /// mark belongs to whichever run happens to reach the edge, and a per-run
    /// version would either mark every run or none of them.
    ///
    /// The mark gets a reserved column for exactly the reason `put_marked` gives:
    /// `ratatui` refuses to write into the continuation cell of a two-column
    /// glyph, so overwriting the last column silently **drops** the mark and a
    /// clipped line is drawn as one that simply ends. `tests/legibility.rs`
    /// sweeps every width for that.
    ///
    /// `clipped` says whether anything was left over, and it is **told** rather
    /// than measured. It cannot be derived here: the runs stop at the pane's
    /// edge, so their total width says nothing about whether the line ended
    /// there or merely ran out of room, and the two have to draw differently.
    /// The caller is the only party that saw the source.
    ///
    /// This used to take the runs' total width and compare it against `limit`,
    /// which worked only because the runs held the *whole* line. Bounding the
    /// walk removed that (#45), and the flag had to replace the width or every
    /// clipped row would silently draw as one that simply ended.
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
        //
        // Seeded from the **first** run rather than from a theme default,
        // because at `limit == 1` the budget is zero and the loop below writes
        // nothing at all. Seeded with `context` instead, a one-column diff row
        // marked in `Reset` where `put_marked` on the same content would have
        // used the caller's style, and the two spellings of one rule had already
        // drifted apart at the one width nothing tests.
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
    ///
    /// Dropped entirely rather than truncated when it does not fit. Half a count
    /// on the right of a line is noise; its absence is at least honest.
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
    ///
    /// The header and the footer are the same shape, and having them share it is
    /// not only brevity. Written twice, one of them eventually stops doing that.
    ///
    /// **The right-hand text is placed first, and what that priority rests on
    /// changed with [#67](https://github.com/breferrari/vigia/issues/67).** It
    /// used to be that the number is what changes and what changes is what a
    /// glance is for. The header's number now sits on the *left*, and the reason
    /// survives the move without depending on it: the right carries the rung the
    /// row must keep longest, which on the header is the mode word and on the
    /// footer is the follow state. Both are facts recoverable from nowhere else
    /// on screen, where a name and a hint bar are not.
    ///
    /// `left` is a **ladder** so the left-hand side can drop a whole item before
    /// the last one is marked, which is what the header needs. The footer passes
    /// a single rung, and that is not a degenerate case padding out a signature:
    /// [`widest_fitting_or_last`] on one rung is the identity, which is exactly
    /// what a notice requires, since a notice is a token to be marked and never
    /// dropped. The footer *has* a ladder of its own — [`HINT_RUNGS`] — and
    /// resolves it earlier only because [`Footer::plan`] has to know the height
    /// before anything is drawn.
    ///
    /// **The two sides resolve at different altitudes on purpose.** The left is
    /// picked here, after [`Painter::put_right`] has reported what it took,
    /// because how much room the left has is not knowable until then. A right-hand
    /// budget is known to the caller before the call, so a caller that has a
    /// choice to make there makes it itself.
    ///
    /// One `style` for the whole left rung, which makes `SPEC.md` §11.1's ruling
    /// that both header facts are drawn in one weight **unrepresentable** to
    /// break rather than merely written down. A ladder of `(String, Style)` would
    /// put that violation one line away.
    fn status_line<S: AsRef<str>>(
        &mut self,
        area: Rect,
        left: &[S],
        style: Style,
        right: &str,
        right_style: Style,
    ) {
        self.buf.set_style(area, self.theme.chrome_dim);
        let taken = self.put_right(area, right, right_style);
        let room = usize::from(area.width).saturating_sub(taken);
        let rung = widest_fitting_or_last(left, room);
        self.put_marked(area.x, area.y, rung, room, style);
    }

    fn header(&mut self, area: Rect, view: &View, chrome: &Chrome) {
        // The worktree name leads the left, which is the one place the layout
        // departs from `assets/preview.svg` on purpose: a title bar reading
        // `vigia` spends six of forty columns telling the reader which program
        // they started, and what they cannot tell by looking is which *tree*.
        // `SPEC.md` §11.1 carries the argument, because §5.1's rule is that a
        // published artifact answering a question is the answer, so a deliberate
        // departure from one has to be written down or it reads as drift.
        //
        // **The changed-file count sits with it**, and #67 is why: the two facts
        // this row used to seat together were about two different subjects, and
        // `watching · 3 files` fused them into a claim the tool does not make.
        // The count is a fact about the tree, like the name; the mode word is a
        // fact about `vigia`, and it now has the other end of the line to itself
        // where it can fuse with nothing.
        //
        // The header never takes a second line the way the footer does. A name
        // is not a list and has nowhere to break, so a second line could not
        // guarantee a fit and would spend a body row on a maybe. Both sides
        // break instead: the left drops a whole rung, and the right drops its
        // one token whole rather than truncating it, which `put_right` does
        // without needing a ladder to say so.
        let right = chrome.mode.word();
        // **A dead watch has to be visible, not merely present.** Drawn in the
        // same dim grey as the count, `not watching` is a word a reader has to
        // go looking for, and a monitor whose failure state looks exactly like
        // its working one has failed twice. `SPEC.md` §5 makes colour half the
        // differentiator, so the abnormal state is loud and the normal one stays
        // quiet.
        //
        // The **footer's own** alert rather than a colour of its own: the notice
        // carrying which failure already uses it, and the two halves of one
        // event should not arrive in two different reds. A reuse of an existing
        // style rather than a palette decision, which stays #11's.
        let right_style = match chrome.mode {
            Mode::Watching => self.theme.chrome_dim,
            Mode::Lost => self.theme.alert,
        };
        // **One style across both facts on the left, and that is a ruling.** The
        // count used to be drawn in the same dim grey as the mode word it sat
        // beside, and keeping that here would give one clause two weights: the
        // reader would be told, in colour, that these are separate claims, which
        // is the seam #67 exists to remove. They are one clause about one
        // subject now, so they are drawn as one.
        self.status_line(
            area,
            &header_left(&chrome.worktree, view.files),
            self.theme.chrome,
            right,
            right_style,
        );
    }

    /// The footer, on the bottom one or two rows of `area`.
    ///
    /// Takes the whole area rather than its own rows, because which rows it owns
    /// is what [`Footer::plan`] decided.
    fn footer(&mut self, area: Rect, view: &View, chrome: &Chrome, footer: &Footer<'_>) {
        let position = position_of(view.top.file, view.files);
        // Clamped to what was reserved, not to the width. The plan handed the
        // rest of the line to the hints, and the drawn position can be narrower
        // than the widest one reserved for it, so a state sized to the width
        // would draw over them.
        let rungs = state_rungs(chrome.following, &position);
        let state = widest_fitting(&rungs, footer.reserved);
        // One string rather than two placements, because `status_line` puts a
        // single right-hand token and lets the left lose characters to it. The
        // gap is owed only where both halves exist: `follow ▶` on a clean
        // worktree has no position after it and no trailing spaces either, and
        // the same has to hold when the diagnostics are the only thing there.
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
            self.status_line(bottom, &[footer.left], style, "", self.theme.chrome_dim);
        } else {
            self.status_line(bottom, &[footer.left], style, &right, self.theme.chrome_dim);
        }
    }

    /// Draw the pinned file list, `SPEC.md` §11.1's upper region.
    ///
    /// Every row goes through [`Painter::file_row`], which is the whole point:
    /// one drawer, one degradation ladder, one set of gates, and no way for the
    /// two regions to disagree about what a file looks like. What is different
    /// here is the **caret column**, and it is applied by insetting the area
    /// rather than by telling `file_row` which region it is in, so that function
    /// stays ignorant of the layout above it.
    ///
    /// The caret is dropped below [`CARET_FLOOR`] rather than squeezing the
    /// path, because [`MIN_PATH_WIDTH`] outranks every glance element and a
    /// marker pointing at a row that no longer names its file points at nothing.
    ///
    /// **`view.list` is authoritative for what to draw and `view.top` for what to
    /// mark**, and neither is recomputed here. The caret comes from where the
    /// walk *landed*, never from the position it was asked for: a request can
    /// overshoot its file, point past a list the agent in the other pane has
    /// shortened, or be backed up to rest the diff's last row on the bottom, and
    /// marking from it would name a file the diff is not in on exactly the frames
    /// that moved. `View::collect` resolves `top` before this ever runs.
    fn list(&mut self, area: Rect, view: &View, pane: u16) {
        // Against the **pane**, not the region: `area` has already lost the bar's
        // columns when one is drawn, and deciding from it would make the caret's
        // presence depend on whether the list happens to be scrollable. See
        // [`CARET_FLOOR`].
        let caret = usize::from(pane) >= CARET_FLOOR;
        let inset = if caret { CARET_WIDTH as u16 } else { 0 };

        for (offset, entry) in view.list.iter().take(usize::from(area.height)).enumerate() {
            let y = area.y + offset as u16;
            // Saturating, because `list_top` is not bounded by the file count:
            // a pane too short for a region hands the reader's request back
            // untouched, so `View::collect` can legitimately report `usize::MAX`
            // here. `position_of` guards the identical hazard one region up.
            if caret && view.list_top.saturating_add(offset) == view.top.file {
                self.put(area.x, y, &CARET.to_string(), CARET_WIDTH, self.theme.pulse);
            }
            self.file_row(
                Rect {
                    y,
                    height: 1,
                    x: area.x + inset,
                    width: area.width.saturating_sub(inset),
                },
                &Heading::of(entry),
                view.peak,
            );
        }
    }

    /// Draw this region's scrollbar if it has one, and hand back the room left
    /// for content.
    ///
    /// **One place asks and one place answers.** Both regions ran the same three
    /// steps — decide, draw, then narrow the `Rect` — and the deciding half was
    /// written twice, which is exactly the shape [`scrollable`] exists to
    /// prevent: a region that gave up a column for a bar the drawer then declined
    /// to draw is a blank column taken off every path on screen. Now the question
    /// is asked once and its answer is the return value.
    ///
    /// `wide` is whether the pane can afford a bar at all, which is one rule for
    /// the whole screen so a reader never sees half a pair.
    fn with_bar(&mut self, region: Rect, wide: bool, at: u64, span: u64, of: u64) -> Rect {
        // **A one-row track is full at every position**, because `scrollable`
        // guarantees `span < of` and therefore `(span * rows) / of < rows`, so the
        // thumb equals the track exactly when `rows == 1`. Drawing it spends two
        // of forty columns on a mark that cannot move, which is the same "column
        // saying there is nothing to scroll" `scrollable` itself exists to
        // refuse.
        if !(wide && region.height > 1 && scrollable(span, of)) {
            return region;
        }
        self.scrollbar(region, at, span, of);
        Rect {
            width: region.width.saturating_sub(BAR_WIDTH as u16),
            ..region
        }
    }

    /// Draw a one-column scrollbar down the right of `area`.
    ///
    /// The thumb covers `at..at + span` of `0..of`, in whatever units the caller
    /// counts in. Two callers, two units, and keeping the arithmetic here rather
    /// than in each of them is what stops the list's bar and the diff's bar
    /// disagreeing about how a fraction becomes rows.
    ///
    /// **The thumb is never shorter than one row**, because a bar whose thumb
    /// rounded away would say "nothing here" about a position that exists. It is
    /// pushed up rather than allowed to overrun, so the last position is drawn at
    /// the bottom rather than off it.
    ///
    /// Draws nothing when `of` is zero or when everything already fits: a full
    /// bar is a column spent saying there is nothing to scroll. [`scrollable`]
    /// is the same question asked before the column is taken away.
    fn scrollbar(&mut self, area: Rect, at: u64, span: u64, of: u64) {
        let rows = u64::from(area.height);
        // **Width guarded here as well as by the caller.** `render` only calls
        // this above `BAR_FLOOR`, so a zero width cannot reach it today, and the
        // subtraction below would underflow if one ever did. A private method
        // whose safety rests on a condition checked in another function is a
        // panic waiting for the day someone adds a third caller.
        if rows == 0 || area.width == 0 || !scrollable(span, of) {
            return;
        }

        let thumb = ((span * rows) / of).max(1).min(rows);
        // **The scroll's travel mapped onto the track's travel**, not the
        // position mapped onto the whole track. Those differ by exactly the
        // thumb's own length, and getting it wrong leaves the bar one row short
        // at the last window on every input where the division does not divide:
        // at seven files in a six-row region it drew the identical column at both
        // ends, so the one readout that says *you are at the end of the changed
        // set* never said it. A `.min(rows - thumb)` clamp hides the overshoot
        // and cannot supply the missing row.
        //
        // `scrollable` guarantees `span < of`, so `travel` is at least one and
        // this cannot divide by zero.
        let travel = of - span;
        let start = (at.min(travel) * (rows - thumb)) / travel;
        let x = area.x + area.width - 1;

        for row in 0..rows {
            let filled = row >= start && row < start + thumb;
            let (glyph, style) = if filled {
                (BAR_THUMB, self.theme.bar)
            } else {
                (BAR_TRACK, self.theme.bar_track)
            };
            // **Cell by cell, not `set_stringn`.** The same choice the heat strip
            // makes two functions down, and for a sharper reason here: a string
            // call allocates per row and then segments the graphemes of a
            // single-character string, which measured ten times the cost of
            // writing the cell. A bar is `list + diff` rows of that, twice a
            // screen.
            self.buf[(x, area.y + row as u16)]
                .set_symbol(glyph.encode_utf8(&mut [0u8; 4]))
                .set_style(style);
        }
    }

    /// Draw the rule that separates the two regions.
    ///
    /// Full width, because a rule that stopped short would read as a box someone
    /// forgot to close. It is chrome rather than content, so it takes the dim
    /// style every other structural mark here takes.
    fn rule(&mut self, area: Rect) {
        // Cell by cell for the reason [`Painter::scrollbar`] gives. The
        // allocation a built string costs is the small half: measured at two
        // hundred columns it was 5% of the row, and segmenting two hundred
        // graphemes was the rest. Writing cells is six times cheaper than either
        // fix to the allocation alone.
        let style = self.theme.chrome_dim;
        let mut glyph = [0u8; 4];
        let glyph = RULE.encode_utf8(&mut glyph);
        for x in area.x..area.x + area.width {
            self.buf[(x, area.y)].set_symbol(glyph).set_style(style);
        }
    }

    fn body(&mut self, area: Rect, view: &View, chrome: &Chrome) {
        if view.files == 0 {
            self.put_marked(
                area.x,
                area.y,
                &empty_state(chrome.branch.as_deref()),
                usize::from(area.width),
                self.theme.chrome_dim,
            );
            return;
        }

        self.gutter = gutter_width(view, usize::from(area.width));
        for (offset, row) in view.rows.iter().take(usize::from(area.height)).enumerate() {
            let y = area.y + offset as u16;
            match row {
                Row::File(entry) => {
                    self.file_row(Rect { y, ..area }, &Heading::of(entry), view.peak)
                }
                Row::Hunk {
                    old_start,
                    old_lines,
                    new_start,
                    new_lines,
                } => {
                    let text = format!(
                        "@@ -{} +{} @@",
                        span(*old_start, *old_lines),
                        span(*new_start, *new_lines)
                    );
                    // Marked rather than clipped, and this is the row where it
                    // matters most: `@@ -258,7 +25` is not a shortened header,
                    // it is a header naming a different line.
                    self.put_marked(area.x, y, &text, usize::from(area.width), self.theme.hunk);
                }
                Row::Note(note) => {
                    let text = format!("  {note}");
                    self.put_marked(area.x, y, &text, usize::from(area.width), self.theme.note);
                }
                Row::Line {
                    kind,
                    number,
                    text,
                    spans,
                } => {
                    // **One row tall, explicitly.** `..area` inherits the body's
                    // whole height, which every other row drawer got away with
                    // because they only ever read `x`, `y` and `width`. This is
                    // the first row that paints a *region*, and an inherited
                    // height made the wash a rectangle running from the line down
                    // to the bottom of the pane: the context rows under it, the
                    // blank rows under those, and the footer. Measured at 200x60,
                    // that was 366,000 cells a frame where 12,000 will do.
                    self.line_row(
                        Rect {
                            y,
                            height: 1,
                            ..area
                        },
                        *kind,
                        *number,
                        text,
                        spans,
                    );
                }
            }
        }
    }

    /// `M src/frame.rs        ● just changed  ▁▃█▅▂▁▁▁      +12 -3`
    ///
    /// Everything to the right of the path is placed right to left, and the
    /// order it is *allocated* in is not the order it is drawn in. Allocation is
    /// priority: the counters first because they are the row's content, then the
    /// pulse, then the sparkline. The pulse outranks the strip for the same
    /// reason `f follow` is the last hint standing, and because its narrow rung
    /// costs one column against the strip's four.
    ///
    /// Nothing is allowed to take the path below [`MIN_PATH_WIDTH`]. A glance
    /// element that cost a reader the name of the file would be spending the
    /// content to decorate it.
    fn file_row(&mut self, area: Rect, heading: &Heading<'_>, peak: u16) {
        let mut right = area;

        let counts = heading
            .churn
            .map(|(added, removed)| format!("+{added} -{removed}"))
            .unwrap_or_default();
        let taken = self.put_right(right, &counts, self.theme.chrome_dim);
        right.width = right.width.saturating_sub(taken as u16);

        // What is left after the kind letter and the path's floor. Saturating,
        // so a row too narrow to hold both simply has no glance budget at all.
        let mut budget = usize::from(right.width).saturating_sub(ROW_FLOOR);

        let pulse = if heading.recency == Recency::Pulse {
            widest_fitting(&PULSE_RUNGS, budget)
        } else {
            ""
        };
        budget = budget.saturating_sub(reserved(width_of(pulse)));

        // The heat strip outranks the sparkline for what is left. Both are
        // glance elements and only one of them is about the diff on screen: the
        // strip says where in *this* file the change the reader is looking at
        // sits, and the sparkline says how busy the file was before any of it
        // was drawn.
        let heat = HEAT_RUNGS
            .iter()
            .copied()
            .find(|&rung| reserved(rung) <= budget)
            .map(|rung| heat_at(heading.heat, rung))
            .unwrap_or_default();
        budget = budget.saturating_sub(reserved(heat.len()));

        // Drawn right to left, so each block knows where the one outside it
        // ended. The strip drawn is the **tail** of the window: dropping buckets
        // means dropping the oldest, and the oldest are on the left.
        if let Some(strip) = spark_of(heading.spark, peak) {
            let buckets = SPARK_RUNGS
                .iter()
                .copied()
                .find(|&rung| reserved(rung) <= budget)
                .unwrap_or(0);
            if buckets > 0 {
                let tail: String = strip[HISTORY_BUCKETS - buckets..].iter().collect();
                let taken = self.put_right(right, &tail, self.theme.spark);
                right.width = right.width.saturating_sub(taken as u16);
            }
        }
        if !heat.is_empty() {
            // Cell by cell rather than as one string: every slice is the same
            // glyph and only the style differs, which is the whole design.
            let x = right.x + right.width - heat.len() as u16;
            for (offset, slice) in heat.iter().enumerate() {
                self.buf.set_stringn(
                    x + offset as u16,
                    right.y,
                    HEAT_BLOCK.to_string(),
                    1,
                    self.theme.heat(*slice),
                );
            }
            right.width = right.width.saturating_sub(reserved(heat.len()) as u16);
        }
        if !pulse.is_empty() {
            let taken = self.put_right(right, pulse, self.theme.pulse);
            right.width = right.width.saturating_sub(taken as u16);
        }

        let mut room = usize::from(right.width);
        let letter = format!("{} ", heading.kind);
        let x = self.put(area.x, area.y, &letter, room, self.theme.kind);
        room = room.saturating_sub(usize::from(x - area.x));

        let mut label = heading.path.to_owned();
        if let Some(from) = heading.from {
            // Which file it *was* is the whole content of a rename, so it is
            // part of the label rather than something to reveal on a keypress.
            label.push_str(" ← ");
            label.push_str(from);
        }
        self.put(
            x,
            area.y,
            &elide_head(&label, room),
            room,
            self.theme.recency(heading.recency),
        );
    }

    /// Walk a line into styled runs, stopping at the pane's edge, and say
    /// whether anything was left over.
    ///
    /// Split out of [`Painter::line_row`] because the step it repeats has three
    /// obligations that have to travel together: bound the walk, add what it
    /// cost to [`PaintStats::examined`], and report that the row continues.
    /// Written twice — once for the classified spans and once for the tail they
    /// did not reach — either copy could quietly drop the counter, and
    /// `tests/paint.rs` would still pass because the other copy still feeds it.
    /// One `push_run` means each obligation is discharged in exactly one place.
    fn content_runs(
        &mut self,
        runs: &mut Vec<(String, Style)>,
        text: &str,
        spans: &[Span],
        content: usize,
    ) -> bool {
        let mut column = 0usize;
        let mut at = 0usize;
        for span in spans {
            let end = (at + span.len).min(text.len());
            let Some(piece) = text.get(at..end) else {
                // A span boundary that is not a character boundary. It should not
                // happen and it must not panic, because the alternative to one
                // uncoloured line is a monitor that dies on a file. The rest of
                // the line is drawn unclassified.
                break;
            };
            at = end;
            if piece.is_empty() {
                continue;
            }
            if self.push_run(runs, piece, span.class, &mut column, content) {
                return true;
            }
        }
        if at < text.len() {
            // Whatever the spans did not reach, which is the whole line when
            // there are none: an unrecognised file type, or a row a test built
            // by hand.
            //
            // Styled through `class` rather than reaching for `context`
            // directly, so that "an empty span list and one `Plain` span reach
            // the screen identically" stays one rule instead of two expressions
            // that happen to agree. #11 gives the classes their own palette, and
            // the first `Plain` that is not `context` would otherwise leave this
            // path quietly on the old colour.
            //
            // Reached only when nothing above returned, so `at` is where the
            // spans genuinely stopped rather than where the pane cut them off.
            return self.push_run(runs, &text[at..], Class::Plain, &mut column, content);
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
    ) -> bool {
        if *column >= content {
            // Room ran out on an earlier run and this one has something to say,
            // so the row continues and nothing more of it is walked.
            return true;
        }
        let printed = printable(piece, column, content);
        self.paint.examined += printed.examined;
        runs.push((printed.text, self.theme.class(class)));
        printed.clipped
    }

    /// `  128 +    let value = 1;`
    ///
    /// **The sigil carries the diff, the text carries the syntax**, which is
    /// `SPEC.md` §11.1's ruling and the mockup's own layout: added, removed and
    /// context lines are highlighted identically, and only the `+` or `-` says
    /// which is which.
    ///
    /// What the picture adds on top is a **row wash and a left bar**, and #11
    /// landed both. The wash is painted first, across the whole row including the
    /// gutter and every trailing blank, which is what makes it read as a band
    /// rather than as a highlight behind some text. It survives everything written
    /// over it because `ratatui`'s `Cell::set_style` only overwrites the fields a
    /// style actually sets, and every run below sets a foreground and no
    /// background. [`Painter::status_line`] has relied on the same behaviour since
    /// the chrome was built.
    ///
    /// The bar is the **sigil cell**, inverted: the diff hue behind, the row's own
    /// wash in front. The mockup draws it as three pixels of a nine-pixel cell, so
    /// it is sub-cell and has no terminal equivalent that does not spend a whole
    /// column, and I6 forbids spending one on decoration. The sigil cell is the one
    /// cell on the row that already means *this line changed*, so it carries the
    /// bar instead of a column being found for it.
    ///
    /// Both are absent on a palette that declines them and on a depth that cannot
    /// express them, and then this draws exactly what it drew before #11: the sigil
    /// alone, which is the loss §11.1 records.
    fn line_row(&mut self, area: Rect, kind: LineKind, number: u32, text: &str, spans: &[Span]) {
        let (diff, sigil) = match kind {
            LineKind::Added => (self.theme.added, '+'),
            LineKind::Removed => (self.theme.removed, '-'),
            LineKind::Context => (self.theme.context, ' '),
        };

        // Patched onto the diff style rather than replacing it, so a palette whose
        // bar is unset leaves the sigil exactly as it was. Writing the bar straight
        // into the run would blank the sigil's own colour on every palette that
        // declines to draw one, which is the default.
        //
        // **And the bar is only meaningful as a pair**, which is the part that had
        // to be found by a gate rather than by reading. Its foreground is the row's
        // own wash, chosen to sit legibly *on* the diff hue behind it. A depth that
        // drops backgrounds keeps that foreground, so patching unconditionally
        // paints the sigil in a near-black wash colour on no background at all, and
        // the one thing still separating an addition from a context line at sixteen
        // colours disappears. So the bar applies only where its background
        // survived.
        let (wash, bar) = match kind {
            LineKind::Added => self.theme.row(true),
            LineKind::Removed => self.theme.row(false),
            LineKind::Context => (Style::new(), Style::new()),
        };
        let sigil_style = if bar.bg.is_some() {
            diff.patch(bar)
        } else {
            diff
        };
        if wash.bg.is_some() {
            self.buf.set_style(area, wash);
        }

        let mut x = area.x;
        let mut room = usize::from(area.width);
        if self.gutter > 0 {
            let gutter = self.gutter;
            let numbered = format!("{number:>gutter$} ");
            x = self.put(x, area.y, &numbered, room, self.theme.gutter);
            room = room.saturating_sub(gutter + 1);
        }

        // Capped by the pane as well as by the span count, because the walk now
        // stops at the edge: a minified line of three hundred spans in an
        // eighty-column pane pushes a handful of runs, and reserving for all
        // three hundred is fourteen kilobytes a row of churn. A run that is
        // pushed at all advances `column` by at least one, so the pane bounds the
        // count too.
        let mut runs = Vec::with_capacity((spans.len() + 2).min(room + 2));
        runs.push((sigil.to_string(), sigil_style));

        // Tab stops are counted from the start of the line's own content, not
        // from the left edge of the screen. The gutter and the sigil shift every
        // row by the same amount, so including them would align tabs to the
        // buffer and leave the file's indentation looking nothing like it does in
        // an editor. The counter therefore runs **across** span boundaries: a tab
        // in the middle of a line advances to the next stop measured from the
        // line's own start, not from the start of whatever run it landed in.
        // The sigil is one column and is pushed before the counter starts, so
        // what is left for content is everything but it.
        //
        // **This is the bound, and it is what makes a row cost the pane rather
        // than the line.** Every run below stops here, and the loop stops asking
        // for runs once it is spent, so a 531-column line in a 74-column pane is
        // walked 74 columns deep instead of 531. Measured before it existed: a
        // 22-row body of Japanese examined 8231 characters to show 1600 columns,
        // which is 5.1x, and `tests/paint.rs` is what fails if it comes back.
        let content = room.saturating_sub(1);
        let clipped = self.content_runs(&mut runs, text, spans, content);
        self.paint.rows += 1;

        // Content is the one thing that can neither break nor elide: wrapping it
        // would move every line below it, and no part of a line is its
        // identifying part the way a path's tail is. So it says it continues and
        // nothing more. `SPEC.md` §11.1 rules that this is not what I6 means by
        // a truncated label.
        self.put_runs_marked(x, area.y, &runs, clipped, room);
    }
}

fn width_of(text: &str) -> usize {
    TextSpan::raw(text).width()
}

/// One side of a hunk header, in git's own shorthand.
///
/// Git omits the count when a side covers exactly one line, and a reader
/// calibrated on `git diff` reads its absence as "one". Reproducing that is
/// cheaper than teaching them a second dialect.
fn span(start: u32, lines: u32) -> String {
    if lines == 1 {
        format!("{start}")
    } else {
        format!("{start},{lines}")
    }
}

/// Digits to reserve for line numbers, or zero to draw none.
///
/// Sized from the largest number actually on screen rather than from the file,
/// so the gutter does not widen for content nobody can see.
fn gutter_width(view: &View, width: usize) -> usize {
    let largest = view
        .rows
        .iter()
        .filter_map(|row| match row {
            Row::Line { number, .. } => Some(*number),
            _ => None,
        })
        .max()
        .unwrap_or(0);

    let digits = largest.max(1).ilog10() as usize + 1;
    // The gutter costs its digits plus a space, and the sigil costs one more.
    if width.saturating_sub(digits + 2) >= MIN_TEXT_WIDTH {
        digits
    } else {
        0
    }
}

/// Keep the tail of `text`, marking the loss, when it will not fit.
///
/// The tail, because the end of a path is the part that identifies the file. A
/// column reading `crates/vigia-core/…` names nothing, which is exactly the
/// truncated-to-useless label I6 forbids. This is the **only** direction on the
/// screen that keeps its end rather than its start, and it is why the two marks
/// are different characters: a path says its head is gone, everything else says
/// its tail continues.
///
/// One column is enough to say so. At `room == 1` the whole path is gone and the
/// result is a bare [`ELIDED`], which is honest about naming nothing rather than
/// showing an arbitrary first character as if it were a name.
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
    ///
    /// Carried out rather than inferred from the text's width, because the two
    /// differ exactly where it matters: a run that ends flush with the pane is
    /// indistinguishable by width from one that was cut there.
    clipped: bool,
}

/// Make one line of file content safe to write into terminal cells.
///
/// Two hazards, both from content nobody wrote for a display. A tab occupies one
/// cell and advances nothing, so everything after it in the row sits at the
/// wrong column. And a control character written straight through can move the
/// cursor or open an escape sequence, which corrupts the whole screen rather
/// than one row.
///
/// Columns are counted from the start of the **line**, which is where the file
/// counts them from too, so `column` is threaded in by the caller and carried
/// across the runs one line is made of. A per-run counter would reset at every
/// syntax boundary and align a tab to the token before it rather than to the
/// line, which is invisible until a file indents with tabs and then wrong on
/// every row of it.
///
/// **It stops at `room`**, which is the same counter and therefore the same
/// units: a pane bounds columns, and a bound written in characters would land a
/// two-column glyph half over the edge. Stopping is not an optimisation of the
/// drawing, since [`Buffer::set_stringn`] clips anyway; it is what stops the
/// *walk*, which is the cost. A row of a 660-byte line used to walk all of it,
/// and allocate all of it, to show 74 columns.
///
/// **And it stops at [`CHARS_PER_COLUMN`] characters as well, because a column
/// bound alone is not a bound.** A zero-width character advances `column` by
/// nothing, so a run made of them satisfies `column < room` forever and walks the
/// whole line however long it is: exactly the cost this function exists to
/// remove, reachable with a combining mark, a ZWJ, a variation selector or a
/// zero-width space. Two counters are needed because the two hazards are in
/// different units.
fn printable(text: &str, column: &mut usize, room: usize) -> Printed {
    // Sized from what will be kept rather than from what was offered. Four bytes
    // a column is the widest UTF-8 encoding, and a tab can expand past the end
    // by at most one stop.
    let mut out = String::with_capacity(
        text.len()
            .min(room.saturating_mul(4).saturating_add(TAB_STOP)),
    );
    // The character bound, in the same terms as the column one so the two can be
    // read together.
    let walk = room
        .saturating_mul(CHARS_PER_COLUMN)
        .saturating_add(TAB_STOP) as u64;
    let mut examined = 0u64;
    for (i, c) in text.char_indices() {
        if *column >= room || examined >= walk {
            // Stopped with source left over, which is the caller's signal to
            // mark the row and stop asking the rest of the spans for anything.
            return Printed {
                text: out,
                examined,
                clipped: true,
            };
        }
        examined += 1;
        match c {
            '\t' => {
                let stop = TAB_STOP - (*column % TAB_STOP);
                out.extend(std::iter::repeat_n(' ', stop));
                *column += stop;
            }
            c if c.is_control() => {
                out.push(UNPRINTABLE);
                *column += 1;
            }
            c if c.is_ascii() => {
                out.push(c);
                *column += 1;
            }
            c => {
                out.push(c);
                // Only the non-ASCII tail pays for a width lookup, which keeps
                // the common line off the measuring path entirely.
                *column += width_of(&text[i..i + c.len_utf8()]);
            }
        }
    }
    Printed {
        text: out,
        // The source ran out rather than the room, so the only way this row
        // still overflows is a two-column glyph that straddled the last cell.
        clipped: *column > room,
        examined,
    }
}
