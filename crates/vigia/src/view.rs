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
    ChangeKind, FileDiff, Frame, HISTORY_BUCKETS, Highlighter, History, Hunk, LineKind, Origin,
    Pass, Recency, Result, SPARK_GROUPS, Span,
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
    /// Which run this row belongs to.
    ///
    /// **What the green staged mark is drawn from**, `SPEC.md` §11.2 **B17**. It is
    /// on the *entry* rather than looked up beside it because one path can be in
    /// both runs at once, so the path is not enough to say which row this is.
    pub origin: Origin,
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

/// One row of the pinned list's window.
///
/// **The list stopped being one file per row in
/// [#313](https://github.com/breferrari/vigia/issues/313)**, where it gained the
/// run separators that make two comparisons on one map readable as two.
///
/// Everything downstream that turns a *row* into a *file* — a click, a digit, the
/// caret — has to skip the separators, and it has to skip exactly the ones the
/// painter drew. [`list_plan`] is the single resolution both read, which is
/// `kept_keyboard`'s rule one region over: a plan taken twice is a plan that can
/// disagree with itself, and here the two copies would be a list whose digits
/// address rows the painter put somewhere else.
///
/// **The stream has no separator of its own, and that is a ruling.** The obvious
/// symmetry is one there too, and it was in #313's plan. It is refused on contact
/// with what a stream row *is*: every row there is a row in the height arithmetic
/// [`span_of`], [`block_of`], [`gap_rows`], [`landing_of`] and
/// [`vigia_core::Frame::height`] all share, and that arithmetic is what the diff's
/// scrollbar, the scroll clamp and I4's counting bound are computed from. A label
/// row there would be a change to the scroll model, bought to say a second time
/// what every heading already says in its kind letter's ink. So the two regions divide
/// it: **the map names the runs, and every row marks its own.**
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListRow {
    /// A run's separator: `──  staged  2 ─────`.
    Group {
        /// Which run begins here.
        origin: Origin,
        /// How many files that run holds **in total**, not how many are visible.
        ///
        /// A window showing two of seven staged files says `7`, so the number
        /// answers *how much is there* rather than restating what is on screen.
        count: usize,
    },
    /// A changed file.
    ///
    /// **Boxed, for the reason [`Row::File`] is**: a `FileEntry` is past 256 bytes
    /// since [#234](https://github.com/breferrari/vigia/issues/234), every variant
    /// of an enum is as large as its largest, and a separator carries two words.
    /// Unboxed, a `Vec<ListRow>` pays a file's worth of bytes for every label in
    /// it. [`ListRow::entry`] and the `From` below are why almost nothing has to
    /// spell the box.
    File(Box<FileEntry>),
}

impl ListRow {
    /// The file this row draws, or `None` for a run separator.
    ///
    /// **Every caller that reads the list wants this**, and it is here so that
    /// none of them has to spell the match: a `if let` per call site is a place a
    /// separator can be quietly treated as a missing file rather than as a row of
    /// a different kind.
    pub fn entry(&self) -> Option<&FileEntry> {
        match self {
            Self::File(entry) => Some(entry),
            Self::Group { .. } => None,
        }
    }
}

impl From<FileEntry> for ListRow {
    fn from(entry: FileEntry) -> Self {
        Self::File(Box::new(entry))
    }
}

/// One row of the plan, before any file has been diffed.
///
/// [`Slot::File`] carries an index into `Frame::files` rather than an entry,
/// because the two callers want different things from it: the painter needs a
/// drawn [`FileEntry`] and the input layer needs only the index, and building an
/// entry costs a whole [`heat_of`] walk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    /// A run's separator.
    Group {
        /// Which run begins here.
        origin: Origin,
        /// How many files that run holds in total.
        count: usize,
    },
    /// The file at this index in `Frame::files`.
    File(usize),
}

/// The rows the pinned list draws for a window of `rows` starting at file `top`.
///
/// **Separators are drawn only when both runs have files in them**, which is the
/// ruling rather than an economy. With the staged run hidden — the default — there
/// is one run and a label naming it says nothing the header does not, so the list
/// is exactly what it has always been and not one cell moves. With the run shown
/// but one side empty the same holds: there is nothing to separate.
///
/// **A window scrolled into the middle of a run opens with that run's label wherever
/// it has a row to spare for one.** Without it the rows at the top of the window are unattributed, which
/// is worse than the label costing a row: a reader who scrolled would be looking
/// at files with no idea which comparison they belong to. So the first row of any
/// grouped window is a separator.
///
/// The window is `rows` **drawn rows**, separators included, so a region deep
/// enough for six files shows fewer of them once it is paying for two labels.
/// That is `SPEC.md` §5.3's *richness is the reward of space* in the direction it
/// actually runs: the reader asked for a second run and the map costs what the
/// second run costs.
pub fn list_plan(files: &[vigia_core::FileChange], top: usize, rows: usize) -> Vec<Slot> {
    plan_with(files, Runs::of(files), top, rows)
}

/// Rows the pinned list wants, which is its files **plus its separators**.
///
/// **Not the file count since
/// [#313](https://github.com/breferrari/vigia/issues/313)**, and the difference is
/// load bearing rather than tidy: the region is sized from what it asks for, so a
/// grouped list sized from its files alone is two rows short and drops the *tail*
/// of the last run. Measured on the first grouped snapshot taken: a four-file view
/// asked for four rows, spent two of them on separators, and drew both unstaged
/// files, the staged run's heading, and **none of the staged files** — the run the
/// reader pressed `a` for, announced and then empty.
///
/// A separator is drawn per run that has files, which is the same condition
/// [`list_plan`] groups on and is resolved the same way, so the request and the
/// plan cannot disagree about how many rows a window needs. That is two for a
/// list holding both runs and one for a list that is entirely staged.
pub fn list_rows_wanted(files: &[vigia_core::FileChange]) -> usize {
    let runs = Runs::of(files);
    files.len() + runs.separators()
}

/// How many files each run holds, counted once.
///
/// **Extracted so the plan is not `O(files)` per call**, which is what made a
/// search over tops quadratic on the frame path: a plan draws at most `rows`
/// things and the only reason it walked the whole changed set was to count the
/// runs. Both searches below count once and plan many times.
#[derive(Debug, Clone, Copy)]
struct Runs {
    unstaged: usize,
    staged: usize,
}

impl Runs {
    /// **Counted by scanning, because a slice does not carry the boundary.**
    /// `Frame::staged_at` is the same number for free where a caller has the
    /// frame, and [`Runs::at`] is that door; this one exists for the public
    /// entry points that take a slice and for tests that build one by hand.
    fn of(files: &[vigia_core::FileChange]) -> Self {
        let staged = files
            .iter()
            .filter(|change| change.origin == Origin::Staged)
            .count();
        Self {
            unstaged: files.len() - staged,
            staged,
        }
    }

    /// The runs, from the boundary [`vigia_core::Frame::advance`] recorded.
    fn at(files: &[vigia_core::FileChange], staged_at: usize) -> Self {
        Self {
            unstaged: staged_at,
            staged: files.len() - staged_at,
        }
    }

    /// Whether the list draws run separators at all.
    ///
    /// **The question is whether a staged run exists, not whether both runs
    /// do.** It used to be both, and that made the view which most needs to
    /// announce itself, the one where *everything* is staged, the one that said
    /// nothing: no heading, and `None` for
    /// [`Heading::origin`](crate::render) to match on, which takes the ink for
    /// unstaged. A reader could not tell it from the default view (reported
    /// 2026-08-26).
    ///
    /// Dropping the labels is still right for a list that is entirely unstaged,
    /// because unstaged is what a reader is looking at unless something says
    /// otherwise, and a label on every row of the only run there is says
    /// nothing. It is wrong for a list that is entirely staged, where the same
    /// silence states the opposite of the truth.
    fn grouped(self) -> bool {
        self.staged > 0
    }

    /// How many separators a grouped list draws: one per run that has files.
    ///
    /// Two while both runs hold files, and one for a list that is entirely
    /// staged. Kept here rather than spelled `2` at the call site because
    /// [`list_rows_wanted`] sizes the region from it, and a region sized for
    /// more separators than the plan draws leaves a blank row while one sized
    /// for fewer drops the tail of the last run
    /// ([#313](https://github.com/breferrari/vigia/issues/313)).
    fn separators(self) -> usize {
        if !self.grouped() {
            return 0;
        }
        usize::from(self.unstaged > 0) + usize::from(self.staged > 0)
    }

    fn count(self, origin: Origin) -> usize {
        match origin {
            Origin::Unstaged => self.unstaged,
            Origin::Staged => self.staged,
        }
    }
}

/// [`list_plan`], with the run counts already taken.
///
/// **The one arithmetic**, so nothing below can compute a window's shape a second
/// way and come to disagree with what the painter draws — which is precisely the
/// class of defect this whole family of functions exists to close.
fn plan_with(files: &[vigia_core::FileChange], runs: Runs, top: usize, rows: usize) -> Vec<Slot> {
    let mut plan = Vec::with_capacity(rows);
    if rows == 0 || top >= files.len() {
        return plan;
    }
    let grouped = runs.grouped();

    let mut run: Option<Origin> = None;
    for (index, change) in files.iter().enumerate().skip(top) {
        if plan.len() == rows {
            break;
        }
        // **A run's label is drawn before any of its files, without exception.**
        //
        // This used to be the opposite rule: furniture gave way before content
        // did, so a window with room for one more thing spent it on the file and
        // dropped the label. That is what put a *staged* file on the last row
        // under a heading that said `unstaged` (reported from a real worktree,
        // 2026-08-26). The two outcomes are not comparable, which is why the
        // reader overruled it. A list one row shorter is merely smaller, and the
        // rail already says there is more to come; a file under the wrong run's
        // heading is the list asserting something false about where the reader's
        // change lives, and no amount of room saved buys that back.
        //
        // The degenerate case the old rule was guarding is still handled, just
        // in the other direction: a one-row grouped window now draws the label
        // and no file, which says less than it could but nothing untrue.
        if grouped && run != Some(change.origin) {
            if plan.len() == rows {
                break;
            }
            plan.push(Slot::Group {
                origin: change.origin,
                count: runs.count(change.origin),
            });
            run = Some(change.origin);
        }
        if plan.len() == rows {
            break;
        }
        plan.push(Slot::File(index));
    }
    plan
}

/// Whether a window of `rows` drawn rows starting at `top` draws `file`.
///
/// **One predicate, two callers**, and it is the question both searches below are
/// actually asking: [`top_showing`] walks tops until it stops being true, and
/// [`following_top`] asks it once about the window it already has. Written twice
/// it was also written two ways, the second collecting a `Vec` of every drawn
/// index to ask about one of them.
fn draws_file(
    files: &[vigia_core::FileChange],
    runs: Runs,
    top: usize,
    rows: usize,
    file: usize,
) -> bool {
    plan_with(files, runs, top, rows)
        .iter()
        .any(|slot| matches!(slot, Slot::File(at) if *at == file))
}

