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
//! The hint bar is the only thing that breaks by taking a **line** — [`Footer`]'s
//! second one, and then by dropping whole rungs of [`HINT_RUNGS`]. Two other
//! things are made of items and break by dropping a rung where they stand: the
//! header's left, whose changed-file count goes whole before the worktree name
//! beside it is ever cut, and a sparkline, which drops whole buckets. Everything
//! else is one token and says which end it lost: [`ELIDED`] on the left for a
//! file path, whose tail names the file, and [`CONTINUES`] on the right for
//! everything else, content included.

use std::time::Duration;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::Span as TextSpan;
use vigia_core::{Class, HISTORY_BUCKETS, LineKind, Recency, SPARK_GROUPS, Span, scale_of};

use crate::glyphs::Glyphs;
use crate::input::{Grabbed, Hovered, Region, Regions, Sheet};
use crate::theme::Theme;
use crate::view::{FileEntry, HEAT_BUCKETS, HeatBucket, Row, Scale, View};

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
/// **Three items, and that is the whole bar** ([#80](https://github.com/breferrari/vigia/issues/80),
/// ruled 2026-08-17 from use: *"keep only q f and ? on the bottom bar"*). The
/// widest rung was forty columns of advice when the keymap lived here, and B12
/// moved the keymap to a sheet, so the bar's job is now to name the way out, the
/// state a reader can lose invisibly, and the door to everything else. `jk scroll`
/// went with `JK files`: both are on the sheet, and a scroll key is the gesture a
/// pager reader tries first without being told.
///
/// The drop order is `SPEC.md` §11.1's, unchanged in its reasoning and shorter in
/// its list: `q quit` goes first because `q` is a pager reflex and four keys reach
/// quit, then `? keys`, and `f follow` is last standing because it is the one that
/// restores a state whose *absence* is invisible.
///
/// **`? keys` outranks `q quit` and yields to `f follow`**, which is the one
/// ordering decision this table adds. It sits above quit because a reader who
/// cannot find the keymap cannot find anything else either, and below follow
/// because a sheet a reader has not opened costs them nothing while a follow mode
/// they cannot see costs them the screen.
///
/// The widest bar is twenty-three columns where it was thirty-eight, so both
/// status readouts arrive far earlier than
/// [#147](https://github.com/breferrari/vigia/issues/147) measured them, and the
/// footer no longer takes a second line at forty columns at all. That issue's own
/// question is *still* not answered: with this table there is no bonus rung to
/// argue about, since [`HINT_BASELINE`] is rung zero and every rung is one a
/// reader is owed.
const HINT_RUNGS: [&str; 4] = [
    "q quit · f follow · ? keys",
    "f follow · ? keys",
    "f follow",
    "",
];

/// The rung whose fit decides whether the footer takes a second line.
///
/// **Rung zero since [#80](https://github.com/breferrari/vigia/issues/80), and it
/// used to be rung one.** The constant exists because the footer grows when the bar
/// cannot sit beside the state on one line, and measuring that against a rung
/// nobody is owed would let an optional hint buy a body row: `JK files` did exactly
/// that, making the widest bar forty columns so the footer took a second line at
/// the width I6 is named for, for every reader including the ones who never
/// pressed `J`.
///
/// **There is no bonus rung left to guard.** The bar is three items a reader is
/// owed at every width that can hold them, twenty-three columns at its widest, and
/// twenty-three plus the state's thirteen plus a gap fits the forty-column pane
/// with two columns to spare. So the thing this constant was invented to prevent
/// cannot happen from this table, and it stays named rather than inlined because
/// what it prevents is a *future* hint added above the baseline without anyone
/// noticing what it spent.
///
/// [`Footer::plan`] still hands the diagnostics whatever survives the state, the
/// gap and the hints, so a wider bar still costs the readouts columns:
/// `crates/vigia/tests/legibility.rs::a_wider_hint_bar_cannot_quietly_push_the_readouts_out`
/// pins where each cell arrives, and
/// [#147](https://github.com/breferrari/vigia/issues/147)'s question survives this
/// change with nothing to point at, which is worth saying plainly rather than
/// letting it read as answered.
const HINT_BASELINE: usize = 0;

/// What joins two hints.
///
/// Exported because `tests/legibility.rs` splits the rendered bar on it to check
/// that every hint on screen is a whole one. A test that restated the separator
/// as its own literal would be a second implementation of the parse, agreeing
/// with itself while disagreeing with the screen. The ladder is deliberately
/// **not** exported for the same reason inverted: a test comparing the rung
/// table against itself proves nothing, so the rungs are observed by rendering.
pub const HINT_SEPARATOR: &str = " · ";

/// How many buckets a sparkline may show, widest rung first.
///
/// **A sparkline is a projection rather than a list**, ruled 2026-08-20
/// ([#234](https://github.com/breferrari/vigia/issues/234)), so `SPEC.md` §11.1
/// has a narrower rung sum adjacent source buckets and draw the whole window at a
/// lower resolution. It dropped whole buckets oldest first until then, which is
/// the misdescription the heat strip's own clause refuses one axis over: the
/// churn band draws the whole window across the masthead, so a strip beside it
/// showing the newest half of that window is a shorter window presented as the
/// window.
///
/// Halving, so the sum is exact and every drawn bucket covers the same span, and
/// so a narrowed strip is an obvious fraction of the picture rather than a shaved
/// one.
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
///
/// **Named rather than written as `[3]`, because the ladder is derived now.**
/// While the rungs were a literal table the trailing zero sat at a fixed index
/// and a bare number was safe. `SPARK_RUNGS` is computed from [`SPARK_GROUPS`],
/// so a fourth grouping would make index three a *real* rung of three buckets
/// and move the empty one to four, which compiles and passes every gate: the
/// three narrowest layouts, which are supposed to draw no strip, would quietly
/// start drawing one. This index moves with the ladder instead.
const SPARK_NONE: usize = SPARK_GROUPS.len();

// **Derived rather than written out and then asserted equal, which is the shape
// this replaced.** [`SPARK_GROUPS`] is the same ladder from the store's side,
// because the *scale* a drawn bucket is measured against is decided there while
// its *width* is decided here. Written as two literal tables they can disagree,
// and the disagreement is silent: a row would draw the right number of buckets
// against a denominator set for a different width, which is a height rather than
// a layout and no width sweep can see it. An assertion catches that after the
// fact; computing one from the other makes it unrepresentable. The trailing rung
// that draws nothing comes from the initialiser, so the ladder cannot lose it
// either.
//
// **And the ladder's direction is asserted, not assumed.** Nothing else here
// reads [`SPARK_GROUPS`]' order, so a reordering would compile, invert this table
// and hand the widest pane the narrowest rung. `the_glance_columns_collapse_in_one_order`
// would catch it, one whole sweep later and blaming the layout table; a `const`
// block catches it at the definition.
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

// **The divisor property is still asserted**, because it is what [`spark_of`]
// rests on and it is worth stating where the reliance is rather than one crate
// over. It follows from `SPARK_GROUPS`' own `const` block, so nothing that runs
// can reach it: if `g` divides `HISTORY_BUCKETS` then so does `HISTORY_BUCKETS /
// g`. That is the same reason the rounding block below is a `const` rather than a
// test, and it starts failing the build the day either ladder stops being exact.
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
///
/// **The rungs stay a count of buckets and the *cost* is what the glyph
/// changes**, which is what keeps [`SPARK_RUNGS`] meaning one thing: a rung is
/// how much of the window a row shows, and how many columns that takes is a
/// property of the terminal rather than of the ladder. At
/// [`Glyphs::Block`] this is the identity and every width boundary in
/// [`ROW_LAYOUTS`] is exactly where it was.
///
/// Rounded **up**, so an odd bucket count still gets a whole cell to sit in
/// rather than being dropped for not filling one.
const fn spark_cells(buckets: usize, glyphs: Glyphs) -> usize {
    buckets.div_ceil(glyphs.density())
}

// **The rounding is asserted rather than only documented, because nothing that
// runs can reach it.** Every rung of [`SPARK_RUNGS`] is even while
// [`HISTORY_BUCKETS`] is even, so `div_ceil` and a plain division agree on
// every input the renderer can produce: swapping one for the other is a mutation
// the whole suite survives, and a claim no gate can fail is a wish. A `const`
// block is the right instrument precisely because the case is unreachable at
// run time, and it starts failing the build the day the window becomes odd.
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
///
/// `SPEC.md` §5.1 draws this as a persisting mark rather than a flash, and as of
/// 2026-08-03 the mark is the whole of it: the `● just changed` rung above this
/// one is gone from the picture and from here together. The label restated a
/// fact the dot had already made, on a row that is also the brightest rung of
/// the recency gradient, and a monitor pays for a label in reading rather than
/// in columns.
///
/// So the ladder is two rungs and the survivor is one column, for the reason
/// `f follow` is the last hint standing: it is the one signal on the row that
/// cannot be recovered from anything else on screen.
const PULSE_RUNGS: [&str; 2] = ["●", ""];

/// One slice of a file, whatever it holds.
///
/// Equal weight at every slice, not a ramp. The sparkline two columns away
/// already encodes magnitude as height, and a second magnitude encoding beside it
/// would be a second dialect for one fact. Here the slice is a **position** and
/// the colour is the meaning, which is what `assets/preview.svg` draws: twelve
/// rects of equal height differing only in fill.
///
/// **A small square rather than a full block, corrected 2026-08-16**
/// ([#196](https://github.com/breferrari/vigia/issues/196)). The picture has
/// always drawn the twelve as *separate* rects: width `11` on a pitch of `14`
/// with `rx="2"`, so three pixels of gap and a rounded corner. `█` twelve times
/// is one continuous bar on a cell grid, and the slices the projection computes
/// are then invisible as slices, because only a change of band marks a boundary
/// and two adjacent slices in one band read as a single wide one. That is fatal
/// to the element's own job, which is *where in this file the work is*: a reader
/// placing a change counts slices, and there was nothing to count.
///
/// A square puts the gap **inside the cell**, so the strip keeps its twelve
/// columns and every rung of [`HEAT_RUNGS`] is untouched: a square cannot fill a
/// cell that is about twice as tall as it is wide, and what it cannot fill is the
/// separation. Its East Asian width is ambiguous, which is not a new hazard: `█`
/// is ambiguous too and has been drawn here since the element existed, and
/// [`width_of`] measures both as one column through the same table the renderer
/// places them with.
///
/// **`■` U+25A0 rather than `▪` U+25AA, and CP437 is the tiebreaker.** The two
/// look alike and the small square shipped first; measuring the pair against
/// `Encoding.GetEncoding(437)` put `▪` **outside** the legacy Windows console's
/// repertoire, where `■` sits at 0xFE. §10's Windows bullet names the heat strip
/// among the things that *"survive whole"* there, and choosing the smaller square
/// would have made that sentence false while nothing failed. That is
/// [#175](https://github.com/breferrari/vigia/issues/175)'s own precedent, where
/// `│` beat `▕` partly on the same membership, and it is why this docblock
/// carries a CP437 line at all: every glyph const around it does.
///
/// It also ends a collision. `tests/legibility.rs` counts a sparkline's buckets
/// by colour **and** glyph precisely because the heat strip drew the same `█` a
/// full bucket does; the two elements now share no glyph at all.
const HEAT_SLICE: char = '■';

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
/// line is narrower than the thumb on purpose: the track is context and the
/// thumb is the reading.
///
/// **Centred in its cell, and it was not until 2026-08-15**
/// ([#175](https://github.com/breferrari/vigia/issues/175)). This was `▕`
/// `U+2595`, RIGHT ONE EIGHTH BLOCK, a filled sliver against the cell's right
/// edge, and reported from use as a bar pushed off the side of the pane rather
/// than a column with a line down it. The narrowness argument was right about
/// what matters and wrong about which glyph delivers it: `│` is narrower than
/// `█` too, so the contrast the old comment was defending survives the change
/// intact, and what went with the half-block was only the flush edge nobody
/// wanted.
///
/// It pays a second time where nothing was looking. `▕` is **outside** CP437 and
/// `│` is **inside** it at `0xB3`, measured rather than assumed, so this takes a
/// name off §10's degradation list one ruling after [#166](https://github.com/breferrari/vigia/issues/166)
/// added two to it. [`RULE`] is the horizontal member of the same box-drawing
/// family and is already inside, so the two structural lines on this screen now
/// come from one set.
const BAR_TRACK: char = '│';

/// The step button at the top of a bar, and the one at the bottom.
///
/// **Outside CP437, and deliberately so.** Measured rather than assumed: the
/// codec refuses `▲`, `▼`, `▴`, `▾`, `↑` and `↓` alike, and the two triangles
/// IBM's graphic table does place are at `0x1E` and `0x1F`, which a console
/// consumes as control codes before they reach a glyph. That is the same trap
/// `SPEC.md` §10 already records for `←`. What settles the choice is one column
/// over: [`BAR_TRACK`] is **itself** outside CP437, so a console that cannot
/// draw a button already cannot draw the track it sits on, and the buttons cost
/// such a reader nothing that survived before. A ladder for characters is
/// [#159](https://github.com/breferrari/vigia/issues/159)'s question and is not
/// invented here.
const STEP_UP: char = '▲';
const STEP_DOWN: char = '▼';

/// Rows a stepped bar spends on buttons: one at each end.
const STEP_ROWS: u16 = 2;

/// The shortest track that can still express more than one position.
///
/// One row is full at every window, which is the "column saying there is nothing
/// to scroll" [`scrollable`] and [`Painter::with_bar`] both already refuse. Two
/// is where a thumb has somewhere to be.
const MIN_TRACK: u16 = 2;

/// The shortest region whose bar carries step buttons.
///
/// **A sum rather than a literal.** The floor is the buttons plus the smallest
/// track worth drawing between them, and writing it as `4` would leave the
/// agreement between the drawer and this threshold clerical: change what a
/// stepped bar spends and every gate over the drawn output stays green while the
/// *boundary* silently keeps the old total.
const STEP_FLOOR: u16 = STEP_ROWS + MIN_TRACK;

/// What marks the row for the file the diff is currently inside.
///
/// **Not a cursor**, and the glyph is chosen to say so. `SPEC.md` §11.2 B4 keeps
/// the list not navigable, so nothing is selected and nothing moves on a
/// keypress that is not a scroll; this points at where the diff already is. A
/// filled block or an inverted row would read as a selection, which is the
/// reviewer-class affordance the ruling refuses.
///
/// A `&str` rather than a `char`, because the one place it is drawn wants one:
/// it was `&CARET.to_string()` at that call, which is a heap allocation per frame
/// for a literal the compiler already has.
const CARET: &str = "▸";

/// The weight that row's **path** takes on top of whatever recency gave it.
///
/// **The caret's second channel, not a second mark**
/// ([#193](https://github.com/breferrari/vigia/issues/193)). `SPEC.md` §5.3 makes
/// every other durable distinction in that region a matter of weight, and the one
/// row a reader most needs to find carried a glyph alone. This is drawn where and
/// only where [`CARET`] is, so the two are one statement said twice: the tie is
/// [`affords_caret`], and below it neither is drawn.
///
/// **A constant here rather than a [`Theme`] key, and the reason is not the one
/// that first stood here.** That said the theme owns colour and the shell owns
/// structure, citing [`CARET`] and [`RULE`] as precedent; `theme.rs` falsifies it
/// four lines at a time, because every other drawn modifier is themed
/// ([`Theme::path`]'s `BOLD`, [`Theme::path_hover`]'s `UNDERLINED`,
/// [`Theme::alert`]'s). Those constants are **glyphs**, and the theme grammar has
/// no glyph vocabulary at all, so they mark where its vocabulary stops rather
/// than a precedent for stopping here. The real reason is narrower: every key in
/// that grammar is a **complete** style that replaces a value, and this is a
/// **delta** composed on top of recency-or-hover, so a key for it would be the
/// first of its kind and wants a ruling of its own rather than arriving as the
/// side effect of a feel row. Tracked as
/// [#195](https://github.com/breferrari/vigia/issues/195).
///
/// **It shares `BOLD` with [`Theme::path`]'s pulse rung and that is not a
/// collision**, because both readings carry a glyph of their own: a pulsing row
/// draws `●` in its reserved slot and this one draws [`CARET`]. Where the pulse
/// column is dropped the caret still is not, so *bold with a caret* and *bold
/// without one* stay two different sentences at every width the caret survives.
const CURRENT_WEIGHT: Modifier = Modifier::BOLD;

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
///
/// **Four rungs since [#161](https://github.com/breferrari/vigia/issues/161), and
/// what moved is the source rather than this ladder's shape.** [`HEAT_BUCKETS`]
/// doubled, so the same halving now reaches twenty-four before it reaches
/// twelve, and every rung that existed keeps its exact width. Which of them a
/// given pane picks is [`Columns::plan`]'s business, and the rung above the
/// settled ladder is the one the share clamp there exists for.
const HEAT_RUNGS: [usize; 4] = [HEAT_BUCKETS, HEAT_BUCKETS / 2, HEAT_BUCKETS / 4, 0];

// **Asserted rather than documented, because a rung that does not divide the
// source is silent.** [`heat_at`] groups `HEAT_BUCKETS / width` and chunks by it,
// so a rung that leaves a remainder draws a short final group carrying fewer
// source slices than its neighbours: a strip whose last slice is quieter than
// the file, with nothing failing. `HEAT_RUNGS` is derived from `HEAT_BUCKETS`
// today and this is what keeps that true if a rung is ever written out by hand.
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
///
/// A heading whose path has been elided past this is a row that has stopped
/// naming its own file, which is exactly the "truncated to useless" shape I6
/// forbids. Twelve leaves `…engine/watch.rs` legible at forty columns, where the
/// counters and the pulse together already want fourteen and the heat strip
/// beside them wants seven more.
const MIN_PATH_WIDTH: usize = 12;

/// Columns the kind letter and its gap take at the head of every file row.
///
/// **Named because three places need it and two of them used to guess.** It is
/// the `2` in [`Painter::file_row`]'s own floor, and both [`affords_caret`] and
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

/// What stands between a content row's sigil and the line itself.
///
/// `assets/preview.svg` has drawn it from the start: the picture's own comment
/// states that one cell at 13.5px is ~8.1px, and it places the sigil at x=72 and
/// every content origin at x=88, so the sigil's one cell ends at 80.1 and a clear
/// column stands after it. [`Painter::line_row`] carries the argument for why it
/// does not degrade with width, which is the only thing about it that needed
/// deciding ([#164](https://github.com/breferrari/vigia/issues/164)).
///
/// **One ASCII byte, one column, and the two have to stay the same number**,
/// because [`SIGIL_WIDTH`] derives from `len()`, which counts bytes where every
/// use of it counts columns. `width_of` is the column-correct spelling and is
/// not `const`, so this comment is the guard: a wider or non-ASCII gap has to
/// change that derivation rather than ride through it.
///
/// A `&str` rather than a `char` for that derivation, and **not** because runs
/// are `String`s, which an earlier draft of this said. The sigil beside it is a
/// `char` pushed the same way through `to_string()`, so the run type forces
/// nothing; `.len()` is what needs the `&str`.
const SIGIL_GAP: &str = " ";

/// Columns a content row spends on the sigil and the gap after it.
///
/// **Three expressions have to agree about this row's prefix and only two of
/// them are in [`Painter::line_row`]**: what is pushed as runs, what is
/// subtracted from the row's room to bound the walk, and what [`gutter_width`]
/// measures a text column against before ruling the line numbers affordable.
/// [`line_origin`] is what the last two read.
///
/// **The third was already wrong when this doc first claimed there were two.**
/// `gutter_width` carried a bare `digits + 2`, exact while the sigil stood alone
/// and a column behind the moment the gap landed: on a pane with a scrollbar, at
/// the narrowest width keeping the gutter, the text column was 23 against
/// [`MIN_TEXT_WIDTH`]'s 24. A constant whose doc names the sites it governs is
/// only as good as that list, and this is the second time this file has recorded
/// that failure ([`KIND_WIDTH`] is the first).
///
/// **The runs-against-bound half is caught, and that was worth measuring rather
/// than assuming.** Mutated to `1` while the gap is still pushed, so the bound
/// sits a column looser than the runs, three gates redden:
/// `legibility.rs::a_wide_glyph_at_the_edge_does_not_swallow_the_mark` and
/// `::a_clipped_content_line_says_it_continues`, plus the forty-column snapshot.
/// The `gutter_width` half was caught by nothing, which is the difference
/// between a number two *drawing* expressions share and a number a **threshold**
/// reads.
const SIGIL_WIDTH: usize = 1 + SIGIL_GAP.len();

/// Columns before a content row's first character of line, gutter included.
///
/// **Written once because two expressions have to agree about it and did not**:
/// [`Painter::line_row`] bounds its content walk with this, and [`gutter_width`]
/// measures a pane against it before ruling the line numbers affordable. They
/// answered differently for one commit, so the gutter survived on a text column
/// [`MIN_TEXT_WIDTH`] forbids.
///
/// The `+ 1` is the gutter's own trailing space, which lives inside a `format!`
/// at the point of drawing and had no name until this. A zero gutter draws
/// neither digits nor space, so it costs neither.
const fn line_origin(gutter: usize) -> usize {
    if gutter == 0 {
        SIGIL_WIDTH
    } else {
        gutter + 1 + SIGIL_WIDTH
    }
}

/// Rows the worktree churn band takes when it is drawn at all.
///
/// **Two, which is sixteen levels over the [`crate::glyphs::SPARK_RAMP`]'s eight per row.** One
/// row would be a sparkline, and the pane already has one of those per file; the
/// band earns its place by being the thing no file row can be, which is the
/// worktree at a resolution a single row cannot carry. Three was weighed and
/// costs the diff a row for a resolution the eye does not spend.
const GRAPH_ROWS: usize = 2;

/// Rows the band leaves blank **below** itself.
///
/// **The band is the one drawn thing on this pane that is not text**, and a
/// graph pressed against a header on one side and a file list on the other reads
/// as a texture rather than as a chart. A blank rather than a rule, which is the
/// opposite of the choice [`RULE`] records between the list and the diff, and the
/// reason that ruling does not reach here is in its own docblock: a blank there
/// *"reads as the diff having nothing at the top"*, and that argument is about a
/// **scrolling text** region. A graph has a baseline and cannot be mistaken for
/// an empty one.
///
/// **Below only, since [#174](https://github.com/breferrari/vigia/issues/174).**
/// The blank this used to keep *above* the band is [`LEAD_ROWS`] now, which the
/// body opens with whether or not a band is drawn. Nothing moved: the sum above
/// the list is `LEAD_ROWS + GRAPH_ROWS + GRAPH_AIR`, which is the four rows the
/// two-sided version already spent, so no pane height gains or loses the band.
const GRAPH_AIR: usize = 1;

/// Rows of air the body opens with, under the header.
///
/// **The one boundary on this pane that was drawn with nothing**
/// ([#174](https://github.com/breferrari/vigia/issues/174)). The header states a
/// fact about the worktree as a whole and the list is a map of it, which are
/// different classes, and the boundary *below* the list already spends a whole
/// [`RULE`] saying so. This one had neither a rule nor a blank.
///
/// **A blank rather than a rule, and that is the ruling.** The ask was
/// separation, not a second boundary mark, and drawing `─` here would put one
/// glyph on two boundaries of visibly different weight. It also needs no new
/// character, so it raises no CP437 question one ruling after
/// [#175](https://github.com/breferrari/vigia/issues/175) took a name off §10's
/// degradation list.
///
/// **Coextensive with the list, exactly as [`Body::rule`] already is.** §11.2 B11
/// leans on `rule: list > 0` making the rule and the list one thing, and this row
/// is given the same shape: a body with no map has nothing to separate the header
/// from, so the row goes back to the diff. Nothing else decides it. The
/// changed-file count, the notice and follow mode cannot move it, which is
/// §11.1's rule against a transient thing jogging a reader's diff.
///
/// **When a band is drawn this row *is* the band's leading air**, so the masthead
/// screen's layout is untouched and only the masthead-off screen (the default
/// since [#204](https://github.com/breferrari/vigia/issues/204)) pays. That cost
/// is one row of roughly fifteen on the 24-row pane this tool is built for, and
/// it is stated rather than buried: it comes out of the diff, like every other
/// row this layout spends.
const LEAD_ROWS: usize = 1;

