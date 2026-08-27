//! One screenful, and nothing more than one screenful.

use vigia_core::{
    ChangeKind, FileDiff, Frame, HISTORY_BUCKETS, Highlighter, History, Hunk, LineKind, Origin,
    Pass, Recency, Result, SPARK_GROUPS, Span,
};

/// One changed file, as everything a row about it needs to be drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    /// Repository-relative path.
    pub path: String,
    /// Which run this row belongs to.
    pub origin: Origin,
    /// Where the content came from, for a rename or a copy.
    pub from: Option<String>,
    /// One letter naming what happened.
    pub kind: char,
    /// Lines added and removed, or `None` when there is no line-level diff.
    pub churn: Option<(u32, u32)>,
    /// This file's churn over the glance window, oldest bucket first.
    pub spark: [u32; HISTORY_BUCKETS],
    /// How recently this file changed, which is what dims a settled row.
    pub recency: Recency,
    /// Whether the newest burst named this file, which is what carries the `●`.
    pub newest: bool,
    /// Where in this file the change is, as counts per slice of its length.
    pub heat: [HeatBucket; HEAT_BUCKETS],
}

/// One row of the pinned list's window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListRow {
    /// A run's separator: `──  staged  2 ─────`.
    Group {
        /// Which run begins here.
        origin: Origin,
        /// How many files that run holds in total, not how many are visible.
        count: usize,
    },
    /// A changed file.
    File(Box<FileEntry>),
}

impl ListRow {
    /// The file this row draws, or `None` for a run separator.
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
pub fn list_plan(files: &[vigia_core::FileChange], top: usize, rows: usize) -> Vec<Slot> {
    plan_with(files, Runs::of(files), top, rows)
}

/// Rows the pinned list wants, which is its files plus its separators.
pub fn list_rows_wanted(files: &[vigia_core::FileChange]) -> usize {
    let runs = Runs::of(files);
    files.len() + runs.separators()
}

/// How many files each run holds, counted once.
#[derive(Debug, Clone, Copy)]
struct Runs {
    unstaged: usize,
    staged: usize,
}

impl Runs {
    /// Counted by scanning, because a slice does not carry the boundary.
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
    fn grouped(self) -> bool {
        self.staged > 0
    }

    /// How many separators a grouped list draws: one per run that has files.
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
        // A run's label is drawn before any of its files, without exception.
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

/// The smallest top a window of `rows` drawn rows can start at and still draw
/// `file`.
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
/// last file, which is the tightest such top rather than the largest.
pub fn last_top(files: &[vigia_core::FileChange], rows: usize) -> usize {
    if files.is_empty() {
        return 0;
    }
    top_showing(files, Runs::of(files), files.len() - 1, rows)
}

/// The window a list following the diff should show, given where the diff is.
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
        spans: Vec<Span>,
        /// Byte ranges of `text` that changed within the line, from
        /// [`vigia_core::Line::emph`]: the pair-aligned word-level diff the renderer
        /// draws as the hotter wash.
        emph: Vec<std::ops::Range<u32>>,
    },
    /// The tail of a [`Row::Line`] that did not fit, on the row below it.
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
        /// width. `render::indent_of` is the rule.
        indent: usize,
    },
    /// Why a file has no lines under it.
    Note(&'static str),
    /// The blank row that closes a file's block.
    Gap,
}

impl Row {
    /// A file heading row.
    pub fn file(entry: FileEntry) -> Self {
        Self::File(Box::new(entry))
    }
}

/// Slices a file's length is divided into for the heat strip.
pub const HEAT_BUCKETS: usize = 24;

/// What a drawn sparkline bucket's height is divided by, one figure per rung.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Scale(pub [u32; SPARK_GROUPS.len()]);

impl Scale {
    /// One figure at every grouping.
    pub const fn flat(figure: u32) -> Self {
        Self([figure; SPARK_GROUPS.len()])
    }

    /// `figure` scaled by each grouping, saturating.
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
    pub fn at(self, group: usize) -> u32 {
        SPARK_GROUPS
            .iter()
            .position(|named| *named == group)
            .map_or(self.0[0], |at| self.0[at])
    }
}

