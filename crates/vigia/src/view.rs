//! One screenful, and nothing more than one screenful.
//!
//! This is the only module that touches a [`vigia_core::Frame`], and it is where
//! the shell keeps its half of I4:
//!
//! > Streams, never buffers. First paint is independent of total diff size.
//!
//! The core makes that possible by fetching content per file, on demand. It
//! cannot make it happen, because whether a frame reads one file or a thousand
//! is decided by whoever calls [`vigia_core::Frame::diff`]. So [`View::collect`]
//! walks forward from the scroll position and stops the moment the viewport is
//! full, which means a frame reads exactly the files it draws. `tests/reads.rs`
//! is the gate on that.
//!
//! The same rule applies *within* a file, and it is the reason this module looks
//! more careful than it needs to. Building a file's rows and then discarding the
//! ones above the window would clone every line of a hundred-thousand-line file
//! to show twenty-four of them, every frame. Rows above the window are counted
//! rather than built, and a hunk entirely above it is skipped by arithmetic, so
//! the work follows the viewport rather than the file.
//!
//! Everything here is owned rather than borrowed, for two reasons.
//! `Frame::diff` hands back a borrow derived from `&mut self`, so a renderer
//! cannot hold two of them and could not build a screen out of borrows at all.
//! And owning the rows is what lets the renderer be tested without a repository:
//! [`vigia_core::FileChange`] cannot be constructed outside its crate, so a view
//! assembled by hand is the only version of one a snapshot test can reach.

use vigia_core::{
    ChangeKind, FileDiff, Frame, HISTORY_BUCKETS, Highlighter, History, LineKind, Pass, Recency,
    Result, Span,
};

/// One changed file, as everything a row about it needs to be drawn.
///
/// **Its own type because it is drawn in two places.** `SPEC.md` §11.1 makes the
/// body two regions, and the pinned file list and the heading inside the diff
/// stream draw the same thing through the same [`crate::render`] path. Two
/// structurally identical shapes would be two degradation ladders to keep in
/// step and two sets of gates to write, and the picture's own split of the
/// elements across the regions is the thing §5.1 records as *not* kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    /// Repository-relative path.
    pub path: String,
    /// Where the content came from, for a rename or a copy.
    pub from: Option<String>,
    /// One letter naming what happened.
    pub kind: char,
    /// Lines added and removed, or `None` when there is no line-level diff.
    pub churn: Option<(u32, u32)>,
    /// This file's churn over the glance window, oldest bucket first.
    ///
    /// Raw counts rather than heights. Which glyph a count becomes is the
    /// renderer's, the same way a [`Row::Line`] carries its spans as classes
    /// and lets the renderer pick the colour: the scale is shared across the
    /// screen and lives on [`View::scale`].
    ///
    /// All zeroes for a file `vigia` has not seen change, which is the
    /// ordinary case for a worktree that was already dirty at startup.
    pub spark: [u32; HISTORY_BUCKETS],
    /// How recently this file changed, which is what dims a settled row and
    /// what puts the pulse on one that just moved.
    pub recency: Recency,
    /// Where in this file the change is, as counts per slice of its length.
    ///
    /// The finest resolution the strip is ever drawn at. A renderer with
    /// fewer columns sums adjacent buckets and classifies the sums, which is
    /// exact; it never draws a prefix of this array, because half a strip
    /// drawn as a whole one says the file's tail is unchanged.
    ///
    /// All zeroes when there is nothing to place: a binary file, a removal,
    /// a conflict.
    pub heat: [HeatBucket; HEAT_BUCKETS],
}

/// What a row of the body is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    /// A changed file's heading, inside the diff stream.
    ///
    /// The same [`FileEntry`] the pinned list draws, so the two regions cannot
    /// drift apart in what they show or in how they degrade.
    File(FileEntry),
    /// A hunk boundary, drawn as git's `@@ -a,b +c,d @@`.
    Hunk {
        /// First line covered on the index side.
        old_start: u32,
        /// Index-side lines covered.
        old_lines: u32,
        /// First line covered on the working-tree side.
        new_start: u32,
        /// Working-tree-side lines covered.
        new_lines: u32,
    },
    /// One line of content.
    Line {
        /// Added, removed or context.
        kind: LineKind,
        /// Line number on whichever side this line exists.
        number: u32,
        /// The text, already stripped of its line ending by the core.
        text: String,
        /// What each run of `text` means, covering it exactly.
        ///
        /// Empty is legal and means "no highlighting", which is what a file type
        /// nothing recognises produces and what a hand-built row in a test
        /// carries. The renderer draws an uncovered tail in the plain style, so
        /// an empty list and a single [`vigia_core::Class::Plain`] span reach the
        /// screen identically.
        spans: Vec<Span>,
    },
    /// Why a file has no lines under it.
    Note(&'static str),
    /// The blank row that closes a file's block.
    ///
    /// **Ruled 2026-08-15** ([#165](https://github.com/breferrari/vigia/issues/165)).
    /// A file's last content row and the next file's heading sat on adjacent
    /// rows, and a heading is a `Painter::file_row` carrying the kind letter,
    /// the path, the pulse, the heat strip, the sparkline and the counters, so a
    /// dense row landed directly under a dense row with nothing but content
    /// marking the boundary. The stream has one such boundary per changed file
    /// and drew none of them, where the boundary between the two *regions* one
    /// level up gets a rule of its own.
    ///
    /// **Trailing rather than leading**, which keeps a heading the **first** row
    /// of its own block. Every jump on this map resolves through `App::jump_to`
    /// to `Position { file, row: 0 }`, so a leading blank would put one above
    /// every heading a reader jumped to, or force the jump to resolve to
    /// heading-minus-one, which is the off-by-one this row model has been bitten
    /// by before.
    ///
    /// **After every file but the last, and that exception is [`gap_rows`]'.**
    /// The first draft of this ruling was uniform, on the argument that a
    /// per-file conditional costs more than one blank row at the bottom of the
    /// stream. It does not, and the reason is not arithmetic: `SPEC.md` §11.1
    /// rules that the bottom of the diff is **content**, and
    /// `tests/scroll.rs::the_bottom_of_the_diff_is_content_rather_than_blank` is
    /// the gate over it, carrying its own warning about having once been
    /// weakened. A blank there would separate the diff from nothing, since the
    /// footer is chrome with a row of its own.
    ///
    /// **A blank row rather than a rule.** §11.2 B11 ruled the rule between the
    /// regions stays bare, so a rule inside the stream would be a second one on
    /// the same screen meaning something different, and §5.3's furniture law
    /// would run it full-bleed across the pane. Space is the quietest thing that
    /// is still a boundary, and it is the heading's own density that makes space
    /// around it read.
    ///
    /// Carries nothing, and the renderer draws nothing for it: an unwritten row
    /// is already blank, which is what every row below a short diff has always
    /// been.
    Gap,
}

/// Slices a file's length is divided into for the heat strip.
///
/// **The source resolution, which is a different number from what any pane
/// draws** ([#161](https://github.com/breferrari/vigia/issues/161)). The renderer
/// sums adjacent slices down to the rung its width affords, so this is the
/// ceiling on that ladder rather than a column count: `heat_at` groups
/// `HEAT_BUCKETS / width`, and a rung wider than this divides to zero.
///
/// **Twelve was this constant until 2026-08-18 and is a rung now**, which keeps
/// `SPEC.md` §5.1's ruling rather than overturning it. That ruling is that a
/// published artifact answering an open question **is** the answer, and
/// `assets/preview.svg` draws exactly twelve slices. What the picture answers is
/// how many slices a pane **at its own width** draws, and its own comment records
/// that width: a 109-column render. So twelve stays the rung a 109-column pane
/// picks, gated at that width in `tests/legibility.rs` rather than left to a
/// constant nobody could check. Doubling the source is what lets a pane wide
/// enough to deserve it draw a finer strip without moving what the picture shows.
///
/// The picture also draws an empty slice as a dark track rather than as a gap,
/// which is why [`FileEntry::heat`] is always this long and why the renderer
/// draws a block for every bucket.
pub const HEAT_BUCKETS: usize = 24;