/// Diff rows the band may not take the pane below.
///
/// **Derived rather than chosen.** `git diff -U3` at its smallest is a hunk
/// header, three lines of context, one changed line, three more of context and
/// the file's own heading, which is nine rows. Ten is that with one to spare, so
/// a pane that draws the band still draws a whole hunk of the thing the tool
/// exists to show. Below it the band is not drawn at all, which is the same
/// order [`Body::split`] already applies to the list: the map gives way to the
/// content, and the band gives way before the map does.
const GRAPH_KEEP: usize = 10;

/// The narrowest band that can draw every sample it holds.
///
/// A band narrower than this would have to drop columns, and a projection that
/// drops items is the shape `SPEC.md` §5.1 refuses for the heat strip in as many
/// words. Below it the band is not drawn.
const GRAPH_FLOOR: usize = 8;

/// Whether a pane this wide can carry the band at all.
///
/// **Asked by [`Body::split`] as well as by [`Painter::band`], and that is the
/// point.** The floor lived only in the painter for one commit, so a pane below
/// it reserved four rows for a region the drawer then declined to draw, and the
/// masthead was four blank rows pushing the list down for nothing.
/// `the_header_never_takes_a_second_line` is what found it, by asking the layout
/// where the body starts and comparing that against where content actually is.
///
/// That is the same shape [`bar_for`] was consolidated for one element over: a
/// threshold two expressions keep by hand is invisible to every test that reads
/// what was drawn, because the drawn side stays correct and only the *width at
/// which* it changes drifts.
///
/// Measured from the **pane** less its inset and less the scrollbar's column
/// whether or not a bar is drawn, for [`affords_caret`]'s reason: a band whose
/// presence depended on whether a seventh file had appeared would move the list
/// under a reader for something that is not about the list.
const fn band_fits(pane: u16) -> bool {
    planning_width(pane, 0) as usize >= GRAPH_FLOOR
}

/// The smallest body a second footer line may leave behind.
///
/// Two rows, because that is the shortest thing that still reads as a diff: a
/// file heading and one line under it. Below that the footer would be buying
/// legibility with the content it exists to make legible.
const MIN_BODY: u16 = 2;

/// The deepest pinned file list that shipped before a rung was added above it.
///
/// **A cap rather than a height**, which is the difference between this and a
/// fixed region: three changed files draw three rows, matching
/// `assets/preview.svg` exactly, and a formatter touching two hundred draws the
/// cap and scrolls. `SPEC.md` §11.1 rules the height a function of pane height
/// and changed-file count alone, which is the same pair `Footer::plan` already
/// takes and for the same reason: both change only when the diff does, so
/// neither can jog a reader's diff.
///
/// **Six, and the reason recorded for it was a proportion wearing an absolute's
/// clothes** ([#160](https://github.com/breferrari/vigia/issues/160)). §11.1
/// derived it as *the largest block that still reads as a glance*, and then gave
/// the working: *on the 24-row pane this tool is built for it leaves fourteen
/// rows of diff after the header, the rule and a one-line footer*. That is
/// arithmetic about **one** pane — 24 less a header, [`LEAD_ROWS`], six, the rule
/// and a footer is fourteen exactly — and six is a quarter of it. So the number
/// is the instance and [`list_cap`] is the rule, which is why deepening the list
/// on a taller pane reproduces this one rather than replacing it.
///
/// **This was `LIST_ROWS` and the name is the change.** It stopped being the
/// rows a list takes the moment there was a rung above it, and a layout constant
/// whose name outlives its meaning is exactly what [`SETTLED`]'s own docblock was
/// rewritten to prevent. `LIST_FLOOR` was the other candidate and is refused:
/// [`GRAPH_FLOOR`] and [`ROW_FLOOR`] both mean *the smallest pane that draws the
/// thing at all*, and this means the opposite end.
///
/// **The digit range is this number**, restated in [`crate::action_for`] rather
/// than imported. `1` to `6` address the rows every pane drawing a list has, so
/// a digit means the same thing on every pane; the rows a taller pane adds above
/// it are reached with `J`/`K`, `n`/`p` and the pointer.
pub const LIST_SETTLED: usize = 6;

/// Rows of pane the list is owed one row of map for, above [`LIST_SETTLED`].
///
/// **A quarter, and it is [`LIST_SETTLED`]'s own derivation read as a rule.**
/// That constant's docblock carries the argument: §11.1 sized the list against a
/// 24-row pane and six is a quarter of one. A rule that reproduces the shipped
/// number at the pane it was chosen against is the same evidence
/// [`GLANCE_NUMER`] was adopted on one region out, which is why this is a share
/// rather than a rung table: there is one axis here and no per-side split, so a
/// division cannot oscillate the way [`MARGIN_RUNGS`] could.
///
/// **One constant where [`GLANCE_NUMER`] needs two, and the difference is
/// arithmetic rather than style.** Two fifths is irreducible, so that share can
/// only be written as a pair; a quarter is `height / 4` exactly, and a numerator
/// of one beside it would be an inert multiply and a name that does no work.
/// Bought symmetry is what [`SETTLED`] was rewritten to stop paying for one
/// element over. Written as a **rate** for the same reason: *one row of list per
/// four rows of pane* is how §11.1 states it and how the step below reads.
///
/// **That step of one is the property the band downstream rests on**, rather
/// than a rounding: [`Body::split`] pays the band out of what the list leaves, so
/// a cap that gained two rows for one row of pane would take a band off a pane
/// that had just grown. A unit numerator holds it at **any** share, since
/// `(h + 1) / n` exceeds `h / n` by at most one for every `n`; what four decides
/// is which number the floor reproduces, not whether the step is safe.
const LIST_SHARE: usize = 4;

/// Rows of list a pane this tall is generous enough to afford.
///
/// Floored by the division, so the share is never rounded **up** into a row the
/// diff was keeping. Same rule as [`generous_of`] one region out.
const fn deep_of(height: u16) -> usize {
    height as usize / LIST_SHARE
}

/// Rows the pinned file list may take on a pane this tall, before the rule.
///
/// **Floored at [`LIST_SETTLED`] rather than applied to it**, which is what makes
/// every pane that shipped draw exactly what it drew: at 24 rows the share *is*
/// the floor, and below 24 the floor wins, so no pane at or under 27 rows can
/// move whatever this share is set to. That is [`Columns::plan`]'s clamp one
/// region out, in the one direction a height ladder has.
///
/// **Read from the pane and never from the body**, which is the ruling
/// [`margin_of`] and [`affords_caret`] already make one axis over. The body is
/// the pane less its header and a footer whose own height depends on the
/// changed-file count, so a cap read off it would deepen the list because a
/// seventh file appeared, which is precisely the jog §11.1 forbids.
///
/// Not exported. A test that imported this would compare the ladder against
/// itself, which is the reason [`HINT_RUNGS`] is deliberately unexported too; the
/// rungs are observed by splitting a pane.
///
/// **A branch rather than `.max`, and it is the const context rather than
/// taste.** `Ord::max` is a default trait method behind `const_cmp`, so no
/// `const fn` in this file can call it; [`Columns::plan`] writes the same clamp
/// as `.max` because it is a plain `fn`. The two say the same thing.
const fn list_cap(height: u16) -> usize {
    let deep = deep_of(height);
    if deep > LIST_SETTLED {
        deep
    } else {
        LIST_SETTLED
    }
}

/// Columns the caret glyph itself occupies.
///
/// **One, and the gap it used to carry is the pane's own inset now**
/// ([#173](https://github.com/breferrari/vigia/issues/173)). This was `2` — a
/// glyph and a trailing space — and the list was indented by the pair, which put
/// its status sigil two columns right of the same sigil on the diff's headings.
/// Reported from use, and visible in a snapshot this repo had been committing all
/// along.
///
/// What the caret *costs a row* is [`caret_gutter`] rather than this, and the two
/// are different numbers at every width where the pane has a margin to lend. This
/// is the glyph; that is the bill.
const CARET_WIDTH: usize = 1;

/// Columns the caret takes off the pinned list's own row.
///
/// **Only what [`inset_of`] cannot already lend it.** The caret is drawn at the
/// pane's leading column, into the blank the margin ladder already keeps there, so
/// from forty-three columns up it costs the row nothing at all and the list and
/// the stream share one origin, one width and one [`Columns`] plan. Below that the
/// ladder gives no inset ([`MARGIN_RUNGS`]), there is no column to move into, and
/// the caret takes one of the row's own.
///
/// **The residual below forty-three is one column and is not hidden.** Three
/// branches were weighed and `SPEC.md` §11.1 carries the ruling. Extending the
/// margin ladder down so the caret always has somewhere to stand was refused on
/// [#119](https://github.com/breferrari/vigia/issues/119)'s own recorded reason,
/// which is still true: that floor exists to protect I6's forty-column pane, and
/// it would spend a column of *code* at exactly forty. Dropping the caret below
/// forty-three buys alignment at every width and costs the narrow pane the mark
/// saying which file the diff is inside, which is information traded for tidiness
/// at the width where every column is contested. Keeping the old two made the gap
/// between caret and sigil appear, vanish and reappear as the pane widened.
///
/// Saturating rather than a branch, so the two rungs are one expression.
const fn caret_gutter(pane: u16) -> usize {
    CARET_WIDTH.saturating_sub(inset_of(pane) as usize)
}

/// Whether a pane this wide can afford the caret at all.
///
/// Below it the caret is dropped and the list draws full width. It is a glance
/// element like any other, and [`ROW_FLOOR`] outranks every glance element: a row
/// that spent the file's name on a marker pointing at it would be naming nothing.
///
/// **A predicate over [`planning_width`] rather than a constant, and that is
/// [#173](https://github.com/breferrari/vigia/issues/173)'s own gate.** It was
/// `CARET_WIDTH + BAR_WIDTH + ROW_FLOOR`, a sum kept by hand beside a drawer that
/// spelled the same pieces out again. That is the exact shape the recorded lesson
/// about hand-kept thresholds names: the drawn side stays correct and only the
/// *width at which* the element vanishes drifts, invisibly to every test that
/// reads what was drawn. Now the threshold reads the same expression
/// [`Painter::list`] plans the row against, so they cannot disagree. Same shape as
/// [`affords_bar`] one element over.
///
/// **It counts [`BAR_WIDTH`] even on a screen with no bar, and that is the
/// ruling**, inherited unchanged from the constant this replaced.
/// [`planning_width`] is where that term lives now. The two ladders otherwise
/// collide: `render` has already taken the bar's columns off the width the region
/// is handed, and whether a bar exists depends on whether the list is
/// *scrollable* — which is a fact about the changed-file count, not about the
/// pane. With both floors at sixteen, a seventh changed file made the caret vanish
/// at sixteen and seventeen columns with nothing about the pane having moved,
/// which is exactly the "reads as the current file changing" failure the ladder's
/// own gate exists to prevent. Paying for the bar unconditionally is what makes
/// the caret's presence a function of pane width alone.
///
/// The floor it works out to is **seventeen** columns, one narrower than the
/// eighteen the constant named, because the caret now bills the row for one column
/// rather than two. Both are far below the forty I6 is named for.
const fn affords_caret(pane: u16) -> bool {
    planning_width(pane, caret_gutter(pane) as u16) as usize >= ROW_FLOOR
}

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
/// Same reasoning as [`affords_caret`], and now the same expression: a bar is a
/// glance element and [`ROW_FLOOR`] outranks every one of them. The parallel
/// between the two floors reads off the source rather than being asserted here.
const BAR_FLOOR: usize = BAR_WIDTH + ROW_FLOOR;

/// Whether a pane of `width` can afford a scrollbar at all.
///
/// **One rule for the whole screen, asked in one place**, so a reader never sees
/// half a pair. [`render`] and [`regions`] each kept this comparison by hand,
/// under two different local names, which is the same shape [`Bar`]'s own doc
/// gives for the predicate beside it: a threshold two expressions agree about
/// clerically stays correct on the drawn side while the *width at which* it
/// changes drifts, and nothing that reads the output can see the difference. The
/// bar's two other decisions already come from [`bar_for`]; this is the term that
/// was left behind.
const fn affords_bar(width: u16) -> bool {
    width as usize >= BAR_FLOOR
}

/// The pane's whole margin, both sides counted together: blank columns it keeps
/// between its own edge and any glyph. Widest pane first.
///
/// `assets/preview.svg` draws its **furniture** to the window's edge and its
/// **text** one to three cells inside it. Measured off the file: the nine row
/// washes span `x=8 width=884` and the region rule runs `x1=8` to `x2=892`, which
/// is the window exactly, while the text proper begins at `x=32`. (The caret was
/// at `x=16` when this was written and is at `x=8` since
/// [#173](https://github.com/breferrari/vigia/issues/173) put it on the pane's own
/// edge, which is a *licensed* glyph outside the margin rather than a counterexample
/// to it: the ladder is about text, and the sentence above is still what the picture
/// says about text.) The shell drew everything from column 0, and by
/// §5.1's own law a picture in a public README is a specification, so that was a
/// fourth undocumented departure from it rather than a choice
/// ([#119](https://github.com/breferrari/vigia/issues/119)).
///
/// **Nothing below forty-three columns**, and that floor is the ladder's point
/// rather than a rounding. I6 is named for forty and every column there is
/// already contested: the sparkline has been bought and sold twice in this file
/// over two columns at exactly that width. A pane that cannot afford a margin
/// does not take one, and a reader at forty gets the pane they got before.
///
/// **Forty-three, not forty-four, and the difference is the odd rung rather than
/// a slip.** #119 proposes the ladder per side, one cell each from 44 and two
/// from 80, and that is exactly what ships at 44 and at 80. The total has to
/// climb one column at a time to stay monotone, so the first of the two columns
/// lands a width early, at 43. What the floor protects is I6's forty-column pane,
/// which is three columns clear of it either way; what it does not claim is that
/// 43 is untouched, because it is not. `the_inset_never_reaches_the_forty_column_pane`
/// gates the claim that is actually load bearing.
///
/// `SPEC.md` §11.1 carries the rungs.
///
/// The top rung is a judgement inside what the picture shows rather than a number
/// read off it: the mockup's own left inset is one cell at the caret and three at
/// the kind letter, on a canvas about a hundred and nine columns wide. §5.3
/// already rules that the picture binds the element set and each element's
/// promises, never glyph-for-glyph fidelity.
///
/// **Counted as a total across both sides rather than per side, and that is the
/// rung table rather than an arithmetic convenience.** A per-side ladder steps
/// its two sides on the same column, so it costs two columns for the one a
/// widening pane just gained: `pane - 2` goes *down* by one exactly where it
/// should go up. That is what [`ROW_LAYOUTS`] is written out to refuse, one
/// element further out, and it is not theoretical. Taking both sides at once grew
/// the footer to two rows at 44 and at 80, spending a body row on a wider pane,
/// and walked the header's worktree name from marked back to whole as the pane
/// *narrowed* across 80. Both were caught by gates that already existed
/// (`a_bonus_hint_rung_never_buys_itself_a_footer_row`,
/// `the_header_facts_degrade_through_one_recorded_sequence`), which is what an
/// invariant with a failing test behind it is for.
///
/// So the total climbs by one per rung and [`margins_of`] splits it. The two odd
/// rungs are the widths where the pane has bought one column of margin and not
/// yet the second: 43 and 79. **The widths #119 actually names are both even**,
/// one cell a side at 44 and two at 80, so the ladder it proposed is what ships
/// and these two rows are the step between them rather than a change to it.
const MARGIN_RUNGS: [(u16, u16); 4] = [(80, 4), (79, 3), (44, 2), (43, 1)];

/// The margin a pane this wide takes, both sides together.
///
/// A table resolved by reading down it, for [`Columns::plan`]'s reason one
/// element out: a written-out ladder cannot oscillate where a formula can, and
/// monotonicity is then a property of the table rather than an argument about the
/// code. `the_pane_insets_its_text_at_every_rung` is what reddens if a rung is
/// ever added out of order, since it sweeps the drawn column rather than reading
/// this table back.
///
/// **Decided from the pane and never from a region**, which is the same ruling
/// [`planning_width`] and [`affords_caret`] already make: a region rect has lost
/// the bar's columns when a bar was drawn, and whether one was is a fact about
/// the *contents*. Reading the inset off one would make a pane at eighty columns
/// take the eighty-column inset until a seventh file changed.
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
///
/// The rung above, **split as evenly as it goes, with the odd column going
/// left.** The odd rung has to land somewhere, and left is the side that reads:
/// spent on the right instead, a 43-column pane would draw its text hard against
/// column zero and hold a blank column on the far side, which is the squeezed
/// look #119 exists to remove wearing the margin on the wrong edge. A reader
/// registers the left as the edge of the page because it is where every line
/// starts.
///
/// At the two widths #119 names the split is even, one cell a side at 44 and two
/// at 80. Only 43 and 79 are lopsided, and each is a single width.
///
/// A region does not call this. [`planning_width`] takes the left alone, because
/// `SPEC.md` §11.1 rules its right-hand columns are the bar's and not a margin.
const fn margins_of(pane: u16) -> (u16, u16) {
    let total = margin_of(pane);
    (total.div_ceil(2), total / 2)
}

/// The column a pane this wide begins drawing text at.
///
/// The left half of [`margins_of`], named because it is what every row is offset
/// by and what [`planning_width`] charges a region.
const fn inset_of(pane: u16) -> u16 {
    margins_of(pane).0
}

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