/// The **smallest** top a window of `rows` drawn rows can start at and still draw
/// `file`.
///
/// **Smallest, and the first version of this returned the largest**, which is a
/// bound that never binds: the largest top showing any file is that file's own
/// index, trivially, so the clamp built on it let the window scroll past the end
/// and leave the last file alone above a region of blanks. The gate missed it by
/// asserting only that the answer *shows* the file, which the wrong answer does.
///
/// **Searched from `file` downward and bounded by `rows`**, because a window of
/// `rows` rows can never draw more than `rows` files, so nothing further back can
/// reach it. That keeps this `O(rows^2)` against a changed set of any size, where
/// walking every index and planning `O(files)` each time was `O(files^2)` on the
/// frame path.
fn top_showing(files: &[vigia_core::FileChange], runs: Runs, file: usize, rows: usize) -> usize {
    if rows == 0 || files.is_empty() {
        return 0;
    }
    let file = file.min(files.len() - 1);
    let draws = |top: usize| draws_file(files, runs, top, rows, file);
    let floor = file.saturating_sub(rows);
    let mut best = file;
    for top in (floor..file).rev() {
        if !draws(top) {
            break;
        }
        best = top;
    }
    best
}

/// The last top a window of `rows` drawn rows can start at and still show the
/// last file, which is the **tightest** such top rather than the largest.
///
/// **`files - rows` is the answer only while every drawn row is a file**, and a
/// grouped window spends one or two of them on separators. Clamped that way the
/// window stops short and the tail of the staged run cannot be reached at all —
/// silently, because nothing on screen says the map has an end it will not show.
///
/// **And it is the *tightest* such top**, so the window is still pulled back the
/// way `take_list` has always pulled it back: the last file rests on the bottom
/// row rather than leaving blanks under it, which matters most when the changed
/// set shrinks under a stale position (an agent committing, resetting or switching
/// branch, which is the ordinary event on the pane this tool is for).
pub fn last_top(files: &[vigia_core::FileChange], rows: usize) -> usize {
    if files.is_empty() {
        return 0;
    }
    top_showing(files, Runs::of(files), files.len() - 1, rows)
}

/// The window a list following the diff should show, given where the diff is.
///
/// **Minimal movement, which is `take_list`'s own rule**: the window is held while
/// the current file is inside it and pushed by exactly the overshoot when it
/// leaves, so scrolling from the start walks the caret down the rows before the
/// list moves under it.
///
/// **What this replaces is `current + 1 - rows`**, which subtracts a count of
/// *drawn rows* from a *file index*. They are the same number only while every row
/// is a file: once a window can hold a separator the arithmetic lands short, the
/// list scrolls, and the file the diff is inside is not among the rows it drew —
/// so the caret marking that file is simply absent, on exactly the frames a reader
/// is watching it move.
pub fn following_top(
    files: &[vigia_core::FileChange],
    from: usize,
    current: usize,
    rows: usize,
) -> usize {
    if files.is_empty() || rows == 0 {
        return 0;
    }
    let runs = Runs::of(files);
    if draws_file(files, runs, from, rows, current) {
        return from;
    }
    if current < from {
        // Off the top: the window starts on it.
        return current;
    }
    // Off the bottom: the smallest window that reaches it, so it lands on the
    // last row rather than the first and the rows above it stay on screen.
    top_showing(files, runs, current, rows)
}

/// The file a drawn list row addresses, or `None` for a separator.
///
/// **The one place a row becomes a file**, read by the click handler and by the
/// digit jumps alike. Both used to add the offset to the window's first *file*,
/// which is the same number only while every row is a file.
pub fn file_at(
    files: &[vigia_core::FileChange],
    top: usize,
    rows: usize,
    row: usize,
) -> Option<usize> {
    match list_plan(files, top, rows).get(row) {
        Some(Slot::File(index)) => Some(*index),
        _ => None,
    }
}

/// What a row of the body is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    /// A changed file's heading, inside the diff stream.
    ///
    /// The same [`FileEntry`] the pinned list draws, so the two regions cannot
    /// drift apart in what they show or in how they degrade.
    ///
    /// **Boxed since [#234](https://github.com/breferrari/vigia/issues/234)**,
    /// which doubled the sparkline's source resolution and took a `FileEntry`
    /// past 256 bytes. Every variant of an enum is as large as its largest, and
    /// the largest is drawn a few dozen times a screen while [`Row::Line`] is
    /// drawn on every row of it, so unboxed the *common* case was paying 264
    /// bytes to carry 53. One allocation per heading buys that back, and it is
    /// the direction `clippy::large_enum_variant` names.
    ///
    /// It does soften a claim recorded when `spark` first landed here, that a
    /// drawn row allocates nothing new. That was about the array being fixed
    /// rather than a `Vec`, which is still true; what is new is one allocation
    /// per *file heading*, where a heading already carries a `String` path.
    File(Box<FileEntry>),
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
        /// Byte ranges of `text` that changed within the line, from
        /// [`vigia_core::Line::emph`]: the pair-aligned word-level diff the
        /// renderer draws as the hotter wash. Empty means whole-line, which is
        /// every context row, every unpaired changed row, and every row of a
        /// diff computed before this field existed.
        emph: Vec<std::ops::Range<u32>>,
    },
    /// The tail of a [`Row::Line`] that did not fit, on the row below it.
    ///
    /// **`SPEC.md` §11.2 B19**, from `w`
    /// ([#272](https://github.com/breferrari/vigia/issues/272)). It exists only
    /// while wrapping is on, it always follows the [`Row::Line`] it belongs to,
    /// and there is never more than one: the cap is two display rows, which is
    /// `delta --wrap-max-lines`' own default and is what bounds the reflow §11.1
    /// refused to permit unbounded.
    ///
    /// **A row of its own rather than a flag on [`Row::Line`]**, and that is the
    /// whole of why the row arithmetic did not have to change. Every guard in
    /// [`View::take_file`] and [`View::collect`] is `self.rows.len() < height`,
    /// so a continuation that *is* a row makes `rows` a count of **display** rows
    /// with no site restated. A flag would have left `rows.len()` counting
    /// logical rows and made every one of those guards a units bug of exactly
    /// the kind this project has already paid for once.
    ///
    /// **It carries the tail already sliced, and its [`Row::Line`] carries only
    /// the head.** [`View::wrap_rows`] decides the split once, so the painter
    /// draws what the row says instead of re-deriving a boundary that could
    /// disagree with the one the row count was taken from. It is also what
    /// suppresses the `›` on the head for free: a pre-sliced head does not clip,
    /// so nothing marks it, and `›` goes on meaning *rightward* only.
    Wrap {
        /// The kind of the line this continues, for the wash, the bar and the
        /// ink on the continuation mark.
        kind: LineKind,
        /// The tail, from the split to the end of the line. Still the whole
        /// tail: past the cap it is the painter that clips it and marks it,
        /// which is `SPEC.md` §11.1's clipping rule reaching the lower row.
        text: String,
        /// [`Row::Line::spans`], re-based onto `text`.
        spans: Vec<Span>,
        /// [`Row::Line::emph`], re-based onto `text` and clipped to it.
        emph: Vec<std::ops::Range<u32>>,
        /// Columns of leading blank before the tail, so nested code keeps its
        /// block shape: Neovim's `'breakindent'`, capped at half the content
        /// width. [`indent_of`] is the rule.
        indent: usize,
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

impl Row {
    /// A file heading row.
    ///
    /// **A constructor because the variant is boxed**
    /// ([#234](https://github.com/breferrari/vigia/issues/234)), and it exists so
    /// the box is written once rather than at every one of the thirty-odd places
    /// that build one. `Row::File` is still the pattern to match on, which is
    /// where the box is invisible anyway.
    pub fn file(entry: FileEntry) -> Self {
        Self::File(Box::new(entry))
    }
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

/// What a drawn sparkline bucket's height is divided by, one figure per rung.
///
/// **A type rather than a number since
/// [#234](https://github.com/breferrari/vigia/issues/234)**, because a rung is a
/// resolution now and a drawn bucket is the sum of `group` source ones. The
/// figure those sums are measured against is not the source figure multiplied:
/// `scale_of` averages the **non-empty** values and grouping merges empties into
/// their neighbours, so the estimate is exact on a busy worktree and wrong by up
/// to the group on a quiet one. `vigia_core::SPARK_GROUPS` carries the
/// measurement that settled it.
///
/// Indexed **by grouping rather than by rung**, so the one number a caller has
/// (how many source buckets this drawn one holds) is the one it looks up with,
/// and no call site has to know where its rung sits in a table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Scale(pub [u32; SPARK_GROUPS.len()]);

impl Scale {
    /// One figure at every grouping.
    ///
    /// **For a fixture rather than for the store**, which never produces one:
    /// the honest set is what `scale_of` says at each grouping, and that is what
    /// [`View::collect`] fills in. This is for a test that wants a stated
    /// denominator, including the inconsistent ones two gates deliberately pass.
    pub const fn flat(figure: u32) -> Self {
        Self([figure; SPARK_GROUPS.len()])
    }

    /// `figure` scaled by each grouping, saturating.
    ///
    /// **Also for a fixture, and exact for the ones this suite writes.** A test
    /// that widened its source by repeating each value has, by construction, the
    /// same non-empty density at every grouping, which is the one case where
    /// multiplying is the right answer rather than an estimate.
    pub const fn spread(figure: u32) -> Self {
        let mut figures = [0; SPARK_GROUPS.len()];
        let mut at = 0;
        while at < SPARK_GROUPS.len() {
            figures[at] = figure.saturating_mul(SPARK_GROUPS[at] as u32);
            at += 1;
        }
        Self(figures)
    }

    /// The figure a bucket summing `group` source buckets is measured against.
    ///
    /// Falls back to the finest figure for a grouping this ladder does not name.
    /// **No rung the ladder produces can ask for one**, because `SPARK_RUNGS` is
    /// computed from [`SPARK_GROUPS`] in `render.rs` rather than written out
    /// beside it, so every grouping a layout divides to is on this table by
    /// construction. What can still reach the fallback is a hand-written
    /// `ROW_LAYOUTS` entry, since `Columns::new` takes a bare `usize`; total
    /// rather than panicking, because `render`'s contract is that any area is
    /// legal and a frame draws something.
    pub fn at(self, group: usize) -> u32 {
        SPARK_GROUPS
            .iter()
            .position(|named| *named == group)
            .map_or(self.0[0], |at| self.0[at])
    }
}

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
    /// Columns the diff region's **glyphs** have, from
    /// [`crate::render::Body::diff`] less the inset and any scrollbar.
    ///
    /// **The one geometry this function needs and did not used to**
    /// ([#272](https://github.com/breferrari/vigia/issues/272)). Every row was
    /// built without reference to how wide the pane was, because clipping is the
    /// painter's and clipping changes no row count. Wrapping does, so the width
    /// has to arrive here.
    ///
    /// Zero is legal and means *do not wrap*: it is what a caller that has not
    /// been told the width passes, and it produces the rows every version before
    /// this one produced.
    pub width: usize,
    /// Whether a content line too wide for the pane continues on the row below.
    ///
    /// `SPEC.md` §11.2 **B19**, from `w`
    /// ([#272](https://github.com/breferrari/vigia/issues/272)). Off on launch.
    ///
    /// A flag beside [`Self::single`] rather than something this function could
    /// work out, for the plainest of the reasons those fields give: it is a
    /// reader's standing request, and nothing about a position says whether one
    /// was made.
    pub wrap: bool,
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
    /// Whether [`Self::position`] was placed by follow and still wants its row.
    ///
    /// **A request, not an answer**, and that is what makes it affordable.
    /// [`crate::App::follow`] takes `&Frame` so that following cannot diff,
    /// cannot read and cannot `stat` (I4), which leaves it nothing to compute a
    /// row from: the loop advances, follows, and only then draws, so the frame
    /// holds the *previous* tick's diff for the one file that just changed.
    /// Carrying the request here defers the arithmetic to the walk below, which
    /// has a fresh diff for that file in hand and pays nothing for it.
    ///
    /// A flag rather than something [`View::collect`] could work out, for the
    /// reason [`Self::anchored`] gives: a position on row zero looks identical
    /// whether follow placed it or a reader pressed `g`, and those want opposite
    /// answers. [`View::landed`] reports back whether it was served.
    pub landing: bool,
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
    /// Whether the diff is pinned to the one file [`Self::position`] is inside.
    ///
    /// `SPEC.md` §11.2 **B16**, from `s`
    /// ([#297](https://github.com/breferrari/vigia/issues/297)). The walk stops
    /// at that file rather than carrying the remainder into the one below it, so
    /// the rows this frame can reach are the file's own, and the reader changes
    /// file through the list rather than by scrolling off the end of one.
    ///
    /// A flag beside [`Self::measured`] rather than a rule this function could
    /// work out, and for the plainest of the reasons those fields give: it is a
    /// reader's standing request, and nothing about a position says whether one
    /// was made.
    ///
    /// **It only ever removes work.** The walk reaches one file where it could
    /// have reached several, and [`View::measure`] reads the total off the
    /// pinned file rather than counting the changed set, so a pinned frame is
    /// bounded by strictly less than an unpinned one and I4 never sees it.
    pub single: bool,
}

impl Default for Viewport {
    /// Hand written for one field, and only that field.
    ///
    /// [`Self::highlight`] defaults to **true**, which `derive` cannot give and
    /// which is the whole reason this impl exists. Every other field's derived
    /// answer is the honest one: row zero, not anchored, no rows, no total, no
    /// landing owed, and the whole diff rather than one file of it.
    /// `false` there means *this frame skips work*, and a caller who forgot the
    /// field gets a cheaper frame that looks the same. [`Self::single`] is that
    /// kind of field twice over: `false` is both the state a reader who has
    /// asked for nothing is in, and the more expensive frame of the two.
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
            width: 0,
            wrap: false,
            list_top: 0,
            list_rows: 0,
            list_follows: false,
            measured: false,
            landing: false,
            highlight: true,
            single: false,
        }
    }
}