/// Changed lines falling in one slice of a file's length.
///
/// Counts rather than a verdict, because the verdict depends on how many columns
/// the renderer has room for: at a narrower width adjacent buckets are **summed**
/// and classified again, which is exact, where classifying here and merging the
/// answers afterwards would not be.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HeatBucket {
    /// Lines added inside this slice.
    pub added: u16,
    /// Lines removed from inside this slice.
    pub removed: u16,
}

impl HeatBucket {
    /// Changed lines of either kind.
    pub fn total(self) -> u32 {
        u32::from(self.added) + u32::from(self.removed)
    }
}

/// Where a working-tree line sits, as a bucket index.
///
/// `line` is 1-based, the way [`vigia_core::Hunk::new_start`] is. Out of range
/// is clamped rather than dropped: a removal at the very end of a file is
/// numbered one past the last line that still exists, and it happened *in* the
/// file rather than after it.
fn bucket_of(line: u32, lines: u32) -> Option<usize> {
    if lines == 0 {
        return None;
    }
    let zero_based = u64::from(line.saturating_sub(1));
    let index = (zero_based * HEAT_BUCKETS as u64) / u64::from(lines);
    Some((index as usize).min(HEAT_BUCKETS - 1))
}

/// Project a file's changed lines onto [`HEAT_BUCKETS`] slices of its length.
///
/// This is the heat strip, and it is the one element of `SPEC.md` §5 that needs
/// a whole-file property rather than a diff. [`vigia_core::FileDiff::lines`]
/// supplies it for free; see that field for why §5.2's predicted cache is not
/// needed.
///
/// **A removed line is placed where it used to be**, which is the working-tree
/// line number the walk has reached. It exists nowhere on the new side by
/// definition, and the alternative to placing it is not placing it, which would
/// draw a file whose only change was a deletion as a file with no changes.
///
/// Pure, and separated from [`View::take_file`] for the reason `SPEC.md` §7
/// gives about rules worth testing directly: every interesting case here is an
/// off-by-one at a boundary, and reaching those through a repository fixture
/// would mean building a file of an exact length for each one.
fn heat_of(diff: &FileDiff) -> [HeatBucket; HEAT_BUCKETS] {
    let mut buckets = [HeatBucket::default(); HEAT_BUCKETS];
    if diff.lines == 0 {
        return buckets;
    }

    for hunk in &diff.hunks {
        // The same walk `take_file` does below, and it has to be the same one:
        // both sides advance per line kind, and a copy that drifted would put
        // the strip's marks somewhere the gutter disagrees with.
        let mut new = hunk.new_start.max(1);
        for line in &hunk.lines {
            match line.kind {
                LineKind::Context => new += 1,
                LineKind::Added => {
                    if let Some(at) = bucket_of(new, diff.lines) {
                        buckets[at].added = buckets[at].added.saturating_add(1);
                    }
                    new += 1;
                }
                // Deliberately does **not** advance `new`: a removed line
                // occupies no working-tree row, so the next line after it sits
                // at the same position.
                LineKind::Removed => {
                    if let Some(at) = bucket_of(new, diff.lines) {
                        buckets[at].removed = buckets[at].removed.saturating_add(1);
                    }
                }
            }
        }
    }
    buckets
}

/// Where the top of the viewport sits.
///
/// A file and an offset into that file's rows, rather than one row number over
/// the whole diff. A single number would have to be resolved against every
/// file's row count to know what to draw, and knowing a file's row count means
/// diffing it, so the representation that looks cheaper is the one that reads
/// the whole worktree on every frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Position {
    /// Index into [`vigia_core::Frame::files`].
    pub file: usize,
    /// Rows of that file already scrolled past.
    ///
    /// May exceed that file's row count on the way in, and never does on the way
    /// out: [`View::collect`] carries the excess into the following files and
    /// reports where it landed as [`View::top`]. Both directions are needed. A
    /// scroll adds rows without knowing how tall the file is, and the file list
    /// is rebuilt by [`vigia_core::Frame::advance`], so the agent in the other
    /// pane can shrink the file underneath a position nobody touched.
    pub row: usize,
}

/// Everything [`View::collect`] needs to know about where the screen is looking.
///
/// A shape rather than five more parameters, and not only for the arity: the
/// diff's position and the list's are two windows onto one file list, and a
/// caller that could pass one without the other would be able to ask for a
/// screen where the two disagree about which file exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Viewport {
    /// Where the diff's top row is, before resolving.
    pub position: Position,
    /// Whether [`Self::position`] was reached by scrolling rather than a jump.
    ///
    /// See [`View::collect`]; it is what decides whether a short tail may back
    /// up to fill the pane.
    pub anchored: bool,
    /// Rows the diff region has, from [`crate::render::Body::diff`].
    pub diff_rows: usize,
    /// First file the pinned list shows, before resolving.
    pub list_top: usize,
    /// Rows the pinned list has, from [`crate::render::Body::list`]. Zero on a
    /// pane too short for a region, which draws no list at all.
    pub list_rows: usize,
    /// Whether the list's window should follow the diff, or stay where a reader
    /// put it.
    ///
    /// **A flag rather than a rule this function can work out**, and the reason
    /// is the same one [`Self::anchored`] gives one field up: the intent is not
    /// recoverable from the numbers. A window sitting away from the current file
    /// looks identical whether a reader browsed there with `J` or the diff moved
    /// out from under it, and those want opposite answers. Snapping
    /// unconditionally makes `J` useless, because the very next frame drags the
    /// window back; never snapping leaves a map that stops agreeing with the diff
    /// the moment anyone touches it.
    pub list_follows: bool,
    /// Whether this frame needs the diff's total height.
    ///
    /// The scrollbar is the only thing that wants it, and it is the only thing in
    /// the frame path not bounded by the window, so a caller says so rather than
    /// paying for it by default. `SPEC.md` §3's I4 is written around that
    /// distinction.
    pub measured: bool,
    /// Whether this frame may parse for colour.
    ///
    /// **False on the first frame of a process, and true forever after.** A
    /// grammar's `fancy_regex` patterns are compiled on first use, which costs
    /// 74-362ms depending on the language, and until this existed that landed on
    /// the one frame I7 gives 50ms to: measured at **105.03ms** over the
    /// hundred-file fixture, 2.1x over budget, while the reader looked at a blank
    /// alternate screen for the whole of it.
    ///
    /// So the first frame draws the diff plain and the next one colours it. What
    /// that buys is not a cheaper screen, it is an **earlier** one: the same
    /// content reaches the reader in a few milliseconds instead of a hundred, and
    /// the compile happens behind a screen that already has the diff on it.
    ///
    /// A flag beside [`Self::measured`] rather than a rule this function could
    /// work out, and for the same reason that field gives: whether a frame is the
    /// first of its process is not recoverable from anything here.
    /// [`crate::App`] holds it.
    ///
    /// The rows are still **built**, and that is the half worth guarding: a plain
    /// frame that also drew fewer rows would satisfy I7 by showing the reader
    /// less, which is why `tests/first_paint.rs` asserts the body is full before
    /// it asserts the clock.
    pub highlight: bool,
}

impl Default for Viewport {
    /// Hand written for one field, and only that field.
    ///
    /// [`Self::highlight`] defaults to **true**, which `derive` cannot give and
    /// which is the whole reason this impl exists. Every other field's derived
    /// answer is the honest one: row zero, not anchored, no rows, no total.
    /// `false` there means *this frame skips work*, and a caller who forgot the
    /// field gets a cheaper frame that looks the same.
    ///
    /// `highlight` is not that kind of field. `false` means the screen comes out
    /// **uncoloured**, so a caller writing `..Viewport::default()` would silently
    /// ask for a plain diff and get one — a visible difference, from an omission,
    /// with nothing to notice it. Two gates were caught by exactly that while
    /// this field was being added, which is evidence enough that the derived
    /// answer is a trap rather than a default.
    ///
    /// The unusual state is the one that has to be asked for, and there is
    /// exactly one caller who wants it: the first frame of a process. See
    /// [`crate::App::view`].
    fn default() -> Self {
        Self {
            position: Position::default(),
            anchored: false,
            diff_rows: 0,
            list_top: 0,
            list_rows: 0,
            list_follows: false,
            measured: false,
            highlight: true,
        }
    }
}