/// The marker inside [`FOLLOWING`], which is drawn green where the word beside
/// it stays dim.
///
/// **The picture's own split, and it is not decoration.** `assets/preview.svg`
/// draws `follow ` in `.dim` and this glyph in `.grn`, and §5.1's rule is that a
/// published artifact answering a question is the answer. It earns the colour:
/// the word names a mode and the mark says the mode is *on*, which is the one
/// thing on the footer a reader checks at a glance rather than reads.
///
/// Restated as a `char` beside the string rather than composed into it, because
/// `concat!` cannot take a `char`. Two spellings of one glyph can drift, and the
/// drift is silent: the recolouring pass would find nothing and the mark would
/// go back to grey. `the_follow_marker_is_the_last_character_of_the_state`
/// catches a change to [`FOLLOWING`], and
/// `the_follow_marker_is_green_where_the_word_beside_it_is_dim` catches a change
/// to this, because it reads the colour this constant is what places.
const FOLLOW_MARK: char = '▶';

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
/// numbers the text still gets **thirty-three**, so the gutter survives the case
/// the invariant is actually about.
///
/// **That number was thirty-four until 2026-08-15, and the figure is not the
/// interesting part.** [#164](https://github.com/breferrari/vigia/issues/164)
/// gave the sigil a clear column, which comes out of content, so every figure
/// here moved by one. The defect it exposed is that [`gutter_width`] was ruling
/// the numbers affordable against a prefix that no longer existed, so at the
/// narrowest pane keeping the gutter it enforced this floor at **23**. On a pane
/// with a scrollbar, which is the ordinary case for a diff taller than its
/// region, that is one width per digit count and all of them sit below 43,
/// inside the band I6 is named for. [`line_origin`] is what both sites read now.
///
/// **And nothing reddened, because this constant had no test.** It was a
/// threshold two expressions agreed about by hand, which is the wish
/// `CLAUDE.md` names rather than the invariant it reads as:
/// `render.rs::the_gutter_gives_way_before_the_text_does` samples 40 and 24 and
/// never the boundary, and `a_diff_taller_than_the_pane_keeps_its_line_numbers`
/// asserts the gutter does not vanish rather than that what survives clears this
/// floor. `crates/vigia/tests/legibility.rs::a_drawn_gutter_leaves_the_text_its_floor`
/// is the sweep that fails when it does not.
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
    /// **It outranks everything else wherever it fits at all**, which is why it
    /// holds the side `Painter::status_line` places first. The changed-file
    /// count at the other end of the row summarises a body that is on screen and
    /// can be recovered by counting; whether the pane is still live is
    /// recoverable from nowhere at all. So the count is what yields when the two
    /// cannot both fit, and that ordering matters most at exactly the widths
    /// where the body has nothing in it to count, which is the empty state this
    /// word exists for.
    ///
    /// **It is not the last thing standing on the row**, and the tidier claim was
    /// written here once and is false. The sides have independent budgets: this
    /// is all-or-nothing at its own width while the worktree name marks its edge
    /// at any width above zero, so a live watch draws the name alone from 5 to 7
    /// columns and this alone at 8 and 9. Widening a pane from 7 to 8 removes the
    /// name. Unchanged behaviour, recorded because there is no gate for the
    /// tidier version and could not be. The widths themselves are gated, by
    /// `tests/legibility.rs::the_header_degrades_at_the_widths_the_spec_records`,
    /// because a measurement that lives only in prose drifts from what it
    /// measured and this one already had.
    ///
    /// **It is never cut**, which is stricter than the marking rule the rest of
    /// the header follows: `wat›` is a state a reader cannot read, and unlike a
    /// path it has no half that identifies it. That is delivered by
    /// `Painter::put_right`, which drops a token whole rather than truncating
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
    /// diff is empty.
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
    /// The cell a step button is being held down on, when one is.
    ///
    /// **Feedback, and it is the reason a click feels registered.** A button that
    /// does not change when pressed reads as inert, and this one has a case where
    /// nothing else says otherwise: pressing *up* at the top of a diff moves no
    /// row, so without a lit cell the reader cannot tell the control from a
    /// decoration.
    ///
    /// A column and a row rather than a direction, because the renderer already
    /// knows where its buttons are and re-deriving which end was pressed would be
    /// a second copy of the geometry `Regions` exists to hand out once.
    ///
    /// `None` on every frame no button is held, which is nearly all of them. It
    /// costs no row, no rect and no wake: the frames it changes are frames the
    /// step was already painting.
    pub pressed: Option<(u16, u16)>,
    /// Which region's bar is being **dragged**, when one is.
    ///
    /// The region rather than its first row, which is what this carried until
    /// [#254](https://github.com/breferrari/vigia/issues/254): the row read as
    /// the cheaper shape because `scrollbar` is handed its own `Rect` and could
    /// compare `area.y`, and it is an identity only while the two regions are
    /// stacked. [`Hovered::Track`] records the rest of that argument.
    ///
    /// It is the same lifetime as [`Chrome::pressed`] and a different gesture: a
    /// press on a step button lights one cell, a press on the track lights the
    /// thumb it is moving.
    pub gripped: Option<Grabbed>,
    /// What the pointer is resting on, when it is on something a click acts on.
    ///
    /// **Acknowledgment before action, which is the whole of `SPEC.md` §11.2
    /// B10.** The two fields above answer *you are doing this*; this one answers
    /// *you are about to be able to*, which is most of what a pointer feels like
    /// on a modern surface and is the one thing this shell had no answer for.
    ///
    /// It is the quietest of the three marks by construction, because a hover
    /// can go **stale**: §11.1's clearing ladder retires it on the next motion
    /// and on `FocusLost`, and leaves a residual where a reader's pointer parks
    /// outside a pane the window still has focus on. A stale mark that costs a
    /// glance nothing is what pays for that, so a hovered button takes
    /// [`Theme::bar`] rather than [`Theme::bar_active`] and a press still wins.
    pub hovered: Option<Hovered>,
    /// Which bar is being scrolled and which way, when one is.
    ///
    /// **The one case where the bar answers something nobody is touching.** A
    /// reader scrolling with `j` or `d` gets the matching arrow lit for as long
    /// as the burst lasts, so the same mark means *this is moving, that way*
    /// whichever device asked. Negative is up, positive is down, `None` is at
    /// rest.
    ///
    /// **Which region, and it was missing until 2026-08-16.** The two bars move
    /// different things and answer different keys, so a bare direction lit the
    /// matching arrow on *both* at once, which is what 0.5.0 shipped.
    /// [`Chrome::gripped`] one field up had carried its region from the start and
    /// was correct throughout, which is why a drag lit only its own bar while a
    /// keypress lit both: the right shape was one field away and the newer mark
    /// did not copy it.
    ///
    /// **The decision was carry the region; the encoding was the region's first
    /// row, and only the encoding changed in
    /// [#254](https://github.com/breferrari/vigia/issues/254).** A top tells the
    /// two bars apart only while they are stacked, and a rail ends that, so what
    /// both marks carry now is the [`Grabbed`] the shell was already holding.
    pub scrolling: Option<(Grabbed, isize)>,
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
    /// Whether the masthead is drawn at all, which `m` toggles.
    ///
    /// **On [`Chrome`] rather than passed to [`Body::split`] separately**, which
    /// is where every other fact the layout needs about the reader already
    /// lives: `following` is here for the same reason, because the footer's own
    /// height depends on it.
    ///
    /// It is the one input to the body split that is not about the pane or the
    /// diff, and §11.1 licenses exactly that: the split refuses to move for a
    /// **transient** thing, so that a notice flickering cannot jog the reader's
    /// diff. A keypress is an instruction rather than a flicker.
    pub masthead: bool,
    /// Whether the gestures sheet is drawn over the pane, which `?` toggles.
    ///
    /// **Unlike [`Chrome::masthead`] this is not an input to the body split at
    /// all**, and that difference is the whole of `SPEC.md` §11.1's B12: the sheet
    /// composites over cells the regions have already drawn, so no rect moves and
    /// no row is spent. It is read by the painter last and by [`regions`] so a
    /// pointer can be told it is over one.
    pub sheet: bool,
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
/// `0.8ms frame   19MiB`, then the frame time alone, then nothing. These are the
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
/// **A worktree that draws no name gets the count and no separator**, which is
/// the same guard [`count_of`] applies to zero and [`empty_state`] applies to a
/// detached head: a separator is only owed where both facts exist.
/// `" · 3 changed"` joins a fact to nothing and promises a subject that is not on
/// the row, which is [#67](https://github.com/breferrari/vigia/issues/67)'s own
/// failure with the halves swapped.
///
/// **The test took four spellings to get right and each was wrong in the same
/// direction**, which is why the wrong ones are recorded rather than tidied away:
/// the progression is the lesson, not the answer.
///
/// | spelling | what it misses |
/// |---|---|
/// | `is_empty()` | names of zero-width characters: a zero-width space, a joiner, a bidi mark, a lone combining accent, a variation selector. Non-empty `String`s that draw nothing |
/// | `width_of(..) == 0` | names of whitespace, which *have* width and show nothing: a space, a no-break space, an ideographic space, a tab |
/// | `width_of(trim()) == 0` | names of control characters. `\u{1B}` measures **one** column and `trim` keeps it, but `ratatui` drops every grapheme containing a control before it reaches a cell |
///
/// Each class is a legal directory name on Linux and macOS, so each arrives
/// through `Worktree::short_name` rather than only through the public [`render`].
///
/// So the question was never "is this empty", nor "how wide is it", but **will
/// the layer that draws it keep anything**, and each earlier spelling asked a
/// question one layer too high. `len` is not width; width is not visibility; and
/// what unicode-width reports is not what the buffer agrees to write. Two
/// characters still escape and are left alone deliberately: `U+2800` and
/// `U+115F` draw a real glyph that happens to be blank, and whether a *font*
/// inks something is not a question this process can ask.
fn header_left(worktree: &str, branch: Option<&str>, files: usize) -> Vec<String> {
    let mut rungs = Vec::with_capacity(3);
    let count = count_of(files);

    // **The branch is drawn always since
    // [#158](https://github.com/breferrari/vigia/issues/158)**, where it was the
    // empty state's alone. It answers *which line of work*, which is the one
    // thing on this pane a reader cannot reconstruct from the body: the file list
    // says what changed and the diff says how, and neither says against what.
    //
    // **Its rung sits between the count and the name**, which is the order
    // §11.1's ladder already implies. The count goes first because the list
    // below repeats it. The name goes last because B3's empty state leans on it
    // to say which repository this is. The branch is in between: nowhere else on
    // screen, but the name is what identifies the pane.
    //
    // A detached HEAD carries `None` and the ladder is what it always was, which
    // is the same refusal `empty_state` makes one function down: `HEAD@abc123`
    // would put a commit id in a monitor that shows no commits.
    let named = branch.map(str::trim).filter(|branch| !branch.is_empty());

    // **A separator is owed only between two facts that are both there**, which
    // is [#67](https://github.com/breferrari/vigia/issues/67)'s rule and the
    // reason the name is measured rather than tested for emptiness: a worktree
    // called `a zero-width space` is a non-empty string that draws nothing, and joining it
    // would head the pane with a leading separator.
    //
    // **Built by joining what exists rather than by branching on what does**,
    // which is the correction #158 needed. Adding the branch as a third fact
    // first spelled the rungs as an if/else-if/else, and the new rung sat outside
    // the guard above: a nameless worktree on a branch drew `" · main"`, which is
    // exactly the seam that guard exists to prevent. One rule applied to every
    // rung cannot grow a second hole the next time a fact is added.
    //
    // `replace` takes any `Pattern`, and `FnMut(char) -> bool` is one on stable.
    // Noted because a reviewer read it as an unstable API: the `Pattern` *trait*
    // is unstable to implement and its impls have been stable to use since 1.0.
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
    if named.is_some() {
        rungs.push(join([name, named, None]));
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
/// **The branch left this line on 2026-08-17**
/// ([#158](https://github.com/breferrari/vigia/issues/158)), and the ruling that
/// put it here is what took it away. It was named because two agents on two
/// worktrees of one repository are otherwise identical on screen, and B3 gave the
/// empty state the job because nothing else on the pane was doing it. The header
/// draws the branch on every frame now, so this line drew it **twice** on the one
/// screen where both are visible.
///
/// The header is the right owner rather than this line: it is there on every
/// frame, where the empty state is there on one, and orientation is a header fact
/// beside the worktree's own name. It degrades with the rest of that ladder, so at
/// a width too narrow for it nothing names the branch, which is what already
/// happens to the changed count and the mode word.
///
/// A detached HEAD is still not invented anywhere: `HEAD@abc123` would put a
/// commit id in a monitor that shows no commits.
fn empty_state() -> String {
    NOTHING_CHANGED.to_owned()
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
    spark: &'r [u32; HISTORY_BUCKETS],
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

/// What a region's scrollbar is, before anything is drawn.
///
/// **One answer, read by the screen and by the pointer.** [`Painter::with_bar`]
/// and [`regions`] each used to spell `wide && rows > 1 && scrollable(..)` for
/// themselves, which was survivable while the two agreed by inspection and stops
/// being so the moment a bar has parts: a threshold two expressions keep by hand
/// is invisible to every test that reads what was drawn, because the drawn side
/// stays correct and only the *width at which* it changes drifts. A stepped bar
/// puts a track somewhere other than the region's own rows, and a pointer told
/// the wrong place would seek from a row the thumb is not on.
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
    ///
    /// Equal to the region itself unless the bar is [`Bar::Stepped`], so a caller
    /// that does not care which it has can ask unconditionally.
    ///
    /// Saturating for [`Painter::scrollbar`]'s stated reason: a private
    /// arithmetic whose safety rests on a floor checked in another function is an
    /// underflow waiting for the day someone calls it with the wrong shape.
    fn track(self, top: u16, rows: u16) -> (u16, u16) {
        match self {
            Self::Stepped => (top.saturating_add(1), rows.saturating_sub(STEP_ROWS)),
            Self::None | Self::Bare => (top, rows),
        }
    }

    /// The whole region a pointer is told about: its rows, and the part of them
    /// this bar leaves as track.
    ///
    /// **The one place the two are paired**, so [`Region`] can never be built
    /// holding one region's rows beside another's track. That is a bug with no
    /// symptom on screen, which is why it is closed by construction here rather
    /// than left to a gate.
    /// **Takes the region's rect rather than its rows**, since
    /// [#251](https://github.com/breferrari/vigia/issues/251), because the bar's
    /// column is now part of the same fact: `Painter::with_bar` narrows a region
    /// from **its own** right edge, so that edge is where this region's bar is,
    /// and a rect is what carries it.
    fn region(self, at: Rect) -> Region {
        Region {
            top: at.y,
            rows: at.height,
            left: at.x,
            width: at.width,
            track: self.track(at.y, at.height),
            // The rect's own right edge, which is where `Painter::scrollbar`
            // draws: it takes the region **before** `with_bar` narrows it and
            // draws down the right of what it was given. `None` where no bar is
            // drawn, so a pointer is never told about a column nothing occupies.
            bar: self.drawn().then(|| bar_column(at)),
        }
    }
}

/// The column a region's scrollbar is drawn in.
///
/// **One formula, read by the painter and by the pointer**, which is the same
/// split [`Body::areas`] closes for rows one type up. `Painter::scrollbar` draws
/// down the right of the region it is handed and [`Bar::region`] tells a pointer
/// where that is; written twice they agree by both being correct, and a future
/// edit to one desyncs the pointer from the screen on the bar's own column.
///
/// Saturating, so a zero-width region asks for a column instead of underflowing.
/// No such region reaches this today, because `regions` returns early on a pane
/// with no width and `affords_bar` refuses one too narrow, and that is a
/// property of two callers rather than of the arithmetic.
const fn bar_column(rect: Rect) -> u16 {
    rect.x.saturating_add(rect.width).saturating_sub(1)
}

/// Decide a region's bar from what it holds and what the pane can afford.
///
/// `wide` is whether the **pane** can afford a bar at all, which is one rule for
/// the whole screen so a reader never sees half a pair. Everything else is about
/// this region.
///
/// **A region shorter than [`MIN_TRACK`] gets nothing**, because [`scrollable`]
/// guarantees `span < of` and therefore `(span * rows) / of < rows`, so the thumb
/// equals the track exactly when `rows == 1`. Drawing it spends two of forty
/// columns on a mark that cannot move. Written against the constant rather than
/// as `rows > 1`, because the two are the same claim and only one of them moves
/// when the shortest track worth drawing does: a hand-spelled floor one screen
/// below the constant that names it is the drift [`STEP_FLOOR`] is written as a
/// sum to refuse.
///
/// **Buttons from [`STEP_FLOOR`] up, and never below it.** The buttons come out
/// of the track, so a region short enough to be all buttons would have a bar with
/// nothing between them to click; below the floor the bar is exactly what it drew
/// before there were any. Monotone by construction: one comparison against one
/// threshold, so a taller region can never lose them.
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
///
/// **The pane, less its own inset, less a caret column it may have, and less a
/// scrollbar column whether or not one is drawn.** The bar's presence is a fact about the
/// contents rather than the pane: [`scrollable`] asks whether what a region
/// holds outruns what it can show, so a seventh changed file or a diff one row
/// taller than the screen makes a bar appear and narrows the region under a
/// layout that was supposed to be a property of the pane.
///
/// Paying it unconditionally is the ruling [`affords_caret`] already made against
/// the identical hazard, and for the identical reason: a decision that flips
/// with the contents flips on the frame a reader is looking at. It costs two
/// columns of path on a pane with nothing to scroll, which is the trade
/// [`Columns`] already makes every time it reserves a slot no row can fill.
///
/// Written once because both regions need it and they must agree; [`Painter::body`]
/// takes no caret, so it passes zero.
///
/// **And less the pane's own inset, which is paid on the left alone.** That is
/// [#119](https://github.com/breferrari/vigia/issues/119) reconciled with the
/// ruling above it rather than layered on top: `SPEC.md` §11.1 rules there is *no
/// trailing reserve beyond the scrollbar column*, and that the two columns a row
/// stops short of the pane's right edge are the bar's rather than a margin. So
/// the inset does **not** buy a second set of blank columns on the right. It buys
/// the matching set on the left, and the pane comes out even at the top rung
/// because [`BAR_WIDTH`] and the widest **left half** of [`MARGIN_RUNGS`] are
/// both two. The half rather than the rung, because that table counts the whole
/// margin across both sides and [`inset_of`] is what this function charges: the
/// widest rung is four.
///
/// **The gate watches the other half, and the two sentences are about different
/// things.** What makes the pane look square at the top rung is the *left* half
/// matching [`BAR_WIDTH`], which is the paragraph above. What makes charging the
/// margin once *correct* is the **trailing** half never outgrowing that reserve,
/// because the reserve is what stands in for it: a right-hand margin wider than
/// two columns would no longer be covered and §11.1's no-trailing-reserve ruling
/// would have to be re-decided. So
/// `the_inset_never_outgrows_the_scrollbars_reserve` asserts the trailing half.
///
/// This sentence claimed the leading half until round 3 of #119's audit, which is
/// the same defect the round before had just fixed **in the test**, restated one
/// layer up in the prose that explains it. It is falsifiable and was falsified:
/// a top rung of `(80, 5)` gives a left half of three and the gate stays green.
const fn planning_width(pane: u16, caret: u16) -> u16 {
    pane.saturating_sub(BAR_WIDTH as u16)
        .saturating_sub(inset_of(pane))
        .saturating_sub(caret)
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

/// Narrow `right` past a slot `width` columns wide, drawn or not.
///
/// **Called unconditionally, which is the ruling rather than a convenience.** A
/// slot is subtracted whether or not this row filled it, because a row that
/// closed the gap it left would slide every element outside it and pull the row
/// out of line with its neighbours, which is exactly what [`Columns`] exists to
/// prevent. Guarding the narrowing would make that true only by way of
/// [`reserved`]'s zero case, where here it is what the code says.
fn past(right: &mut Rect, width: usize) {
    right.width = right.width.saturating_sub(reserved(width) as u16);
}

/// Whether this file has any heat strip to draw at all.
///
/// Named rather than inlined into [`heat_at`]'s guard, so that "has anything to
/// draw" is one predicate wherever it is asked. It was briefly asked twice, when
/// a column was reserved only where some drawn row could fill it; that rule is
/// gone and every slot is reserved from the pane, so this has one caller today.
fn has_heat(buckets: &[HeatBucket; HEAT_BUCKETS]) -> bool {
    buckets.iter().any(|bucket| bucket.total() > 0)
}

/// Columns one half of the counts cell occupies, whatever that half says.
///
/// **A constant, and that is the whole of [#77](https://github.com/breferrari/vigia/issues/77)'s
/// second half.** Sizing the field to the widest count in the drawn window made
/// the layout a function of the *contents*, so scrolling a list until a busy
/// file entered the window widened the field and slid every heat strip and
/// sparkline on every row. `assets/preview.svg` right-anchors `+N` at one `x`
/// and `-M` at another no matter what any row says, and a fixed frame is what
/// makes that possible: the slots are a property of the pane, so nothing a
/// reader scrolls past can move them.
///
/// **Five, and it does not degrade, because five is the narrowest width at which
/// the abbreviation is total.** A narrower rung was tried and shipped a wrong
/// number: at three columns [`churn_of`] has two characters to work with, and a
/// 250-line change has no truthful form in two characters, so the search fell
/// through to the thousands unit and drew `+0k`. `+0M` for 999,999 likewise. It
/// was reachable at exactly forty columns, the width I6 is named for, which is
/// the worst place in the tool to round a number to zero.
///
/// At four characters every `u32` has a form and none of them is a lie: `9999`
/// plain, `10k` to `999k`, `1M` to `999M`, `1G` to `4G`. A field wide enough for
/// `+4294967295` would instead spend eleven columns a row forever on a number no
/// file reaches.
///
/// What it cost when it landed was the sparkline between 36 and 39 columns
/// (38 to 41 against today's [`planning_width`], which moved every boundary two
/// columns after this was measured), where the wider
/// counts field no longer leaves room for it. That is the honest trade: a
/// glance element at four widths against a number that says zero when it means
/// two hundred and fifty.
const COUNT_CELL: usize = 5;

/// Every shape a file row's right-hand side may take, widest first.
///
/// **Each row gives up exactly one thing against the one above it and gains
/// nothing**, which is what makes narrowing monotone: widening a pane can never
/// remove an element. Read down the table for the drop order, which is the
/// ladder `SPEC.md` §11.1 states: the sparkline's resolution is the first luxury
/// to go, then the strip's, then the sparkline entirely, then the strip, then
/// the pulse, and the counts last, because they are the row's content rather
/// than a signal drawn beside it.
///
/// Two rungs have left this table and neither moved a boundary under it, which
/// is a property of where they sat rather than luck. The counts' *width* was one,
/// and it is gone because [`COUNT_CELL`] no longer has a narrow rung to give up:
/// every row here carries the same cell. The pulse's *label* was the other, and
/// it opened the ladder, so removing it only removed the widest layout: a
/// layout's width is the sum of its own slots, and none of the six below it
/// changed.
///
/// **Nine entries since [#234](https://github.com/breferrari/vigia/issues/234),
/// and the drop order stated above is now true from top to bottom.** #161 had
/// amended it at the top end: the sparkline had no rung above its widest to give
/// up, so the step below the top gave up the *strip's* resolution instead. The
/// sparkline has one now, so the two elements alternate all the way down and the
/// amendment retires with the reason for it.
///
/// **The rungs above [`SETTLED`] are added and nothing below moves, by
/// construction rather than by a sweep.** A new layout is wider than `SETTLED` by
/// definition, and [`Columns::plan`]'s share is floored at `SETTLED`'s own width,
/// so every width that had a layout keeps exactly the one it had. That is the
/// mechanism; `tests/legibility.rs::the_glance_columns_collapse_in_one_order` is
/// the evidence, and the two are not the same claim.
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

/// The widest layout that shipped before a rung was added above it.
///
/// **What makes "no boundary below the new rung moves" true by construction
/// rather than by a swept comparison.** [`Columns::plan`]'s share clamp is
/// floored at this layout's width, so every layout from here down keeps exactly
/// the budget it had, whatever the share is set to and whatever rungs are added
/// on top. The gate that sweeps it is evidence; this is the mechanism.
///
/// **By value, and used *as* the table's entry, so an index cannot rot.** This
/// was `const SETTLED_RUNG: usize = 1` and the number is the hazard: inserting a
/// row above index 1 silently moves the floor onto the generous rung, which then
/// arrives at the width where it merely fits and the published picture becomes
/// false, with only a distant gate catching it and blaming the wrong thing.
/// Written out here and referenced from [`ROW_LAYOUTS`], the two cannot disagree
/// however the table is edited.
const SETTLED: Columns = Columns::new(COUNT_CELL, PULSE_RUNGS[0], HEAT_RUNGS[1], SPARK_RUNGS[1]);

/// The share of a row the glance elements may take, above the settled ladder.
///
/// **Two questions, not one, and the table only ever answered the first.** Below
/// [`SETTLED`] the question is *what survives*: a narrowing pane drops
/// elements until the path is safe, and [`ROW_FLOOR`] is the floor that decides
/// it. Above it the question is *what is worth spending*, and "does it fit" is
/// the wrong test, because a fixed-sum table takes a rung the instant it fits.
/// Twenty-four slices fit inside a seventy-one column pane, which is narrower
/// than the 109-column render `assets/preview.svg` is measured from, so without
/// this the widest strip would arrive at a width where the picture says it does
/// not exist.
///
/// **Two in five, checked against [#161](https://github.com/breferrari/vigia/issues/161)'s
/// own targets rather than against that constraint.** That issue asked for
/// twenty-four slices near 140 columns and forty-eight near 200; this rule puts
/// them at **134** and **194**, having been derived from neither. A rule that
/// reproduces both numbers it was not fitted to is a rule. Half the row was the
/// other candidate and it clears the picture by a single column, which is a
/// coincidence rather than a margin.
const GLANCE_NUMER: usize = 2;
/// The denominator of [`GLANCE_NUMER`]'s share.
const GLANCE_DENOM: usize = 5;

/// Columns the glance elements may spend on a row this wide, above the settled
/// ladder.
///
/// Floored by the division, so the share is never rounded **up** into a column
/// the path was keeping.
const fn generous_of(width: u16) -> usize {
    width as usize * GLANCE_NUMER / GLANCE_DENOM
}

/// Columns a whole counts cell of `cell`-wide halves occupies, its space included.
const fn counts_width(cell: usize) -> usize {
    if cell == 0 { 0 } else { cell * 2 + 1 }
}

/// One half of a counts cell, right-aligned by its caller into [`COUNT_CELL`].
///
/// **Magnitude gives way to a shorter form rather than to more digits**, which
/// is the rule the status bar's frame and memory cells already follow when they
/// draw `>1s` and `>1GiB`. A file with more than 9,999 added lines is a
/// generated one, and whether it was 15,032 or 15,036 tells a reader nothing the
/// `15k` does not, while the extra column would move the strip beside it.
///
/// **Total for every `u32` at [`COUNT_CELL`], and the totality is the whole
/// argument for that width.** Four characters cover `9999` plain, `10k` to
/// `999k`, `1M` to `999M` and `1G` to `4G`, and every step up happens before the
/// step below runs out, so no value falls between two units.
///
/// The zero guard is what a narrower cell taught. With two characters a
/// 250-line change has no truthful form, and the search fell through to the
/// thousands unit and returned `+0k`: a number that says none where it means two
/// hundred and fifty. Unreachable at five columns, kept because the failure is
/// silent and its cost is one comparison.
///
/// The loop walks units rather than branching on magnitude, so a change to
/// [`COUNT_CELL`] cannot leave one unit unreachable.
///
/// **Takes no width.** It had one while the cell was a two-rung ladder, and kept
/// it for a while after the ladder went, which left two branches below that no
/// caller could reach and no gate could cover: at the only width ever passed,
/// the zero guard never fires and the fallthrough never runs. Reading
/// [`COUNT_CELL`] directly is what makes the totality argument above checkable
/// against the constant it is about.
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
///
/// Named `Half` because [`count_of`] one screen up already means the header's
/// changed-file count, and because "one half of a counts cell" is what
/// [`churn_of`] and [`Columns::cell`] have called this since #77 split the pair.
///
/// The two travel together so that "what a counts cell says" and "how it says
/// it" cannot drift apart into two functions taking the same input, which is the
/// shape [`counts_of`]'s own doc exists to refuse.
struct Half {
    /// `+42`, `-7`, or nothing where there is no line diff to count.
    text: String,
    /// [`Theme::added`], [`Theme::removed`], or [`Theme::chrome_dim`] where this
    /// half has nothing to say.
    ink: Style,
}

/// The two halves of a file row's counts cell, or empty strings when there is no
/// line diff to count.
///
/// **Two, because the picture draws two.** `assets/preview.svg` right-anchors
/// `+N` at one `x` and `-M` at another, which is what lets a reader run an eye
/// down the additions of three files and compare them. One field with the pair
/// inside it aligns only whichever end it is anchored to and leaves the other
/// ragged, which is the same complaint [#77](https://github.com/breferrari/vigia/issues/77)
/// makes about the row as a whole, one element in.
///
/// **A half takes its diff colour where it has something to say, and the row's
/// dim grey where it does not**, which is `SPEC.md` §5.1's third departure
/// closed on its colour half ([#157](https://github.com/breferrari/vigia/issues/157)).
/// The picture has drawn `+42` in `.grn` and `-7` in `.red` from the start, and
/// §5.3 licenses the loan in the same sentence that licenses the footer's follow
/// marker: green and red are lent out "only where they restate the same fact".
///
/// **A `-0` restates none, which is why the rule is value-dependent rather than
/// positional.** The picture states it too and it is easy to read past: the
/// mockup's third row draws `+2` in `.grn` beside a `-0` in `.faint`. What it
/// protects is the reading the element exists for. On a worktree an agent is
/// only adding to, an unconditional red would put red on every row, and a
/// reader's question — is anything being removed — would have no answer left on
/// screen. The two halves are still drawn or dropped together
/// ([`Columns::cell`]); it is only their ink that is per half.
///
/// Which grey is deliberately **not** settled here. The picture's is `.faint`
/// `#6e7681` where [`Theme::chrome_dim`] is `#8b949e` on `dark`, and that is a
/// question inside one role rather than about it; §5.1 records it as open, and
/// the shell's greys already depart from the picture's classes elsewhere.
///
/// Named rather than inlined into [`Painter::file_row`] so that "what a counts
/// cell says" is one definition. It was lifted out to keep a *measurement* and a
/// drawing in agreement, back when the columns were sized from the widest cell
/// among the drawn rows; that design is gone and nothing measures now, so what
/// the split still earns is a name, a place for the empty case to live, and the
/// colour rule above stated once for both halves.
fn counts_of(churn: Option<(u32, u32)>, theme: &Theme) -> (Half, Half) {
    let half = |sigil: char, lines: u32, ink: Style| Half {
        text: churn_of(sigil, lines),
        ink: if lines == 0 { theme.chrome_dim } else { ink },
    };
    // Beside `half` rather than inside the arm, and the two are the same rule
    // reached by two routes: a half says nothing when its own count is zero, and
    // both halves say nothing when there is no line diff behind them. `Half` is
    // not `Clone`, so this is a closure rather than one value used twice.
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

/// Where each glance element sits on **every** file row of one region.
///
/// `assets/preview.svg` puts the same element at the same `x` on every row, and
/// [`Painter::file_row`] used to right-pack, so each element's position was a
/// function of the widths of the elements outside it on that row. Three
/// sparklines then failed to read as one small-multiples chart, which is the
/// thing a file *list* exists to be ([#77](https://github.com/breferrari/vigia/issues/77)).
///
/// So the ladder runs **once for the region** and every row draws into the slots
/// it produced, keeping a slot a row cannot fill rather than letting its
/// neighbours slide. The sparkline's unfillable slot is no longer blank when it
/// gets there: [#78](https://github.com/breferrari/vigia/issues/78) fills it
/// with the track. The heat strip's still is, and [`heat_at`] says why the two
/// differ.
///
/// **Decided from the pane and never from the contents**, which is the half a
/// first attempt got wrong and which only running the tool showed. Sizing the
/// counts field to the widest count in the drawn window made the layout a
/// function of the rows, so scrolling a list until a busy file entered it
/// widened the field by six columns and slid every heat strip and sparkline on
/// every row. The columns held *within* a window and moved *between* windows,
/// which is the same defect one axis over and is worse for being intermittent.
/// `assets/preview.svg` never had it: its positions do not depend on what any
/// row says. So every slot here is a constant or a rung of the pane's width, and
/// nothing a reader scrolls past can move anything.
///
/// **One of these per region, never one per screen**, and since
/// [#173](https://github.com/breferrari/vigia/issues/173) the two usually come
/// out the same. The list insets by [`caret_gutter`], which is **zero from
/// forty-three columns up**, because the caret stands in the pane's own margin
/// there instead of taking a column from the row; below that the ladder lends
/// nothing and the gutter is one. So the regions share a width at every width
/// I6 is concerned with, and differ by a single column on the narrow pane.
/// [`planning_width`] has both pay the scrollbar column whether or not one is
/// drawn, which was the other difference and is gone.
///
/// Kept per region rather than folded into one, because the two are still
/// *entitled* to differ and `SPEC.md` §11.1 still rules that they need not align
/// glyph for glyph. What changed is that they now do, at the widths that matter.
///
#[derive(Clone, Copy)]
struct Columns {
    /// Columns each half of the counts cell occupies, or zero where the pair
    /// does not fit at all. The two halves stand or fall together, because `+42`
    /// with no `-7` beside it reads as a total rather than as half a pair.
    cell: usize,
    /// The pulse rung reserved on every row, or empty when none fits.
    ///
    /// **A column after all, and the first attempt was wrong twice over.** It was
    /// dropped from this set for costing fourteen columns at forty, but that was
    /// a bug in the choosing rather than a fact about the pulse: the rung was
    /// picked without counting the gap it needs, so the label was taken at widths
    /// where it left nothing for the strip. Chosen with [`reserved`] it degraded
    /// to the one-column mark instead, which is what `PULSE_RUNGS` ruled that it
    /// should. Drawn from the path's room instead, it became the *last* thing on
    /// the row to survive narrowing, where its own doc says it is among the
    /// first. Kept as the history of a slot that now has one rung to reserve:
    /// the fourteen-column label went in 2026-08-03, and the argument that it
    /// belongs to *this* set rather than to the path's leftovers is the half of
    /// that episode which outlived it.
    ///
    /// Reserved whether or not this row is pulsing, like every other slot here,
    /// because a pulse lasts one tick and a slot that came and went with it would
    /// reflow the row on the frame a reader is most likely to be looking at.
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
    ///
    /// **Nothing here reads a row**, and that is the ruling rather than an
    /// economy: a slot whose width depended on the rows would move whenever the
    /// rows did, which is what scrolling a list does.
    ///
    /// **Every slot is reserved whether or not anything can fill it**, including
    /// the sparkline in a region where no file has a history yet, which at launch
    /// is every file ([#78](https://github.com/breferrari/vigia/issues/78)). The
    /// alternative was tried: reserving only what some drawn row could fill moves
    /// every column on the tick the first file is written, which is precisely the
    /// moment a reader is looking at the screen.
    ///
    /// **One table rather than four searches, and that is what makes it a
    /// frame.** Allocating element by element in priority order is what the rest
    /// of this file does, and for a *shared* layout it produces a ladder that
    /// oscillates: swept across every width, the greedy form lost the sparkline
    /// at 37 columns, got it back at 40 and lost both glance elements at 41,
    /// because each element took the widest rung it could afford and starved
    /// whatever came after. Widening a pane must never take something away, and
    /// greedy allocation over variable rungs cannot promise that.
    ///
    /// Those three widths were measured when the counts cell still had a narrow
    /// rung, so they do not reproduce against [`COUNT_CELL`] today and are kept
    /// as the evidence that retired the greedy form rather than as a claim about
    /// the current table. What survives the change is the shape: greedy
    /// allocation over variable rungs oscillates, and a written-out table cannot.
    ///
    /// So the layouts are written out, widest first, and **each step gives up
    /// exactly one thing and never gains any**. That makes the whole ladder
    /// monotone by construction rather than by argument, which is the property a
    /// reader dragging a pane edge actually notices.
    /// **`glyphs` enters here rather than changing the table**, and that is what
    /// keeps the ladder one ladder. The rungs are slices of the *window* and
    /// their order is unchanged at every rung of the glyph ladder; what a denser
    /// glyph moves is the *width* each one costs, so the same six steps are
    /// simply reached earlier. Monotonicity is therefore still by construction
    /// rather than by argument, and it holds separately at each rung, which is
    /// what `tests/legibility.rs` sweeps rather than assumes.
    ///
    /// **Two floors, because the table answers two questions**
    /// ([#161](https://github.com/breferrari/vigia/issues/161)). [`ROW_FLOOR`] is
    /// what survival costs and it decides every rung the tool shipped with.
    /// [`generous_of`] is what generosity costs and it decides only the rungs
    /// above [`SETTLED`], because "does it fit" stops being the right test
    /// once a row has room to spare: a fixed-sum table takes a rung the instant
    /// it fits, and the widest strip fits inside a pane narrower than the one the
    /// published picture is measured from. Both constants carry the argument.
    ///
    /// **Monotone still, and still by construction.** The share grows with the
    /// width it is taken from, so a rung once affordable stays affordable, and
    /// the `max` against the settled layout means no width that had a layout can
    /// lose it. Widening a pane cannot remove an element, which is the one
    /// promise this whole function exists to keep.
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

/// How busy one thing is, against whatever the caller measures it against.
///
/// **Scale-agnostic, and it was file-scoped until
/// [#196](https://github.com/breferrari/vigia/issues/196)**, which gave it a
/// second caller measuring something else. The denominator belongs to the caller
/// and the two differ on purpose: [`heat_at`] passes this **file's** busiest
/// slice, because a strip is read *across* one row to find where the work is,
/// and [`spark_of`] passes the busiest bucket **anywhere on screen**, because a
/// sparkline is compared *down* a list to find which file is busiest. Each states
/// its own choice; this type states none.
///
/// Three, because that is what the depth ladder can draw, which is
/// [`Theme::heat_added`]'s own reasoning. `assets/preview.svg` ramps across
/// three greens and asks for it, and the sentence that used to stand here made
/// the strip *"the one element whose intensity the picture actually specifies"*,
/// which was never true: the picture ramps its sparkline across five greens too,
/// and #196 is the row that noticed. It used to be two: sixteen
/// foreground-only colours hold a normal and a bright of each hue and no third
/// stop, so the ramp was as wide as the palette could draw rather than as wide as
/// the picture asked for.
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
    /// Which band `total` falls in, against the `busiest` the caller chose.
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
    if width == 0 || !has_heat(buckets) {
        return Vec::new();
    }

    // **Saturating, because a projection must not be able to kill the pane.**
    // Folding a group of `u16` counts with `sum()` overflows and panics in debug
    // for a file busy enough to fill them, and a monitor that dies on a file is
    // the failure `SPEC.md` §11.1 rules out for `core.safecrlf` one paragraph
    // over. Saturating loses nothing that is drawn: the sum is only ever
    // compared against the busiest group to pick a band, and a group at
    // `u16::MAX` is the busiest either way.
    //
    // Reachable at ordinary widths rather than in theory. The six-slice rung
    // groups two buckets, and #77's layout table makes that rung the one a
    // forty-column pane picks.
    let group = HEAT_BUCKETS / width;
    let summed: Vec<HeatBucket> = buckets
        .chunks(group)
        .map(|chunk| {
            chunk
                .iter()
                .fold(HeatBucket::default(), |sum, bucket| HeatBucket {
                    added: sum.added.saturating_add(bucket.added),
                    removed: sum.removed.saturating_add(bucket.removed),
                })
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

/// One bucket of a drawn sparkline: what it says, before how it looks.
///
/// **[`Heat`]'s small private cousin, and private for the reason `Heat` is
/// not.** `Theme::heat` resolves a kind-by-band cross product and is public API
/// because the theme has to name every one of those styles; the distinction here
/// is one bit and it never leaves this file, so an enum costs no public surface
/// and no allocation, being a fixed-size array on the stack either way. The
/// *payload* is a glyph rather than a bit, which the last paragraph is about.
///
/// What it buys is what [`Painter::scrollbar`] gets from its `filled` boolean:
/// **the style is chosen from the variant rather than read back off the
/// glyph.** [`spark_of`] briefly returned bare `char`s and the painter decided
/// the style by testing against `SPARK_TRACK`, which worked only while that
/// glyph stayed outside [`crate::glyphs::SPARK_RAMP`] — a convention a test defends rather than
/// one the compiler does. It also had a live failure case in the other
/// direction: on the `scale == 0` path every bucket draws the track *whatever its
/// count says*, so a painter branching on the count instead would have drawn a
/// track glyph in the bar's colour. Neither spelling of "derive one from the
/// other" is safe, and deriving both from a third thing is.
///
/// **What this does not do is constrain the payload**, and the difference is
/// worth stating rather than implying: `Written(SPARK_TRACK, ..)` is
/// constructible and would draw the track glyph in the bar's style. What rules it
/// out is that [`spark_of`] is the only producer and fills it from
/// [`Glyphs::glyph`], not the type. An index would move the same hole one level
/// down rather than close it.
///
/// **The payload is a pair since [#196](https://github.com/breferrari/vigia/issues/196)**,
/// because the sparkline ramps now and height alone no longer decides the ink.
/// The [`Band`] rides here rather than being recomputed by the painter for the
/// paragraph above's exact reason: a drawer deriving the band back out of the
/// glyph's rung would be reading one derivation out of another.
///
/// **And that derivation is not merely indirect, it is lossy**, which is worth
/// stating because it turns a stylistic argument into an arithmetic one. The
/// glyph is a rung of an eight-step ramp and [`Band::of`] splits at a third and
/// two thirds. Rung six covers the ratio interval `(0.625, 0.75]`, and the `Hot`
/// boundary at `0.667` falls **inside** it, so one rung answers to two bands and
/// no function of the glyph can tell which. Recomputing would quantise the colour
/// to the height ramp and change what is drawn.
#[derive(Clone, Copy)]
enum Bucket {
    /// Nothing was written in this cell's slice of the window.
    ///
    /// **A cell rather than a bucket since the ladder landed**, the same
    /// correction [`Bucket::Written`] carries: at a dense rung one of these
    /// stands for *two* buckets, and it is `Empty` only when both are.
    Empty,
    /// Written: the glyph [`Glyphs::glyph`] spelled it with, and its stop of the
    /// ramp.
    ///
    /// **The glyph is no longer always an eighth-block**, which this said until
    /// the ladder landed: at a dense rung it is a packed 2x4 cell standing for
    /// *two* buckets, so nothing may read a single bucket's height back out of
    /// it. That is the same rule the paragraphs above give the [`Band`], now
    /// true of the character as well.
    Written(char, Band),
}

impl Bucket {
    /// What this bucket draws and what it is drawn in, together.
    ///
    /// **The rung reaches only the track's glyph, never its style**, and that
    /// split is this type's whole contract kept rather than bent: the *variant*
    /// still decides the ink, so nothing here reads a style back off a
    /// character. What a rung changes is how "nothing happened" is spelled,
    /// because `_` sits under an eighth-block ramp and has no meaning inside a
    /// 2x4 cell, where the baseline row is the track. [`Glyphs::glyph`] is total
    /// over both, so level zero is the track at whichever rung is drawing and
    /// this arm needs no rung of its own to ask about.
    fn drawn(self, theme: &Theme, glyphs: Glyphs) -> (char, Style) {
        match self {
            Self::Empty => (glyphs.glyph(0, 0), theme.spark_track),
            Self::Written(glyph, band) => (glyph, theme.spark_at(band)),
        }
    }
}

/// Which level of `levels` a count reaches, against the busiest count on screen.
///
/// **One implementation, and since #232 it is the only one.** The band used to
/// reach this through a rows-shaped adapter and then build its own glyph, so the
/// rule that keeps one write from drawing as empty existed twice and could move
/// in one element and not the other. Both elements now scale here and spell the
/// result through [`Glyphs::glyph`], which is what lets the band carry a baseline
/// and two sub-columns per cell without a second drawer.
///
/// Rounded **up**, so any non-zero count reaches at least the first level: a
/// write that drew as empty would be a write the element failed to report.
///
fn level_to(total: u32, scale: u32, levels: usize) -> usize {
    if scale == 0 || total == 0 || levels == 0 {
        return 0;
    }
    let scaled = (total as u64 * levels as u64).div_ceil(scale as u64) as usize;
    scaled.clamp(1, levels)
}

/// A path's window, re-projected onto `rung` buckets and packed into the cells
/// those buckets are drawn in.
///
/// **Only the first `spark_cells(rung, glyphs)` entries are meaningful.** At a
/// dense rung one cell carries two buckets and the tail stays `Empty`, so a
/// caller iterating the whole array paints spurious track cells. The count is
/// derivable from `rung` and `glyphs` rather than returned, because returning it
/// made every caller destructure a pair to learn something it already knew.
///
/// **A narrower rung re-projects rather than dropping**
/// ([#234](https://github.com/breferrari/vigia/issues/234)): `rung` divides
/// [`HISTORY_BUCKETS`], and each drawn bucket is the sum of the source buckets
/// under it, so every rung covers the whole window at a lower resolution. It took
/// a suffix until then, which drew the newest half of the window beside a churn
/// band drawing all of it. `heat_at` is the same projection one element over, and
/// `SPEC.md` §11.1 now rules them together.
///
/// `scale` is [`scale_of`]'s figure over every bucket of every **tracked path**,
/// so two rows drawn side by side can be compared by height and a row scrolling
/// into view cannot rescale the ones already there. **It carries one figure per
/// rung**, because a drawn bucket summing `group` source buckets has to be
/// measured against what `group` source buckets are worth, and that is not the
/// finest figure multiplied: [`Scale`] and `vigia_core::SPARK_GROUPS` carry the
/// measurement that settled it. A bucket with
/// anything in it is never blank: it takes the lowest block, because "one write"
/// and "no writes" are the distinction the strip exists to make and rounding the
/// first down to nothing would erase it. A bucket with nothing in it takes
/// `SPARK_TRACK`, which is what keeps that distinction a matter of *shape*.
///
/// **Total, where this returned an `Option` before
/// [#78](https://github.com/breferrari/vigia/issues/78).** There is no longer a
/// row that draws no strip, so there is no absence for a caller to handle, and
/// the `has_spark` predicate that answered "is there anything here at all" went
/// with it. What is left of that guard is the `scale == 0` line below, which is
/// the one state where the ramp has no denominator: an empty store, which is
/// every launch.
fn spark_of(
    buckets: &[u32; HISTORY_BUCKETS],
    rung: usize,
    scale: Scale,
    glyphs: Glyphs,
) -> [Bucket; HISTORY_BUCKETS] {
    let mut drawn = [Bucket::Empty; HISTORY_BUCKETS];
    // Nothing anywhere on screen has been written, so every bucket is empty and
    // the division below has no denominator. Returning here rather than guarding
    // inside the loop, because the two say different things: this is "there is
    // no scale yet", and the loop's `busiest == 0` is "this cell is empty on a
    // screen that has one".
    //
    // A rung of zero is the row that draws no strip at all.
    if rung == 0 {
        return drawn;
    }
    // **Total for a rung wider than the window, and that is a release hazard
    // rather than tidiness.** `ROW_LAYOUTS` is a hand-written table and
    // `Columns::new` takes a bare `usize`, so a rung above [`HISTORY_BUCKETS`]
    // divides to a group of **zero**, and `slice::chunks` opens with a plain
    // `assert!` that ships: `[profile.release]` sets `panic = "abort"`, so the
    // process would go down without restoring the terminal, which is I8's failure
    // reached by the one path that skips its handler. `Painter::file_row`'s clamp
    // used to be the guard for exactly this and can no longer reach it, because
    // the division now happens here, one function earlier. It still clamps its
    // own slice, for the half of the hazard that is this array's length.
    let group = HISTORY_BUCKETS / rung;
    if group == 0 {
        return drawn;
    }
    let yardstick = scale.at(group);
    if yardstick == 0 {
        return drawn;
    }
    // **Summed into the rung first, then packed into cells**, which is two
    // projections rather than one and they answer different questions: the first
    // is how much of the window one drawn bucket covers, the second is how many
    // buckets one terminal cell can hold. `SPARK_RUNGS`'s `const` block is what
    // makes the division exact, so no group is short and the newest column covers
    // the same span as the rest.
    //
    // Saturating, for `heat_at`'s reason verbatim: a group summing past its type
    // is already at the top of the ramp, and wrapping would draw the busiest file
    // in the worktree as the quietest.
    //
    // **`History::repeak` groups the same buckets the same way**, one crate over,
    // to compute the denominator this numerator is divided by. The two are
    // deliberately not one function: that one accumulates a sum and a non-empty
    // count without materialising the groups, this one materialises them into a
    // fixed array so a drawn row allocates nothing. What holds them together is
    // `tests/rows.rs::a_recorded_tick_reaches_the_drawn_sparkline`, which pins the
    // figures the store answers with, so a grouping that moved here and not there
    // reddens rather than quietly redrawing every height on screen.
    let mut summed = [0u32; HISTORY_BUCKETS];
    for (at, chunk) in buckets.chunks(group).enumerate() {
        summed[at] = chunk.iter().copied().fold(0, u32::saturating_add);
    }
    // **One loop at every rung, and the density is the only thing that moves.**
    // A block cell holds one bucket and a dense cell holds two, which is a
    // chunk width rather than a second algorithm: the levels, the band and the
    // track rule are identical either side, and the glyph is
    // [`Glyphs::glyph`]'s business.
    for (cell, pair) in drawn
        .iter_mut()
        .zip(summed[..rung].chunks(glyphs.density()))
    {
        let (left, right) = (pair[0], pair.get(1).copied().unwrap_or(0));
        // **The busier of the pair decides both**, and at the block rung the
        // pair is one bucket so this is that bucket. A `Cell` carries a single
        // `Style`, so two buckets sharing one cannot hold two bands; taking the
        // busier is the direction that never understates activity, where the
        // flatter would hide the thing the element exists to show. Their
        // *heights* stay separate, which is the channel `SPEC.md` §5.1 gives
        // this element.
        let busiest = left.max(right);
        if busiest == 0 {
            continue;
        }
        // **Through [`level_to`], which is where the rounding rule lives.**
        // Written twice, the rule that keeps one write from drawing as empty
        // could move at one rung and not the other.
        let level = |count: u32| level_to(count, yardstick, glyphs.levels());
        // **Against the same `scale` the heights are scaled from**, which is
        // `scale_of`'s figure over every bucket on screen rather than over this
        // file. Height and
        // colour then say one thing at one scale, where two denominators would
        // let a row read tall and cool at once.
        let band = Band::of(busiest, yardstick);
        *cell = Bucket::Written(glyphs.glyph(level(left), level(right)), band);
    }
    drawn
}

/// The widest rung of `ladder` that fits in `room`.
///
/// Ladders are written widest first, so this is the first that fits. Every
/// ladder *this* takes ends in an empty rung, so the fallback is unreachable
/// rather than a silent default. [`header_left`] is the one that does not, which
/// is why it goes through [`widest_fitting_or_last`] instead.
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
/// **It is reached from two call sites, both through [`Painter::status_line`],
/// and the doc used to credit one**, which is worth stating because the missing
/// one is what makes the `or_else` arm look like scaffolding.
/// The header's left ends in the worktree name, which marks its edge instead of
/// being dropped, so falling through to the empty string would delete the one
/// fact on the row a reader cannot recover by looking at the body. **And the
/// footer's left is a notice**, or the hints rung [`Footer::plan`] already
/// resolved, passed as a single-rung ladder. For the notice this is the only
/// thing standing between an over-long one and a blank row; for the hints the
/// arm is unreachable by construction, because the plan fitted them first.
/// Delete it as one-caller scaffolding and notices vanish at narrow widths.
///
/// So the two are not interchangeable, and the difference is which failure they
/// produce. Used on a ladder that ends in nothing this returns that empty rung,
/// which is [`widest_fitting`]'s own answer; used the other way round, a token
/// too long for its room would vanish rather than be marked.
///
/// `unwrap_or` is reachable only for an empty `ladder`, which no caller passes.
/// It is what makes this total rather than a case anyone has to think about.
///
/// **`last` rather than `first` is unpinned by any test, deliberately.** Every
/// ladder here is prefix-nested — the header's bare name is a prefix of its
/// clause, and the footer's is one rung — so [`Painter::put_marked`] draws the
/// same columns either way and no test can tell them apart. The single input
/// that separates them is a worktree name that draws nothing, where `last`
/// yields a bare mark and `first` a marked count, and neither is obviously the
/// better screen. A gate here would pin a coin toss rather than a ruling, so
/// what is recorded is that the choice is open and why nothing fails.
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
    /// Whether a rule is drawn above the footer's text.
    ///
    /// **The same mark the list already puts over the diff**, asked for from a
    /// live pane: the bottom bar is chrome sitting under content with nothing
    /// saying so, where every other boundary on this screen is drawn. Counted
    /// into [`Footer::height`] rather than into [`Footer::rows`], because `rows`
    /// is what decides the one-line-or-two ladder and a rule is neither line.
    ///
    /// It yields on a short pane like every other piece of furniture here: a rule
    /// that cost the diff its last row would be chrome announcing a region rather
    /// than separating one.
    rule: bool,
    /// The frame-time and memory cells, already narrowed to what is left after
    /// the hints and the state have taken theirs.
    ///
    /// Owned where its three siblings are borrowed or copied, because these are
    /// formatted numbers with nowhere to live in the [`Chrome`]. Bounded by
    /// [`FRAME_CELL`] plus [`MEMORY_CELL`] plus a gap, so I3 never sees it.
    diagnostics: String,
}

impl<'a> Footer<'a> {
    /// Rows the footer takes off the pane, its rule included.
    ///
    /// **What every layout caller wants, where [`Footer::rows`] is what the
    /// drawer wants.** The two differ by the rule, and reaching for the wrong one
    /// puts the diff's last row under the mark.
    fn height(&self) -> u16 {
        self.rows + u16::from(self.rule)
    }

    /// Decide the footer's shape from the width, the state, and the file count.
    ///
    /// **From the file count, never the scroll position.** `{files}/{files}` is
    /// the widest position that count can produce, so reserving it means the
    /// footer cannot gain or lose a row while a reader scrolls from file 9 to
    /// file 10. A layout that reflowed under scrolling would be worse than one
    /// that is occasionally a column meaner than it had to be, and the meanness
    /// only shows below seventeen columns.
    fn plan(area: Rect, chrome: &'a Chrome, files: usize) -> Self {
        // **The room the footer's glyphs actually get**, which is the pane less
        // its inset on both sides: chrome has no scrollbar reserve standing in
        // for the right-hand half the way a glance row does. Planned here rather
        // than at each caller because this runs from three of them — [`render`],
        // [`regions`] and [`body_layout`] — and a footer whose height was decided
        // against one width and drawn against another would put the diff's last
        // row under the hints.
        //
        // Derived from `area.width` rather than from a `Painter`, since this is a
        // free function and the pane is what it is handed. [`margins_of`] is a
        // pure function of the pane, so the three callers cannot disagree.
        //
        // **Both margins, and the pair is what keeps this monotone.** The footer's
        // height must never grow as a pane widens, and a margin that took its two
        // sides on the same column would do exactly that at 44 and at 80.
        // [`margins_of`] carries the argument.
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
        //
        // **[`CELL_GAP`] rather than one column, corrected 2026-08-17 by
        // [#80](https://github.com/breferrari/vigia/issues/80).** One column was
        // invisible while the baseline rung was twenty-nine wide, because
        // `29 + 13 + 1` overflows a forty-column pane and the footer took its
        // second line before the two could ever meet. A three-item bar is
        // twenty-six, `26 + 13 + 1` is exactly forty, and the pane drew
        // `? keys follow ▶` with a single space between them, where the hint
        // separator is ` · ` and every group on this row is two columns from the
        // next. State read as a fourth hint. The gap the diagnostics group already
        // uses is the one this owes too.
        let taken = if reserved == 0 {
            0
        } else {
            reserved + CELL_GAP.len()
        };

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
        // **Charged the way the second footer line is**, against the same floor:
        // one header, the footer's own rows, the rule, and a body still worth
        // showing under it. Below that the mark is the first thing to go, which is
        // `SPEC.md` §5.3's rule that richness is the reward of space.
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
            rule,
            diagnostics,
        }
    }
}

/// How the body divides between the regions `SPEC.md` §11.1 rules.
///
/// The rows between the header and the footer are a masthead, a pinned file
/// list, a rule, and the scrolling diff. Every number comes from one function
/// because they have to agree: a caller that derived the diff's height by
/// subtracting its own idea of the others would be a second layout rule, and the
/// two would disagree on exactly the pane heights where a region is giving way.
///
/// **Three regions since [#158](https://github.com/breferrari/vigia/issues/158)**,
/// where it was two. [`Body::band_rows`], [`Body::above_list`] and [`Body::rows`] exist so that a
/// caller adding them up does not become the fourth place the geometry is
/// written: six sites open-coded `graph + air` within a day of the region
/// landing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Body {
    /// Blank rows between the header and whatever the body opens with.
    ///
    /// [`LEAD_ROWS`] when a list is drawn and zero otherwise, which is
    /// [`Body::rule`]'s own shape one boundary up. When a band is drawn this is
    /// the blank above it, so the masthead spends no leading air of its own.
    pub lead: usize,
    /// Rows the worktree churn band takes, zero when the pane cannot spare them.
    ///
    /// **The newest luxury and the first thing to go**, which is the order
    /// [`Body::split`] applies: the list is a map of the diff and the band is a
    /// fact about the tree, so the band yields to both. `SPEC.md` §5.3 rules that
    /// richness is the reward of space, and this is what earning it looks like
    /// from the layout's side.
    pub graph: usize,
    /// The blank row under the band, zero whenever the band is.
    ///
    /// Counted separately from [`Body::graph`] so a caller can tell the drawn
    /// rows from the ones deliberately left empty. **Ask [`Body::band_rows`] for
    /// the two together, or [`Body::above_list`] for everything over the map**,
    /// which is what every caller actually wants and what stops the sum being
    /// written by hand in a seventh place.
    pub air: usize,
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
            lead: 0,
            graph: 0,
            air: 0,
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
    /// ruling. It wants one row per changed file; it may not exceed
    /// [`list_cap`]; and it may not take the diff below [`MIN_BODY`], counting
    /// the rule it costs. A monitor whose diff has been squeezed out by the map
    /// of the diff has stopped being one, which is the same argument
    /// `Footer::plan` makes one region down about its second line.
    ///
    /// **The middle clamp is a function of the pane since
    /// [#160](https://github.com/breferrari/vigia/issues/160)** and was
    /// [`LIST_SETTLED`] flat. The order did not move with it, which is the whole
    /// of why the deepening is safe: the cap is asked *before* `affordable`, so a
    /// taller pane can only spend rows the diff was never keeping.
    ///
    /// **Nothing here reads the notice**, deliberately. §11.1 forbids a transient
    /// thing from moving content, and a region that appeared and vanished as
    /// files were named and read would jog the reader's diff exactly the way a
    /// growing footer would. The inputs are pane height, footer height and
    /// changed-file count, all of which change only when the diff does.
    ///
    /// Saturating rather than clamped so a one-row terminal asks for nothing
    /// instead of underflowing.
    pub fn split(area: Rect, footer_rows: u16, files: usize, masthead: bool) -> Self {
        let body = usize::from(area.height).saturating_sub(1 + usize::from(footer_rows));

        // The rule costs a row and [`LEAD_ROWS`] costs another, so the diff needs
        // `LEAD_ROWS + MIN_BODY + 1` before the list may have its first. No
        // changed files is B3's empty state, which reaches the same answer through
        // `files.min(..)` rather than through a branch of its own: one line of
        // prose in the diff region, no list, no rule over nothing because a rule
        // above an absent region is chrome announcing it, and no lead blank
        // either, for the same reason one boundary up.
        //
        // **Charged before the list is sized rather than after**, which is what
        // makes the row a property of the layout instead of a leftover: a lead
        // taken out of what remained would be present on a tall pane and absent on
        // a short one at the same file count.
        let affordable = body.saturating_sub(LEAD_ROWS + usize::from(MIN_BODY) + 1);
        let list = files.min(list_cap(area.height)).min(affordable);
        if list == 0 {
            return Self::diff_only(body);
        }
        let after = body - LEAD_ROWS - list - 1;

        // **The band is last in the clamp order and that is the ruling.** The
        // list is a map of the diff and gives way to the diff; the band is a
        // fact about the worktree and gives way to both, so it is paid for out
        // of what is left rather than out of either. A pane that could afford a
        // band by shrinking the list draws its whole cap of list and no band, on
        // purpose: §5.3's "richness is the reward of space" means extra space,
        // not space taken from the map.
        //
        // **A taller pane cannot take the band away, and that is a property of
        // [`list_cap`]'s step rather than of this expression.** `after` is
        // `body - list - 2`, and one more row of pane adds one to `body` and at
        // most one to `list`, so it never falls. A cap that gained two rows for
        // one row of pane would undraw a band on a pane that had just grown,
        // which is the same "bigger container holds less" failure the margin
        // ladder is a table to avoid.
        //
        // Tied to the **list's** presence as well, which is one branch rather
        // than two: B3's empty state replaces the whole region with a sentence,
        // and a graph above a sentence saying nothing changed is a graph of
        // nothing. `diff_only` above is that path and draws neither.
        //
        // **The band's own cost is one air row now, not two, and the condition is
        // arithmetically unchanged** ([#174](https://github.com/breferrari/vigia/issues/174)).
        // `after` is one smaller because the lead was charged above, and `framed`
        // is one smaller because the blank over the band is that lead, so
        // `after >= framed + GRAPH_KEEP` picks out exactly the pane heights it
        // picked out before. Worth stating rather than leaving to be re-derived:
        // adding a row to the body is normally how a ladder silently re-rungs, and
        // this one does not.
        let framed = GRAPH_ROWS + GRAPH_AIR;
        let graph = if masthead && band_fits(area.width) && after >= framed + GRAPH_KEEP {
            GRAPH_ROWS
        } else {
            0
        };
        let air = if graph > 0 { GRAPH_AIR } else { 0 };
        Self {
            lead: LEAD_ROWS,
            graph,
            air,
            list,
            rule: true,
            diff: after - graph - air,
        }
    }

    /// Every row the band occupies, drawn and blank together.
    ///
    /// **Named because six sites open-coded `graph + air` within a day**, which
    /// is the same hand-kept geometry [`band_fits`] was consolidated to remove
    /// one layer down. `regions` and the painter now agree by construction
    /// rather than by both being written correctly.
    ///
    /// **This was `masthead()` and it counted the leading blank too.** Renamed
    /// rather than left in place beside a new method
    /// ([#174](https://github.com/breferrari/vigia/issues/174)): every caller
    /// asking where the list starts wanted the sum *including* that blank, and a
    /// method that quietly stopped counting one row would have been right at every
    /// call site that was rewritten and wrong at every one that was not. Deleting
    /// the name makes the compiler ask each of them.
    pub fn band_rows(&self) -> usize {
        self.graph + self.air
    }

    /// Every row between the header and the list.
    ///
    /// The lead blank plus the band and its own air. **This is what a caller
    /// looking for the list's first row wants**, and it is the only thing that
    /// should ever be added to `area.y + 1`.
    pub fn above_list(&self) -> usize {
        self.lead + self.band_rows()
    }

    /// Every row the body holds, across every region it has.
    ///
    /// What a caller checking that the pane tiles wants, and what two test files
    /// had each written out for themselves.
    pub fn rows(&self) -> usize {
        self.above_list() + self.list + usize::from(self.rule) + self.diff
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
        // **The band and the lead blank go with the list**, for `Body::split`'s
        // own reason: the three are one region saying what the worktree is doing,
        // and a stale view with no entries draws B3's sentence rather than a graph
        // over blank rows under a blank row.
        let (lead, graph, air) = if list > 0 {
            (self.lead, self.graph, self.air)
        } else {
            (0, 0, 0)
        };
        // **The diff takes what is left of the body, rather than a give-back
        // term per field.** This was a sum of five differences, one added every
        // time the body gained a row category, and it is the shape
        // [`Body::air`]'s own doc argues against one field earlier: a hand-kept
        // total is only as good as whoever remembers to extend it. Subtracting
        // the new regions from the old body cannot omit a term, because there is
        // no term to omit.
        let rule = list > 0;
        Self {
            lead,
            graph,
            air,
            list,
            rule,
            diff: self.rows() - (lead + graph + air + list + usize::from(rule)),
        }
    }
}

/// Where each part of the body sits inside a pane.
///
/// **The geometry written once, for the two consumers that used to derive it
/// separately** ([#251](https://github.com/breferrari/vigia/issues/251)).
/// [`render`] walked a running `y` cursor down the body and [`regions`] rebuilt
/// the same offsets from the same [`Body`], so the painter and the pointer agreed
/// only by both being written correctly. That is the argument
/// [`Body::above_list`] already makes about the rows above the list; this is the
/// rest of it.
///
/// **Rects rather than rows, and that is the point rather than a convenience.**
/// A row offset can only describe regions stacked one above another.
/// [#252](https://github.com/breferrari/vigia/issues/252) puts the list *beside*
/// the diff on a wide pane, where the two share a `y` range and differ in `x`,
/// and a `Rect` can already say that. So the type that carries this does not have
/// to change again for the rail: what changes is what [`Body::areas`] computes,
/// and in that layout [`Areas::rule`] is simply empty.
///
/// A part that is not drawn gets a rect of **zero height** rather than an
/// `Option`, because every consumer's question is "how many rows do I paint
/// here", which zero already answers, and because the tiling property a gate
/// checks is a sum over four rects rather than over four maybes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Areas {
    /// The worktree churn band, empty when the pane cannot spare it.
    ///
    /// The blank row under it is [`Body::air`] and is deliberately *not* part of
    /// this rect: it is skipped rather than painted, which is what a blank row is
    /// here, and a rect claiming it would make the band look like furniture that
    /// owns a row it never draws in.
    pub band: Rect,
    /// The pinned file list, empty when there is no room for one.
    pub list: Rect,
    /// The rule under the list, empty when there is no list to put it under.
    pub rule: Rect,
    /// Everything left for the diff.
    pub diff: Rect,
}

impl Body {
    /// Where each part of this body sits inside `area`.
    ///
    /// `area` is the whole pane, header and footer included: the header's row is
    /// skipped here exactly as [`render`] skips it, and the footer is whatever is
    /// left under [`Body::diff`]. Both are drawn from `area` directly and neither
    /// is a region, so neither is here.
    ///
    /// Saturating throughout, for [`Body::split`]'s reason one level up: a pane
    /// too short to hold what it was asked for should draw something cramped
    /// rather than underflow.
    pub fn areas(&self, area: Rect) -> Areas {
        // The header's row, then the lead blank. `above_list` is the sum of that
        // blank and the whole masthead, and it is what `regions` has always added
        // to `area.y + 1`, so the band's own top is the only offset here that
        // cannot be asked for by name.
        let top = area.y.saturating_add(1);
        let band = Rect {
            y: top.saturating_add(self.lead as u16),
            height: self.graph as u16,
            ..area
        };
        let list = Rect {
            y: top.saturating_add(self.above_list() as u16),
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
    Body::split(
        area,
        Footer::plan(area, chrome, files).height(),
        files,
        chrome.masthead,
    )
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
    let body =
        Body::split(area, footer.height(), view.files, chrome.masthead).clamped_to(view.list.len());
    let wide = affords_bar(area.width);

    // **The same geometry the painter draws into**, asked for by name rather than
    // rebuilt here ([#251](https://github.com/breferrari/vigia/issues/251)). This
    // was two offsets computed from `Body` a second time, which kept the pointer
    // and the screen one answer only while both expressions stayed correct: a
    // hover resolved against a stale row offset lights the file above the one
    // under the pointer, and nothing would have said so.
    let areas = body.areas(area);

    // **Asked through `bar_for`, which is what `render` asks.** A bar column only
    // where a bar is actually drawn, and only where it can express more than one
    // position: a one-row track is full at every window, so it says nothing and
    // would still swallow a click. The same call also says where each track is,
    // which is the part a pointer cannot derive for itself.
    let list_bar = bar_for(wide, areas.list.height, body.list as u64, view.files as u64);
    let diff_bar = bar_for(
        wide,
        areas.diff.height,
        body.diff as u64,
        view.total_rows as u64,
    );

    Regions {
        list: list_bar.region(areas.list),
        diff: diff_bar.region(areas.diff),
        // **From the same plan the painter draws**, which is what keeps the
        // pointer and the screen one answer: a sheet the reader can see but the
        // pointer cannot would swallow nothing and let a click seek a bar behind
        // it. `None` when it is not up, and also when it is up on a pane too small
        // to draw it, since a target nobody can see must not eat a gesture.
        sheet: chrome
            .sheet
            .then(|| sheet_plan(area, footer.height(), margins_of(area.width)))
            .flatten()
            .map(|plan| plan.target()),
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
    glyphs: Glyphs,
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
    let body =
        Body::split(area, footer.height(), view.files, chrome.masthead).clamped_to(view.list.len());
    let margins = margins_of(area.width);

    let mut painter = Painter {
        buf,
        theme,
        // A parameter beside `theme` rather than a field on `chrome`, and for
        // the same reason `theme` is one: both are properties of the terminal,
        // resolved once before the screen was taken and unchanged for the
        // session, where `chrome` carries what *this frame* says. Nothing in the
        // layout below `render` reads it, so putting it on the per-frame struct
        // meant two of its three callers stamping a field the function they fed
        // never looked at.
        glyphs,
        gutter: 0,
        // **From the pane, once, before anything is drawn.** Every row below is
        // handed a `Rect` that has already lost the bar's columns on the screens
        // where a bar exists, so a drawer that resolved the ladder from what it
        // was given would take a different inset on the two regions of one pane
        // and change it when a seventh file changed. See [`inset_of`].
        inset: margins.0,
        trailing: margins.1,
        paint: PaintStats::default(),
        pressed: chrome.pressed,
        gripped: chrome.gripped,
        hovered: chrome.hovered,
        scrolling: chrome.scrolling,
    };

    painter.header(Rect { height: 1, ..area }, view, chrome);

    if footer.rows > 0 {
        painter.footer(area, view, chrome, &footer);
    }

    // Wide enough for a bar at all. Each region then decides separately whether
    // it has anywhere to scroll, because a list of three files and a diff of
    // thirty thousand rows are different questions.
    let bars = affords_bar(area.width);

    // **The geometry, once, from the same method the pointer reads**
    // ([#251](https://github.com/breferrari/vigia/issues/251)). This used to be a
    // running `y` cursor walked down the body here and rebuilt from the same
    // `Body` in `regions`, so the painter and the pointer agreed only by both
    // being written correctly.
    //
    // **The body still opens with air, band or no band**
    // ([#174](https://github.com/breferrari/vigia/issues/174)), and that blank is
    // inside `Body::lead` rather than drawn. Skipped rather than painted is what a
    // blank row is here: the pane's own background, with no furniture claiming a
    // row it does not use. Nothing below has to know whether the row is the
    // header's separator or the band's leading air, because it is both.
    let areas = body.areas(area);

    if areas.band.height > 0 {
        painter.band(areas.band, view);
    }

    if body.list > 0 {
        let region = areas.list;
        // Counted in **files**, which is exactly what this region shows.
        let full = region;
        let (region, bar) = painter.with_bar(region, bars, body.list as u64, view.files as u64);
        // **Before the content here, and it does not matter which**, because a list
        // row carries no wash: the bar's cell and the row's cells never overlap. The
        // diff region below is the one where the order is load-bearing, and it draws
        // the other way round for a reason stated there.
        if bar.drawn() {
            painter.scrollbar(
                full,
                Grabbed::List,
                bar,
                view.list_top as u64,
                body.list as u64,
                view.files as u64,
            );
        }
        painter.list(region, view, area.width);
    }

    if areas.rule.height > 0 {
        painter.rule(areas.rule);
    }

    if body.diff > 0 {
        let region = areas.diff;
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
        // 46ms for the same work. `SPEC.md` §3's I4 notes carry the rewording that
        // admits the walk, and `what_a_row_exact_scrollbar_would_cost` is the
        // measurement it rests on.
        //
        // Zero total means the caller did not ask for one, which is a pane too
        // short for a bar, and `with_bar` draws nothing when told a whole of
        // zero.
        let full = region;
        let (region, bar) = painter.with_bar(
            region,
            bars,
            u64::from(body.diff as u16),
            view.total_rows as u64,
        );
        // **The wash spans the region's whole width, the bar's own column
        // included** ([#239](https://github.com/breferrari/vigia/issues/239)).
        //
        // It reached one column further than the content until then, filling the
        // blank `reserved` keeps in front of the bar
        // ([#214](https://github.com/breferrari/vigia/issues/214)), and stopped at
        // the bar. #214's reason is *"one column of pane background between the
        // band and the bar, so the band reads as clipped and the bar as standing
        // in a notch"*, and that reason reaches its own remainder: the bar's
        // column was the last cell of pane on a changed row, so the notch was one
        // column narrower and still there, running the height of the diff.
        // Reported from a real pane. `SPEC.md` §5.3's furniture rule says the same
        // thing from the other side, and `assets/preview.svg` draws every washed
        // row `x="8" width="884"`, the pane exactly, with the bar painting over
        // it.
        //
        // **The bar draws after the content here, and that order is the fix rather
        // than a detail.** It drew first at the start of #239, on the reading that
        // `Buffer::set_style` merges and so the wash would repaint only the
        // background and leave the track's glyph and colour alone. That is true of
        // `fg` and `bg` and **false of modifiers**: `Cell::set_style` inserts
        // `add_modifier` unconditionally, so a wash carrying one handed it to the
        // bar, and `VIGIA_THEME` lets a reader put `reverse` on a row wash. Round 2
        // of #239's audit found it on the change that introduced it.
        //
        // Drawing the bar last puts the band underneath and lets
        // [`Painter::bar_cell`] own the cell outright except for its background,
        // which is where that guarantee now lives. The band ends up behind the bar
        // rather than instead of it, which is what the picture draws.
        //
        // The reserve is untouched. It is about **glyph adjacency**, so that a
        // full-block thumb never reads as part of a `-6`, and a blank cell whose
        // background matches the band is still a blank cell.
        //
        // **This equals `area.width`, the `pane` argument below, and the two are
        // deliberately not merged.** `region` is built `..area`, so today the wash's
        // width and the pane's width are the same number arriving twice, which is
        // fair to read as derivable state. They mean different things: this is *the
        // region's* extent, which is what furniture spans, and `pane` is the
        // screen's, which is what glyph placement is planned against. The
        // coincidence is that the diff region currently spans the whole pane.
        // [#162](https://github.com/breferrari/vigia/issues/162) ends that on
        // purpose, giving the diff *"the remaining width as its own region"* beside
        // a left rail, and on that layout the wash must follow the region and not
        // the screen. Collapsing them now would be correct and would plant a defect
        // for that row to land on, so the seam stays and this paragraph is why.
        let washed = full.width;
        painter.body(region, washed, view, area.width);
        if bar.drawn() {
            painter.scrollbar(
                full,
                Grabbed::Diff,
                bar,
                view.rows_above as u64,
                u64::from(body.diff as u16),
                view.total_rows as u64,
            );
        }
    }

    // **Last, over everything, and only if a reader asked.** `SPEC.md` §11.1's
    // B12: the sheet is the one drawn thing on this pane that takes no row from
    // any region, so it is composited after all of them and the plan above is
    // untouched by whether it is up.
    if chrome.sheet {
        if let Some(plan) = sheet_plan(area, footer.height(), margins) {
            painter.sheet(&plan);
        }
    }

    painter.paint
}

/// One row of the gestures sheet: how to ask for a thing, and what it does.
///
/// Both fields carry **two spellings, the wide one first**, and the ladder picks
/// one level for the whole sheet rather than per row: a table whose rows each
/// chose their own width would not be a table. `SPEC.md` §11.1's B12.
struct Gesture {
    /// The keys cell. Aliases live inside it rather than in rows of their own.
    keys: [&'static str; 2],
    /// What the gesture does.
    verb: [&'static str; 2],
}

/// The keyboard half, in the order a reader meets it.
///
/// **Aliases are spelled inside the keys cell**, which is the README table's
/// shape and B12's answer to *gestures or keys*: `j`, `k`, `↓` and `↑` are one
/// gesture with four spellings, not four gestures.
///
/// **The last three rows are the ones the ladder keeps**, and that order is
/// §11.1's own rather than a preference: the unguessable outlives the reflexive.
/// `f` restores a state whose absence is invisible, `m` names a region a reader
/// may not know exists, and `?` is this sheet, which is how everything above it
/// is found at all.
const KEYBOARD: [Gesture; 11] = [
    Gesture {
        keys: ["q  Esc  Ctrl+C  Ctrl+D", "q  Esc"],
        verb: ["quit", "quit"],
    },
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
        keys: ["n  /  p", "n  /  p"],
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
        keys: ["?", "?"],
        verb: ["this sheet", "this sheet"],
    },
];

/// The mouse half, which is the first thing the height ladder drops.
///
/// It goes before any keyboard row for the reason the hint bar drops `JK files`
/// first: a mouse gesture announces itself by being tried, where a key does not
/// exist until somebody says it does.
const MOUSE: [Gesture; 5] = [
    Gesture {
        keys: ["wheel", "wheel"],
        verb: ["scroll what you point at", "scroll what you point at"],
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
        verb: ["one row, and repeats held", "one row, repeats held"],
    },
    Gesture {
        keys: ["click a listed file", "click a file"],
        verb: ["jump the diff to it", "jump the diff to it"],
    },
];

/// Keyboard rows the ladder may never drop: `f`, `m` and `?`.
const SHEET_KEEP: usize = 3;

/// Rows the sheet's frame costs, one border at each end.
const SHEET_FRAME: usize = 2;

/// What the sheet's own title bar spells, corner excluded.
const SHEET_TITLE: &str = "─ gestures ";

/// The close control, which is the pane's first.
///
/// **Outside CP437**, exactly as the scrollbar's `▲` and `▼` are, and inheriting
/// that ruling rather than reopening it: shape is what tells a control from the
/// frame it sits in, and no ASCII glyph reads as *close* without a legend.
const SHEET_CLOSE: char = '✕';

/// The three widths a rung of the sheet needs: keys field, verb field, and the
/// whole sheet.
///
/// The fields are measured over the rows that rung actually draws, which is what
/// makes dropping the mouse group narrow the sheet as well as shorten it: its
/// `drag a scrollbar` is the widest keys cell on the whole table.
fn sheet_fields(level: usize, from: usize, mouse: bool) -> (usize, usize, usize) {
    let drawn = KEYBOARD[from..]
        .iter()
        .chain(mouse.then_some(&MOUSE[..]).into_iter().flatten());
    let (mut keys, mut verb) = (0, 0);
    for row in drawn {
        keys = keys.max(width_of(row.keys[level]));
        verb = verb.max(width_of(row.verb[level]));
    }
    // Border, space, keys, two of gap, verb, space, border. Floored at the width
    // the title bar needs, so a narrow table cannot draw a truncated heading.
    let total = (keys + verb + 6).max(width_of(SHEET_TITLE) + 6);
    (keys, verb, total)
}

/// Rows a rung draws, frame excluded.
fn sheet_rows(from: usize, mouse: bool) -> usize {
    (KEYBOARD.len() - from) + if mouse { 1 + MOUSE.len() } else { 0 }
}

/// Where the gestures sheet goes, or `None` on a pane that cannot hold one.
///
/// **Two axes, in the order B12 rules them.** Width picks the spelling level,
/// because the alternative is a table with a truncated verb and §11.1 forbids
/// truncating an item that has no identifying half. Height then drops the mouse
/// group, and after that keyboard rows from the top down, which leaves the three
/// [`KEYBOARD`] names its own docblock says survive.
///
/// **Centred in the body, never over the header or the footer**, which is B12's
/// reason for a box rather than a full-pane sheet: a reader reading instructions
/// must still be able to see that the tool is alive behind them.
///
/// Both floors live here rather than in [`Painter::sheet`], which is
/// [#158](https://github.com/breferrari/vigia/issues/158)'s correction inherited
/// rather than re-earned: a floor known only to the drawer lets the layout
/// promise a region the painter then declines to draw.
fn sheet_plan(area: Rect, footer_rows: u16, margins: (u16, u16)) -> Option<SheetPlan> {
    let body = area.height.saturating_sub(1 + footer_rows);
    let room = area.width.saturating_sub(margins.0 + margins.1);
    let level = usize::from(sheet_fields(0, 0, true).2 > usize::from(room));

    // The order is the ruling's: the mouse group before any keyboard row.
    let rungs = std::iter::once((true, 0))
        .chain((0..=KEYBOARD.len() - SHEET_KEEP).map(|from| (false, from)));
    for (mouse, from) in rungs {
        let (keys, verb, total) = sheet_fields(level, from, mouse);
        let height = sheet_rows(from, mouse) + SHEET_FRAME;
        if total > usize::from(room) || height > usize::from(body) {
            continue;
        }
        let width = total as u16;
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
            level,
            from,
            mouse,
            keys,
            verb,
            // Three in from the right edge: `┐`, the space before it, and this.
            close: (left + width - 3, top),
        });
    }
    None
}

/// A laid-out gestures sheet: where it goes and which rung it draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SheetPlan {
    /// The whole sheet, frame included.
    area: Rect,
    /// Which spelling every cell takes: 0 wide, 1 tight.
    level: usize,
    /// First keyboard row drawn. Rows above it were dropped for height.
    from: usize,
    /// Whether the mouse group is drawn.
    mouse: bool,
    /// The keys field's width.
    keys: usize,
    /// The verb field's width.
    verb: usize,
    /// The close control's cell.
    close: (u16, u16),
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
    ///
    /// Beside `gutter` and for `gutter`'s reason: a measurement every row shares
    /// has to be taken once, or the rows disagree about the screen they are on.
    /// The two are not always equal, and [`margins_of`] says why.
    inset: u16,
    /// The right-hand half of the pair above.
    trailing: u16,
    /// What the content rows have cost so far, returned by [`render`].
    paint: PaintStats,
    /// The cell a step button is being held on, from [`Chrome::pressed`].
    ///
    /// Copied onto the painter for the reason `inset` is: it is decided once for
    /// the whole screen, and a drawer that reached back into the chrome for it
    /// would be one more thing the two regions could answer differently.
    pressed: Option<(u16, u16)>,
    /// Which region's bar is being dragged, from [`Chrome::gripped`].
    gripped: Option<Grabbed>,
    /// What the pointer is resting on, from [`Chrome::hovered`].
    hovered: Option<Hovered>,
    /// Which bar the keys are scrolling and which way, from
    /// [`Chrome::scrolling`].
    scrolling: Option<(Grabbed, isize)>,
}

impl Painter<'_> {
    /// The columns of `area` a glyph may use.
    ///
    /// The complement of what this type draws **furniture** into, which stays
    /// `area` itself: `SPEC.md` §5.3 rules that washes and rules run to the pane's
    /// edge while text stands back from it, and that the two roles must not swap.
    /// A wash that stopped short would read as a misaligned highlight and text at
    /// column zero reads as squeezed, so [`Painter::status_line`] paints its row
    /// from `area` and then places its glyphs through this.
    ///
    /// **Chrome only.** The header and the footer are the two rows handed the
    /// whole pane, with no scrollbar reserve standing in the columns a right-hand
    /// margin wants, so they are the rows that owe both sides. A glance row pays
    /// the left alone through [`planning_width`], and the diff's content rows have
    /// their own rule in [`Painter::region_text`], because their rect may already
    /// have lost the bar's columns and would otherwise be charged twice.
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
    ///
    /// **The bar's columns and the trailing margin are the same blank, so the
    /// stop is whichever is already further in rather than both.**
    /// [`Painter::with_bar`] hands [`Painter::body`] a rect that has lost
    /// [`BAR_WIDTH`] on the screens where a bar is drawn, and those are exactly
    /// the columns a right-hand margin would have wanted. Subtracting the margin
    /// on top charged it twice: at eighty columns a heading's last glyph sat in
    /// column 77 while the content line under it stopped at 75, and the two
    /// agreed only while the diff was short enough to need no bar. A layout that
    /// moves when the diff outgrows the pane is the defect [`planning_width`]
    /// exists to refuse, reaching the one row class the ruling had not been
    /// applied to.
    ///
    /// **What this deliberately does not do is give these rows the bar's reserve
    /// unconditionally.** That would make them agree with a heading at every
    /// width, which is tempting and is [#77](https://github.com/breferrari/vigia/issues/77)'s
    /// ruling one row class further, but it costs every content line two columns
    /// at *every* width including the forty I6 is named for. That is a spec
    /// question rather than a margin question, so this keeps the region's own
    /// right edge where it already was and only declines to charge the margin a
    /// second time.
    ///
    /// **So a residual survives, and it is smaller than what was here before
    /// rather than new.** The movement when a diff grows a bar is [`BAR_WIDTH`]
    /// less the trailing margin: **two** below forty-four columns, where the
    /// margin has no right-hand half yet, **one** from forty-four to
    /// seventy-nine, and **none** from eighty up where the two are equal. On
    /// `main` it was two at every width.
    ///
    /// Forty-three is deliberately *not* in the improved band, which is the kind
    /// of thing a range written from memory gets wrong: its rung is the odd one
    /// and spends its single column on the left, so its trailing margin is zero
    /// and it behaves exactly as `main` did.
    ///
    /// Reduced and bounded is what this issue is entitled to buy; removing it
    /// outright is the same I6 trade as the paragraph above, and it is the reason
    /// `a_diff_outgrowing_its_pane_does_not_move_the_content_rows_edge` asserts
    /// the *barred* screen against its own heading rather than asserting that
    /// nothing ever moves.
    fn region_text(&self, area: Rect, pane: u16) -> Rect {
        // **The two derivations of the margin have to be the same one.**
        // `self.trailing` was resolved in [`render`] from the pane it was handed,
        // and [`planning_width`] resolves [`inset_of`] from the `pane` a caller
        // passes down. They are the same function of the same number today and
        // nothing in the types says so, which is one refactor away from a region
        // laid out against a width the chrome above it disagrees with. Asserted
        // rather than consolidated: folding the pane onto [`Painter`] would churn
        // three signatures that predate this issue, and a check that runs in every
        // debug test is what actually catches the drift.
        debug_assert_eq!(
            margins_of(pane),
            (self.inset, self.trailing),
            "a region is being drawn against a different pane than the painter was \
             built for, so its margin and the chrome's have come apart"
        );
        let stop = area.width.min(pane.saturating_sub(self.trailing));
        Rect {
            x: area.x.saturating_add(self.inset),
            width: stop.saturating_sub(self.inset),
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
    /// Dropped entirely rather than truncated when it does not fit. Half a token
    /// on the right of a line is noise; its absence is at least honest.
    ///
    /// **The header's mode word rests on this and on nothing else**, which is
    /// worth saying here because the rule is otherwise located only at the
    /// caller. `SPEC.md` §11.1 requires that `watching` is drawn whole or not at
    /// all, since `wat›` is a state nobody can read; it had a ladder of its own
    /// until [#67](https://github.com/breferrari/vigia/issues/67) left that
    /// ladder with one rung, and this line is what replaced it.
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
    /// that both header facts are drawn in one weight hard to break rather than
    /// merely written down: a ladder of `(String, Style)` would put that
    /// violation one line away. It is not *unrepresentable* — two `put_marked`
    /// calls would still do it — so
    /// `the_headers_two_tree_facts_are_drawn_in_one_weight` gates which weight
    /// the clause actually gets, and reddens alone when it is changed.
    ///
    /// **Every left ladder gets mark-the-last-rung semantics**, because this
    /// hard-codes [`widest_fitting_or_last`] rather than letting the caller pick.
    /// A future left-hand ladder that wants to degrade to nothing has to end in
    /// an empty rung to say so, the way every ladder resolved by
    /// [`widest_fitting`] already does.
    fn status_line<S: AsRef<str>>(
        &mut self,
        area: Rect,
        left: &[S],
        style: Style,
        right: &str,
        right_style: Style,
    ) {
        // **The wash takes the whole row and the text takes the inset one**,
        // which is §5.3's furniture rule and the reason these two lines address
        // different rectangles. See [`Painter::text_area`].
        self.buf.set_style(area, self.theme.chrome_dim);
        let text = self.text_area(area);
        let taken = self.put_right(text, right, right_style);
        let room = usize::from(text.width).saturating_sub(taken);
        let rung = widest_fitting_or_last(left, room);
        self.put_marked(text.x, text.y, rung, room, style);
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
        // **A dead watch has to be visible, not merely present.** Drawn in
        // the header's dim grey, `not watching` is a word a reader has to
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
            &header_left(&chrome.worktree, chrome.branch.as_deref(), view.files),
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

        // **Above the footer's own rows, and full bleed like the one over the
        // diff.** `Painter::rule` takes the pane's rect rather than an inset one,
        // so both marks run edge to edge: a rule that stopped at the margin would
        // read as a box someone forgot to close, which is that method's own
        // reason and applies to this boundary identically.
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
        //
        // **Against the inset row rather than the whole one**, because that is
        // where `status_line` puts the text: measuring from the pane's edge would
        // put `placed` two columns right of the string it names, and the tint
        // would recolour the blank margin while the readouts stayed grey.
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
    ///
    /// `assets/preview.svg` draws `0.8ms` and `24MiB` in `.cyn`, the word
    /// `frame` beside them in `.dim`, and the follow marker in `.grn`. The
    /// shipped footer drew all of it in one grey, so the two numbers a reader
    /// checks at a glance and the mode marker looked like the words around them.
    /// §5.1's rule is that a published artifact answering a question is the
    /// answer.
    ///
    /// **A second pass over drawn cells rather than a second placement**, and
    /// that is the load-bearing choice. Each of these is part of a token the
    /// ladder picks *whole*: `0.8ms frame   19MiB` is one rung of
    /// [`diagnostic_rungs`] and `follow ▶  1/3` is one rung of [`state_rungs`].
    /// Splitting them to place each colour separately would mean the ladders no
    /// longer decide what the row draws, and `Footer::plan`'s width arithmetic
    /// would have to be told about colours to stay correct. Tinting after the
    /// fact cannot move a column.
    ///
    /// **Bounded to the diagnostics' own columns** for the numbers, because the
    /// state carries a number too and `1/3` is a position rather than a
    /// measurement. The picture gives no colour for it, so it keeps its grey.
    ///
    /// The styles are reused rather than named anew: [`Theme::chrome`] is the
    /// picture's `.cyn` and [`Theme::added`] its `.grn`, both to the byte on the
    /// dark palette. A colour of their own would be a palette decision, which
    /// stays [#11](https://github.com/breferrari/vigia/issues/11)'s, and it is
    /// the same reuse the header's `not watching` makes of the footer's alert.
    fn tint_readouts(&mut self, row: Rect, at: u16, readouts: usize) {
        // A measurement and its unit: a run opening with a digit or the
        // over-magnitude sigil, carried through the letters that name the unit.
        // The label `frame` opens with a letter and so is never picked up.
        //
        // **The opening cell is consumed by the run whatever it says**, and that
        // is a termination argument rather than a detail. `>` opens a run and
        // carries nothing, so a loop that asked both questions of the same cell
        // made no progress on `>1s` and spun forever with the pane frozen. Every
        // pass of the outer loop now advances `x` by at least one column.
        //
        // **Clipped to the buffer, not to the area.** `render`'s contract is
        // that any area is legal, and every other writer reaches the cells
        // through `Buffer::set_stringn` or `set_style`, both of which clip. This
        // walk indexes directly.
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

        // **Bounded to the state's own columns**, which start where the
        // diagnostics end. Scanning the whole row let a `▶` in the *notice* take
        // the green: a notice is an error string carrying a path, `▶` is a legal
        // filename character, and the first match won. That fabricated the one
        // glyph on the footer a reader checks rather than reads, saying the view
        // was live when it was not, and when follow really was on it lit the
        // wrong glyph and left the real marker grey.
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
    ///
    /// **The hero element, and the only drawn thing on this pane that is about
    /// the worktree rather than about a file in it**
    /// ([#158](https://github.com/breferrari/vigia/issues/158)). Every other
    /// glance element answers *which file*; this answers *how hot is this tree
    /// now, and was it hotter a minute ago*.
    ///
    /// **It invents nothing.** The series is arithmetic over state I10 already
    /// bounds, kept current by a walk `History::record` was already making, so
    /// the band costs no wake, no write and no clock and draws on frames that
    /// were going to happen. That is §5.3's "ink is priced at zero" spent on the
    /// rung §5.3 names.
    ///
    /// **Inset like text rather than bled like furniture**, which is the one
    /// place this element could have gone either way. §5.3 rules washes and
    /// rules to the pane's edge and glyphs inside it; a graph is made of glyphs
    /// and is read rather than looked past, so it takes the same inset every row
    /// of the pane takes and stops clear of the scrollbar's column whether or not
    /// a bar is drawn. Bleeding it would put it under the bar at the one width
    /// where the bar appears.
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
        let width = usize::from(planning_width(area.width, 0));

        // **One value per sub-column, and a zero draws the baseline**, which is
        // `btop`'s own answer to this exact signal
        // ([#232](https://github.com/breferrari/vigia/issues/232)). Its network
        // graph is the closest analogue in that tool: bursty, zero most of the
        // time, and it is built with `no_zero = true`, which forces one dot on
        // the bottom row so an idle interface draws an axis rather than nothing.
        // Read from `src/btop_draw.cpp`, `Graph::_create`, rather than recalled.
        //
        // **That supersedes [#223](https://github.com/breferrari/vigia/issues/223)'s
        // coarsening, and the correction is worth stating rather than
        // absorbing.** That row diagnosed the right defect, a save drawing a
        // one-column hairline between blanks, and reached for a wider column to
        // fix it. `btop` fixes the same defect by drawing the floor, and once the
        // floor is drawn a narrow column is a spike on an axis instead of a mark
        // in a void. Coarsening then costs resolution and buys nothing, and it
        // was what made the band read as separated blocks
        // ([#232](https://github.com/breferrari/vigia/issues/232), reported from
        // a live pane).
        //
        // `btop` sizes its buffer to the pane for the same reason, keeping
        // `width * 2` samples so no value is ever stretched across cells. Here
        // the window is fixed by I10, so the projection does the same job from
        // the other side: it aggregates when the pane holds fewer sub-columns
        // than the window holds samples, and repeats when it holds more.
        let density = self.glyphs.density();
        // **The pane can ask for more sub-columns than the window holds samples,
        // and then values repeat rather than run out.** `btop` never meets this
        // because its buffer is sized to the pane, keeping `width * 2` samples;
        // I10 fixes this window at two minutes instead, so past 120 sub-columns
        // the projection has nothing further to divide and each value covers more
        // than one. Asking `projected` for the wider number silently returned a
        // short series, which drew a graph that stopped partway across the pane
        // and left bare axis after it.
        let slots = width * density;
        // **The level, not the events** ([#242](https://github.com/breferrari/vigia/issues/242)).
        // `assets/preview.svg` draws this as a wave and a write is a point event,
        // so the raw series is zero almost everywhere and an area chart of it is a
        // spike train. `Churn::levels` reads it as a density through the same
        // kernel the file sparklines use, which is #234's coherence requirement
        // met by construction: the two elements cannot disagree about what they
        // are showing because there is one kernel and one constant.
        let series = view.worktree_churn.levels(slots);
        // **`vigia_core::scale_of`, the same rule the sparkline divides by.** It
        // lived here while the band was the only element that had it, which is
        // exactly how the sparkline was left dividing by a maximum over the same
        // byte samples. The two denominators stay different quantities, one
        // worktree-wide and one per file, which is `SPEC.md` §11.1's ruling; what
        // they may not disagree about is the rule.
        let scale = scale_of(series.iter().copied());
        // **No data, no axis**, which is what keeps
        // [#158](https://github.com/breferrari/vigia/issues/158)'s reported
        // defect fixed while the axis exists at all. That ruling came from a
        // first real run: a window holding nothing drew a hundred columns of `_`,
        // "a dashed rule the pane did not ask for", and every session opens in
        // exactly that state because a worktree already dirty when `vigia`
        // started has no tick behind it yet. `btop` draws its floor for an idle
        // interface and would draw one here too; the distinction it does not have
        // to make is between *quiet* and *not started*, and this is that line. An
        // empty column inside a live window is quiet and stands on the floor; an
        // empty window has no graph to put a floor under. The rows stay reserved
        // either way, so the first write does not jog the list.
        //
        // **It does not distinguish "not started" from "quiet for two minutes",
        // and that is deliberate rather than an oversight.** Both are an empty
        // window, and #158's ruling is about what an empty window draws, not about
        // how it got that way. A reader back from lunch sees the band go bare and
        // the axis return with the first write, which is the same thing the row
        // count already does at launch.
        if scale == 0 {
            return;
        }
        let rows = usize::from(area.height);
        // Levels one row carries. The block ramp gives eight, a 2x4 cell gives
        // its dot rows less the baseline, and [`Glyphs::glyph`] already spells
        // both, floor included: level zero is the axis rather than nothing.
        let levels = self.glyphs.levels();

        for cell in 0..width {
            // A dense cell carries two sub-columns, left older than right, which
            // is `Glyphs::glyph`'s own order and the sparkline's. At the block
            // rung the density is one and the right half is never read.
            // `projected` returns exactly what it was asked for, repeating where
            // the window holds fewer samples than the pane holds sub-columns, so
            // this is a plain index rather than a second copy of that mapping.
            let at = |sub: usize| series[cell * density + sub];
            let (older, newer) = (at(0), at(density - 1));
            let full = |total: u32| level_to(total, scale, rows * levels);
            let (left, right) = (full(older), full(newer));
            // Against the same denominator the heights are scaled from, so
            // colour and shape say one thing at one scale.
            let band = Band::of(older.max(newer), scale);
            for row in 0..rows {
                // Drawn bottom up, so `row` counts from the baseline and the
                // buffer's `y` counts down from the top.
                let y = area.y + (rows - 1 - row) as u16;
                let fill = |level: usize| level.saturating_sub(row * levels).min(levels);
                let (low, high) = (fill(left), fill(right));
                // **Sky above the bar is left alone; the baseline row is not.**
                // A graph's empty upper rows are background, and painting them
                // would draw a solid block the height of the band on every
                // column. The bottom row is the axis and is always drawn.
                if row > 0 && low == 0 && high == 0 {
                    continue;
                }
                let glyph = self.glyphs.glyph(low, high);
                let x = left_edge.saturating_add(cell as u16);
                self.bar_cell(x, y, glyph, self.theme.spark_at(band));
            }
        }
    }

    /// Draw the pinned file list, `SPEC.md` §11.1's upper region.
    ///
    /// Every row goes through [`Painter::file_row`], which is the whole point:
    /// one drawer, one degradation ladder, one set of gates, and no way for the
    /// two regions to disagree about what a file looks like. What is different
    /// here is the **caret**, and it is applied by insetting the area rather than
    /// by telling `file_row` which region it is in, so that function stays
    /// ignorant of the layout above it.
    ///
    /// **The inset is [`caret_gutter`] and is usually zero**
    /// ([#173](https://github.com/breferrari/vigia/issues/173)). The caret is
    /// drawn at the pane's own leading column, into the blank [`margin_of`]
    /// already keeps there, so from forty-three columns up this region's origin
    /// and width are the stream's exactly and the two sigils sit in one column.
    /// It used to inset by a flat two at every width, which put the list's status
    /// sigil two columns right of the same sigil on a heading, and that is what
    /// the reader reported.
    ///
    /// The caret is dropped below [`affords_caret`] rather than squeezing the
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
        // [`affords_caret`], which is now that comparison written once and read
        // by the width below as well, rather than a constant kept beside it.
        let caret = affords_caret(pane);
        let gutter = if caret { caret_gutter(pane) as u16 } else { 0 };

        // **The caret sits on the pane's own leading column, and the row starts
        // after whatever margin is left over.** `assets/preview.svg` puts the
        // window edge at `x=8`, the caret at `x=8` and every content origin at
        // `x=32`: the marker stands *outside* the text rather than pushing it
        // right, which is the arrangement
        // [#173](https://github.com/breferrari/vigia/issues/173) restores. The
        // shell used to inset the whole region by the caret and then draw the
        // caret inside that inset, so both the marker and the text moved right
        // together and the list's sigil never lined up with a heading's.
        //
        // **Flush at the edge rather than one cell in, and that is the reader's
        // call rather than the picture's.** The mockup keeps a cell to the left of
        // its caret, on a canvas about a hundred and nine columns wide; #173 asks
        // for the marker *"flush against the pane's left edge with no margin of
        // its own"*, and taking it literally is what makes the gap between caret
        // and sigil grow with the pane instead of appearing and vanishing. §5.3
        // rules the picture binds the element set and each element's promises,
        // never glyph-for-glyph fidelity, and `SPEC.md` §11.1 records this as the
        // one glyph licensed to stand on the pane's own edge so it reads as a
        // decision rather than as drift.
        let left = area.x;

        // From the **pane**, less the caret's gutter and less a scrollbar column
        // whether or not one was taken, for [`affords_caret`]'s reason one element
        // out. `area` has already lost the bar's columns when a bar was drawn,
        // and whether it was is a fact about the *contents*: `scrollable` asks
        // whether the file count outruns the region. Planning from `area` put the
        // whole layout back under the contents' control on that one axis, which
        // is the defect this type exists to remove. It was visible rather than
        // theoretical: at 28 columns a seventh changed file crossed a rung
        // boundary and took the counts cell off every row of the list, and at
        // forty it slid every element two columns sideways.
        //
        // So the bar's columns are paid unconditionally here, exactly as
        // [`affords_caret`] pays them. It costs the path two columns on a pane
        // with nothing to scroll, which is the same trade this type already makes
        // for every glance slot it reserves whether or not a row can fill it.
        // Bound once and used twice, so the slots and the rows drawn into them
        // cannot disagree about how wide the region is. The rows are given this
        // width rather than the region's own, which is what keeps the *anchor*
        // still as well as the rung: every element is placed from the right edge,
        // so a region that grew two columns when the bar went away would slide
        // all of them even while the layout stayed the same. Those two columns
        // are left blank when there is no bar.
        //
        // **`gutter` is zero from forty-three columns up, so this is the stream's
        // own width and the same [`Columns`] plan**, which is what makes the two
        // regions agree about where the path starts as well as the sigil. Below
        // that it is one, and the one column is the residual `caret_gutter`
        // documents.
        let shown = usize::from(area.height);
        let inner = planning_width(pane, gutter);
        let columns = Columns::plan(inner, self.glyphs);
        // Hoisted beside the width it goes with, because it is a property of the
        // pane and not of any row. Saturating for the reason `left` above is:
        // `render` contracts that any area is legal, and an origin near the top
        // of the range is the one part of that contract nothing on screen would
        // ever exercise.
        let origin = area.x.saturating_add(self.inset).saturating_add(gutter);

        for (offset, entry) in view.list.iter().take(shown).enumerate() {
            let y = area.y + offset as u16;
            // Saturating, because `list_top` is not bounded by the file count:
            // a pane too short for a region hands the reader's request back
            // untouched, so `View::collect` can legitimately report `usize::MAX`
            // here. `position_of` guards the identical hazard one region up.
            // **One predicate, two channels.** The glyph and [`CURRENT_WEIGHT`]
            // are the same statement said twice, so they are resolved once here
            // and the weight is handed down rather than re-derived by the drawer.
            // That also ties the weight to [`affords_caret`] for free: `caret` is
            // false below it and neither mark is drawn.
            let current = caret && view.list_top.saturating_add(offset) == view.top.file;
            if current {
                self.put(left, y, CARET, CARET_WIDTH, self.theme.pulse);
            }
            self.file_row(
                Rect {
                    y,
                    height: 1,
                    // `area.x` is the pane's own leading column, not the region's: `render`
                    // builds this rect with `..area` and `with_bar` narrows the *width* on
                    // the right without moving the origin. Worth saying, because everything
                    // else in this function reads `pane` precisely because `area` cannot be
                    // trusted for a width.
                    // **The pane's inset plus whatever the caret could not take
                    // out of it**, which is the stream's own origin exactly
                    // whenever `gutter` is zero.
                    x: origin,
                    width: inner,
                },
                &Heading::of(entry),
                view.scale,
                &columns,
                current,
            );
        }
    }

    /// Decide this region's scrollbar, and hand back the room left for content
    /// along with the shape decided.
    ///
    /// **It stopped drawing on 2026-08-18**
    /// ([#239](https://github.com/breferrari/vigia/issues/239)), which is why it
    /// returns the `Bar` rather than keeping it. Deciding and drawing had to come
    /// apart because the two regions now want them in opposite orders: the diff's
    /// bar draws *after* its content so the row band lands underneath it, and the
    /// list's draws before, where the order is free because a list row carries no
    /// wash. A single call that did both could only serve one of them, and the one
    /// it served silently was the wrong one.
    ///
    /// The consolidation the paragraphs below describe is untouched: the question
    /// is still asked in exactly one place, and its answer is still the return
    /// value. What moved out is the drawing, and each caller now says out loud
    /// when it happens.
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
    ///
    /// **The deciding half now lives in [`bar_for`]**, which is the same
    /// consolidation one layer out: `regions` asks it too, so the pointer and the
    /// screen cannot disagree about whether a bar exists or where its track is.
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
    ///
    /// **`bar` is passed rather than re-decided**, because [`bar_for`] needs the
    /// pane's width and this has only a region's. Re-deriving it here is the one
    /// way the pointer and the screen could come to hold different tracks.
    fn scrollbar(&mut self, area: Rect, whose: Grabbed, bar: Bar, at: u64, span: u64, of: u64) {
        // **Width and height guarded here as well as by the caller.** `render`
        // only calls this above `BAR_FLOOR` and only through `bar_for`, so a zero
        // width cannot reach it today, and the subtractions below would underflow
        // if one ever did. A private method whose safety rests on a condition
        // checked in another function is a panic waiting for the day someone adds
        // a third caller.
        if area.height == 0 || area.width == 0 || !scrollable(span, of) {
            return;
        }

        // **The track, not the region.** A stepped bar spends its first and last
        // row on buttons, and every term below is scaled against what is left:
        // getting one of them to keep the region's own height is precisely the
        // defect the travel comment records, arriving through a different door.
        let (track_top, track_rows) = bar.track(area.y, area.height);
        let rows = u64::from(track_rows);
        if rows == 0 {
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
        // Through [`bar_column`], so the column a pointer is told about and the
        // column this paints in are one formula rather than two that agree.
        let x = bar_column(area);

        // **Lit while the reader is dragging this bar**, which is the same
        // reading the step buttons already carry one block down: bright means
        // *you are doing this now*. The thumb is the thing being moved, so it is
        // the thing that answers.
        // **The thumb carries no hover mark, and the reason is the rule rather
        // than the element: a click has to be brighter than a hover.** The
        // buttons below can honour that because they have three weights to spend
        // — `bar_track` at rest, `bar` under a pointer, `bar_active` under a
        // gesture. A thumb starts at `bar` and ends at `bar_active`, so there is
        // no rung left between them, and using `bar_active` for a hover would
        // make a drag indistinguishable from a pointer merely resting there.
        //
        // Inventing a fourth style is what `SPEC.md` §5.3 refuses (a colour is
        // taken by taking a role, never by being distinct), and adding a theme
        // key is exactly the cost that put the list rows in
        // [#189](https://github.com/breferrari/vigia/issues/189) rather than
        // here. **So the thumb is the same finding as a list row, one element
        // over**: it is a surface a click acts on with nowhere to draw a mark,
        // and it waits for the same ruling.
        // **Three rungs here too, now that there is one to spend.** A drag beats
        // a hover for the reason a press beats one on the buttons below: the
        // brighter reading has to mean *you are doing this*, or the two states
        // stop being tellable apart on the element a reader is actually moving.
        //
        // [#186](https://github.com/breferrari/vigia/issues/186) shipped this
        // with no hover mark at all, reasoning that the thumb rests at
        // [`Theme::bar`] and drags at [`Theme::bar_active`] with nothing
        // between. That was true and the conclusion drawn from it was wrong: the
        // missing rung was a thing to build rather than a reason to decline.
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

        // **The buttons last, and after the track rather than around it.** They
        // sit on rows the loop above never reaches, so the order is not a
        // correctness claim; drawing them here keeps the one arithmetic in one
        // place and leaves this as what it reads like, two cells at the ends.
        //
        // [`Theme::bar_track`] rather than [`Theme::bar`], so at rest the thumb
        // stays the only lit thing on the column and the readout is unchanged: a
        // button is chrome, and shape is what tells it from the track it shares a
        // weight with. It costs no theme field and no palette entry.
        //
        // **Except while it is being pressed**, when it takes the thumb's own
        // style. That is the whole of the feedback and it reuses a colour that is
        // already on this column, so a reader who has learned what lit means on a
        // bar has learned this too. It matters most where nothing else can say
        // anything: pressing *up* at the top of a diff moves no row, and without
        // this the control is indistinguishable from a decoration.
        if matches!(bar, Bar::Stepped) {
            let bottom = area.y + area.height - 1;
            for (y, glyph, way) in [(area.y, STEP_UP, -1isize), (bottom, STEP_DOWN, 1)] {
                // Three ways an arrow lights, and they are one state rather than
                // three: the reader is holding *this* button, the reader is
                // dragging *this* bar, or the keys are moving this region *that*
                // way. All three mean the same sentence, so all three take the
                // same style and the eye learns it once.
                //
                // The key case is the only one with nothing under a finger, and
                // it is why the arrows answer a keyboard at all: a reader holding
                // `j` sees the mark that says which way, on the element whose
                // whole job is which way.
                let held = self.pressed == Some((x, y));
                // **This bar's own scroll, not any scroll.** The two regions move
                // different things: `j` moves the diff's viewport and `J` moves
                // the list's window, so a mark that carried only a direction lit
                // the matching arrow on both bars at once. The region is what
                // says which, exactly as it does for a drag one block up.
                let keyed = self
                    .scrolling
                    .is_some_and(|(on, by)| on == whose && by.signum() == way)
                    && self.gripped != Some(whose);
                // **Three rungs, and a press beats a hover.** `bar_track` at
                // rest, `bar` under a pointer, `bar_active` while a gesture is
                // on it. The ordering is the load-bearing half: #166 added the
                // pressed mark for the case where nothing else can answer, since
                // pressing *up* at the top of a diff moves no row, and a hover
                // that outranked it would take that answer away in exactly the
                // case it was built for.
                //
                // The middle rung costs no theme field and no palette entry: it
                // is [`Theme::bar`], which this column already draws the thumb
                // in, so a reader who has learned what the column's weights mean
                // has learned this too. `SPEC.md` §5.3's rule that a new element
                // earns a colour by taking a role rather than by being distinct
                // is what rules out inventing a fourth.
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
    ///
    /// **Cell by cell, not `set_stringn`.** The same choice the heat strip makes
    /// two functions down, and for a sharper reason here: a string call allocates
    /// per row and then segments the graphemes of a single-character string,
    /// which measured ten times the cost of writing the cell. A bar is
    /// `list + diff` rows of that, twice a screen.
    ///
    /// Clipped, for the reason the heat strip gives.
    fn bar_cell(&mut self, x: u16, y: u16, glyph: char, style: Style) {
        if let Some(cell) = self.buf.cell_mut((x, y)) {
            // **The band's background is kept; the bar owns everything else.**
            //
            // Written field by field rather than through `set_style`, and that is
            // the whole point rather than a style preference. Since
            // [#239](https://github.com/breferrari/vigia/issues/239) the row wash
            // runs under this column, so the cell arrives already painted, and
            // `Cell::set_style` would leave two of the wash's marks on it:
            // `bg`, which is wanted, and any **modifier**, which is not.
            // `ratatui-core-0.1.2/src/buffer/cell.rs:204` does
            // `modifier.insert(add_modifier)` **unconditionally**, gated on nothing,
            // so a wash carrying one hands it to the bar.
            //
            // That is user-reachable rather than theoretical: `VIGIA_THEME` accepts
            // a trailing modifier word on every key `Theme::KEYS` names, row washes
            // included, so `removed_row = on #45222a reverse` would put `REVERSED`
            // on the thumb and swap the two colours every contrast gate in
            // `tests/palette.rs` was written to prove. Found by round 2 of #239's
            // own audit, on the change that introduced it.
            //
            // So the modifier is **assigned**, not merged, and the foreground is
            // taken from the style with the background put back underneath. The bar
            // then reads identically whatever the row behind it says, except for the
            // one field it deliberately borrows.
            //
            // **Both colours fall back to the cell, and the symmetry is the
            // point.** A bar style that declares neither leaves the band showing
            // through, which is every shipped palette. One that declares a
            // background gets it, opting that palette out of the band under its
            // own bar rather than having the value silently dropped: a theme file
            // may write `bar_track = #57606a on #21262d`, and a draw site that
            // discarded it would be the failure this module's own parser notes
            // rail against, one that reports no error and changes nothing.
            //
            // The foreground's fallback is the same shape and matters more,
            // because the cell it falls back to is the **wash's** foreground under
            // a band. A bar style without one would draw the diff's colour and
            // look deliberate. `palette.rs::every_bar_style_says_what_bar_cell_reads`
            // is what stops a palette shipping that.
            let behind = cell.bg;
            cell.set_symbol(glyph.encode_utf8(&mut [0u8; 4]));
            cell.fg = style.fg.unwrap_or(cell.fg);
            cell.bg = style.bg.unwrap_or(behind);
            cell.modifier = style.add_modifier;
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
            // Clipped, for the reason the heat strip gives.
            let Some(cell) = self.buf.cell_mut((x, area.y)) else {
                continue;
            };
            cell.set_symbol(glyph).set_style(style);
        }
    }

    /// Draw the gestures sheet over whatever the regions already drew.
    ///
    /// **Last of everything, and that is the whole mechanism** (`SPEC.md` §11.1's
    /// B12). Nothing above this method knows the sheet exists: no rect is smaller
    /// for it, no region yielded a row to it, and closing it needs no relayout,
    /// because the cells it covered are redrawn by the next frame like every other
    /// cell on the pane.
    ///
    /// The blank pass is `ratatui::widgets::Clear`'s job done cell by cell, for
    /// the reason [`Painter::rule`] gives about building strings, and it is not
    /// optional: without it the table's own gaps would show the diff through, and
    /// a half-transparent sheet reads as a rendering fault rather than as a
    /// window.
    ///
    /// **`Cell::reset` and not `set_style`, which is the defect this shipped
    /// with.** `set_style` *patches*: it merges the style it is given into the one
    /// already in the cell, so a background nothing overwrites survives. Every
    /// added and removed row under the sheet kept its wash, and the sheet drew as
    /// green and red bands with the table's text on top. `Theme::chrome_dim`
    /// carries a foreground and no background, so there was nothing in the patch to
    /// displace them. Reported from use within an hour of the release.
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
                    // **Reset first, then style.** `reset` puts the cell back to a
                    // space with default colours and no modifiers, which is the
                    // pane's own background: the sheet is a hole punched in the
                    // diff rather than a tint over it. Styling without resetting is
                    // what let the rows beneath show through.
                    cell.reset();
                    cell.set_symbol(" ").set_style(frame);
                }
            }
        }

        // The title bar, with the close control's cell left blank and written
        // after it in its own weight: a control is not part of the frame it sits
        // in, which is [#166](https://github.com/breferrari/vigia/issues/166)'s
        // rule for the step buttons one element over.
        let mut top = String::with_capacity(width * 3);
        top.push('┌');
        top.push_str(SHEET_TITLE);
        // Corner, title, dashes, then the three cells the control sits in and the
        // closing corner: `width - 16` of rule for an eleven-column title.
        for _ in 0..width.saturating_sub(width_of(SHEET_TITLE) + 5) {
            top.push(RULE);
        }
        top.push_str("   ┐");
        self.put(area.x, area.y, &top, width, frame);
        // **The control takes a hover rung, which is what says it is clickable.**
        // B10's ladder, and the same three weights the step buttons use one element
        // over: chrome at rest, [`Theme::bar_hover`] under the pointer, and
        // [`Theme::bar_active`] while it is being pressed. A close control that
        // never brightened was a glyph a reader had to guess at, reported from use
        // with the wash defect above.
        let pressed = self.pressed == Some(plan.close);
        let hovered = self.hovered == Some(Hovered::Button(plan.close.0, plan.close.1));
        let control = if pressed {
            self.theme.bar_active
        } else if hovered {
            self.theme.bar_hover
        } else {
            lit
        };
        self.put(
            plan.close.0,
            plan.close.1,
            &SHEET_CLOSE.to_string(),
            1,
            control,
        );

        let mut y = area.y + 1;
        let rows = KEYBOARD[plan.from..].iter();
        for row in rows {
            self.sheet_row(area, y, row, plan);
            y += 1;
        }

        if plan.mouse {
            // The group's own rule, which is the same shape the title bar has and
            // for the same reason: a heading inside a table is furniture, so it
            // runs to the frame rather than standing back from it.
            let mut label = String::with_capacity(width * 3);
            label.push('│');
            label.push_str(" mouse ");
            for _ in 0..width.saturating_sub(width_of(" mouse ") + 3) {
                label.push(RULE);
            }
            label.push_str(" │");
            self.put(area.x, y, &label, width, frame);
            y += 1;
            for row in MOUSE.iter() {
                self.sheet_row(area, y, row, plan);
                y += 1;
            }
        }

        let mut bottom = String::with_capacity(width * 3);
        bottom.push('└');
        for _ in 0..width.saturating_sub(2) {
            bottom.push(RULE);
        }
        bottom.push('┘');
        self.put(area.x, area.y + area.height - 1, &bottom, width, frame);
    }

    /// One row of the sheet: the keys cell lit, the verb dim, the frame at both
    /// ends.
    ///
    /// **Both cells are clipped to their planned field and never truncated with a
    /// mark**, which is the difference between this table and a line of content:
    /// the plan measured every row it was going to draw, so a cell that did not
    /// fit would mean the plan was wrong rather than that the pane is narrow.
    fn sheet_row(&mut self, area: Rect, y: u16, row: &Gesture, plan: &SheetPlan) {
        let right = area.x + area.width - 1;
        self.put(area.x, y, "│", 1, self.theme.chrome_dim);
        self.put(
            area.x + 2,
            y,
            row.keys[plan.level],
            plan.keys,
            self.theme.chrome,
        );
        self.put(
            area.x + 2 + plan.keys as u16 + 2,
            y,
            row.verb[plan.level],
            plan.verb,
            self.theme.chrome_dim,
        );
        self.put(right, y, "│", 1, self.theme.chrome_dim);
    }

    fn body(&mut self, area: Rect, washed: u16, view: &View, pane: u16) {
        // **Two rects, because this region draws both roles.** A heading is placed
        // against the pane through [`planning_width`]; everything else here keeps
        // the region's own right edge and only stands back from it, through
        // [`Painter::region_text`], which is where the reason lives. The region
        // rect itself stays untouched for the wash, which is furniture.
        //
        // Resolved once rather than per row, since both are properties of the
        // pane and the region rather than of any line.
        let glyphs = self.region_text(area, pane);
        if view.files == 0 {
            self.put_marked(
                glyphs.x,
                glyphs.y,
                &empty_state(),
                usize::from(glyphs.width),
                self.theme.chrome_dim,
            );
            return;
        }

        // The stream's own width rather than the list's: the two regions are
        // different widths, and `SPEC.md` §11.1 rules they need not align glyph
        // for glyph. Neither reads a row, so a heading scrolling past cannot
        // move the pinned region above it, nor its own neighbours.
        //
        // From the pane and net of a scrollbar column whether or not one was
        // drawn, for the reason [`Painter::list`] carries in full: this region's
        // bar appears when the diff outgrows the pane, so planning from `area`
        // let the diff's own height decide the layout of every heading in it.
        let shown = usize::from(area.height);
        let inner = planning_width(pane, 0);
        let columns = Columns::plan(inner, self.glyphs);

        // **The gutter comes from the same width, and that is
        // [#77](https://github.com/breferrari/vigia/issues/77)'s ruling one
        // element over.** `area` has already lost the scrollbar's columns when a
        // bar was drawn, and the stream's bar appears when the diff outgrows the
        // pane, so measuring the gutter against it made the line numbers a
        // function of the *diff's height*. At 29 and 30 columns a diff crossing
        // the pane height took the whole gutter off every content row: not a
        // reflow but an all-or-nothing disappearance, on the tick an agent's
        // edit made the diff long.
        self.gutter = gutter_width(view, usize::from(inner));
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
                    &Heading::of(entry),
                    view.scale,
                    &columns,
                    // **A literal, which is the whole reason this is a
                    // parameter.** No heading in the stream is *the* file the
                    // diff is inside; every one of them is a file the diff
                    // contains. Saying so here confines the mark by
                    // construction, where a mark the painter carried would be
                    // confined only by the two regions never sharing a `y`.
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
                // **Empty on purpose, and the arm exists to say so.** A gap is
                // the blank that closes a file's block
                // ([#165](https://github.com/breferrari/vigia/issues/165)), and
                // an unwritten row is already blank: it is what every row below
                // a short diff has always been. Writing spaces would be the same
                // screen at the cost of a row of cells, and leaving the variant
                // out of this match is not available, which is the half that
                // makes the silence safe rather than an omission.
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
                } => {
                    // **One row tall, explicitly.** `..area` inherits the body's
                    // whole height, which every other row drawer got away with
                    // because they only ever read `x`, `y` and `width`. This is
                    // the first row that paints a *region*, and an inherited
                    // height made the wash a rectangle running from the line down
                    // to the bottom of the pane: the context rows under it, the
                    // blank rows under those, and the footer. Measured at 200x60,
                    // that was 366,000 cells a frame where 12,000 will do.
                    //
                    // **Two rects, because this row has both roles on it.** The
                    // wash is furniture and takes the region's own width; the
                    // glyphs take the pane-planned width every other row here
                    // takes. Handing one rect and letting the drawer derive the
                    // other is what charged the trailing margin twice.
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
                        *number,
                        text,
                        spans,
                    );
                }
            }
        }
    }

    /// `M src/engine/watch.rs                ● ████████████ __▁▂▆▄▆█   +42    -7`
    ///
    /// Everything to the right of the path goes into a slot [`Columns`] already
    /// chose, drawn right to left so each block knows where the one outside it
    /// ended. **Nothing here is sized from this row**, which is what makes the row a
    /// column rather than a cluster: the widths are the region's, not this row's,
    /// so a row with nothing to put in a slot **keeps** it rather than closing
    /// it. What it puts there differs by element and that difference is a ruling
    /// rather than an inconsistency: the sparkline fills its slot with track, so
    /// the two leading `_` above are a window with no writes in its oldest
    /// quarter ([#78](https://github.com/breferrari/vigia/issues/78)), while a
    /// file with no line diff leaves the heat strip's slot blank for the reason
    /// [`heat_at`] gives. The drawn order, left to right, is pulse, heat strip,
    /// sparkline, counters; the order they *survive* narrowing in is the layout
    /// table's.
    ///
    /// Nothing is allowed to take the path below [`MIN_PATH_WIDTH`]. A glance
    /// element that cost a reader the name of the file would be spending the
    /// content to decorate it.
    fn file_row(
        &mut self,
        area: Rect,
        heading: &Heading<'_>,
        scale: Scale,
        columns: &Columns,
        current: bool,
    ) {
        let mut right = area;

        // **Every slot is subtracted whether or not this row fills it**, which is
        // the whole of [#77](https://github.com/breferrari/vigia/issues/77)'s
        // ruling: a row without a sparkline used to
        // let its neighbours' elements slide right into the space, and a row
        // with a two-column-narrower counts cell moved everything outside it.
        // Both were ordinary rather than exotic, since `spark_of` yielded nothing
        // until a file had been written once and `heat_at` yields nothing for a
        // file with no line diff.
        //
        // **The sparkline half of that is gone and the ruling is what survives
        // it.** Since [#78](https://github.com/breferrari/vigia/issues/78)
        // `spark_of` is total, so no row can fail to fill that slot; the strip's
        // can still be empty, and reserving from the pane is what keeps the two
        // cases from being visible to a reader as two different layouts.
        if columns.cell > 0 {
            // Each half right-anchored in its own sub-column, so the digits
            // change under a reader without moving anything beside them, and an
            // eye running down the additions of three files compares them. Same
            // shape the status bar's frame and memory cells already use, one
            // element wider.
            //
            // Two statements rather than a loop over the pair: the offset from
            // the right edge coincides with the width for one half and not the
            // other, and a loop asks a reader to work that out.
            //
            // **The ink is per half and comes with the text**, which is
            // [`counts_of`]'s ruling: green and red are lent to a half that has
            // something to say and withheld from a `-0` that has not. Their
            // *presence* is still shared, because `columns.cell` drops the pair
            // together.
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
        // ended. The strip drawn is the **whole** window at every rung since
        // [#234](https://github.com/breferrari/vigia/issues/234); it used to be
        // the tail of one, and the oldest were on the left.
        //
        // **Unconditional past the width check**, where this used to skip a row
        // whose file had no history: since
        // [#78](https://github.com/breferrari/vigia/issues/78) an empty bucket
        // draws the track, so the reserved slot is always filled and a launch
        // into a worktree that was already dirty draws the column rather than a
        // blank.
        // **Resolved once for the row, in cells.** `Columns::spark` counts
        // buckets of the window and a cell may hold more than one of them, so
        // every consumer that measures the *slot* has to convert. Doing it here
        // is what stops the drawer and the advance below from ever disagreeing
        // about how wide this element is.
        let slot = spark_cells(columns.spark, self.glyphs);
        if columns.spark > 0 {
            // Cell by cell rather than as one string, for the reason the heat
            // strip below gives and one more that the strip does not have: the
            // style differs *per cell* now, so a single styled write could not
            // draw this row anyway. It also drops the per-row `String` the tail
            // used to collect into.
            // The precondition the slice below rests on, checked here rather
            // than assumed, for `Painter::scrollbar`'s stated reason: a private
            // method whose safety rests on a condition held in another function
            // is a panic waiting for the day someone adds a third caller.
            // `SPARK_RUNGS` derives every rung from `HISTORY_BUCKETS`, but
            // `ROW_LAYOUTS` is a hand-written table and `Columns::new` takes a
            // bare `usize`.
            //
            // **On the ladder rather than merely dividing, since
            // [#234](https://github.com/breferrari/vigia/issues/234)**, and the
            // difference is a silent wrong picture rather than a loud one.
            // [`spark_of`] groups `HISTORY_BUCKETS / rung`, and [`Scale::at`]
            // looks the yardstick up by that grouping: a rung of eight divides
            // twenty-four perfectly well, groups to three, finds no such grouping
            // on the table and falls back to the finest figure, so every height
            // and every band on the row would be measured against a denominator
            // set for a different width. Nothing panics and no gate reddens.
            // Checking membership is what refuses that, where checking
            // divisibility alone let it through.
            debug_assert!(
                SPARK_RUNGS.contains(&columns.spark),
                "a layout asked for {} sparkline buckets, which is not a rung of \
                 {SPARK_RUNGS:?}, so its yardstick would be one set for another \
                 width",
                columns.spark
            );
            // **Counted in cells rather than buckets, which is the one thing the
            // glyph rung changes here.** `columns.spark` is a resolution of the
            // *window*, and how many columns that costs is the terminal's
            // business: at a dense rung twenty-four buckets arrive in twelve
            // cells, so a strip measured in buckets would advance the cursor
            // twice as far as it drew and slide every element left of it.
            let strip = spark_of(heading.spark, columns.spark, scale, self.glyphs);
            // The cells `spark_of` actually filled. **This rung rather than the
            // whole window since
            // [#234](https://github.com/breferrari/vigia/issues/234)**: the
            // projection happens in there now, so a narrower rung fills fewer
            // cells with wider buckets instead of filling the same cells and
            // handing back a tail to slice.
            //
            // **Clamped as well as asserted, because the assert is not in the
            // binary that ships.** `debug_assert!` compiles out under
            // `--release`, and `[profile.release]` sets `panic = "abort"`, so a
            // rung wider than the window would index the slice out of range and
            // take the process down without restoring the terminal: I8's failure
            // reached by the one path that skips its handler. The assert keeps
            // the message a developer needs and this keeps the promise a reader
            // needs.
            //
            // **This and [`spark_of`]'s own early return are one hazard guarded
            // at two layers, not the same guard twice.** A hand-written
            // `ROW_LAYOUTS` entry above the window breaks two different things:
            // over there it divides to a group of zero and `chunks` asserts, and
            // here it would slice past what the array holds. Neither clamp can
            // reach the other's case, so both stay, and the value below never
            // moves from `slot` for any rung the ladder actually produces.
            //
            // **Clamped to the room as well, and that is a second promise rather
            // than the same one twice.** `Columns::plan` guarantees `right.width`
            // exceeds every reserved slot, so the two are the same number today;
            // one says the strip fits what was projected, the other says it fits
            // the rect it was handed. The room term is unreachable today, since
            // `Painter::list` passes the same `inner` width `plan` used and moves
            // `x` rather than the width. It stays because what it guards is
            // arithmetic rather than a claim about callers: a bare subtraction
            // does not even panic in release, since `u16` wraps, `x` lands near
            // the top of the range and `x + offset` wraps back into the pane, so
            // a strip wider than its rect would draw in the wrong column rather
            // than not at all. The heat strip below has the identical expression.
            let filled = slot.min(spark_cells(HISTORY_BUCKETS, self.glyphs));
            let take = filled.min(right.width as usize);
            let x = right.x + right.width.saturating_sub(take as u16);
            for (offset, bucket) in strip[filled - take..filled].iter().enumerate() {
                // Both out of the one value, which is [`Bucket`]'s whole reason.
                let (glyph, style) = bucket.drawn(self.theme, self.glyphs);
                // `set_char` rather than an `encode_utf8` into a local buffer,
                // which is what `Cell::set_char` does internally: the heat strip
                // below can hoist its encode out of the loop because every slice
                // is the same glyph, and this cannot, so the hand-rolled form
                // here would be the library's own three lines written again.
                //
                // `cell_mut` for `Painter::scrollbar`'s reason: `render`'s
                // contract is that any area is legal, and a direct index would
                // abort the whole paint on a strip wider than its buffer.
                if let Some(cell) = self.buf.cell_mut((x + offset as u16, right.y)) {
                    cell.set_char(glyph).set_style(style);
                }
            }
        }
        // **`slot` here and `take` above, deliberately.** `slot` is
        // `columns.spark` measured in cells, and the two differ
        // only in the impossible case the clamp exists for, and there the layout
        // is what everything left of this must agree with: the slot was reserved
        // at the planned width, so the cursor moves past the planned width or
        // every element inside it shifts. A rung the projection could not fill
        // would then draw what it had in a wider slot and leave the remainder
        // blank, which is a degraded row rather than a corrupted one.
        past(&mut right, slot);

        // Unguarded, because `heat_at` opens by returning nothing for a zero
        // width, so an outer `if` would be the same precondition twice.
        let heat = heat_at(heading.heat, columns.heat);
        if !heat.is_empty() {
            // Cell by cell rather than as one string: every slice is the same
            // glyph and only the style differs, which is the whole design.
            //
            // Written through `set_symbol` rather than `set_stringn`, for the
            // reason `Painter::scrollbar` gives: a string call allocates per
            // cell and then segments the graphemes of a single-character
            // string, and there are twelve of these on every file row of every
            // frame.
            let mut glyph = [0u8; 4];
            let glyph = HEAT_SLICE.encode_utf8(&mut glyph);
            let x = right.x + right.width - heat.len() as u16;
            for (offset, slice) in heat.iter().enumerate() {
                // `cell_mut` rather than `Index`, because this used to be a
                // `set_stringn` call and that clipped. Trading it for a direct
                // write to stop the per-cell allocation traded away the clipping
                // too, and `render`'s contract is that any area is legal: an
                // area wider than its buffer aborted the whole paint on the one
                // row carrying a strip.
                if let Some(cell) = self.buf.cell_mut((x + offset as u16, right.y)) {
                    cell.set_symbol(glyph).set_style(self.theme.heat(*slice));
                }
            }
        }
        past(&mut right, columns.heat);
        // Into its reserved slot like everything else, so a file starting or
        // stopping to pulse moves nothing.
        if heading.recency == Recency::Pulse && !columns.pulse.is_empty() {
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
        let letter = format!("{} ", heading.kind);
        let x = self.put(area.x, area.y, &letter, room, self.theme.kind);
        room = room.saturating_sub(usize::from(x - area.x));

        // Which file it *was* is the whole content of a rename, so it is part of
        // the label rather than something to reveal on a keypress.
        //
        // **But it is a rung, not part of the token**, and that is a fix rather
        // than a refinement. `elide_head` cuts the head because a path's *tail*
        // identifies the file, and that premise is false of `new ← old`: cutting
        // the head of the pair leaves `…src/main.rs`, which names the file the
        // rename came *from* and never mentions the one on screen. The whole
        // pair or the new path alone, never a cut that changes the subject.
        //
        // Latent before the row had fixed slots and ordinary after: the pair
        // stopped fitting at 107 columns where it used to stop at 60.
        let full = heading
            .from
            .map(|from| format!("{} ← {from}", heading.path));
        let label = match &full {
            Some(pair) if width_of(pair) <= room => pair.as_str(),
            _ => heading.path,
        };
        // **Two marks on one path, and they are resolved on two channels rather
        // than as a priority order.** The pointer chooses the colour and the
        // underline; the caret adds the weight. Ranking them would cost a fact
        // the reader has on screen: whichever lost would take either *where the
        // pointer is* or *which file the diff is in* off the row.
        //
        // §5.3 rules that intensity carries recency and nothing else, so a hover
        // that merely brightened would say *recent* about a file nothing had
        // touched. [`Theme::path_hover`] answers that with the **underline**,
        // which no recency weight carries at any depth. It used to answer with
        // brightness as well and that half is gone
        // ([#193](https://github.com/breferrari/vigia/issues/193)): a mark that
        // can go stale outranking every claim about the worktree is the thing
        // §5.3's own B10 rule forbids.
        //
        // Hover answers on the **list's** rows only. `Hovered::Row` is resolved
        // from the list region alone, and a diff heading's row is never inside
        // it, so this comparison cannot light one: the diff is not clickable and
        // a mark there would imply it is. `current` is confined more strongly
        // still, being a literal `false` at the stream's call site rather than a
        // comparison that happens never to match.
        let ink = if self.hovered == Some(Hovered::Row(area.y)) {
            self.theme.path_hover
        } else {
            self.theme.recency(heading.recency)
        };
        let ink = if current {
            ink.add_modifier(CURRENT_WEIGHT)
        } else {
            ink
        };
        self.put(x, area.y, &elide_head(label, room), room, ink);
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
    fn line_row(
        &mut self,
        area: Rect,
        glyphs: Rect,
        kind: LineKind,
        number: u32,
        text: &str,
        spans: &[Span],
    ) {
        let (diff, sigil) = match kind {
            LineKind::Added => (self.theme.added, '+'),
            LineKind::Removed => (self.theme.removed, '-'),
            LineKind::Context => (self.theme.context, ' '),
        };

        let (wash, bar) = match kind {
            LineKind::Added => self.theme.row(true),
            LineKind::Removed => self.theme.row(false),
            LineKind::Context => (Style::new(), Style::new()),
        };
        let sigil_style = diff;
        if wash.bg.is_some() {
            self.buf.set_style(area, wash);
        }

        // **§5.1's left bar, and it costs no column**
        // ([#218](https://github.com/breferrari/vigia/issues/218)). The pane's
        // leading cell is blank margin that the wash above has already bled under,
        // so setting its background spends nothing that was carrying content. It is
        // therefore a **width rung**: drawn wherever [`inset_of`] lends a column,
        // absent below forty-three where it does not and where the wash and the
        // sigil carry the signal alone.
        //
        // **The leading cell of the inset, never all of it.** At eighty columns the
        // margin is two and the bar takes the first, which is the picture's own
        // proportion: three pixels of a cell, hard against the window's edge, with
        // clear space between it and the text.
        //
        // Background only, and gated on the background surviving: a bar whose
        // colour was dropped by the depth ladder would otherwise paint a blank cell
        // in nothing at all, which is a no-op that still costs a write. `area.x` is
        // the pane's own leading column here, because `render` builds this region
        // with `..area` and `with_bar` narrows the width without moving the origin.
        if bar.bg.is_some() && self.inset > 0 {
            if let Some(cell) = self.buf.cell_mut((area.x, area.y)) {
                cell.set_style(bar);
            }
        }

        // **The wash above took the whole row and the glyphs take the inset
        // one**, which is the half of `SPEC.md` §5.3 that makes the inset design
        // rather than padding: a band the content sits *on* reads as a band, and
        // a band that stopped where the text stopped would read as a highlight
        // someone misaligned. [#119](https://github.com/breferrari/vigia/issues/119)
        // is explicit that it is the pane's furniture that bleeds and only its
        // text that stands back.
        //
        // The wash is still `area` rather than the pane on a screen with a
        // scrollbar, and that is the existing ruling
        // `a_wash_runs_under_the_scrollbar_column` rather than something this
        // changed. That gate was `a_wash_stops_before_the_scrollbar_column` until
        // [#239](https://github.com/breferrari/vigia/issues/239) inverted it; the
        // sentence above is unaffected either way, because `area` is the region
        // and the question is only how much of it the wash covers.
        let mut x = glyphs.x;
        let mut room = usize::from(glyphs.width);
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
        //
        // **Three fixed runs and not two**, which #164 moved and which is worth
        // the word because the term is invisible from outside: the sigil, its
        // gap, and the tail `content_runs` pushes for whatever the spans did not
        // reach. Left at two the hint was a run short exactly when `spans` is
        // empty, which is **every content row of a process's first frame** (the
        // shell draws that one plain, and it is the frame I7 gives 50ms to), so
        // each of those rows grew its `Vec` from 2 to 4. Measured with a counting
        // allocator over a 40-row body: 3 allocations and 111 bytes a row before
        // the gap, 5 and 272 with the gap at `+ 2`, 4 and 152 at `+ 3`. The
        // `.min` arm is unchanged and still has room: two fixed runs plus at most
        // `room - 2` content runs is `room`.
        let mut runs = Vec::with_capacity((spans.len() + 3).min(room + 2));
        runs.push((sigil.to_string(), sigil_style));

        // **The gap `assets/preview.svg` has drawn since before any of this
        // existed** ([#164](https://github.com/breferrari/vigia/issues/164)).
        // The picture states its own grid in a comment, one cell at 13.5px being
        // ~8.1px, and it puts the sigil at x=72 and every content origin at
        // x=88: the sigil is one cell, 72 to 80.1, so a clear column has always
        // stood between the two. §5.1's departure list is meant to be the
        // complete set of licensed disagreements with the picture and this was
        // not on it, so it was an omission rather than a decision.
        //
        // **It does not ladder**, where every other spacing decision here does.
        // The sigil column *is* the diff signal at any depth or on any palette
        // that cannot wash the row, which §5.1 records and `tests/colour.rs`
        // gates, so a column that keeps that signal legible is part of the
        // signal rather than decoration beside it, and I6's "every cell is
        // contested" does not reach it. A gap that vanished at 43 columns would
        // take the signal's legibility with it exactly where the pane is most
        // crowded.
        //
        // **Its own run, and styled `diff` rather than `sigil_style`.** Folding
        // it into the sigil's own string is the obvious form and it is wrong:
        // `sigil_style` carries the row's bar where a theme sets one, and §5.1
        // rules that bar to be the sigil cell, so a two-character string would
        // draw it two columns wide. `diff` is the same foreground *before* that
        // patch, which sets no background at all: the wash shows through for the
        // reason this function's own doc gives, and it is invisible on a space
        // either way. It is not `Style::new()` because [`Painter::put_runs_marked`]
        // seeds its continuation mark from the last run it managed to write, and
        // at the one width where the mark lands immediately past this gap an
        // unset style would draw it in nothing at all.
        runs.push((SIGIL_GAP.to_owned(), diff));

        // Tab stops are counted from the start of the line's own content, not
        // from the left edge of the screen. The gutter and the sigil shift every
        // row by the same amount, so including them would align tabs to the
        // buffer and leave the file's indentation looking nothing like it does in
        // an editor. The counter therefore runs **across** span boundaries: a tab
        // in the middle of a line advances to the next stop measured from the
        // line's own start, not from the start of whatever run it landed in.
        // The sigil and its gap are pushed before the counter starts, so what is
        // left for content is everything but them, and the counter's own origin
        // is unmoved: [`Painter::content_runs`] opens at zero whatever precedes
        // it, which is what keeps a tab measured from the line rather than from
        // the buffer.
        //
        // **This is the bound, and it is what makes a row cost the pane rather
        // than the line.** Every run below stops here, and the loop stops asking
        // for runs once it is spent, so a 531-column line in a 74-column pane is
        // walked 74 columns deep instead of 531. Measured before it existed: a
        // 22-row body of Japanese examined 8231 characters to show 1600 columns,
        // which is 5.1x, and `tests/paint.rs` is what fails if it comes back.
        let content = room.saturating_sub(SIGIL_WIDTH);
        // **The agreement with [`gutter_width`], asserted rather than left to be
        // read.** That function rules the line numbers affordable by measuring a
        // pane against [`line_origin`]; this reaches the same number in two
        // steps, since `room` has already lost the gutter and its space. The two
        // answered differently for one commit and nothing said so, because one of
        // them is a *threshold* and no drawing gate can see a threshold. Compiled
        // out of the shipped binary, which is the honest limit of it: what holds
        // this in release is
        // `legibility.rs::a_drawn_gutter_leaves_the_text_its_floor`.
        debug_assert_eq!(
            content,
            usize::from(glyphs.width).saturating_sub(line_origin(self.gutter)),
            "the content bound and the gutter's affordability rule disagree \
             about what a row spends before its first character"
        );
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
    // **Through [`line_origin`] rather than a literal**, which is #164's own
    // correction: this was `digits + 2`, exact while the sigil stood alone and a
    // column behind the moment the sigil got its gap, so the gutter survived on
    // 23 columns of text where `MIN_TEXT_WIDTH` rules 24. It is the one site
    // reading this quantity as a *threshold* rather than drawing with it, which
    // is why no drawing gate could see it.
    if width.saturating_sub(line_origin(digits)) >= MIN_TEXT_WIDTH {
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

#[cfg(test)]
mod tests {
    //! What [`Painter::scrollbar`] draws when two regions start on one row.
    //!
    //! **`tests/render.rs` cannot reach this, which is why it is here.** That
    //! file drives the whole of [`render`], and every layout it can ask for
    //! stacks the list above the diff, so `list.y < diff.y` on every fixture
    //! that exists. A drawer that told the two apart by the `y` of the rect it
    //! was handed was therefore correct on every screen anyone could draw and
    //! wrong on the one [#252](https://github.com/breferrari/vigia/issues/252)
    //! draws, and no gate above this level could see the difference.
    //!
    //! This calls the private drawer with the rail's own shape: same `y`, same
    //! height, different columns. It is the smallest thing that can express the
    //! case at all.

    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    use super::*;

    /// Paint both regions' bars into one buffer, side by side over one row range.
    ///
    /// Returns the buffer, and the two bar columns are 29 and 79. The only thing
    /// telling the regions apart is the [`Grabbed`] each is painted with: their
    /// `y` and their height are deliberately identical.
    fn rail(gripped: Option<Grabbed>, scrolling: Option<(Grabbed, isize)>) -> Buffer {
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
            hovered: None,
            scrolling,
        };
        // Scrollable by a wide margin, so both bars draw a thumb well short of
        // their track and the arrows exist to be read.
        painter.scrollbar(list, Grabbed::List, Bar::Stepped, 0, 10, 100);
        painter.scrollbar(diff, Grabbed::Diff, Bar::Stepped, 0, 10, 100);
        buf
    }

    #[test]
    fn only_the_scrolled_regions_arrows_light_when_both_start_on_one_row() {
        // **The assertion that would have caught 0.5.0**, on the layout that
        // brings the defect back. A bare direction lit the matching arrow on
        // both bars; a direction plus a row fixed it only for as long as the two
        // rows differ.
        let theme = Theme::default();
        let buf = rail(None, Some((Grabbed::Diff, -1)));
        let fg = |x: u16, y: u16| buf[(x, y)].style().fg;

        assert_eq!(
            fg(79, 1),
            theme.bar_active.fg,
            "the scrolled region's own up arrow did not light"
        );
        assert_eq!(
            fg(29, 1),
            theme.bar_track.fg,
            "scrolling the diff lit the *list's* up arrow, because both bars \
             start on one row"
        );
        // The direction half, so a mark that lit its whole bar would still fail.
        assert_eq!(
            fg(79, 20),
            theme.bar_track.fg,
            "the arrow the scroll moves away from lit"
        );
    }

    #[test]
    fn only_the_gripped_regions_thumb_lights_when_both_start_on_one_row() {
        // The drag mark, on the same fixture. `Chrome::gripped` carried its
        // region from the start and was correct throughout, and it was correct
        // *because* the tops differed rather than because it said which region.
        let theme = Theme::default();
        let buf = rail(Some(Grabbed::Diff), None);
        let thumbs = |x: u16| {
            (2..20)
                .filter(|y| buf[(x, *y)].symbol() == BAR_THUMB.to_string())
                .map(|y| buf[(x, y)].style().fg)
                .collect::<Vec<_>>()
        };

        let (list, diff) = (thumbs(29), thumbs(79));
        assert!(
            !list.is_empty() && !diff.is_empty(),
            "the fixture drew no thumb on one of the bars"
        );
        assert!(
            diff.iter().all(|f| *f == theme.bar_active.fg),
            "the gripped region's thumb did not light"
        );
        assert!(
            list.iter().all(|f| *f == theme.bar.fg),
            "gripping the diff lit the *list's* thumb, because both bars start \
             on one row"
        );
    }
}