/// Changed lines falling in one slice of a file's length.
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
fn bucket_of(line: u32, lines: u32) -> Option<usize> {
    if lines == 0 {
        return None;
    }
    let zero_based = u64::from(line.saturating_sub(1));
    let index = (zero_based * HEAT_BUCKETS as u64) / u64::from(lines);
    Some((index as usize).min(HEAT_BUCKETS - 1))
}

/// Project a file's changed lines onto [`HEAT_BUCKETS`] slices of its length.
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
                // Deliberately does not advance `new`: a removed line
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
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Position {
    /// Index into [`vigia_core::Frame::files`].
    pub file: usize,
    /// Rows of that file already scrolled past.
    pub row: usize,
}

/// Everything [`View::collect`] needs to know about where the screen is looking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Viewport {
    /// Where the diff's top row is, before resolving.
    pub position: Position,
    /// Whether [`Self::position`] was reached by scrolling rather than a jump.
    pub anchored: bool,
    /// Rows the diff region has, from [`crate::render::Body::diff`].
    pub diff_rows: usize,
    /// Columns the diff region's glyphs have, from
    /// [`crate::render::Body::diff`] less the inset and any scrollbar.
    pub width: usize,
    /// Whether a content line too wide for the pane continues on the row below.
    pub wrap: bool,
    /// First file the pinned list shows, before resolving.
    pub list_top: usize,
    /// Rows the pinned list has, from [`crate::render::Body::list`]. Zero on a
    /// pane too short for a region, which draws no list at all.
    pub list_rows: usize,
    /// Whether the list's window should follow the diff, or stay where a reader
    /// put it.
    pub list_follows: bool,
    /// Whether this frame needs the diff's total height.
    pub measured: bool,
    /// Whether [`Self::position`] was placed by follow and still wants its row.
    pub landing: bool,
    /// Whether this frame may parse for colour.
    pub highlight: bool,
    /// Whether the diff is pinned to the one file [`Self::position`] is inside.
    pub single: bool,
}

impl Default for Viewport {
    /// Hand written for one field, and only that field.
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
    pub rows: Vec<Row>,
    /// Digits reserved for line numbers on every content row, or zero for none.
    pub gutter: Option<usize>,
    /// The pinned file list, top to bottom, at most `Viewport::list_rows` long.
    pub list: Vec<ListRow>,
    /// How many files the pinned list's scrollbar treats as one screenful.
    pub list_span: usize,
    /// Whether this frame shows both runs, and therefore draws the run separators.
    pub grouped: bool,
    /// Which file the pinned list starts at, once the request was resolved
    /// against the files that exist and against where the diff is.
    pub list_top: usize,
    /// Rows the block the diff is inside contributes: heading, content, and the
    /// blank that closes it where one does.
    pub current_span: usize,
    /// Rows the whole diff is, every changed file counted.
    pub total_rows: usize,
    /// Rows of the whole diff above this screen's top row.
    pub rows_above: usize,
    /// Changed files in the whole worktree, not just the visible ones.
    pub files: usize,
    /// Where the top row actually came from, once the request was resolved
    /// against the files that exist and how tall they are.
    pub top: Position,
    /// Whether this frame resolved the landing [`Viewport::landing`] asked for.
    pub landed: bool,
    /// Files this viewport asked the frame for, drawn or merely crossed.
    pub read: usize,
    /// [`FileEntry`] values built for the record rather than for a row.
    pub recorded: usize,
    /// The busiest bucket any tracked file holds, which every sparkline on this
    /// screen is drawn against.
    pub scale: Scale,
    /// The whole worktree's churn over the window, oldest sample first.
    pub worktree_churn: vigia_core::Churn,
}

/// The letter shown for a kind of change.
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
fn span_of(kind: &ChangeKind, diff: &FileDiff) -> usize {
    if note_for(kind, diff).is_some() {
        return 2;
    }
    1 + diff.hunks.iter().map(hunk_span).sum::<usize>()
}

/// The syntax runs covering one byte range of a line, re-based onto it.
fn spans_in(spans: &[Span], from: usize, to: usize) -> Vec<Span> {
    let mut kept = Vec::with_capacity(spans.len());
    let mut pos = 0usize;
    for span in spans {
        let end = pos + span.len;
        let start = pos.max(from);
        let stop = end.min(to);
        if stop > start {
            kept.push(Span {
                len: stop - start,
                class: span.class,
            });
        }
        pos = end;
        if pos >= to {
            break;
        }
    }
    kept
}