/// A screenful of rows, plus what the chrome needs to describe it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct View {
    /// The rows to draw, top to bottom.
    pub rows: Vec<Row>,
    /// The pinned file list, top to bottom, at most `Viewport::list_rows` long.
    ///
    /// Bounded by the region and never by the changed set, which is the whole of
    /// how `SPEC.md` §11.1 keeps the list inside I4: one `Frame::diff` per row
    /// drawn, and under I2a a file that did not change is a `stat` and a cache
    /// hit that reads no bytes. Empty when the pane is too short for a region.
    pub list: Vec<FileEntry>,
    /// Which file the pinned list starts at, once the request was resolved
    /// against the files that exist and against where the diff is.
    ///
    /// A caller stores this back the way it stores [`View::top`] back, and for
    /// the same reason: resolution belongs in one place, and the place is
    /// whichever code knows where the diff actually landed.
    pub list_top: usize,
    /// Rows the block the diff is inside contributes: heading, content, and the
    /// blank that closes it where one does
    /// ([#165](https://github.com/breferrari/vigia/issues/165)).
    ///
    /// The **block** rather than the file, because `rows_above` clamps against
    /// it and is positioned in the same units the total is.
    ///
    /// **Free, and that is the whole reason the diff's scrollbar can move within
    /// a file at all.** The file the viewport is inside has been diffed by
    /// definition, so its height is already known, and positioning `rows_above`
    /// inside it is what keeps the bar smooth while a reader scrolls one long
    /// file rather than stepping once per file.
    ///
    /// It carried an argument that every *other* file's height was the read
    /// §11.1 rules out for `G` and I4 forbids generally, so the bar could only
    /// be file-granular between files. **Both halves of that expired on
    /// 2026-08-01**: counting a height costs a fiftieth of materialising the
    /// diff it describes, I4 was narrowed to admit the walk, and `total_rows`
    /// below is the result. See §3's I4 note.
    ///
    /// Zero when there is nothing to be inside, which a renderer must read as
    /// "no bar" rather than dividing by.
    pub current_span: usize,
    /// Rows the **whole** diff is, every changed file counted.
    ///
    /// What the diff's scrollbar is scaled against, and the one number here that
    /// is not bounded by the window. `SPEC.md` §3's I4 notes carry the rewording that
    /// admits it; the short version is that counting a file's height is not the
    /// same work as building it, and doing it the cheap way put the reference
    /// fixture at **8.76ms** where materialising every diff was 442.71ms.
    ///
    /// Zero means it was not asked for, which a renderer reads as "no bar"
    /// rather than dividing by.
    pub total_rows: usize,
    /// Rows of the whole diff above this screen's top row.
    ///
    /// The scrollbar's position, and it is free once [`Self::total_rows`] is
    /// known: the same walk that totals every file can stop at the current one
    /// and add the offset into it.
    pub rows_above: usize,
    /// Changed files in the whole worktree, not just the visible ones.
    ///
    /// Free: [`vigia_core::Frame::files`] knows it without reading anything.
    /// Repository-wide `+`/`-` totals are not free in the same way, which is why
    /// the header does not show them. `SPEC.md` §10 carries the question.
    pub files: usize,
    /// Where the top row actually came from, once the request was resolved
    /// against the files that exist and how tall they are.
    ///
    /// A caller should store this back as its scroll position. That is what
    /// keeps a frame's cost at one file: an unresolved position walks the files
    /// it scrolls over, a resolved one starts on the file it draws.
    pub top: Position,
    /// Files this viewport asked the frame for, drawn or merely crossed.
    ///
    /// Reported so a test can hold the *shell* to I4, not just the core. One,
    /// once the position has settled and a single file fills the screen.
    pub read: usize,
    /// The busiest bucket any tracked file holds, which every sparkline on this
    /// screen is drawn against.
    ///
    /// One scale for the whole view rather than one per row. The question a
    /// reader asks down a file list is which file is busiest, and a row scaled
    /// against its own maximum draws every file at full height the moment it is
    /// the busiest thing it has ever been, which answers a question nobody asked.
    ///
    /// **Zero means nothing is tracked, which is a scale a renderer must not
    /// divide by.** It used to say a renderer must read zero as "draw no
    /// sparkline", and that is the ruling
    /// [#78](https://github.com/breferrari/vigia/issues/78) reversed: an empty
    /// bucket draws a track, so a scale of zero means every bucket is empty and
    /// every one of them is still drawn. `vigia_core::History::scale` carries the
    /// same correction, and this is the copy a renderer actually reads.
    pub scale: u32,
    /// The whole worktree's churn over the window, oldest sample first.
    ///
    /// **What the masthead's band draws** ([#158](https://github.com/breferrari/vigia/issues/158)),
    /// and the one thing on the pane that is about the worktree rather than
    /// about a file in it. Every other glance element answers *which file*; this
    /// answers *how hot is this tree right now, and was it hotter a minute ago*.
    ///
    /// Carried on the view rather than fetched by the painter, for the reason
    /// [`View::scale`] is: the renderer is handed what to draw and does not reach
    /// back into a store for it. It is a copy of a field the history keeps
    /// current on a walk it was already making, so collecting it costs a move.
    pub worktree_churn: vigia_core::Churn,
}

/// The letter shown for a kind of change.
///
/// Git's own letters, because they are the ones a reader already knows. `I` for
/// intent-to-add is ours: git renders that as a staged addition, and a monitor
/// of the working tree has to distinguish it from content that is really there.
fn letter(kind: &ChangeKind) -> char {
    match kind {
        ChangeKind::Added => 'A',
        ChangeKind::Modified => 'M',
        ChangeKind::Removed => 'D',
        ChangeKind::Renamed { .. } => 'R',
        ChangeKind::Copied { .. } => 'C',
        ChangeKind::TypeChange => 'T',
        ChangeKind::Conflict => 'U',
        ChangeKind::IntentToAdd => 'I',
    }
}

/// The path content moved from, for the kinds that have one.
fn source_of(kind: &ChangeKind) -> Option<&str> {
    match kind {
        ChangeKind::Renamed { from } | ChangeKind::Copied { from } => Some(from),
        _ => None,
    }
}

/// The one-line stand-in for a file with no line-level diff, if it needs one.
fn note_for(kind: &ChangeKind, diff: &FileDiff) -> Option<&'static str> {
    match kind {
        ChangeKind::Conflict => Some("unresolved conflict"),
        ChangeKind::TypeChange => Some("type changed"),
        _ if diff.binary => Some("binary"),
        _ => None,
    }
}

/// Rows one file contributes: its heading, then either a note or its hunks.
///
/// **Without the blank that closes the block**, which is [`gap_rows`]' and is
/// kept out of here deliberately: whether a file is followed by one is a fact
/// about its *position*, and this function is handed a file rather than a place
/// in a list. Every caller that sums these adds the other term.
fn span_of(kind: &ChangeKind, diff: &FileDiff) -> usize {
    if note_for(kind, diff).is_some() {
        return 2;
    }
    1 + diff
        .hunks
        .iter()
        .map(|hunk| 1 + hunk.lines.len())
        .sum::<usize>()
}

/// The blank row closing the block of the file at `index`, as a count.
///
/// One row after every file **but the last**
/// ([#165](https://github.com/breferrari/vigia/issues/165)); [`Row::Gap`] carries
/// why there is one at all and why it trails rather than leads.
///
/// **Not uniform, and the last file is the whole of the exception.** `SPEC.md`
/// §11.1 rules that scrolling past the end rests on the last *screenful* and
/// that the bottom of the diff is **content**, which
/// `tests/scroll.rs::the_bottom_of_the_diff_is_content_rather_than_blank` holds
/// and which carries its own warning about having once been weakened. A blank
/// there would separate the diff from nothing, since the footer is chrome with a
/// row of its own.
///
/// **Private, and reached through [`block_of`] and [`block_rows`] rather than
/// added by hand.** The first draft of this was `pub` and let all six
/// expressions that sum file heights add the term themselves, under a doc
/// enumerating them. That doc named four of the six on the day it landed and two
/// callers were missed: `tests/reads.rs`'s cost diagnostic broke by exactly
/// `files - 1`, and `tests/scroll.rs`'s drag gate stayed green only because two
/// omissions cancelled. It is the third time on this branch that a quantity
/// spelled at each site drifted from the doc naming the sites, and [`crate::Body`]
/// one region up already had the answer: *"All three numbers come from one
/// function because they have to agree."* So a caller holding a position asks
/// for a **block** and cannot forget the term.
fn gap_rows(index: usize, files: usize) -> usize {
    usize::from(index + 1 < files)
}