/// A screenful of rows, plus what the chrome needs to describe it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct View {
    /// The rows to draw, top to bottom.
    ///
    /// **Display rows since [#272](https://github.com/breferrari/vigia/issues/272)**,
    /// where they were logical rows and the two were one number. A wrapped
    /// content line is a [`Row::Line`] carrying its head and a [`Row::Wrap`]
    /// carrying its tail, so this length is what the terminal draws and the
    /// diff's own row count is what [`crate::rows_of`] and its twins still
    /// report. `SPEC.md` §11.2 **B19** is the ruling and says why the scrollbar
    /// stayed on the second of those.
    pub rows: Vec<Row>,
    /// Digits reserved for line numbers on every content row, or zero for none.
    ///
    /// **Decided where the rows are decided, and carried rather than
    /// re-derived** ([#272](https://github.com/breferrari/vigia/issues/272)).
    /// [`crate::render::gutter_width`] used to be the painter's alone, which was
    /// safe while nothing else needed it. Wrapping needs it *before* the painter
    /// runs, because the width left for text is what decides whether a line
    /// wraps at all, and the row set the painter would size it from is the one
    /// wrapping has already truncated. Two derivations over two different row
    /// sets is a pane whose text bound and whose gutter disagree by a column.
    ///
    /// So [`View::wrap_rows`] takes it once, over the rows the walk reached, and
    /// the painter reads it. With wrapping off that is byte-identical to what the
    /// painter used to compute for itself, which `tests/render.rs` holds as a
    /// buffer comparison.
    ///
    /// **`None` is *nobody decided*, and the painter then decides for itself**,
    /// which is every version of this before
    /// [#272](https://github.com/breferrari/vigia/issues/272) and is what a
    /// [`Viewport`] carrying no width produces. It is a third state rather than a
    /// zero because zero is a legal answer: it is what a pane too narrow for line
    /// numbers gets, and a field that could not tell the two apart would make a
    /// hand-built view claim a decision it never took.
    pub gutter: Option<usize>,
    /// The pinned file list, top to bottom, at most `Viewport::list_rows` long.
    ///
    /// Bounded by the region and never by the changed set, which is the whole of
    /// how `SPEC.md` §11.1 keeps the list inside I4: one `Frame::diff` per row
    /// drawn, and under I2a a file that did not change is a `stat` and a cache
    /// hit that reads no bytes. Empty when the pane is too short for a region.
    ///
    /// **Not one file per row since
    /// [#313](https://github.com/breferrari/vigia/issues/313)**: a window showing
    /// both runs opens each with a separator. [`list_plan`] is what decides, and
    /// it is what a caller turning a row back into a file must resolve through.
    pub list: Vec<ListRow>,
    /// How many files the pinned list's scrollbar treats as one screenful.
    ///
    /// **The complement of the window's ceiling**, so the bar's travel
    /// (`files - span`) is exactly the travel `Action::ListTo` scales the track
    /// onto, and the two cannot disagree about where the bottom is.
    ///
    /// **Not the row count and not what this window happens to show**, which were
    /// the two previous answers and were each wrong in their own way. The row
    /// count over-reports, because a grouped window spends rows on separators. What
    /// *this* window shows varies with where it starts — a window opening on a run
    /// boundary spends one separator and one inside a run spends two — so the thumb
    /// changed length mid-drag and walked away from where the reader dropped it.
    ///
    /// A screenful is a property of the list, not of the position, which is what
    /// makes this constant across a scroll and what makes a drag round-trip.
    pub list_span: usize,
    /// Whether this frame shows both runs, and therefore draws the run separators.
    ///
    /// **A fact about the frame rather than about any row**, which is why it is
    /// here and not on [`FileEntry`]: a separator names a run, and whether there
    /// are two runs to name is not something a single row can answer. It is the
    /// same quantity [`list_plan`] groups on, resolved once so the list's
    /// separators and the stream's cannot disagree about it.
    ///
    /// **It used to decide a column as well**, and that is gone with
    /// [#316](https://github.com/breferrari/vigia/issues/316): the staged mark is
    /// the kind letter's own ink now, so nothing about the row's geometry depends
    /// on this and a grouped pane lays out exactly like an ungrouped one.
    pub grouped: bool,
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
    /// Whether this frame resolved the landing [`Viewport::landing`] asked for.
    ///
    /// **What lets a caller clear the request only once it was served.**
    /// [`View::collect`] returns before the walk when the pane has no diff
    /// region, and a request dropped there would leave a reader on the heading
    /// for good: the tick that would have re-armed it has already been spent.
    /// That one state, and not the other early return: a worktree with no
    /// changed files has no heading to be stranded on, and
    /// [`crate::App`]'s own staleness test drops the request there anyway. False
    /// whenever nothing was asked for, so a caller reading it needs no separate
    /// test for that.
    pub landed: bool,
    /// Files this viewport asked the frame for, drawn or merely crossed.
    ///
    /// Reported so a test can hold the *shell* to I4, not just the core. One,
    /// once the position has settled and a single file fills the screen.
    pub read: usize,
    /// [`FileEntry`] values built for the **record** rather than for a row.
    ///
    /// [`Self::read`]'s sibling and it exists for the same reason: a cost the
    /// core's `FrameStats` cannot see, because building one is a walk over lines
    /// the frame already holds rather than a read of anything. `entry_of` runs
    /// the heat projection, which is O(the file's diff), so the difference
    /// between building one and not is real and invisible from outside.
    ///
    /// **Only the recorded ones**, which is narrower than every entry built and
    /// is the number the one guard here turns on. [`Changed::listed`] stops the
    /// walk recording an entry for the file the viewport sits inside when the
    /// pane has no list to serve it to, and an entry drawn as a row is built
    /// whatever the pane is. A counter over both would be satisfied by either,
    /// so deleting the increment that matters left it green: this counts the
    /// site the guard is about and nothing else.
    ///
    /// A frame that backs up counts the file it crossed twice, because the walk
    /// runs again and the record is deliberately kept across the restart. That
    /// is the same doubling [`Self::read`] has, and it is accurate rather than
    /// wrong: two entries really are built.
    pub recorded: usize,
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
    ///
    /// **One figure per rung since
    /// [#234](https://github.com/breferrari/vigia/issues/234)**, because a
    /// narrower rung draws sums of source buckets and `scale_of` is not linear in
    /// the grouping. [`Scale`] says why, and `vigia_core::SPARK_GROUPS` carries
    /// the measurement.
    pub scale: Scale,
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
    1 + diff.hunks.iter().map(hunk_span).sum::<usize>()
}

/// Rows one hunk occupies: its `@@` header and then its lines.
///
/// **Named because three places count it**, and [`gap_rows`] one function down
/// records what happens when a quantity is spelled at each site instead: it
/// drifted from the doc naming the sites, and that was the third time a
/// quantity spelled at each site had drifted on this branch. [`span_of`]
/// sums it, [`View::take_file`] steps over a hunk above the window with it, and
/// [`landing_of`] walks to a hunk's header row with it. All three want the same
/// number by the same route, unlike [`span_of`] and [`rows_of`], which are twins
/// precisely because their routes differ.
/// Split a line's syntax runs at a byte offset, for a wrapped row.
///
/// [`vigia_core::Span`] carries a length rather than a range, so the runs are a
/// tiling read forward from zero, and the run the cut lands inside becomes two.
/// The alternative, handing the whole list to both rows and letting the painter
/// offset it, was refused for the reason [`Row::Wrap`] carries its tail already
/// sliced: one decision, taken where the row count is taken.
///
/// A cut past the end leaves the tail empty, which is legal and draws nothing:
/// an empty span list and an uncovered tail already reach the screen identically
/// ([`Row::Line::spans`]).
fn split_spans(spans: &[Span], at: usize) -> (Vec<Span>, Vec<Span>) {
    let mut head = Vec::with_capacity(spans.len());
    let mut tail = Vec::with_capacity(spans.len());
    let mut pos = 0usize;
    for span in spans {
        let end = pos + span.len;
        if end <= at {
            head.push(*span);
        } else if pos >= at {
            tail.push(*span);
        } else {
            head.push(Span {
                len: at - pos,
                class: span.class,
            });
            tail.push(Span {
                len: end - at,
                class: span.class,
            });
        }
        pos = end;
    }
    (head, tail)
}