/// The word-emphasis ranges covering one byte range of a line, re-based onto it.
fn emph_in(emph: &[std::ops::Range<u32>], from: usize, to: usize) -> Vec<std::ops::Range<u32>> {
    let (from, to) = (from as u32, to as u32);
    emph.iter()
        .filter_map(|range| {
            let start = range.start.max(from);
            let end = range.end.min(to);
            (end > start).then(|| start - from..end - from)
        })
        .collect()
}

/// Rows one hunk occupies: its `@@` header and then its lines.
fn hunk_span(hunk: &Hunk) -> usize {
    1 + hunk.lines.len()
}

/// The blank row closing the block of the file at `index`, as a count.
fn gap_rows(index: usize, files: usize) -> usize {
    usize::from(index + 1 < files)
}

/// Rows the block of the file at `index` occupies: the file's own rows and the
/// blank that closes it.
fn block_of(kind: &ChangeKind, diff: &FileDiff, index: usize, files: usize) -> usize {
    span_of(kind, diff) + gap_rows(index, files)
}

/// Rows into a file's block where follow should put the top of the viewport.
fn landing_of(kind: &ChangeKind, diff: &FileDiff, height: usize, content: Option<usize>) -> usize {
    if note_for(kind, diff).is_some() {
        return 0;
    }

    // Row zero is the heading, so the first header sits at one. Walked rather than
    // indexed because a hunk's height is its own line count, which is exactly the sum
    // `span_of` takes.
    let rows_of_line = |text: &str| match content {
        Some(content) if content > 0 => 1 + crate::render::breaks_of(text, content, height).len(),
        _ => 1,
    };

    let mut row = 1;
    let mut seen = 1;
    let mut busiest = 0;
    let mut landing = 0;
    let mut landing_seen = 0;
    let mut change_seen = 0;
    for hunk in &diff.hunks {
        let mut changed = 0;
        let mut lead = None;
        for (at, line) in hunk.lines.iter().enumerate() {
            if line.kind != LineKind::Context {
                changed += 1;
                lead.get_or_insert(at);
            }
        }
        if let Some(lead) = lead
            && changed > busiest
        {
            busiest = changed;
            landing = row;
            landing_seen = seen;
            change_seen = seen
                + 1
                + hunk.lines[..lead]
                    .iter()
                    .map(|line| rows_of_line(&line.text))
                    .sum::<usize>();
        }
        row += hunk_span(hunk);
        // The exact count stops once it has passed the pane, which bounds the text this
        // walks to roughly one screenful rather than to the file.
        seen += if seen < height {
            1 + hunk
                .lines
                .iter()
                .map(|line| rows_of_line(&line.text))
                .sum::<usize>()
        } else {
            1 + hunk.lines.len()
        };
    }

    // Two questions, and a landing has to answer both. `height` is the diff
    // region's, so this is the one place the rule depends on the pane, and it is
    // why a reader who makes the pane taller stops being moved off the heading.

    // Already drawn from the heading, so the jump would cost the heading and buy
    // nothing.
    if change_seen < height {
        return 0;
    }
    // And still not drawn from the landing, which is `Body::split`'s floor: a one-row
    // region draws the `@@` and nothing under it, and one bare hunk header is strictly
    // less than the heading it replaced, which carries the path, the counts, the sigil
    // and the strip.
    if change_seen - landing_seen >= height {
        return 0;
    }
    landing
}

/// The same block, counted from the span cache rather than from a diff.
pub fn block_rows(frame: &mut Frame, index: usize) -> Result<usize> {
    let files = frame.files().len();
    Ok(frame.rows_of(index, rows_of)? + gap_rows(index, files))
}

/// Rows the whole diff occupies, the blanks between files included.
pub fn diff_rows(frame: &mut Frame) -> Result<usize> {
    let files = frame.files().len();
    Ok(frame.height(rows_of)? + files.saturating_sub(1))
}