/// Rows the block of the file at `index` occupies: the file's own rows and the
/// blank that closes it.
///
/// **Where a position is known, so where [`gap_rows`] is added.** The counting
/// twins stay per-file because they are handed a file; this is the quantity a
/// caller that knows *where* the file sits actually wants, and asking for it is
/// what stops the term being forgotten.
fn block_of(kind: &ChangeKind, diff: &FileDiff, index: usize, files: usize) -> usize {
    span_of(kind, diff) + gap_rows(index, files)
}

/// The same block, counted from the span cache rather than from a diff.
///
/// [`block_of`]'s twin by route rather than by quantity, and the distinction is
/// I4's: this is a `stat` against a span the tick has already proved, where
/// [`rows_in`] pays [`vigia_core::Frame::diff`]. A caller that has totalled the
/// diff walks part of it again through here for free.
pub fn block_rows(frame: &mut Frame, index: usize) -> Result<usize> {
    let files = frame.files().len();
    Ok(frame.rows_of(index, rows_of)? + gap_rows(index, files))
}

/// Rows the whole diff occupies, the blanks between files included.
///
/// The counting twin of [`View::collect`]'s walk at the level a scrollbar needs:
/// [`vigia_core::Frame::height`] sums what each file draws and knows nothing
/// about the blanks between them, because [`crate::rows_of`] is handed a file
/// rather than a position. `files - 1` is exactly the sum of [`gap_rows`] over
/// every file, and it is added here rather than at each caller so the two
/// callers cannot come to disagree. [`block_of`] and [`block_rows`] are the
/// per-file half of the same rule.
///
/// `tests/scroll.rs::the_counting_twins_agree_with_the_rows_drawn` is what fails
/// when this and the walk part company.
pub fn diff_rows(frame: &mut Frame) -> Result<usize> {
    let files = frame.files().len();
    Ok(frame.height(rows_of)? + files.saturating_sub(1))
}

/// One changed file as the walk has it: what happened, what it diffs to, and
/// where it sits in the frame's list.
///
/// Three things that always travel together — `Frame::diff` hands back the first
/// two and the caller already holds the third — so they are one parameter rather
/// than three. That also keeps [`View::take_file`] inside the argument count
/// clippy is willing to read.
struct Changed<'f> {
    kind: &'f ChangeKind,
    diff: &'f FileDiff,
    index: usize,
    /// Whether a blank closes this file's block, which is every file but the
    /// last ([`gap_rows`]).
    ///
    /// Carried here rather than passed beside it for the reason this struct
    /// exists: it is what keeps [`View::take_file`] inside the argument count
    /// clippy is willing to read, and adding the fourth fact as a fourth
    /// parameter is what put it over.
    closes: bool,
}

/// Everything a row about this file needs, for either region.
///
/// One function because `SPEC.md` §11.1 draws the pinned list and the stream's
/// heading identically, and two constructors would be two chances for them to
/// disagree about what a file looks like.
///
/// The two `history` lookups are hash probes against a store fed from the watch:
/// no read, no `stat` and no diff, which is why the glance elements cost the
/// frame nothing that `tests/reads.rs` can see.
fn entry_of(kind: &ChangeKind, diff: &FileDiff, history: &History) -> FileEntry {
    FileEntry {
        path: diff.path.clone(),
        from: source_of(kind).map(str::to_owned),
        kind: letter(kind),
        churn: (note_for(kind, diff).is_none()).then_some((diff.added, diff.removed)),
        spark: history.level(&diff.path).unwrap_or([0; HISTORY_BUCKETS]),
        recency: history.recency(&diff.path),
        heat: heat_of(diff),
    }
}

/// How many rows a file occupies, from its span rather than from its diff.
///
/// The counting twin of [`span_of`], and the two have to agree exactly: one is
/// what the screen draws and the other is what the scrollbar is scaled against.
/// It is passed to [`vigia_core::Frame::height`] because what a conflict or a
/// binary file occupies is this crate's ruling rather than the engine's, and
/// `SPEC.md` §6 keeps that decision here.
pub fn rows_of(change: &vigia_core::FileChange, span: &vigia_core::FileSpan) -> usize {
    // A note is a heading and one line saying why, which is exactly what
    // `note_for` produces for the same three cases.
    //
    // **The blank between two files is not counted here**, for the reason
    // [`gap_rows`] gives: it depends on where a file sits and this is handed a
    // file. [`diff_rows`] is the total that adds them, and it exists so the two
    // callers of this cannot come to disagree about that.
    if matches!(change.kind, ChangeKind::Conflict | ChangeKind::TypeChange) || span.binary {
        return 2;
    }
    1 + span.hunks as usize + span.lines as usize
}

/// How many rows the **block** of the file at `index` would occupy.
///
/// **The blank that closes it is included**, unlike [`span_of`], because a
/// position is exactly what this is for. That is the difference between this and
/// [`crate::rows_of`], and it is the one a caller summing both has to hold:
/// `tests/reads.rs`'s cost diagnostic asserted the two totals equal and broke by
/// `files - 1` when it stopped being true.
///
/// Costs whatever [`vigia_core::Frame::diff`] costs, which for a file already
/// held between frames is one `stat` and no read. That is what makes it usable
/// from the scroll arithmetic, where a keypress can walk back over several
/// files to find where it lands.
///
/// # Panics
///
/// If `index` is out of range, the same way [`vigia_core::Frame::diff`] does.
pub fn rows_in(frame: &mut Frame, index: usize) -> Result<usize> {
    let files = frame.files().len();
    let (change, diff) = frame.diff(index)?;
    Ok(block_of(&change.kind, diff, index, files))
}

impl View {
    /// How many distinct files this screen's diff region draws.
    ///
    /// The span the diff's scrollbar thumb covers, and it is free: the walk
    /// already produced the rows, so counting the headings among them costs
    /// nothing and needs no file the frame did not draw.
    ///
    /// The top file is counted whether or not its heading is on screen, because a
    /// viewport resting deep inside one file is still showing that file. Every
    /// [`Row::File`] after it is another one, and a heading on the **first** row
    /// is the top file's own rather than a second.
    pub fn shown_files(&self) -> usize {
        if self.rows.is_empty() {
            return 0;
        }
        let headings = self
            .rows
            .iter()
            .filter(|row| matches!(row, Row::File(_)))
            .count();
        if matches!(self.rows.first(), Some(Row::File(_))) {
            headings.max(1)
        } else {
            headings + 1
        }
    }

    /// Whether the whole diff is already on screen.
    ///
    /// True only when nothing has been scrolled past and the walk ran out of rows
    /// before it ran out of room, which together mean there is nowhere to go. It
    /// is what decides that the diff's scrollbar would be a column spent saying
    /// there is nothing to scroll.
    ///
    /// Free, and deliberately not "did I draw every file": a screen can show
    /// every changed file and still be scrolled, and a screen can show one file
    /// and be complete.
    pub fn fits(&self, rows: usize) -> bool {
        self.top == Position::default() && self.rows.len() < rows
    }