/// Split a line's word-emphasis ranges at a byte offset, for a wrapped row.
///
/// The tail's ranges are re-based onto the tail, because that is what the tail
/// row's `text` is: a range left in the whole line's coordinates would paint the
/// hot background somewhere else entirely, and `Painter::push_split` has no way
/// to know which of the two rows it is drawing.
///
/// A range straddling the cut becomes two, one on each side, which is what keeps
/// a word that wraps looking like one word.
fn split_emph(
    emph: &[std::ops::Range<u32>],
    at: usize,
) -> (Vec<std::ops::Range<u32>>, Vec<std::ops::Range<u32>>) {
    let cut = at as u32;
    let mut head = Vec::with_capacity(emph.len());
    let mut tail = Vec::with_capacity(emph.len());
    for range in emph {
        if range.start < cut {
            head.push(range.start..range.end.min(cut));
        }
        if range.end > cut {
            tail.push(range.start.max(cut) - cut..range.end - cut);
        }
    }
    (head, tail)
}

fn hunk_span(hunk: &Hunk) -> usize {
    1 + hunk.lines.len()
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

/// Rows into a file's block where follow should put the top of the viewport.
///
/// I5 is *the viewport goes to what just changed*, and until
/// [#257](https://github.com/breferrari/vigia/issues/257) that was read as the
/// file. On a file whose diff is one screenful the two are the same place and
/// the promise is kept by accident of size; on a Swift test file carrying a
/// 76-line deletion under three one-line tweaks they are twenty-odd rows apart,
/// and the element built to show a reader what an agent just did showed them a
/// filename. Reported from a live pane.
///
/// **The heading whenever it is free, and the busiest hunk when it is not.**
/// Zero means the heading stays on the top row, which is what every other jump
/// on this map resolves to and what the reader wants when the change is already
/// drawn: the heading carries the path, the counts, the sigil and the heat
/// strip, so moving it off screen for a file that fits is a loss with no gain.
///
/// **What has to be on screen for that is a changed *line*, not the `@@` header
/// above it**, and the first draft of this tested the header. On a pane one row
/// shorter than the hunk's lead-in that draws the reader a bare hunk header with
/// none of its content under it, which is #257's own reported symptom with the
/// gate reporting it handled. So the visibility test is the busiest hunk's first
/// changed line, and the row landed on is still its header, because a change
/// arrives with the `@@` that says where it is.
///
/// **Which makes it two tests rather than one**, and the second is the floor:
/// a landing is worth the heading only if the change is drawn *from the
/// landing* as well as not drawn from the heading. `Body::split` gives the diff
/// a single row at its floor, where the first draft put one bare `@@` in the
/// whole region and the heading it replaced carried the path, the counts, the
/// sigil and the strip.
///
/// **The busiest hunk rather than the first.** A block is a heading and then a
/// header plus lines per hunk ([`span_of`]), so the first hunk's header is
/// always row **1** and landing on it moves the pane by one row: it cannot
/// reach the case this exists for. What a reader asks of a monitor is *what did
/// it just do*, and the largest concentration of changed lines is the closest
/// the diff alone comes to answering it.
///
/// **A hunk rather than a heat bucket.** #257 proposes [`heat_of`]'s busiest
/// slice, which is the same intent in the wrong unit: a bucket names a
/// **working-tree line**, and a viewport needs a **row**. The thing a row falls
/// inside is a hunk, so going through the projection would mean a second rule to
/// keep in step with `heat_of` for an answer the hunks already give exactly.
///
/// **What this is not is the *newest* hunk**, which is what the reader is really
/// after and which the diff cannot say: it is a comparison against the previous
/// tick's, and that is a design with staleness rules of its own. Busiest is
/// never worse than the heading it replaces and is right in the reported shape.
/// `SPEC.md` §11.1 records it as the case to reopen on.
///
/// Ties keep the earlier hunk, because a reader scrolls forward more readily
/// than back. A note block ([`note_for`]) and a file with no hunks have nothing
/// to land on and stay on their heading.
fn landing_of(kind: &ChangeKind, diff: &FileDiff, height: usize, wrap: bool) -> usize {
    if note_for(kind, diff).is_some() {
        return 0;
    }

    // Row zero is the heading, so the first header sits at one. Walked rather
    // than indexed because a hunk's height is its own line count, which is
    // exactly the sum `span_of` takes.
    let mut row = 1;
    let mut busiest = 0;
    let mut landing = 0;
    // Two rows per hunk, and they answer different questions: the header is
    // where the viewport goes, and the first changed line under it is what has
    // to be on screen for going there to be unnecessary.
    let mut change = 0;
    for hunk in &diff.hunks {
        // One pass for both, because the second one cannot fail once the first
        // has counted anything: a hunk with a changed line has a first changed
        // line. Written as a pair rather than as a `position` with an
        // `unwrap_or` under the `changed > busiest` guard, which is the same
        // claim expressed as a branch nothing can reach.
        let mut changed = 0;
        // A hunk opens with up to `CONTEXT` unchanged lines, and with fewer at
        // the very start of a file, so the lead-in is counted rather than
        // assumed.
        let mut lead = None;
        for (at, line) in hunk.lines.iter().enumerate() {
            if line.kind != LineKind::Context {
                changed += 1;
                lead.get_or_insert(at);
            }
        }
        // Strictly greater, which is what keeps the earlier of two equal hunks.
        if let Some(lead) = lead
            && changed > busiest
        {
            busiest = changed;
            landing = row;
            change = row + 1 + lead;
        }
        row += hunk_span(hunk);
    }

    // **Two questions, and a landing has to answer both.** `height` is the diff
    // region's, so this is the one place the rule depends on the pane, and it is
    // why a reader who makes the pane taller stops being moved off the heading.
    //
    // **And the rows a pane holds are not the rows the terminal has once lines
    // wrap** ([#272](https://github.com/breferrari/vigia/issues/272)). Both tests
    // below compare a row of this file's block, which is a *logical* row, against
    // `height`, which counts the terminal's, and the two are one number only
    // while nothing wraps. With `w` on a change judged visible from the heading
    // can be below the fold, which is I5's own promise broken on the one frame it
    // exists for.
    //
    // **A bound rather than a measurement, and deliberately the loose one.**
    // Measuring the landing in display rows needs the text width, the text width
    // needs the gutter, and the gutter is taken from the rows this decision
    // helps choose: the circularity §11.2 B19 records for the scrollbar's total,
    // one element over. What is available without it is the cap: a line occupies
    // at most two display rows, so a screen of `height` display rows always holds
    // at least half that many rows of the diff. Erring here moves the reader onto
    // the change they were not going to see, never away from one they were, which
    // is the direction I5 is written in.
    let height = if wrap { (height / 2).max(1) } else { height };

    // Already drawn from the heading, so the jump would cost the heading and buy
    // nothing.
    if change < height {
        return 0;
    }
    // And still not drawn from the landing, which is `Body::split`'s floor: a
    // one-row region draws the `@@` and nothing under it, and one bare hunk
    // header is strictly less than the heading it replaced, which carries the
    // path, the counts, the sigil and the strip. Moving the reader for a change
    // they still cannot see is the defect this whole rule exists to fix, so a
    // pane too short to show one keeps what it had.
    if change - landing >= height {
        return 0;
    }
    landing
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
    /// Which run this file is in, for the ink on the row's kind letter.
    origin: Origin,
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
    /// Whether the pane has a pinned list at all.
    ///
    /// The walk records an entry for the file the viewport sits inside even
    /// though its heading is above the window, so that [`View::take_list`] does
    /// not ask the frame for that file a second time. On a pane too short for a
    /// region there is no `take_list` to serve: it returns at `rows == 0`, the
    /// record is dropped unread, and building it is a whole [`heat_of`] walk
    /// over that file's diff, every frame, bought for nothing.
    ///
    /// Only the *undrawn* entry is conditional. A drawn heading needs one
    /// whatever the pane is, because it is the row.
    listed: bool,
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
fn entry_of(kind: &ChangeKind, origin: Origin, diff: &FileDiff, history: &History) -> FileEntry {
    FileEntry {
        path: diff.path.clone(),
        origin,
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

/// Rows the file at `index` draws, without the blank that would close its block.
///
/// [`rows_in`]'s sibling, and the pair exists because a pinned diff and an
/// unpinned one measure different things. Unpinned, a position is a row of the
/// whole diff and the blank between two files is one of those rows, so
/// [`rows_in`] is what a caller holding a position wants. Pinned, the frame
/// draws one file and no blank follows it, so the reachable rows are the file's
/// own and this is what the scrollbar's total and a drag on it are both scaled
/// against.
///
/// **Both, rather than one with a flag**, because the two are asked for by
/// different callers for different reasons and a boolean parameter at each site
/// is the shape [`gap_rows`]' own docblock records drifting: a term spelled at
/// every site is a term a site eventually forgets.
///
/// **The cache route, which is [`block_rows`]'s and not [`rows_in`]'s**, and the
/// first draft of this took the other one on a reason that was false. It read
/// *"costs what `Frame::diff` costs, which for a file the frame is already
/// holding is one `stat` and no read"*, which is `block_rows`' cost quoted under
/// `rows_in`'s call: [`vigia_core::Frame::diff`] **re-reads a file written in the
/// last two seconds by design**, and the pinned file is the file an agent is
/// writing, so that is the one file for which it is never a `stat`. It made two
/// whole-file diffs of that file per gesture where one would do, the second of
/// them microseconds later inside [`View::collect`].
///
/// **And the cheaper route is also the correct one, which is what settled it.**
/// A drag has to invert the arithmetic of the bar **already on screen**, and that
/// bar was drawn on the *previous* paint from the diff the frame held then.
/// [`vigia_core::Frame::rows_of`] reads exactly that generation; a fresh
/// `Frame::diff` can return a height newer than anything the reader has seen, so
/// the gesture would resolve against a bar that was never drawn. It is also the
/// route the **unpinned** drag already takes through [`block_rows`], so the two
/// arms of one gesture now read one generation rather than two.
///
/// [`span_of`] and [`crate::rows_of`] are the counting twins and agree exactly,
/// which `tests/scroll.rs::the_counting_twins_agree_with_the_rows_drawn` holds,
/// so reading the second here is the same number the walk builds from the first.
///
/// # Panics
///
/// If `index` is out of range, the same way [`vigia_core::Frame::rows_of`] does.
/// [`crate::App::pinned_file`] is what keeps the pinned callers off that index.
pub fn span_in(frame: &mut Frame, index: usize) -> Result<usize> {
    frame.rows_of(index, rows_of)
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

    /// How many **files** the pinned list is showing, which is not how many rows
    /// it drew.
    ///
    /// **Not the bar's span**, which is [`View::list_span`]: this is what *this*
    /// window shows and that is a property of the position, so a bar drawn from it
    /// changes length as a reader scrolls. It was the bar's span for one round of
    /// #313's audit and the correction is recorded there.
    ///
    /// What it is for is asking a drawn screen what it drew, which is what the
    /// gates in `tests/list.rs` want and what nothing in the renderer does.
    pub fn listed_files(&self) -> usize {
        self.list.iter().filter_map(ListRow::entry).count()
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
            width,
            wrap,
            list_top,
            list_rows,
            list_follows,
            measured,
            landing,
            highlight,
            single,
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
        // Resolved from the changed set rather than from the toggle, so a reader
        // who asks for the staged run and has nothing staged gets the pane they
        // already had rather than a column and a label saying nothing. The header
        // is what acknowledges the keypress in that case.
        //
        // **From the boundary the walk recorded**, which is a subtraction where a
        // filter over the changed set would be a pass: `Frame::advance`
        // concatenates the runs and knows where they part.
        let grouped = Runs::at(frame.files(), frame.staged_at()).grouped();
        let mut view = Self {
            grouped,
            // **Initialised to "nothing to scroll" rather than to zero**, so every
            // path out of this function leaves a span a scrollbar can be asked
            // about. `take_list` returns before it computes one on a pane with no
            // list region, and `collect` returns before *it* on an empty worktree;
            // a leftover zero reads as "this window shows none of the list", which
            // is `scrollable` saying yes. Nothing draws a bar there today, but only
            // because `bar_for`'s own `rows >= MIN_TRACK` guard happens to catch
            // it first, and a field that is safe by a guard somewhere else is the
            // shape this module keeps finding.
            list_span: files.max(1),
            // Bounded by the screen, not by the diff. The cap keeps a caller
            // asking for an absurd height from allocating for it up front.
            rows: Vec::with_capacity(height.min(64)),
            list: Vec::with_capacity(list_rows.min(64)),
            // Resolved below, once the walk has said where the diff landed. Both
            // start where they were asked to, so a frame with no room to draw
            // reports the request back unchanged and a caller keeps its place.
            list_top,
            gutter: None,
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
            landed: false,
            read: 0,
            recorded: 0,
            scale: Scale(history.scales()),
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
        // Bounded by the viewport: one per file the walk reaches, which is one
        // per heading that fits plus the file the viewport is sitting inside.
        let mut drawn: Vec<(usize, FileEntry)> = Vec::new();

        // **The one bound the pin costs, and every use of it below reads this
        // rather than `files`.** Under `SPEC.md` §11.2 B16 the diff is the file
        // the position is inside and nothing after it, so the walk's exclusive
        // end is that file rather than the changed set's.
        //
        // Fixed before the loop because `view.top.file` is fixed before the
        // loop: it is the request clamped into range, the walk starts there, and
        // under a pin the walk cannot advance off it. The restart below resolves
        // through [`Self::last_screenful`], which is given the same file at both
        // ends and so cannot move it either.
        //
        // **[`gap_rows`] reads it too, which is what drops the closing blank.**
        // That row separates two *drawn* files ([#165](https://github.com/breferrari/vigia/issues/165))
        // and a pinned frame draws one, so the file's last content row is the
        // last row a reader can reach: the same rule the last file of an
        // unpinned diff already gets, applied to the only file there is.
        //
        // **Both ends in one binding, and the pair is why.** The walk's start is
        // also the floor the short back-up below compares against, and the two
        // were written as separate `if single` expressions two hundred lines
        // apart with only prose saying they were the same range. That is the
        // shape B16's own recorded defect took: a bound that reads as *the
        // floor* and means *the floor of some other walk*. One binding leaves
        // nothing for a future edit to move half of.
        let (first, stop) = if single {
            (view.top.file, view.top.file + 1)
        } else {
            (0, files)
        };

        let mut index = view.top.file;
        let mut skip = position.row;
        let mut placed = false;
        // **Whether the position this walk settled on is the diff's *bottom*.**
        //
        // Set by the two places here that rest the diff's last row on the pane's
        // last row, and read by nothing but [`Self::wrap_rows`]. It is a units
        // question and it did not exist before
        // [#272](https://github.com/breferrari/vigia/issues/272): both compare a
        // **logical** span against `height`, a count of **display** rows, and
        // while the two were one number that was exact. With wrapping on the
        // clamp lands too far back, and dropping the excess off the bottom the
        // way every other frame does would put the end of the diff out of reach
        // of the gesture that exists to reach it.
        //
        // **Set rather than derived, and the derivation was tried first.**
        // `index >= stop` after the walk reads as *the walk ran out of files* and
        // is not: `take_file` breaks when the window fills and the walk increments
        // `index` regardless, so the test is true whenever the walk merely
        // **reached** the last file. A page step into the middle of a long file
        // then trimmed as though it were the end, and the pane opened nine rows
        // past where the position said. The three sites outside this function
        // that clamp the same way are fixed where they are, by measuring a
        // screenful in the rows a screen actually draws: see `App::screenful`.
        let mut at_bottom = false;
        // Whether the last file the walk touched was drawn to the end of its
        // block. See the assignment for what it is for.
        let mut consumed = false;
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

            while index < stop && view.rows.len() < height {
                view.read += 1;
                let (change, diff) = frame.diff(index)?;
                // Both halves of the tuple are immutable borrows of the same
                // frame, so the kind needs no clone to be read alongside the
                // diff.
                let span = block_of(&change.kind, diff, index, stop);

                // **Here, and not in [`crate::App::follow`], because this is
                // where a fresh diff exists.** The file follow named is the one
                // the walk starts on, so the `frame.diff` above has already
                // fetched it and the landing costs the arithmetic and nothing
                // else. Asking for it a second call earlier would be a second
                // whole-file *read*: `vigia_core::Frame::diff` re-reads a file
                // written in the last two seconds by design, and the file being
                // followed is always inside that margin.
                //
                // Once, and on the file the request was made against without
                // having to test for it. Two things make that hold and the
                // weaker one is the one a future edit would break: a landing is
                // only ever asked for with **row zero**, because
                // [`crate::App::jump_to_newest`] goes through `App::jump_to`,
                // and `block_of` is never less than one, so the walk cannot skip
                // a file whole before it reaches here. The stronger one is that
                // [`landing_of`] returns a row strictly inside the block it was
                // handed, so the resolution can never make the walk overshoot
                // whatever it is handed. `landed` is what stops the restart
                // below resolving it a second time, and what tells the caller it
                // may forget the request.
                if landing && !view.landed {
                    skip = landing_of(&change.kind, diff, height, wrap);
                    view.landed = true;
                }

                if !placed {
                    if skip >= span {
                        if index + 1 < stop {
                            // Wholly above the window. Carrying the remainder
                            // into the next file rather than clamping is what
                            // makes scrolling off the end of a short file
                            // continue into the one below instead of stopping
                            // there.
                            skip -= span;
                            index += 1;
                            continue;
                        }
                        // **Past the end of the last file the walk can reach,
                        // which lands the reader on the last screenful and not
                        // on the last row.** Those are not the same place, and
                        // taking the second for the first is what
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
                        //
                        // **Under a pin the last file the walk can reach is the
                        // pinned one**, so this is also the whole of what
                        // *scrolling clamps at the file's lower bound* means:
                        // the rule the end of the diff already had, applied to a
                        // smaller subject, which is B16's own argument for the
                        // gesture.
                        if span >= height {
                            skip = span - height;
                            at_bottom = true;
                        } else {
                            // That file cannot fill the screen by itself, so the
                            // top is in a file further back and this walk has no
                            // way to reach it. Resolved after the borrow ends,
                            // and under a pin there is no file further back to
                            // resolve to: [`Self::last_screenful`] is handed the
                            // pinned file at both ends and answers row zero,
                            // which is the honest top of a file shorter than the
                            // pane.
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

                // **Whether this file's block was drawn to its end**, which is
                // the half of *at the bottom* the walk's own index cannot say:
                // `index` is incremented whether `take_file` ran out of block or
                // ran out of window, so `index >= stop` means the walk **reached**
                // the last file and not that it consumed it. Recorded as a
                // subtraction over the rows this call pushed, which is the only
                // thing that tells the two apart.
                let before = view.rows.len();
                let asked = skip.min(span);
                view.take_file(
                    Changed {
                        kind: &change.kind,
                        origin: change.origin,
                        diff,
                        index,
                        closes: gap_rows(index, stop) > 0,
                        listed: list_rows > 0,
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
                consumed = view.rows.len() - before == span - asked;
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
            // **Only when the reader scrolled here, or when follow landed
            // inside a file.** A position placed by a jump is a claim about what
            // should be at the *top*: `G` **unpinned** puts the last file there
            // and a digit puts the file it names there. (Pinned, `G` is a claim
            // about the *bottom* and anchors deliberately, which is B16 and is
            // `App::apply`'s `Action::Bottom` arm; naming it here as a jump that
            // must not anchor is what a later session would read and revert.) If the tail is shorter than the pane,
            // backing up to fill it would move that file off the top row and make
            // the reader hunt for what the jump was for. Blank rows under a
            // deliberately placed file are the honest answer; blank rows under a
            // file the reader scrolled to are not.
            //
            // The two are indistinguishable from a `Position` alone, which is why
            // this is a parameter rather than something inferable here.
            //
            // **A landing is the third case and it goes with the scroll**
            // ([#257](https://github.com/breferrari/vigia/issues/257)). Its claim
            // is weaker than a jump's: it is that the change is *visible*, not
            // that a particular row is at the top, and it is satisfied from any
            // row the change is drawn from. So the pane is filled and the change
            // moves down it, where honouring the row exactly would draw a handful
            // of rows over blanks. That is reachable wherever the rows after the
            // landing run out, which is the last file and every file followed
            // only by hunkless ones: a rename, a mode change, a note. A clamp on
            // the last file alone was the first fix here and covered one of
            // those.
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
            // **A landing that resolved to row zero is not the third case**, it
            // is the ordinary jump: the file follow named belongs on the top row
            // and backing up would take it off. Caught by
            // `a_position_survives_the_file_it_points_at_being_committed`, which
            // follows the last file of a forty-file fixture whose blocks are five
            // rows: every one of those frames is short, and dropping this term
            // backed every one of them up off the heading it had just placed.
            // **And the floor is the walk's, not the diff's**, which is the
            // one line of this guard a pin moves. `Position::default()` is
            // *file zero, row zero*: the top of the first file the walk can
            // reach, which under a pin is the pinned file rather than the first
            // changed one. Read literally it would compare against a position
            // the pinned walk can never resolve to, so a pinned file shorter
            // than the pane would be `short` on every frame, restart on every
            // frame, and pay the treadmill this guard's own paragraph above
            // exists to describe: three walks and six `Frame::diff` calls a
            // frame against two, on the one file an agent is writing to, which
            // `Frame::diff` re-reads inside the settle margin by design.
            //
            // Reached by pinning any file but the first and scrolling, which is
            // the ordinary use of the gesture.
            // `tests/single.rs::a_pinned_file_shorter_than_the_pane_walks_once`
            // is the gate, and it counts walks rather than asserting rows,
            // because every row of the screen is identical either way.
            //
            // `first` rather than a second `if single`, for the reason the
            // binding above gives: this is the same range's other end.
            // **`single` is the pin's own licence, and it is a term here rather
            // than a flag the pin writes.** The back-up is what rests a short
            // screen's last row on the bottom, and a pinned position that
            // overruns its file wants exactly that; `anchored` alone gave it only
            // to a reader who had scrolled, so pressing `s` after a drag on the
            // diff's bar left a short screen with blanks under it.
            //
            // Writing `anchored` from the pin was the other way to fix it and it
            // leaked: the flag outlives the pin, so a jump onto a short tail
            // followed by `s` and `s` left an **unpinned** frame anchored and
            // backed the reader up out of the file the jump was for. A term costs
            // nothing and cannot outlive anything.
            //
            // **It is excluded on a pinned jump for free.** Every pin-side jump
            // resolves to row zero of the pinned file, which is exactly the floor
            // below, so `top != floor` is false and the guard stays quiet: the
            // file `n`, `p`, a digit or a click asked for keeps the top row.
            let landed_inside = view.landed && view.top.row > 0;
            // **A screen that is display-full is not short, however few of the
            // diff's own rows it holds** ([#272](https://github.com/breferrari/vigia/issues/272)).
            // This compared the rows collected, which are the diff's, against
            // `height`, which counts the terminal's, and the two are one number
            // only while nothing wraps. With `w` on a full pane read as short, so
            // the walk backed up to rest a last screenful that was already
            // resting, and the change follow had just landed on fell off the
            // bottom: I5's promise broken on the one frame it exists for.
            //
            // Counted rather than estimated, over rows the walk already holds and
            // with no file read, which is [`Self::wrap_rows`]' own arithmetic
            // asked one question earlier.
            let filled = view.display_rows(width, wrap);
            let short = (anchored || landed_inside || single)
                && filled < height
                && view.top
                    != Position {
                        file: first,
                        row: 0,
                    };
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

            // **Both ends, because a pin narrows the range this may resolve
            // into.** Unpinned it is the whole changed set; pinned it is the one
            // file, so the walk back stops where the walk forward started and
            // cannot hand back a position in a file the frame is not showing.
            // The same `(first, stop)` the walk above used, passed whole.
            view.top = Self::last_screenful(frame, first, stop, height, &mut view.read)?;
            index = view.top.file;
            skip = view.top.row;
            placed = true;
            // **Except where the restart landed on the walk's own floor**, which
            // is `last_screenful`'s answer for a diff shorter than the pane. A
            // trim there drops rows with nothing above them to scroll back to,
            // and one `j` on a diff that fits in the diff's own rows and not in
            // the terminal's put the reader there permanently: the heading, the
            // path and the counts were unreachable, with no bar drawn to say
            // anything was hidden. The derived term below carries the same guard
            // and for the same reason.
            at_bottom = view.top
                != Position {
                    file: first,
                    row: 0,
                };
        }

        // **And the clamp has to be re-derived, not remembered**
        // ([#272](https://github.com/breferrari/vigia/issues/272), found by the
        // second audit round against the first round's own fix). The two
        // assignments above fire on the frame a reader reaches the end and on no
        // frame after it: `last_screenful` stores a position from which the next
        // walk collects exactly `height` logical rows, so nothing overshoots,
        // nothing is short, and the frame after the gesture cut the tail off the
        // bottom again. A tick, an ageing wake or a press of `?` was enough to
        // scroll the reader silently back off the end they had just reached.
        //
        // The walk knows on every frame, and three terms are needed. It reached
        // the last file it could, **and** it drew that file's block to the end
        // rather than filling the window part-way through it, **and** the reader
        // is somewhere they scrolled or pinned rather than somewhere a jump put
        // them. The third is [`Self::short`]'s own guard a hundred lines up, in
        // its own words: a jump to a file is a claim about what belongs on the
        // **top** row, and a file whose wrapped content overruns the pane still
        // has rows below the fold for the reader to scroll to. Without it, `1`
        // through `6` on the last file drew its ending and hid its heading, and
        // the heading could not be scrolled back to because the position was
        // already row zero.
        //
        // **And never at the walk's own floor**, which is the fourth term and the
        // one the third does not imply. `single` alone satisfies *the reader is
        // where they scrolled or pinned* at row zero, so pressing `s` on a file
        // whose rows fit the pane and whose **display** rows do not trimmed its
        // heading away on the frame the pin landed, and `k` at row zero moves
        // nothing, so there was no gesture that could bring it back. A trim is
        // only ever honest where there is somewhere above to scroll to. It is
        // `short`'s floor guard, which this expression borrows the rest of its
        // reasoning from and had left behind.
        //
        // **And a landing follow has just served is not a scroll.** I5 puts the
        // viewport on what changed, and where the change sits exactly one
        // screenful from the end the trim would drop the landing row itself, so
        // the one frame that exists to show a reader what an agent just did would
        // not show it.
        //
        // **That term is unproven, and saying so is the point.** The audit that
        // raised it could not say whether any pane reaches the equality, and two
        // fixtures built for it could not either: `a_landing_follow_served_is_not_trimmed_off_its_own_row`
        // sweeps thirty-eight heights, serves landings at many of them, and the
        // trim never fires there with the term removed. So it is a guard whose
        // violation nobody has produced, which this repo treats as
        // indistinguishable from a guard that cannot fire. It is kept because it
        // is conservative in the one direction that matters, it costs a
        // comparison, and the alternative is dropping a term nobody has shown to
        // be safe to drop. A reader who finds the case should expect this comment
        // to be the only thing that told them it was open.
        let floor = Position {
            file: first,
            row: 0,
        };
        let at_bottom = at_bottom
            || (index >= stop
                && consumed
                && (anchored || single || (view.top.row > 0 && !view.landed))
                && view.top != floor);

        // **Here, and after the walk, because this is the first point at which
        // the rows exist and nothing more will be read.**
        // ([#272](https://github.com/breferrari/vigia/issues/272).) It reads no
        // file and asks the frame for nothing: wrapping only ever *grows* the
        // row count, so the `height` logical rows the walk already collected are
        // always enough to fill `height` display rows, and I4 never sees this.
        //
        // Before [`Self::take_list`] rather than after it, because the list is
        // planned against the rows this leaves and the gutter it records. **It is
        // no longer because the trim moves [`Self::top`]**, which is what this
        // said until the second audit round read it against the code: the trim
        // stopped moving anything stored when it became a display offset, and a
        // sentence describing the mechanism that shipped the trap is the sentence
        // a later session would restore it from.
        view.wrap_rows(width, wrap, height, at_bottom);

        // **After the walk, because only the walk knows where the diff landed.**
        // The position handed in may overshoot its file, point past a list the
        // agent in the other pane has shortened, or have been backed up to rest
        // the last row on the bottom. Marking the caret from the *request* would
        // put it on a file the diff is not in on exactly the frames that moved,
        // which is every frame a monitor exists to show.
        view.take_list(frame, history, list_rows, list_follows, &drawn)?;
        view.measure(frame, measured, single)?;

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
    ///
    /// **A pinned frame walks nothing at all**, which is `SPEC.md` §11.2 B16 and
    /// the one place the gesture makes the frame path *cheaper*. What the bar
    /// measures is what the reader can reach, and under a pin that is the pinned
    /// file, whose height the walk above has already recorded as
    /// [`Self::current_span`]. So the only opt-in cost in this function, and the
    /// only thing in the frame path not bounded by the window, is skipped
    /// outright rather than made smaller.
    ///
    /// **The drag on that bar reads the same quantity**, through
    /// [`span_in`], because a readout and the gesture performed on it are one
    /// contract: a bar drawn from one total and dragged against another agrees
    /// only at the two ends, and the middle is where they come apart.
    /// `crates/vigia/tests/single.rs::the_total_is_this_file_and_a_drag_lands_where_the_thumb_says`
    /// is that as a gate.
    fn measure(&mut self, frame: &mut Frame, wanted: bool, single: bool) -> Result<()> {
        if !wanted || self.files == 0 {
            return Ok(());
        }
        if single {
            self.total_rows = self.current_span;
            self.rows_above = self.top.row.min(self.current_span);
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
        // **The furthest start that still reaches the last file, which is not
        // `files - rows` once a window can hold rows that are not files**
        // ([#313](https://github.com/breferrari/vigia/issues/313)). See
        // [`last_top`].
        let ceiling = last_top(frame.files(), rows);
        // A screenful in files, taken from the ceiling so the bar's travel is the
        // drag's travel. See [`View::list_span`].
        // `last_top` never exceeds `files - 1`, so the subtraction is at least one
        // and needs no floor; `take_list` is not reached with an empty changed set
        // because `collect` returns before it.
        self.list_span = self.files - ceiling;
        let mut top = self.list_top.min(ceiling);
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
            // **Resolved through the plan rather than by arithmetic on `rows`**,
            // which is [`following_top`]'s own docblock: `current + 1 - rows`
            // subtracts drawn rows from a file index and lands short the moment a
            // window can hold a separator, leaving the caret's own file off the map
            // it just computed.
            top = following_top(frame.files(), top, self.top.file, rows).min(ceiling);
        }
        self.list_top = top;

        for slot in list_plan(frame.files(), top, rows) {
            let index = match slot {
                Slot::Group { origin, count } => {
                    self.list.push(ListRow::Group { origin, count });
                    continue;
                }
                Slot::File(index) => index,
            };
            self.read += 1;
            // **Searched from the back.** The walk's restart keeps `drawn` (see
            // there for why that is right), and the second walk starts earlier,
            // so an index both walks drew has two entries. `Frame::diff` re-reads
            // a file written in the last two seconds, so the two are only
            // certainly equal outside the settle margin — which is the one state
            // the current file is never in. Taking the newest is what keeps the
            // two regions from disagreeing about one file for one frame.
            match drawn.iter().rev().find(|(at, _)| *at == index) {
                Some((_, entry)) => self.list.push(ListRow::from(entry.clone())),
                None => {
                    let (change, diff) = frame.diff(index)?;
                    let entry = entry_of(&change.kind, change.origin, diff, history);
                    self.list.push(ListRow::from(entry));
                }
            }
        }
        Ok(())
    }

    /// Where the viewport starts so the diff's **last row rests at the bottom**.
    ///
    /// Walks back from the file before `stop` until it has `height` rows behind
    /// it, stopping at `first`. That reads only the files the screen is about to draw, so I4 is
    /// untouched: it is bounded by the window exactly like everything else here,
    /// and the `frame.diff` calls it makes are the same ones the walk above is
    /// about to make, which under I2a are cache hits rather than reads.
    ///
    /// **The range is the caller's rather than the frame's, and that is
    /// `SPEC.md` §11.2 B16.** Unpinned it is `0..files`, which is what this
    /// walked before the pin existed. Pinned it is the one file, so *the last
    /// row* means the pinned file's and the top it falls back to is that file's
    /// heading rather than the diff's first. A bound taken from
    /// `frame.files().len()` here would have been a second answer to *what is
    /// this frame showing*, disagreeing with the walk above on exactly the
    /// frames that ran short.
    ///
    /// **Half open, matching the walk's own bound**, so the caller passes the
    /// pair it already holds. An inclusive `last` made the caller write
    /// `stop - 1` and this body write `last + 1` back, which is a round trip
    /// through an off-by-one at the one boundary in this file where an
    /// off-by-one draws a plausible screen.
    ///
    /// **Not what `Action::Bottom` does**, and the difference is the whole
    /// reason this is affordable. `G` goes to the last *file* from its top,
    /// because finding the diff's last row from the *start* would mean adding up
    /// every file's height, which is the read I4 forbids. Backing off from an
    /// end already in hand costs a screenful.
    ///
    /// A diff shorter than the screen resolves to the top, which is the honest
    /// answer: the blank rows under it are the ones the diff does not have. So
    /// does a pinned file shorter than the screen, for the same reason and with
    /// `first` deciding which top that is.
    ///
    /// **It counts its own reads, so an overshoot frame reports roughly twice
    /// the files it draws**, and that is accurate rather than sloppy:
    /// [`View::read`] is "files this viewport asked the frame for", and this asks
    /// for the same files the walk above is about to ask for again. Under I2a
    /// the second ask is a cache hit that reads no bytes, so it is a count that
    /// doubles and not work that does. It lasts one frame either way, because
    /// the caller stores the resolved position back and the next frame starts on
    /// the file it draws.
    /// How many rows of the terminal the rows this walk has collected would take.
    ///
    /// **The same arithmetic [`Self::wrap_rows`] does, asked before the walk
    /// decides whether it came up short** ([#272](https://github.com/breferrari/vigia/issues/272)).
    /// It reads no file and asks the frame for nothing: the rows are in hand and
    /// the width is the caller's, so this is bounded by the window exactly as the
    /// expansion is.
    ///
    /// `self.rows.len()` when nothing wraps, which is every pane with `w` off and
    /// every caller that named no width, so the guard that reads this is the
    /// guard it has always been there.
    fn display_rows(&self, width: usize, wrap: bool) -> usize {
        if !wrap || width == 0 {
            return self.rows.len();
        }
        let gutter = crate::render::gutter_width(&self.rows, width);
        let content = crate::render::content_width(gutter, width);
        if content == 0 {
            return self.rows.len();
        }
        self.rows
            .iter()
            .map(|row| match row {
                Row::Line { text, .. } => {
                    1 + usize::from(crate::render::split_at(text, content).is_some())
                }
                _ => 1,
            })
            .sum()
    }

    /// Turn logical rows into display rows, and record the gutter they were
    /// measured against.
    ///
    /// **`SPEC.md` §11.2 B19**, from `w`
    /// ([#272](https://github.com/breferrari/vigia/issues/272)).
    ///
    /// **It reads nothing and asks the frame for nothing**, which is what keeps
    /// I4 exactly where it was. Wrapping only ever *grows* the row count, so the
    /// `height` logical rows [`View::collect`]'s walk already collected are
    /// always enough to fill `height` display rows, and this never needs a file
    /// the walk did not reach.
    ///
    /// **The gutter is taken here rather than by the painter**, and over the rows
    /// the walk reached rather than over the ones that survive. The circularity
    /// is why: the width left for text decides which lines wrap, wrapping decides
    /// which rows survive, and [`crate::render::gutter_width`] sizes itself from
    /// the largest line number among the rows. Taking it once, before the
    /// expansion, breaks the loop at the only point where the answer is a
    /// property of the walk rather than of the wrap. With wrapping off it is
    /// byte-identical to what the painter used to compute for itself.
    ///
    /// **A line whose tail will not fit is drawn whole instead**, clipped and
    /// marked `›` exactly as it is today. That is the honest answer at the last
    /// row of the pane: a pre-sliced head with its continuation off screen would
    /// read as a complete line, which is the one thing §11.1's marking rule
    /// exists to prevent.
    fn wrap_rows(&mut self, width: usize, wrap: bool, height: usize, at_bottom: bool) {
        // **Only where a width was passed**, so a caller that named none leaves
        // the decision where it has always been. See [`View::gutter`].
        self.gutter = (width > 0).then(|| crate::render::gutter_width(&self.rows, width));
        if !wrap || width == 0 || height == 0 || self.rows.is_empty() {
            return;
        }
        let content = crate::render::content_width(self.gutter.unwrap_or(0), width);
        if content == 0 {
            return;
        }

        // Where each logical row breaks, if it does. Taken once: the trim below
        // reads it backwards and the expansion reads it forwards, and a second
        // walk is a second chance to disagree.
        let splits: Vec<Option<usize>> = self
            .rows
            .iter()
            .map(|row| match row {
                Row::Line { text, .. } => crate::render::split_at(text, content),
                _ => None,
            })
            .collect();
        let cost = |at: usize| 1 + usize::from(splits[at].is_some());
        let total: usize = (0..splits.len()).map(cost).sum();

        // **Nothing on this screen wraps, so nothing below it has anything to
        // do.** The ordinary screenful of an ordinary diff, and it is worth an
        // early return rather than falling through: the walk below rebuilds the
        // row vector unconditionally, so without this a reader with `w` on paid
        // one `Vec<Row>` allocation and one deallocation on every frame that
        // draws no long line. The `!wrap` and `content == 0` arms above are the
        // same shape and this is the third of them.
        if total == splits.len() {
            return;
        }

        // **The bottom clamp, in the units it now has to be in.**
        //
        // [`Self::last_screenful`] and `collect`'s overshoot branch both rest the
        // diff's last **logical** row on the pane's last **display** row, by
        // comparing a logical span against `height`. That was exact while the two
        // were one number. With wrapping on it places the top too far back, and
        // dropping the excess off the bottom the way every other frame does would
        // put the end of the diff out of reach of the gesture that exists to
        // reach it.
        //
        // So this one case trims from the **front**: the smallest number of
        // leading logical rows whose removal lets the rest fit. Removing one costs
        // one row or two, so the last removal can overshoot by exactly one and
        // leave a blank at the bottom, which §11.1 rules the diff does not end
        // with. `unwrapped` is that case and it is the same rule as the last row
        // of an ordinary screen: keep the row, draw it whole, let it clip and say
        // so.
        let mut from = 0usize;
        let mut unwrapped = false;
        if at_bottom && total > height {
            let mut tail = 0usize;
            let mut at = splits.len();
            while at > 0 && tail + cost(at - 1) <= height {
                at -= 1;
                tail += cost(at);
            }
            from = at;
            if tail < height && from > 0 {
                from -= 1;
                unwrapped = true;
            }
        }

        // **[`Self::top`] is not moved, and that is what makes the end of the
        // diff a place a reader can leave.**
        //
        // The first draft advanced it by the rows the trim dropped, which reads
        // as the tidy thing and is a **one-way trap**. `top` is stored back as the
        // scroll position, and from the advanced row fewer than `height` logical
        // rows remain, so the next frame is `short`, restarts through
        // [`Self::last_screenful`], lands on the same row and trims to the same
        // place: every `k` and every wheel-up after that resolves to the position
        // it started from. Two audit remits found it independently, one as a cost
        // (two walks and a `last_screenful`'s reads per frame) and one as the
        // defect it is.
        //
        // So the trim is a **display** offset and nothing else. The stored
        // position stays the row the walk placed, which is a row the walk can
        // fill again, and `at_bottom` is re-derived from that same position on
        // every frame, so the screen is stable without the position moving.
        //
        // **What it costs is one narrow thing, stated rather than hidden**: at a
        // wrapped bottom whose last screenful spans two files, the first row
        // drawn can belong to the file after the one `top` names, so the caret in
        // the list marks the reader's position rather than the topmost drawn row.
        // The bar is unaffected, because `rows_above` is `top`'s and `top` means
        // *the last screenful* there exactly as it always did.
        let mut out: Vec<Row> = Vec::with_capacity(height);
        for (at, row) in self.rows.drain(..).enumerate() {
            if at < from {
                continue;
            }
            if out.len() >= height {
                break;
            }
            let Row::Line {
                kind,
                number,
                text,
                spans,
                emph,
            } = row
            else {
                out.push(row);
                continue;
            };
            // Two rows or none. `out.len() + 2 <= height` is the last row of the
            // pane, and `at == from && unwrapped` is the first, which the trim
            // above chose deliberately.
            let split = splits[at]
                .filter(|_| !(at == from && unwrapped))
                .filter(|_| out.len() + 2 <= height);
            let Some(cut) = split else {
                out.push(Row::Line {
                    kind,
                    number,
                    text,
                    spans,
                    emph,
                });
                continue;
            };
            let indent = crate::render::indent_of(&text, content);
            let (head_spans, tail_spans) = split_spans(&spans, cut);
            let (head_emph, tail_emph) = split_emph(&emph, cut);
            let tail = text[cut..].to_owned();
            let mut head = text;
            head.truncate(cut);
            out.push(Row::Line {
                kind,
                number,
                text: head,
                spans: head_spans,
                emph: head_emph,
            });
            out.push(Row::Wrap {
                kind,
                text: tail,
                spans: tail_spans,
                emph: tail_emph,
                indent,
            });
        }
        self.rows = out;
    }

    fn last_screenful(
        frame: &mut Frame,
        first: usize,
        stop: usize,
        height: usize,
        read: &mut usize,
    ) -> Result<Position> {
        let mut index = stop - 1;
        let mut have = 0usize;
        loop {
            *read += 1;
            let (change, diff) = frame.diff(index)?;
            // `stop` is the walk's own exclusive end, so the blank closing the
            // final file is not counted here any more than it is drawn there.
            // Unpinned that is `files` and this is unchanged; pinned it is the
            // one file, and counting a gap the pin does not draw would rest the
            // last row one line above the bottom.
            have += block_of(&change.kind, diff, index, stop);
            if have >= height {
                return Ok(Position {
                    file: index,
                    row: have - height,
                });
            }
            if index == first {
                return Ok(Position {
                    file: first,
                    row: 0,
                });
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
            origin,
            diff,
            index,
            closes,
            listed,
        } = file;
        let mut n = 0usize;

        // **Built for the row when the heading fits, and recorded when it does
        // not and a list exists to read the record.**
        // The record is what lets [`View::take_list`] draw the same file without
        // asking the frame for it a second time, and a `Frame::diff` for a file
        // written in the last two seconds is a second whole-file *read* rather
        // than a cache hit. That is precisely the file this branch misses: a
        // viewport sitting inside a file has that file's heading above the
        // window, and since
        // [#257](https://github.com/breferrari/vigia/issues/257) follow puts it
        // there deliberately, on the one file the settle margin is certain to
        // cover. `SPEC.md` §11.1's *"the two regions hand entries to each other
        // rather than asking twice"* is the rule, and it was true only while the
        // heading happened to be drawn.
        //
        // **What it costs, stated at its real size.** [`entry_of`] is two
        // history probes and a clone *plus* [`heat_of`], which walks every line
        // of every hunk in the file, so this is an O(diff) walk rather than a
        // hash lookup. It is still the right trade and the two halves are not
        // the same size: what it replaces is a `Frame::diff`, which inside the
        // settle margin re-reads the file from disk and allocates a `String` per
        // line, and outside it is a `stat` and a cache hit. So the margin, which
        // is where the followed file always is, saves a read; a settled tree
        // pays a walk over lines already in memory instead of a syscall.
        //
        // **Exactly one extra per walk**, because `skip` is zeroed after the
        // first file this places: every earlier file was skipped whole without
        // reaching here, and every later one draws its heading.
        if n >= skip {
            let entry = entry_of(kind, origin, diff, history);
            drawn.push((index, entry.clone()));
            self.rows.push(Row::file(entry));
        } else if listed {
            self.recorded += 1;
            // Moved rather than cloned, because there is no row to draw it in.
            drawn.push((index, entry_of(kind, origin, diff, history)));
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
                let span = hunk_span(hunk);
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
                            emph: line.emph.clone(),
                            // `None` is the plain first frame, and empty spans
                            // are already a legal, drawn state: it is what a
                            // file type with no grammar produces, so the
                            // renderer needs no new case for this. See
                            // `Viewport::highlight`.
                            spans: match highlighter.as_deref_mut() {
                                Some(pass) => pass
                                    .spans(
                                        &diff.path,
                                        ordinal,
                                        hunk,
                                        within,
                                        diff.first_line.as_deref(),
                                    )
                                    .to_vec(),
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
    //! The heat projection and the follow landing, tested as the arithmetic
    //! they are.
    //!
    //! Every case here is a boundary: the first line of a file, the last, a
    //! removal past the end, a file shorter than the bucket count. Reaching any
    //! of them through a repository fixture would mean building a file of an
    //! exact length for each, and `vigia_core::FileChange` cannot be constructed
    //! outside its crate anyway, so a `FileDiff` built by hand is the only
    //! version of one these can reach. `SPEC.md` §7 names this shape.

    use vigia_core::Line;

    use super::*;

    fn line(kind: LineKind) -> Line {
        Line {
            kind,
            text: String::new(),
            emph: Vec::new(),
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

    /// Line kinds for a hunk of `context` unchanged lines around `changed`
    /// changed ones, which is the shape every hunk a diff produces has.
    fn kinds(context: usize, changed: usize) -> Vec<LineKind> {
        let mut lines = vec![LineKind::Context; context];
        lines.extend(std::iter::repeat_n(LineKind::Removed, changed));
        lines
    }

    /// A file of three hunks whose middle one is by far the busiest.
    ///
    /// Rows, because every expectation below is one of them: the heading is 0,
    /// the first header is 1 over eight lines, so the second header is **10**
    /// over sixteen, and the third is **27**.
    fn three_hunks() -> FileDiff {
        diff(
            400,
            vec![
                hunk(10, &kinds(6, 2)),
                hunk(100, &kinds(6, 10)),
                hunk(300, &kinds(6, 2)),
            ],
        )
    }

    #[test]
    fn the_busiest_hunk_is_where_a_tall_file_lands() {
        assert_eq!(
            landing_of(&ChangeKind::Modified, &three_hunks(), 8, false),
            10,
            "the landing is not the second hunk's header row"
        );
    }

    #[test]
    fn a_hunk_is_measured_by_what_changed_rather_than_by_how_tall_it_is() {
        // A hunk is mostly context, so a rule that counted rows would land on
        // whichever hunk was longest and call a wall of unchanged lines the
        // busiest thing in the file. Here the first hunk is the tallest by far
        // and the second is the only one with more than a line of change in it.
        let tall_and_quiet = diff(400, vec![hunk(10, &kinds(40, 1)), hunk(200, &kinds(6, 9))]);

        assert_eq!(
            landing_of(&ChangeKind::Modified, &tall_and_quiet, 8, false),
            43,
            "the landing followed the tallest hunk rather than the busiest"
        );
    }

    #[test]
    fn a_tie_lands_on_the_earlier_hunk() {
        // A reader scrolls forward more readily than back, and an arbitrary
        // winner would move the pane between two frames of one unchanged file.
        //
        // The tied pair is the second and third hunk rather than the first and
        // second, so the answer is a row the heading branch cannot also produce:
        // the first header is row 1 and any pane at all draws it.
        let even = diff(
            400,
            vec![
                hunk(10, &kinds(6, 1)),
                hunk(100, &kinds(6, 4)),
                hunk(300, &kinds(6, 4)),
            ],
        );

        assert_eq!(landing_of(&ChangeKind::Modified, &even, 8, false), 9);
    }

    #[test]
    fn a_busiest_hunk_already_on_screen_keeps_the_heading() {
        // Both sides of the edge, because "already drawn" is what decides
        // whether the heading is worth spending and an off-by-one here is a
        // heading lost for nothing.
        //
        // **The edge is the hunk's first changed line, not its header.** In
        // `three_hunks` the busiest header is row 10 over six context lines, so
        // its first removal is row 17: an eighteen-row region draws it and a
        // seventeen-row one does not.
        let file = three_hunks();

        assert_eq!(landing_of(&ChangeKind::Modified, &file, 18, false), 0);
        assert_eq!(landing_of(&ChangeKind::Modified, &file, 17, false), 10);
    }

    #[test]
    fn a_hunk_header_with_no_content_under_it_is_not_a_change_on_screen() {
        // The edge one row at a time, because the version that tested the
        // *header* passed this whole battery while drawing the reader a bare
        // `@@` line with nothing under it, which is the symptom
        // [#257](https://github.com/breferrari/vigia/issues/257) was reported
        // for. Row 10 is the header and 11 through 16 are its six context lines,
        // so row 17 is the first removal and a seventeen-row region stops one
        // short of it.
        let file = three_hunks();

        for height in 11..=17 {
            assert_eq!(
                landing_of(&ChangeKind::Modified, &file, height, false),
                10,
                "a {height}-row region draws the busiest hunk's header and none \
                 of what it changed, and the heading was kept anyway"
            );
        }
    }

    #[test]
    fn a_pane_too_short_to_draw_the_change_keeps_the_heading() {
        // **The second half of the rule**, and the half the first draft of it
        // did not have: a landing is worth the heading only when the change is
        // drawn *from the landing*. `three_hunks`' busiest hunk opens with six
        // context lines, so a pane of seven rows or fewer draws the `@@` and
        // none of what is under it, and one bare hunk header is strictly less
        // than the heading it replaced.
        //
        // `Body::split`'s floor is the case that matters: one row for the whole
        // diff region. It was landing on the hunk header there, under a test
        // whose name said the opposite of what it asserted.
        let file = three_hunks();

        for height in 1..=7 {
            assert_eq!(
                landing_of(&ChangeKind::Modified, &file, height, false),
                0,
                "a {height}-row region cannot draw the change from the landing, \
                 so the landing costs the heading and buys nothing"
            );
        }
        // And one row further up it is worth it again: the header and its six
        // context lines fit in seven, so the eighth row is the first removal.
        assert_eq!(landing_of(&ChangeKind::Modified, &file, 8, false), 10);
    }

    #[test]
    fn an_addition_counts_the_same_as_a_removal_when_the_busiest_is_picked() {
        // Every other case here is decided by removals, so `!= Context` and
        // `== Removed` are the same rule over this battery and the second one
        // survives. What a reader watches an agent do is mostly *writing*.
        let mut added = vec![LineKind::Context; 3];
        added.extend(std::iter::repeat_n(LineKind::Added, 9));
        let file = diff(400, vec![hunk(10, &kinds(6, 2)), hunk(200, &added)]);

        // **At the heights that tell the busiest hunk from the heading.** The
        // busiest hunk's header is row 10 over three context lines, so its first
        // addition is row 14: a fourteen-row region stops one short of it and a
        // fifteen-row one draws it. Two heights rather than one because the pair
        // pins the edge itself rather than a point to one side of it, which is
        // what every other case in this battery does.
        assert_eq!(
            landing_of(&ChangeKind::Modified, &file, 14, false),
            10,
            "the busiest hunk is nine additions and the landing went elsewhere"
        );
        assert_eq!(
            landing_of(&ChangeKind::Modified, &file, 15, false),
            0,
            "the ninth addition is drawn from the heading and the heading was \
             spent anyway"
        );
    }

    #[test]
    fn a_note_block_has_no_hunk_to_land_on() {
        // A conflict, a type change and a binary file draw a heading and one
        // line saying why. There is nowhere to land and `span_of` gives them two
        // rows, so a landing computed from hunks would point past the block.
        //
        // **At a height where the same diff would land**, which height one is
        // not: there the rule's second clause returns zero for every file, so
        // the early return was doing nothing the test could see and deleting it
        // left the suite green. `three_hunks` lands on row 10 from an eight-row
        // region.
        assert_eq!(
            landing_of(&ChangeKind::Modified, &three_hunks(), 8, false),
            10
        );

        assert_eq!(
            landing_of(&ChangeKind::Conflict, &three_hunks(), 8, false),
            0
        );
        // A real binary diff carries no hunks either, so it reaches the same
        // answer by the ordinary route. Pinned so that stays true.
        let mut binary = three_hunks();
        binary.binary = true;
        binary.hunks.clear();
        assert_eq!(landing_of(&ChangeKind::Modified, &binary, 8, false), 0);
    }

    #[test]
    fn a_file_with_no_hunks_has_nowhere_to_land() {
        assert_eq!(
            landing_of(&ChangeKind::Modified, &diff(400, Vec::new()), 1, false),
            0
        );
    }
}