/// One changed file as the walk has it: what happened, what it diffs to, and
/// where it sits in the frame's list.
struct Changed<'f> {
    kind: &'f ChangeKind,
    /// Which run this file is in, for the ink on the row's kind letter.
    origin: Origin,
    diff: &'f FileDiff,
    index: usize,
    /// Whether a blank closes this file's block, which is every file but the
    /// last ([`gap_rows`]).
    closes: bool,
    /// Whether the pane has a pinned list at all.
    listed: bool,
}

/// Everything a row about this file needs, for either region.
fn entry_of(kind: &ChangeKind, origin: Origin, diff: &FileDiff, history: &History) -> FileEntry {
    FileEntry {
        path: diff.path.clone(),
        origin,
        from: source_of(kind).map(str::to_owned),
        kind: letter(kind),
        churn: (note_for(kind, diff).is_none()).then_some((diff.added, diff.removed)),
        spark: history.level(&diff.path).unwrap_or([0; HISTORY_BUCKETS]),
        recency: history.recency(&diff.path),
        newest: history.newest(&diff.path),
        heat: heat_of(diff),
    }
}

/// How many rows a file occupies, from its span rather than from its diff.
pub fn rows_of(change: &vigia_core::FileChange, span: &vigia_core::FileSpan) -> usize {
    // A note is a heading and one line saying why, which is exactly what
    // `note_for` produces for the same three cases.
    if matches!(change.kind, ChangeKind::Conflict | ChangeKind::TypeChange) || span.binary {
        return 2;
    }
    1 + span.hunks as usize + span.lines as usize
}

/// Rows the file at `index` draws, without the blank that would close its block.
/// # Panics
///
/// If `index` is out of range, the same way [`vigia_core::Frame::rows_of`] does.
/// `App::pinned_file` keeps the pinned callers off that index.
pub fn span_in(frame: &mut Frame, index: usize) -> Result<usize> {
    frame.rows_of(index, rows_of)
}

/// How many rows the block of the file at `index` would occupy.
pub fn rows_in(frame: &mut Frame, index: usize) -> Result<usize> {
    let files = frame.files().len();
    let (change, diff) = frame.diff(index)?;
    Ok(block_of(&change.kind, diff, index, files))
}

impl View {
    /// How many distinct files this screen's diff region draws.
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

    /// How many files the pinned list is showing, which is not how many rows
    /// it drew.
    pub fn listed_files(&self) -> usize {
        self.list.iter().filter_map(ListRow::entry).count()
    }

    /// Collect the rows visible from `position`, and no others.
    ///
    /// # Errors
    ///
    /// A file the window reaches cannot be read or measured.
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
        // One pass, dropped at every exit including the `?`s below, which is what keeps
        // the highlight cache bounded by the viewport. The guard rather than a pair of
        // calls is `vigia_core::Highlighter::pass`'s business and its doc says why.
        let original = highlighter;
        let mut highlighter = original.pass();
        let files = frame.files().len();
        // Resolved from the changed set rather than from the toggle, so a reader who
        // asks for the staged run and has nothing staged gets the pane they already had
        // rather than a column and a label saying nothing.
        let grouped = Runs::at(frame.files(), frame.staged_at()).grouped();
        let mut view = Self {
            grouped,
            // Initialised to "nothing to scroll" rather than to zero, so every path out
            // of this function leaves a span a scrollbar can be asked about.
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
            // Until the walk below runs, the request is passed through with only its
            // file clamped.
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
            // The list still resolves.
            view.take_list(frame, history, list_rows, list_follows, &[])?;
            return Ok(view);
        }

        // Entries the body built, so the list can reuse rather than re-diff.
        // Bounded by the viewport: one per file the walk reaches, which is one
        // per heading that fits plus the file the viewport is sitting inside.
        let mut drawn: Vec<(usize, FileEntry)> = Vec::new();

        // The one bound the pin costs, and every use of it below reads this rather than
        // `files`.
        let (first, stop) = if single {
            (view.top.file, view.top.file + 1)
        } else {
            (0, files)
        };

        let mut index = view.top.file;
        let mut skip = position.row;
        let mut placed = false;
        // Whether the position this walk settled on is the diff's *bottom*.
        let landing_content = (wrap && width > 0).then(|| crate::render::content_width(10, width));
        let mut at_bottom = false;
        // Whether the last file the walk touched was drawn to the end of its
        // block. See the assignment for what it is for.
        let mut consumed = false;
        // At most one restart, whichever of the two reasons below triggered it.
        let mut restarted = false;