    /// Collect the rows visible from `position`, and no others.
    ///
    /// `height` is the body's height in rows. Zero is legal and gives an empty
    /// view: a terminal can be short enough that the header and footer leave
    /// nothing between them, and that has to render rather than panic.
    ///
    /// Resolving the position and drawing from it are one pass, not two. They
    /// need the same diffs, and [`vigia_core::Frame::diff`] re-reads a file
    /// written in the last two seconds every time it is called, by design, so a
    /// separate normalising pass would read the file being edited twice per
    /// frame. That is the one file for which twice is never cheap.
    ///
    /// `position.file` is clamped rather than trusted, because
    /// [`vigia_core::Frame::diff`] panics on an index that has fallen off the
    /// end, and a scroll position is exactly the index that can.
    ///
    /// `history` is read per **drawn** row and never per changed file, so the
    /// glance elements follow the viewport the way the reads already do. It
    /// costs no file read, no `stat` and no diff: everything it answers was
    /// recorded from the filesystem event that woke the loop. That is what
    /// `tests/reads.rs` asserts by not moving.
    ///
    /// No clock is passed in and none is read here. `History` advances on ticks,
    /// so recency is whatever the last event left it as, which is what keeps I1
    /// intact and what makes every test over these rows deterministic.
    pub fn collect(
        frame: &mut Frame,
        highlighter: &mut Highlighter,
        history: &History,
        viewport: Viewport,
    ) -> Result<Self> {
        let Viewport {
            position,
            anchored,
            diff_rows: height,
            list_top,
            list_rows,
            list_follows,
            measured,
            highlight,
        } = viewport;
        // One pass, dropped at every exit including the `?`s below, which is
        // what keeps the highlight cache bounded by the viewport. The guard
        // rather than a pair of calls is `vigia_core::Highlighter::pass`'s
        // business and its doc says why.
        // Held by name as well as by guard, so the restart below can retire what
        // an abandoned walk parsed. See there for why that matters.
        let original = highlighter;
        let mut highlighter = original.pass();
        let files = frame.files().len();
        let mut view = Self {
            // Bounded by the screen, not by the diff. The cap keeps a caller
            // asking for an absurd height from allocating for it up front.
            rows: Vec::with_capacity(height.min(64)),
            list: Vec::with_capacity(list_rows.min(64)),
            // Resolved below, once the walk has said where the diff landed. Both
            // start where they were asked to, so a frame with no room to draw
            // reports the request back unchanged and a caller keeps its place.
            list_top,
            current_span: 0,
            total_rows: 0,
            rows_above: 0,
            files,
            // Until the walk below runs, the request is passed through with only
            // its file clamped. That matters for `height == 0`: a frame with no
            // room to draw resolved nothing, so it has nothing to say about where
            // the reader is, and reporting a row of zero would drag them to the
            // top of the file for as long as the pane stayed too short to have a
            // body at all. A caller storing this back keeps its place.
            top: Position {
                file: position.file.min(files.saturating_sub(1)),
                row: position.row,
            },
            read: 0,
            scale: history.scale(),
            worktree_churn: history.worktree_churn(),
        };
        if files == 0 {
            // Nothing to point at, so nothing to preserve either.
            view.top.row = 0;
            view.list_top = 0;
            return Ok(view);
        }
        if height == 0 {
            // **The list still resolves.** The two regions are independent, and
            // a pane whose diff has been squeezed to nothing has not lost its
            // map: `body_layout` never produces this pair, but `collect` is
            // public and a caller asking for one region without the other must
            // get the one it asked for rather than silently neither.
            view.take_list(frame, history, list_rows, list_follows, &[])?;
            return Ok(view);
        }

        // Entries the body built, so the list can reuse rather than re-diff.
        // Bounded by the viewport: one per heading that fits on screen.
        let mut drawn: Vec<(usize, FileEntry)> = Vec::new();

        let mut index = view.top.file;
        let mut skip = position.row;
        let mut placed = false;
        // At most one restart, whichever of the two reasons below triggered it.
        // `last_screenful` resolves to a position that can fill the body whenever
        // the diff has the rows for it, so a second pass is never the answer to a
        // third: without this the short-diff case would restart forever.
        let mut restarted = false;

        // Restarted at most once, and only from [`Self::last_screenful`] below.
        // The walk cannot resolve that case in place: `change` and `diff` borrow
        // the frame until `take_file` has used them, so there is nowhere inside
        // the loop to ask it about a file further back.
        loop {
            let mut overshot = false;

            while index < files && view.rows.len() < height {
                view.read += 1;
                let (change, diff) = frame.diff(index)?;
                // Both halves of the tuple are immutable borrows of the same
                // frame, so the kind needs no clone to be read alongside the
                // diff.
                let span = block_of(&change.kind, diff, index, files);

                if !placed {
                    if skip >= span {
                        if index + 1 < files {
                            // Wholly above the window. Carrying the remainder
                            // into the next file rather than clamping is what
                            // makes scrolling off the end of a short file
                            // continue into the one below instead of stopping
                            // there.
                            skip -= span;
                            index += 1;
                            continue;
                        }
                        // **Past the end of the last file, which lands the
                        // reader on the last screenful and not on the last
                        // row.** Those are not the same place, and taking the
                        // second for the first is what
                        // [#57](https://github.com/breferrari/vigia/issues/57)
                        // was: resting the diff's final row at the *top* of the
                        // viewport draws one line of content and blanks every
                        // row under it, while the header goes on truthfully
                        // saying how many files changed. A pager rests that row
                        // at the bottom.
                        //
                        // Reachable two ways and only one of them is scrolling.
                        // The other is the diff **shrinking** under a position
                        // that was reasonable when it was taken: an agent in the
                        // other pane running `git reset --hard`, switching
                        // branch, or reverting its own work. That is an ordinary
                        // event on the pane this tool exists for, and it is
                        // exactly when someone looks over.
                        if span >= height {
                            skip = span - height;
                        } else {
                            // The last file cannot fill the screen by itself, so
                            // the top is in a file further back and this walk
                            // has no way to reach it. Resolved after the borrow
                            // ends.
                            overshot = true;
                            break;
                        }
                    }
                    view.top = Position {
                        file: index,
                        row: skip,
                    };
                    placed = true;
                }

                // The height of the file the viewport is inside, recorded where
                // it is already known. Checked every iteration rather than only
                // on placement, because the restart path below sets `view.top`
                // from `last_screenful` with `placed` already true and would
                // otherwise leave this at zero on exactly the frames that moved.
                if index == view.top.file {
                    view.current_span = span;
                }

                view.take_file(
                    Changed {
                        kind: &change.kind,
                        diff,
                        index,
                        closes: gap_rows(index, files) > 0,
                    },
                    // The pass is taken whatever this frame does with it, so the
                    // sweep in its `Drop` still runs and the cache stays bounded
                    // the way I3 needs. What `highlight` decides is only whether
                    // anything asks it for spans.
                    highlight.then_some(&mut highlighter),
                    history,
                    skip,
                    height,
                    &mut drawn,
                );
                skip = 0;
                index += 1;
            }

            // **Two ways to finish with a body that is not full, and until #59
            // only one of them backed up.**
            //
            // `overshot` is the position landing past the end of the last file,
            // which is what [#57](https://github.com/breferrari/vigia/issues/57)
            // fixed. The other is quieter and is what a reader actually does:
            // scroll down a row at a time until fewer than a screenful remain
            // below the top. Then `skip` is still *inside* the current file, so
            // nothing overshoots, the walk simply runs out of files, and the pane
            // draws what is left over a growing block of blank rows. It starts one
            // row short and loses another with every keystroke.
            //
            // Both want the same answer, which is the one a pager gives: rest the
            // diff's last row on the bottom of the viewport.
            //
            // A diff genuinely shorter than the pane is **not** either case, and
            // must not be turned into one. [`Self::last_screenful`] returns the top
            // for it, so the blank rows underneath stay, which is honest: they are
            // the rows the diff does not have.
            // **Only when the reader scrolled here.** A position placed by a jump
            // is a claim about what should be at the *top*: follow (I5) puts the
            // file that just changed there, and `G` puts the last file there. If
            // the tail is shorter than the pane, backing up to fill it would move
            // that file off the top row and make the reader hunt for what the jump
            // was for. Blank rows under a deliberately placed file are the honest
            // answer; blank rows under a file the reader scrolled to are not.
            //
            // The two are indistinguishable from a `Position` alone, which is why
            // this is a parameter rather than something inferable here.
            // **And not already at the top**, which is the difference between a
            // back-up and a treadmill. When the whole diff is shorter than the
            // pane, `last_screenful` resolves to `Position::default()`, the next
            // frame is short again from the same place, and it restarts on every
            // paint forever: measured at three walks and six `frame.diff` calls a
            // frame against two. `Frame::diff` re-reads any file written in the
            // last two seconds, so the file being edited was diffed three times
            // per frame, which is exactly what this function's one-pass design
            // exists to prevent. It also breached I3's bound on the highlight
            // cache, which is what turned it red on Windows CI rather than here.
            let short = anchored && view.rows.len() < height && view.top != Position::default();
            if restarted || !(overshot || short) {
                break;
            }
            restarted = true;

            // Cleared, unlike the overshoot path, and this is the one line where
            // the two differ. Overshooting is decided before anything is drawn,
            // because `take_file` runs only after placing; running short is
            // decided *after* a partial screen has already been built, so the
            // restart has to throw it away or the second pass appends to it.
            view.rows.clear();
            // **`drawn` is deliberately kept.** Clearing it looked like the tidy
            // thing to do beside the rows and is wrong: an entry is a pure
            // function of an index, this frame's cached diff, and a history that
            // cannot move mid-collect, so an entry the abandoned walk built for
            // file N is identical to the one the new walk would build. Throwing
            // it away buys nothing and costs a `Frame::diff` for any file the
            // second walk no longer draws but the list still shows. Mutation
            // found it: removing the clear left every gate green, which is the
            // tell that the line was doing nothing.

            // **And the parses go with them.** Clearing the rows discards what was
            // drawn; it does not discard what drawing *cost*, because a hunk's
            // parse lives in the pass rather than in the row. So a frame that
            // built a screenful and threw it away left a screenful of parses
            // behind, and the walk below added a second: I3 bounds the highlight
            // cache by one viewport, and the soak caught six held on a screen that
            // could ask for five.
            //
            // Taking a fresh pass is what makes the discard real.
            // `Highlighter::pass` marks every entry dead on creation and sweeps on
            // drop, so retaking it here retires exactly the hunks the abandoned
            // walk touched and nothing the new one is about to. The borrow ends
            // with the drop, which is why this can re-borrow at all.
            drop(highlighter);
            highlighter = original.pass();

            view.top = Self::last_screenful(frame, files, height, &mut view.read)?;
            index = view.top.file;
            skip = view.top.row;
            placed = true;
        }

        // **After the walk, because only the walk knows where the diff landed.**
        // The position handed in may overshoot its file, point past a list the
        // agent in the other pane has shortened, or have been backed up to rest
        // the last row on the bottom. Marking the caret from the *request* would
        // put it on a file the diff is not in on exactly the frames that moved,
        // which is every frame a monitor exists to show.
        view.take_list(frame, history, list_rows, list_follows, &drawn)?;
        view.measure(frame, measured)?;

        Ok(view)
    }

    /// Total the diff's rows, and how many of them are above this screen.
    ///
    /// **The only thing here that walks the whole changed set**, and it is opt-in
    /// per frame rather than always: a caller that draws no scrollbar passes
    /// `false` and pays nothing, which keeps every gate that bounds a screen by
    /// its window able to ask for a screen with no total in it.
    ///
    /// `rows_above` comes out of the same walk. Stopping at the current file and
    /// adding the offset into it is what makes the position exact rather than
    /// interpolated, which is the whole reason for doing this at all.
    fn measure(&mut self, frame: &mut Frame, wanted: bool) -> Result<()> {
        if !wanted || self.files == 0 {
            return Ok(());
        }
        self.total_rows = diff_rows(frame)?;

        // Everything before the file the viewport is in, plus how far into it.
        // `frame.height` has already filled the span cache, so this second walk
        // reads nothing.
        let mut above = 0usize;
        for index in 0..self.top.file.min(self.files) {
            above += block_rows(frame, index)?;
        }
        self.rows_above = above + self.top.row.min(self.current_span);
        Ok(())
    }

    /// Fill the pinned file list, and resolve where it starts.
    ///
    /// **Bounded by `rows` and never by the changed set**, which is the whole of
    /// how `SPEC.md` §11.1 keeps this region inside I4: the cost follows the
    /// window exactly as the body's does. Each row is one
    /// [`vigia_core::Frame::diff`], which under I2a is a `stat` and a cache hit
    /// reading **zero bytes** for a file that did not change.
    ///
    /// Two clamps, and they answer different questions. The window is always
    /// pulled back so the last file can rest on the bottom row rather than
    /// leaving blanks a reader would read as "no more files"; that is validity
    /// and holds however the window got there. It is **snapped** onto the current
    /// file only when `follows` says the window is the diff's to move, which is
    /// every frame except those a reader has browsed with `J` and not yet
    /// overtaken. See [`Viewport::list_follows`] for why that cannot be worked
    /// out from the numbers here.
    ///
    /// **Nothing here diffs a file the walk already diffed.** `drawn` carries the
    /// entries the body built, and this consults it before asking the frame. That
    /// is not an optimisation: [`View::collect`]'s whole one-pass design exists
    /// because `Frame::diff` **re-reads** any file written in the last two
    /// seconds, so the file an agent just wrote is not cached, is always the
    /// current file, and is always inside this window while the list follows.
    /// Asking for it again read and diffed it a second time on **every frame a
    /// monitor exists for** — 258,790 bytes where 36,970 will do, measured at
    /// twenty files of five hundred lines. It is the exact cost this module's own
    /// docs say the one pass was written to avoid.
    ///
    /// It also spares a second `heat_of`, which walks every hunk line of the
    /// file, for each entry the body already built.
    ///
    /// Reads are still counted into [`View::read`] per row, including the reused
    /// ones. That is deliberate: the number is "files this viewport asked the
    /// frame for", and a reader of `tests/reads.rs` comparing two fixtures wants
    /// the region's full ask rather than a figure that moves with how much the
    /// two regions happen to overlap.
    fn take_list(
        &mut self,
        frame: &mut Frame,
        history: &History,
        rows: usize,
        follows: bool,
        drawn: &[(usize, FileEntry)],
    ) -> Result<()> {
        // **A pane with no region resolved nothing, so it says nothing.** The
        // request is handed back unchanged, which is exactly what the diff's own
        // walk does for `height == 0` one screen up and for the same reason: a
        // reader who drags a pane edge below the region's floor and back has
        // expressed no intent about where the map should look, and `SPEC.md`
        // §11.1 rules a resize "no state change". Zeroing here threw the browsed
        // window away, and since only a diff-moving action hands the map back,
        // it never recovered.
        if rows == 0 {
            return Ok(());
        }
        // No `files == 0` branch: `View::collect` returns before this on an
        // empty worktree, and a second guard here was unreachable. Replacing its
        // body with `unreachable!()` left the whole workspace green, which is the
        // tell.

        // Always pulled back so the last file can rest on the bottom row rather
        // than leaving blanks a reader would read as "no more files". That is
        // validity, and holds however the window got there.
        let mut top = self.list_top.min(self.files.saturating_sub(rows));
        if follows {
            // And snapped onto the current file, but **only** when the window is
            // the diff's to move. A reader who browsed away with `J` keeps their
            // place until the diff lands somewhere the list cannot show; see
            // [`Viewport::list_follows`] for why that cannot be worked out from
            // the numbers here. The caret is **not** suppressed while they
            // browse: it says *the diff is in this file*, which stays true, so
            // `Painter::list` marks the row whenever the window still shows the
            // current file and simply has nothing to mark once the window has
            // moved off it.
            //
            // **The current file goes to the top of the window, not the bottom.**
            // The diff draws that file and then whatever fits below it, so a
            // window ending on it is a map of the rows a reader has already
            // passed. Reported from use: scrolled to the last screenful of a
            // seventeen-file tree, the diff showed the last six files and the
            // list showed the six *before* them, with the caret pinned to its own
            // bottom row. Starting there makes the region a map of the screen.
            //
            // Still pulled back by the first clamp when the tail is shorter than
            // the window, so the last file can rest on the bottom row rather than
            // leaving blanks under it.
            // **The window moves the least it can, so the caret travels.** Held
            // while the current file is inside it, and pushed by exactly the
            // overshoot when the file leaves: forward off the bottom, back off
            // the top. Scrolling from the start therefore walks the caret down
            // the rows, and only then does the list move under it.
            //
            // **Both fixed positions were tried first and both were wrong**, in
            // opposite directions and for the same reason: a rule that puts the
            // current file at a constant row is not following it, it is dragging
            // the window on every step and pinning the caret. Ending the window
            // on the current file showed the six files *before* the six the diff
            // was drawing. Starting the window on it fixed that and pinned the
            // caret to the first row, which is what a reader sees as the list
            // scrolling while the marker never moves. Reported from use both
            // times.
            //
            // Minimal movement subsumes them. At the top of the changed set the
            // caret sits on the first row because the file is the first, not
            // because the row is; at the end the clamp above rests the last file
            // on the bottom row, and the caret is there because it belongs there.
            //
            // Still **not navigable**, which is §11.2 B4: the caret cannot be
            // moved on its own, nothing is selected, and no key changes meaning.
            // What travels is a marker, not a cursor.
            let current = self.top.file;
            if current < top {
                top = current;
            } else if current >= top + rows {
                top = current + 1 - rows;
            }
            top = top.min(self.files.saturating_sub(rows));
        }
        self.list_top = top;

        for index in top..(top + rows).min(self.files) {
            self.read += 1;
            // **Searched from the back.** The walk's restart keeps `drawn` (see
            // there for why that is right), and the second walk starts earlier,
            // so an index both walks drew has two entries. `Frame::diff` re-reads
            // a file written in the last two seconds, so the two are only
            // certainly equal outside the settle margin — which is the one state
            // the current file is never in. Taking the newest is what keeps the
            // two regions from disagreeing about one file for one frame.
            match drawn.iter().rev().find(|(at, _)| *at == index) {
                Some((_, entry)) => self.list.push(entry.clone()),
                None => {
                    let (change, diff) = frame.diff(index)?;
                    let entry = entry_of(&change.kind, diff, history);
                    self.list.push(entry);
                }
            }
        }
        Ok(())
    }