        // Restarted at most once, and only from [`Self::last_screenful`] below.
        loop {
            let mut overshot = false;

            while index < stop && view.rows.len() < height {
                view.read += 1;
                let (change, diff) = frame.diff(index)?;
                // Both halves of the tuple are immutable borrows of the same
                // frame, so the kind needs no clone to be read alongside the
                // diff.
                let span = block_of(&change.kind, diff, index, stop);

                // Here, and not in [`crate::App::follow`], because this is where a
                // fresh diff exists.
                if landing && !view.landed {
                    skip = landing_of(&change.kind, diff, height, landing_content);
                    view.landed = true;
                }

                if !placed {
                    if skip >= span {
                        if index + 1 < stop {
                            // Wholly above the window.
                            skip -= span;
                            index += 1;
                            continue;
                        }
                        // Past the end of the last file the walk can reach, which lands
                        // the reader on the last screenful and not on the last row.
                        if span >= height {
                            skip = span - height;
                            at_bottom = true;
                        } else {
                            // That file cannot fill the screen by itself, so the top is
                            // in a file further back and this walk has no way to reach
                            // it.
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

                // The height of the file the viewport is inside, recorded where it is
                // already known.
                if index == view.top.file {
                    view.current_span = span;
                }

                // Whether this file's block was drawn to its end, which is the half of
                // *at the bottom* the walk's own index cannot say: `index` is
                // incremented whether `take_file` ran out of block or ran out of
                // window, so `index >= stop` means the walk reached the last file and
                // not that it consumed it.
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
                    // The pass is taken whatever this frame does with it, so the sweep
                    // in its `Drop` still runs and the cache stays bounded the way I3
                    // needs.
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

            // Two ways to finish with a body that is not full, and only one
            // of them is obvious.
            let landed_inside = view.landed && view.top.row > 0;
            // A screen that is display-full is not short, however few of the diff's own
            // rows it holds.
            let short = (anchored || landed_inside || single)
                // After the cheap terms, so an ordinary follow frame never pays for it:
                // this walks every collected row and the three conditions above are
                // field reads.
                && view.display_rows(width, wrap, height) < height
                && view.top
                    != Position {
                        file: first,
                        row: 0,
                    };
            if restarted || !(overshot || short) {
                break;
            }
            restarted = true;

            // Cleared, unlike the overshoot path, and this is the one line where the
            // two differ.
            view.rows.clear();
            // `drawn` is deliberately kept.

            // And the parses go with them. Clearing the rows discards what was drawn;
            // it does not discard what drawing *cost*, because a hunk's parse lives in
            // the pass rather than in the row.
            drop(highlighter);
            highlighter = original.pass();

            // Both ends, because a pin narrows the range this may resolve into.
            view.top = Self::last_screenful(frame, first, stop, height, &mut view.read)?;
            index = view.top.file;
            skip = view.top.row;
            placed = true;
            // Except where the restart landed on the walk's own floor, which is
            // `last_screenful`'s answer for a diff shorter than the pane.
            at_bottom = view.top
                != Position {
                    file: first,
                    row: 0,
                };
        }

        // And the clamp has to be re-derived, not remembered.
        let floor = Position {
            file: first,
            row: 0,
        };
        let at_bottom = at_bottom
            || (index >= stop
                && consumed
                && (anchored || single || (view.top.row > 0 && !view.landed))
                && view.top != floor);

        view.wrap_rows(width, wrap, height, at_bottom);

        // After the walk, because only the walk knows where the diff landed.
        view.take_list(frame, history, list_rows, list_follows, &drawn)?;
        view.measure(frame, measured, single)?;

        Ok(view)
    }

    /// Total the diff's rows, and how many of them are above this screen.
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
    fn take_list(
        &mut self,
        frame: &mut Frame,
        history: &History,
        rows: usize,
        follows: bool,
        drawn: &[(usize, FileEntry)],
    ) -> Result<()> {
        // A pane with no region resolved nothing, so it says nothing.
        if rows == 0 {
            return Ok(());
        }
        // No `files == 0` branch: `View::collect` returns before this on an empty
        // worktree, and a second guard here was unreachable.

        // Always pulled back so the last file can rest on the bottom row rather than
        // leaving blanks a reader would read as "no more files". That is validity, and
        // holds however the window got there.
        let ceiling = last_top(frame.files(), rows);
        // A screenful in files, taken from the ceiling so the bar's travel is the
        // drag's travel. See [`View::list_span`].
        self.list_span = self.files - ceiling;
        let mut top = self.list_top.min(ceiling);
        if follows {
            // And snapped onto the current file, but only when the window is the diff's
            // to move.
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
            // Searched from the back.
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

    /// How many rows of the terminal the rows this walk has collected would take.
    fn display_rows(&self, width: usize, wrap: bool, height: usize) -> usize {
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
                // `breaks_of`, not `split_at`, and it was the second for one commit.
                Row::Line { text, .. } => 1 + crate::render::breaks_of(text, content, height).len(),
                _ => 1,
            })
            .sum()
    }

    /// Turn logical rows into display rows, and record the gutter they were
    /// measured against.
    fn wrap_rows(&mut self, width: usize, wrap: bool, height: usize, at_bottom: bool) {
        // Only where a width was passed, so a caller that named none leaves
        // the decision where it has always been. See [`View::gutter`].
        self.gutter = (width > 0).then(|| crate::render::gutter_width(&self.rows, width));
        if !wrap || width == 0 || height == 0 || self.rows.is_empty() {
            return;
        }
        let content = crate::render::content_width(self.gutter.unwrap_or(0), width);
        if content == 0 {
            return;
        }

        // Where each collected row breaks, and how many rows of terminal it therefore
        // takes.
        let breaks: Vec<Vec<usize>> = self
            .rows
            .iter()
            .map(|row| match row {
                Row::Line { text, .. } => {
                    crate::render::breaks_of(text, content, height.saturating_add(1))
                }
                _ => Vec::new(),
            })
            .collect();
        let cost = |at: usize| breaks[at].len() + 1;
        let total: usize = (0..breaks.len()).map(cost).sum();

        // Nothing on this screen wraps, so nothing below it has anything to do.
        if total == breaks.len() {
            return;
        }

        // The bottom clamp, in the units it now has to be in.
        let mut from = 0usize;
        let mut above = 0usize;
        if at_bottom && total > height {
            let mut tail = 0usize;
            let mut at = breaks.len();
            while at > 0 && tail < height {
                at -= 1;
                tail += cost(at);
            }
            from = at;
            above = tail.saturating_sub(height);
        }

        // [`Self::top`] is not moved, and that is what makes the end of the
        // diff a place a reader can leave.
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
            if breaks[at].is_empty() {
                out.push(Row::Line {
                    kind,
                    number,
                    text,
                    spans,
                    emph,
                });
                continue;
            }

            let indent = crate::render::indent_of(&text, content);
            // Rows of this line to pass over, which is the display offset above.
            let skip = if at == from { above } else { 0 };
            // A line taller than the pane is the one case a mark is still honest.
            let taller_than_pane = breaks[at].len() + 1 > height;
            let mut start = 0usize;
            let cuts: Vec<usize> = breaks[at]
                .iter()
                .copied()
                .chain(std::iter::once(text.len()))
                .collect();
            for (piece, cut) in cuts.iter().copied().enumerate() {
                if out.len() >= height {
                    break;
                }
                if piece < skip {
                    start = cut;
                    continue;
                }
                let last = out.len() + 1 == height && piece + 1 < cuts.len();
                let cut = if last && taller_than_pane {
                    text.len()
                } else {
                    cut
                };
                let kept = spans_in(&spans, start, cut);
                let kept_emph = emph_in(&emph, start, cut);
                let slice = text[start..cut].to_owned();
                if piece == 0 {
                    out.push(Row::Line {
                        kind,
                        number,
                        text: slice,
                        spans: kept,
                        emph: kept_emph,
                    });
                } else {
                    out.push(Row::Wrap {
                        kind,
                        text: slice,
                        spans: kept,
                        emph: kept_emph,
                        indent,
                    });
                }
                start = cut;
            }
        }
        self.rows = out;
    }

    /// Where the viewport starts so the diff's last row rests at the bottom.
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
            // `stop` is the walk's own exclusive end, so the blank closing the final
            // file is not counted here any more than it is drawn there.
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

        // Built for the row when the heading fits, and recorded when it does not and a
        // list exists to read the record.
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

        // A labelled block so the block's closing gap has one push site.
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
                            // `None` is the plain first frame, and empty spans are
                            // already a legal, drawn state: it is what a file type with
                            // no grammar produces, so the renderer needs no new case
                            // for this.
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

        // The blank that closes the block, on the same terms as every row above it.
        if closes && n >= skip && self.rows.len() < height {
            self.rows.push(Row::Gap);
        }
    }
}

#[cfg(test)]
mod tests {
    //! The heat projection and the follow landing, tested as the arithmetic
    //! they are.

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

    /// A hundred and twenty lines over [`HEAT_BUCKETS`] slices puts line 1 in the first
    /// and line 61 exactly halfway, whatever the source resolution is.
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
            landing_of(&ChangeKind::Modified, &three_hunks(), 8, None),
            10,
            "the landing is not the second hunk's header row"
        );
    }

    /// [`three_hunks`] with every line long enough to wrap at a narrow content.
    fn three_hunks_wide() -> FileDiff {
        let mut diff = three_hunks();
        for hunk in &mut diff.hunks {
            for line in &mut hunk.lines {
                line.text = "x".repeat(60);
            }
        }
        diff
    }

    #[test]
    fn a_content_of_nothing_is_not_a_content_of_one() {
        // The floor `View::collect` hands this can saturate to zero, because it is the
        // pane's width less the widest gutter a `u32` line number can need, which is
        // thirteen columns.
        let none = landing_of(&ChangeKind::Modified, &three_hunks_wide(), 8, None);
        assert_eq!(
            landing_of(&ChangeKind::Modified, &three_hunks_wide(), 8, Some(0)),
            none,
            "a pane with no room for text was read as a pane one column wide"
        );
        assert_ne!(
            landing_of(&ChangeKind::Modified, &three_hunks_wide(), 8, Some(1)),
            none,
            "a content of one column gave the same answer as no wrapping at all, \
             so this comparison cannot tell the two apart"
        );
    }

    #[test]
    fn a_wrapped_pane_follows_only_a_change_it_can_show() {
        // The budget is measured, not halved, which is what removing the wrap cap
        // forced: with a cap of two rows a change at logical offset `d` sat at display
        // row at most `2d`, and halving `height` was an exact guarantee for nothing.

        // A file whose lines do not wrap is followed exactly as it is unwrapped.
        // The halving failed this, and it is the common case: `w` is global, so a
        // reader with it on is in this state for every file that fits.
        let plain = landing_of(&ChangeKind::Modified, &three_hunks(), 8, None);
        assert!(
            plain > 0,
            "the fixture declines even unwrapped, so this compares two refusals"
        );
        assert_eq!(
            landing_of(&ChangeKind::Modified, &three_hunks(), 8, Some(200)),
            plain,
            "a file whose lines all fit was followed differently with wrapping on"
        );

        // And a file whose lines do wrap pushes the change further down the pane,
        // so a landing the same height honoured is withdrawn once it cannot be
        // guaranteed drawn.
        assert_eq!(
            landing_of(&ChangeKind::Modified, &three_hunks_wide(), 8, Some(20)),
            0,
            "a pane that cannot guarantee the change is drawn from the landing \
             moved the reader off the heading anyway"
        );

        // The other end of the same rule: a change guaranteed drawn from the heading
        // unwrapped can be below the fold once its lines take three rows each, and then
        // the pane does move to it.
        let tall = 24;
        assert_eq!(
            landing_of(&ChangeKind::Modified, &three_hunks(), tall, None),
            0,
            "the fixture draws its change from the heading at {tall} rows, so the \
             wrapped answer below is not a comparison"
        );
        assert!(
            landing_of(&ChangeKind::Modified, &three_hunks_wide(), tall, Some(20)) > 0,
            "a change drawn from the heading unwrapped is below the fold once \
             every line takes three rows, and the pane did not move to it"
        );
    }

    #[test]
    fn a_hunk_is_measured_by_what_changed_rather_than_by_how_tall_it_is() {
        // A hunk is mostly context, so a rule that counted rows would land on whichever
        // hunk was longest and call a wall of unchanged lines the busiest thing in the
        // file.
        let tall_and_quiet = diff(400, vec![hunk(10, &kinds(40, 1)), hunk(200, &kinds(6, 9))]);

        assert_eq!(
            landing_of(&ChangeKind::Modified, &tall_and_quiet, 8, None),
            43,
            "the landing followed the tallest hunk rather than the busiest"
        );
    }

    #[test]
    fn a_tie_lands_on_the_earlier_hunk() {
        // A reader scrolls forward more readily than back, and an arbitrary
        // winner would move the pane between two frames of one unchanged file.
        let even = diff(
            400,
            vec![
                hunk(10, &kinds(6, 1)),
                hunk(100, &kinds(6, 4)),
                hunk(300, &kinds(6, 4)),
            ],
        );

        assert_eq!(landing_of(&ChangeKind::Modified, &even, 8, None), 9);
    }

    #[test]
    fn a_busiest_hunk_already_on_screen_keeps_the_heading() {
        // Both sides of the edge, because "already drawn" is what decides
        // whether the heading is worth spending and an off-by-one here is a
        // heading lost for nothing.
        let file = three_hunks();

        assert_eq!(landing_of(&ChangeKind::Modified, &file, 18, None), 0);
        assert_eq!(landing_of(&ChangeKind::Modified, &file, 17, None), 10);
    }

    #[test]
    fn a_hunk_header_with_no_content_under_it_is_not_a_change_on_screen() {
        let file = three_hunks();

        for height in 11..=17 {
            assert_eq!(
                landing_of(&ChangeKind::Modified, &file, height, None),
                10,
                "a {height}-row region draws the busiest hunk's header and none \
                 of what it changed, and the heading was kept anyway"
            );
        }
    }

    #[test]
    fn a_pane_too_short_to_draw_the_change_keeps_the_heading() {
        // The second half of the rule: a landing is worth the heading only when the
        // change is drawn *from the landing*.
        let file = three_hunks();

        for height in 1..=7 {
            assert_eq!(
                landing_of(&ChangeKind::Modified, &file, height, None),
                0,
                "a {height}-row region cannot draw the change from the landing, \
                 so the landing costs the heading and buys nothing"
            );
        }
        // And one row further up it is worth it again: the header and its six
        // context lines fit in seven, so the eighth row is the first removal.
        assert_eq!(landing_of(&ChangeKind::Modified, &file, 8, None), 10);
    }

    #[test]
    fn an_addition_counts_the_same_as_a_removal_when_the_busiest_is_picked() {
        // Every other case here is decided by removals, so `!= Context` and
        // `== Removed` are the same rule over this battery and the second one
        // survives. What a reader watches an agent do is mostly *writing*.
        let mut added = vec![LineKind::Context; 3];
        added.extend(std::iter::repeat_n(LineKind::Added, 9));
        let file = diff(400, vec![hunk(10, &kinds(6, 2)), hunk(200, &added)]);

        // At the heights that tell the busiest hunk from the heading.
        assert_eq!(
            landing_of(&ChangeKind::Modified, &file, 14, None),
            10,
            "the busiest hunk is nine additions and the landing went elsewhere"
        );
        assert_eq!(
            landing_of(&ChangeKind::Modified, &file, 15, None),
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
        assert_eq!(
            landing_of(&ChangeKind::Modified, &three_hunks(), 8, None),
            10
        );

        assert_eq!(
            landing_of(&ChangeKind::Conflict, &three_hunks(), 8, None),
            0
        );
        // A real binary diff carries no hunks either, so it reaches the same
        // answer by the ordinary route. Pinned so that stays true.
        let mut binary = three_hunks();
        binary.binary = true;
        binary.hunks.clear();
        assert_eq!(landing_of(&ChangeKind::Modified, &binary, 8, None), 0);
    }

    #[test]
    fn a_file_with_no_hunks_has_nowhere_to_land() {
        assert_eq!(
            landing_of(&ChangeKind::Modified, &diff(400, Vec::new()), 1, None),
            0
        );
    }
}