    /// Where the viewport starts so the diff's **last row rests at the bottom**.
    ///
    /// Walks back from the final file until it has `height` rows behind it. That
    /// reads only the files the screen is about to draw, so I4 is untouched: it
    /// is bounded by the window exactly like everything else here, and the
    /// `frame.diff` calls it makes are the same ones the walk above is about to
    /// make, which under I2a are cache hits rather than reads.
    ///
    /// **Not what `Action::Bottom` does**, and the difference is the whole
    /// reason this is affordable. `G` goes to the last *file* from its top,
    /// because finding the diff's last row from the *start* would mean adding up
    /// every file's height, which is the read I4 forbids. Backing off from an
    /// end already in hand costs a screenful.
    ///
    /// A diff shorter than the screen resolves to the top, which is the honest
    /// answer: the blank rows under it are the ones the diff does not have.
    ///
    /// **It counts its own reads, so an overshoot frame reports roughly twice
    /// the files it draws**, and that is accurate rather than sloppy:
    /// [`View::read`] is "files this viewport asked the frame for", and this asks
    /// for the same files the walk above is about to ask for again. Under I2a
    /// the second ask is a cache hit that reads no bytes, so it is a count that
    /// doubles and not work that does. It lasts one frame either way, because
    /// the caller stores the resolved position back and the next frame starts on
    /// the file it draws.
    fn last_screenful(
        frame: &mut Frame,
        files: usize,
        height: usize,
        read: &mut usize,
    ) -> Result<Position> {
        let mut index = files - 1;
        let mut have = 0usize;
        loop {
            *read += 1;
            let (change, diff) = frame.diff(index)?;
            have += block_of(&change.kind, diff, index, files);
            if have >= height {
                return Ok(Position {
                    file: index,
                    row: have - height,
                });
            }
            if index == 0 {
                return Ok(Position::default());
            }
            index -= 1;
        }
    }

    /// Append this file's rows that fall inside the window.
    ///
    /// `skip` rows are passed over and `height` bounds the total. Skipped rows
    /// are counted, never built: `n` tracks the row index within this file, and
    /// a hunk wholly above the window advances it in one step.
    ///
    /// Highlighting is asked for **only on a row that is actually pushed**, so
    /// it follows the screen the way reads already do. Within a hunk it cannot:
    /// `syntect` parses a line from the state the line before it left, so
    /// drawing row five hundred of a hunk parses the five hundred above it. That
    /// is paid once, because [`vigia_core::Highlighter`] keeps what it parsed.
    fn take_file(
        &mut self,
        file: Changed<'_>,
        mut highlighter: Option<&mut Pass<'_>>,
        history: &History,
        skip: usize,
        height: usize,
        drawn: &mut Vec<(usize, FileEntry)>,
    ) {
        let Changed {
            kind,
            diff,
            index,
            closes,
        } = file;
        let mut n = 0usize;

        if n >= skip {
            // Asked for only on a row actually pushed, the same rule the
            // highlighter follows below. A heading scrolled past above the
            // window costs two hash lookups it would never have drawn.
            //
            // Recorded as well as pushed, so [`View::take_list`] can draw the
            // same file without asking the frame for it a second time. Bounded
            // by the viewport, because this only runs for a heading that fits.
            let entry = entry_of(kind, diff, history);
            drawn.push((index, entry.clone()));
            self.rows.push(Row::File(entry));
        }
        n += 1;

        // **A labelled block so the block's closing gap has one push site.**
        // The three paths below used to `return`, and each is a place a row can
        // run out of room; with a trailing [`Row::Gap`] to add
        // ([#165](https://github.com/breferrari/vigia/issues/165)) that would
        // have been three copies of one push, which is how the copies come to
        // disagree. Breaking out instead leaves the gap as a single tail,
        // guarded by the same `n >= skip && rows.len() < height` every other row
        // here is: a path that broke for want of room fails that guard on its
        // own, so no branch has to remember it.
        'block: {
            if let Some(note) = note_for(kind, diff) {
                if n >= skip && self.rows.len() < height {
                    self.rows.push(Row::Note(note));
                }
                n += 1;
                break 'block;
            }

            for (ordinal, hunk) in diff.hunks.iter().enumerate() {
                if self.rows.len() >= height {
                    break 'block;
                }

                // A hunk entirely above the window costs one addition. The
                // line numbers restart from the next hunk's header, so nothing
                // has to be carried across the ones that are skipped.
                let span = 1 + hunk.lines.len();
                if n + span <= skip {
                    n += span;
                    continue;
                }

                if n >= skip {
                    self.rows.push(Row::Hunk {
                        old_start: hunk.old_start,
                        old_lines: hunk.old_lines,
                        new_start: hunk.new_start,
                        new_lines: hunk.new_lines,
                    });
                }
                n += 1;

                // The core carries line numbers per hunk rather than per line,
                // so both sides are counted forward from the header. Every line
                // advances the side it exists on; context advances both.
                let mut old = hunk.old_start;
                let mut new = hunk.new_start;
                for (within, line) in hunk.lines.iter().enumerate() {
                    let number = match line.kind {
                        LineKind::Removed => {
                            old += 1;
                            old - 1
                        }
                        LineKind::Added => {
                            new += 1;
                            new - 1
                        }
                        LineKind::Context => {
                            old += 1;
                            new += 1;
                            new - 1
                        }
                    };
                    if n >= skip {
                        if self.rows.len() >= height {
                            break 'block;
                        }
                        self.rows.push(Row::Line {
                            kind: line.kind,
                            number,
                            text: line.text.clone(),
                            // `None` is the plain first frame, and empty spans
                            // are already a legal, drawn state: it is what a
                            // file type with no grammar produces, so the
                            // renderer needs no new case for this. See
                            // `Viewport::highlight`.
                            spans: match highlighter.as_deref_mut() {
                                Some(pass) => {
                                    pass.spans(
                                        &diff.path,
                                        ordinal,
                                        hunk,
                                        within,
                                        diff.first_line.as_deref(),
                                    )
                                    .to_vec()
                                }
                                None => Vec::new(),
                            },
                        });
                    }
                    n += 1;
                }
            }
        }

        // The blank that closes the block, on the same terms as every row above
        // it. [`Row::Gap`] carries the ruling and [`gap_rows`] carries the one
        // exception; what matters here is that it is one counted row like any
        // other, so a viewport resting on it is a legal position and a window
        // that stops before it simply drew fewer rows.
        if closes && n >= skip && self.rows.len() < height {
            self.rows.push(Row::Gap);
        }
    }
}

#[cfg(test)]
mod tests {
    //! The heat projection, tested as the arithmetic it is.
    //!
    //! Every case here is a boundary: the first line of a file, the last, a
    //! removal past the end, a file shorter than the bucket count. Reaching any
    //! of them through a repository fixture would mean building a file of an
    //! exact length for each, and `vigia_core::FileChange` cannot be constructed
    //! outside its crate anyway, so a `FileDiff` built by hand is the only
    //! version of one these can reach. `SPEC.md` §7 names this shape.

    use vigia_core::{Hunk, Line};

    use super::*;

    fn line(kind: LineKind) -> Line {
        Line {
            kind,
            text: String::new(),
        }
    }

    /// A diff of `lines` total, carrying `hunks`.
    fn diff(lines: u32, hunks: Vec<Hunk>) -> FileDiff {
        FileDiff {
            path: "src/lib.rs".to_owned(),
            binary: false,
            hunks,
            added: 0,
            removed: 0,
            lines,
            first_line: None,
            bytes: 0,
        }
    }

    /// A hunk starting at working-tree line `new_start` with these line kinds.
    fn hunk(new_start: u32, kinds: &[LineKind]) -> Hunk {
        Hunk {
            old_start: 1,
            old_lines: kinds.len() as u32,
            new_start,
            new_lines: kinds.len() as u32,
            lines: kinds.iter().copied().map(line).collect(),
        }
    }

    fn touched(buckets: &[HeatBucket; HEAT_BUCKETS]) -> Vec<usize> {
        buckets
            .iter()
            .enumerate()
            .filter(|(_, bucket)| bucket.total() > 0)
            .map(|(at, _)| at)
            .collect()
    }

    /// A hundred and twenty lines over [`HEAT_BUCKETS`] slices puts line 1 in the
    /// first and line 61 exactly halfway, whatever the source resolution is.
    /// Written against the constant rather than against the twelve it was, so
    /// raising the source moves the fixture with it.
    #[test]
    fn a_hunk_lands_in_the_buckets_its_lines_fall_in() {
        let map = heat_of(&diff(
            120,
            vec![
                hunk(1, &[LineKind::Added]),
                hunk(61, &[LineKind::Added, LineKind::Added]),
            ],
        ));

        let middle = HEAT_BUCKETS / 2;
        assert_eq!(touched(&map), vec![0, middle]);
        assert_eq!(map[0].added, 1);
        assert_eq!(map[middle].added, 2);
    }

    /// The last line of the file is the last bucket and never one past it.
    ///
    /// The index arithmetic is `(line - 1) * BUCKETS / lines`, which for the
    /// final line is exactly `BUCKETS - 1` only because of the `- 1`. Without it
    /// the division reaches `BUCKETS` and the clamp is doing the work silently.
    #[test]
    fn a_hunk_at_the_end_of_the_file_lands_in_the_last_bucket_and_not_past_it() {
        let map = heat_of(&diff(120, vec![hunk(120, &[LineKind::Added])]));

        assert_eq!(touched(&map), vec![HEAT_BUCKETS - 1]);
    }

    /// A removal at the very end is numbered one past the last line that still
    /// exists. It happened in the file rather than after it, so it is clamped
    /// into the last bucket rather than dropped.
    #[test]
    fn a_removal_past_the_last_line_is_clamped_into_the_file() {
        let map = heat_of(&diff(10, vec![hunk(11, &[LineKind::Removed])]));

        assert_eq!(touched(&map), vec![HEAT_BUCKETS - 1]);
        assert_eq!(map[HEAT_BUCKETS - 1].removed, 1);
    }

    /// Both kinds in one slice, which is the case `SPEC.md` §5.1 left unruled
    /// and which the renderer draws as [`crate::Heat::Mixed`].
    ///
    /// A hundred and twenty lines, so a slice is ten lines wide and two adjacent
    /// changes really do share one. At twelve lines a slice is a single line and
    /// no two changes can ever be mixed, which is a fixture that tests the
    /// arithmetic rather than the case.
    #[test]
    fn a_bucket_holding_both_kinds_records_both() {
        let map = heat_of(&diff(
            120,
            vec![hunk(1, &[LineKind::Added, LineKind::Removed])],
        ));

        assert_eq!(
            touched(&map),
            vec![0],
            "the two changes did not share a slice"
        );
        assert_eq!(map[0].added, 1);
        assert_eq!(map[0].removed, 1);
    }

    /// A removed line occupies no working-tree row, so the line drawn after it
    /// sits at the same number. Advancing on a removal would drift every mark
    /// after the first deletion in the file.
    #[test]
    fn a_removal_does_not_advance_the_working_tree_position() {
        // Twelve lines, twelve buckets: one line each, so a drift of one row is
        // a drift of one bucket and is visible.
        let map = heat_of(&diff(
            12,
            vec![hunk(
                1,
                &[LineKind::Removed, LineKind::Removed, LineKind::Added],
            )],
        ));

        assert_eq!(
            touched(&map),
            vec![0],
            "the addition drifted away from the removals above it"
        );
        assert_eq!(map[0].removed, 2);
        assert_eq!(map[0].added, 1);
    }

    /// Fewer lines than buckets. Every bucket still has to be reachable, or a
    /// short file would draw all its change at the left edge.
    ///
    /// **What it does not claim is that the result is a *solid* strip**, and
    /// [#230](https://github.com/breferrari/vigia/issues/230) is that gap. Three
    /// changed lines of a three-line file light three slices out of the source's
    /// resolution and leave the rest cool, so a file changed throughout draws as
    /// dashes rather than as a block. Spreading was chosen over bunching when
    /// this was written and both alternatives were wrong; the third, giving a
    /// line the whole span of slices it covers, is what that issue is for. The
    /// expected slices are written against [`HEAT_BUCKETS`] so this fixture keeps
    /// stating what the projection actually does as the source moves.
    #[test]
    fn a_file_shorter_than_the_bucket_count_still_projects() {
        let map = heat_of(&diff(
            3,
            vec![
                hunk(1, &[LineKind::Added]),
                hunk(2, &[LineKind::Added]),
                hunk(3, &[LineKind::Added]),
            ],
        ));

        assert_eq!(
            touched(&map),
            vec![0, HEAT_BUCKETS / 3, 2 * HEAT_BUCKETS / 3]
        );
    }

    /// A file with no working-tree side has nowhere to place anything. That is a
    /// removal, a binary file and a conflict, and it must be empty rather than
    /// collapsed into bucket zero.
    #[test]
    fn a_file_with_no_lines_is_all_cool() {
        let map = heat_of(&diff(0, vec![hunk(1, &[LineKind::Removed])]));

        assert!(touched(&map).is_empty());
    }

    #[test]
    fn a_file_with_no_hunks_is_all_cool() {
        assert!(touched(&heat_of(&diff(100, Vec::new()))).is_empty());
    }

    /// Context lines advance the position and are not change. A hunk is mostly
    /// context, so counting it would paint every strip solid.
    #[test]
    fn context_moves_the_position_without_marking_anything() {
        let map = heat_of(&diff(
            120,
            vec![hunk(
                1,
                &[
                    LineKind::Context,
                    LineKind::Context,
                    LineKind::Context,
                    LineKind::Context,
                    LineKind::Context,
                    LineKind::Context,
                    LineKind::Context,
                    LineKind::Context,
                    LineKind::Context,
                    LineKind::Context,
                    LineKind::Added,
                ],
            )],
        ));

        assert_eq!(
            touched(&map),
            vec![HEAT_BUCKETS / 12],
            "the addition is on line 11, which is a twelfth of the way into a \
             120-line file"
        );
    }
}
