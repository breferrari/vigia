//! One screenful, and nothing more than one screenful.

use std::collections::HashMap;

use vigia_core::{
    ChangeKind, FileDiff, Frame, HISTORY_BUCKETS, Highlighter, History, Hunk, LineKind, Note,
    Origin, Pass, Placement, Recency, Result, SPARK_GROUPS, Side, Span, Status, resolve, run_of,
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
    /// What the reader's notes on this file put on its row and its heat strip.
    pub notes: FileNotes,
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
    Reason(String),
    /// One display row of a reader's note, under the line it is pinned to or
    /// under the file's heading once that line is gone (`SPEC.md` §11.2 B21).
    /// A display row the bar does not count, exactly as [`Row::Wrap`] is.
    Note {
        /// The note this row belongs to, by id.
        id: String,
        /// What stands at the content origin.
        lead: NoteLead,
        /// This row's piece of the body or of the reply, already broken at the
        /// content width less the lead.
        text: String,
        /// The note's state: every row's `▎` takes its ink, the last draws the word.
        state: &'static str,
        /// Whether this is that last row.
        last: bool,
        /// Whether the whole row takes the dim weight, which is a note whose
        /// line was edited under it.
        faded: bool,
    },
    /// One display row of the note box, under the line it is being written for.
    Box {
        /// Which row of the box this is.
        part: BoxPart,
    },
    /// The blank row that closes a file's block.
    Gap,
}

/// Which of a note's two texts a row is part of, which is where one line out
/// ends and the next begins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NoteVoice {
    Reader,
    Agent,
}

/// What a note row draws at the content origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteLead {
    /// The enclosure's top edge.
    Top,
    /// The reader's words, between its two sides.
    Body,
    /// The bottom edge, carrying the word.
    Bottom,
    /// The rung below the enclosure, on a pane too narrow to hold one.
    Bar,
    /// The arrow on the first row of the agent's line.
    Reply,
    /// Nothing, under a continued arrow.
    Blank,
}

impl NoteLead {
    /// Which text this row is part of; `None` on the rows drawing the frame.
    fn voice(self) -> Option<NoteVoice> {
        match self {
            // The enclosure's body and the narrow rung's are one text at two widths.
            Self::Body | Self::Bar => Some(NoteVoice::Reader),
            Self::Reply | Self::Blank => Some(NoteVoice::Agent),
            Self::Top | Self::Bottom => None,
        }
    }
}

/// One row of the note box.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoxPart {
    /// The top edge, carrying the anchor.
    Top {
        /// `path:line`, whole; the painter elides it to the edge's room.
        label: String,
    },
    /// One row of the reader's text, between the box's sides.
    Body {
        /// This row's piece, already broken at the box's inner width.
        text: String,
        /// The column the caret stands in, on the one row that holds it.
        caret: Option<usize>,
    },
    /// The bottom edge, carrying the two keys.
    Bottom,
}

impl Row {
    /// Whether this row continues the line above rather than starting one.
    pub fn is_wrap(&self) -> bool {
        matches!(self, Self::Wrap { .. })
    }

    /// Whether this is a display row the bar does not count: a continuation, a
    /// note row or a row of the box. The bar's question and not the copy's: a
    /// `Wrap` is the line above it cut, and a `Note` is not that line at all.
    pub fn is_display(&self) -> bool {
        matches!(
            self,
            Self::Wrap { .. } | Self::Note { .. } | Self::Box { .. }
        )
    }

    /// Whether this row carries text of its own: a note's edges draw its frame
    /// and its status word, and a box is a draft that is not a line yet.
    pub fn owns_text(&self) -> bool {
        !matches!(
            self,
            Self::Note {
                lead: NoteLead::Top | NoteLead::Bottom,
                ..
            } | Self::Box { .. }
        )
    }

    /// A file heading row.
    pub fn file(entry: FileEntry) -> Self {
        Self::File(Box::new(entry))
    }
}

/// Whether `row` is a later piece of the text `above` began. Apart from [`View`]
/// because the layout asks it while it is still building the rows.
fn continues(row: &Row, above: Option<&Row>) -> bool {
    if row.is_wrap() {
        return true;
    }
    // A note continues only itself: two notes can share a line, and the answer
    // sits under the words inside one note, so neither field alone cuts the run.
    let (
        Row::Note { id, lead, .. },
        Some(Row::Note {
            id: over,
            lead: before,
            ..
        }),
    ) = (row, above)
    else {
        return false;
    };
    id == over && lead.voice().is_some() && lead.voice() == before.voice()
}

/// Where a note is pinned: what a press read off the row it landed on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Anchor {
    /// Repository-relative path of the file the line is in.
    pub path: String,
    /// Which side the line is numbered on.
    pub side: Side,
    /// Its number on that side.
    pub line: u32,
    /// Its whole text, which is what finds it again after an edit above moves
    /// the number.
    pub text: String,
    /// The run the row was drawn in. A path in both runs draws the same line
    /// twice, so nothing else says which of them the reader pressed.
    pub origin: Origin,
}

/// A display row that carries a note's mark, and whose note it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marked {
    /// Index into [`View::rows`].
    pub row: usize,
    /// The note's id, which is what a click on the row withdraws.
    pub id: String,
    /// Whether the note has no body, so the row draws the icon rather than
    /// its number in the icon's ink: nothing else on screen says it is marked.
    pub bare: bool,
}

/// What this screen knows about the reader's notes beyond the rows themselves.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Noted {
    /// Every row carrying a mark, in row order.
    pub marked: Vec<Marked>,
    /// The first row of each file's rows on this screen, that file's path and
    /// the run it was drawn in, so a row can be traced to the entry it is in
    /// without a heading on screen.
    pub segments: Vec<(usize, String, Origin)>,
    /// Notes whose file is not in the diff, drawn nowhere and counted in the
    /// footer.
    pub adrift: usize,
    /// The row of the line the note box is open under, when that line is on
    /// this screen, so the number keeps the note's ink while the box is up.
    pub boxed: Option<usize>,
}

/// Columns a note row spends before its text: the lead and its gap.
const NOTE_LEAD: usize = 2;

/// Body rows the note box grows to before it scrolls inside itself.
pub const BOX_ROWS: usize = 4;

/// Columns the box's two sides and their gaps cost a body row.
pub const BOX_FRAME: usize = 4;

/// The note box as the walk places it, before the display pass makes its rows.
#[derive(Debug, Clone)]
struct BoxPin {
    /// Index into the logical rows.
    row: usize,
    /// Whether `row` is the anchored line rather than the file's heading.
    marks: bool,
    /// `path:line`, for the top edge.
    label: String,
    lines: Vec<String>,
    /// As a line and a character within it.
    cursor: (usize, usize),
}

/// A note the walk placed on a logical row, before the display pass draws it.
#[derive(Debug, Clone)]
struct Pin {
    /// Index into the logical rows.
    row: usize,
    id: String,
    body: String,
    reply: Option<String>,
    /// The word on the last body row.
    word: &'static str,
    /// Whether the note's line was edited under it, which dims its rows.
    faded: bool,
    /// Whether `row` is the note's own line, which carries the mark, rather than
    /// the file's heading.
    marks: bool,
    /// Whether the agent has resolved it, so its reply alone is drawn, or its
    /// body where the agent left no reply.
    resolved: bool,
}

/// `text` in rows of at most `room` columns, broken the way prose breaks: at
/// every newline, then at the last blank that fits, and inside a word only when
/// the word alone is wider than the row. The blank a row breaks on is drawn on
/// neither row. Empty text is one empty row, so a note with no body still has a
/// row for its word.
fn prose_rows(text: &str, room: usize) -> Vec<String> {
    text.split('\n')
        .flat_map(|paragraph| {
            let paragraph = crate::render::detabbed(paragraph);
            prose_pieces(&paragraph, room)
                .into_iter()
                .map(|piece| paragraph[piece].to_owned())
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Where one paragraph breaks into rows of at most `room` columns, as byte
/// ranges: the blank a break drops sits between two ranges and inside neither.
fn prose_pieces(text: &str, room: usize) -> Vec<std::ops::Range<usize>> {
    let mut pieces = Vec::new();
    let mut start = 0;
    while room > 0 && crate::render::width_of(&text[start..]) > room {
        let rest = &text[start..];
        let Some(cut) = crate::render::split_at(rest, room) else {
            break;
        };
        let at = if rest[cut..].starts_with(' ') {
            cut
        } else {
            match rest[..cut].rfind(' ') {
                Some(space) if space > 0 => space,
                _ => cut,
            }
        };
        pieces.push(start..start + at);
        let dropped = rest[at..].len() - rest[at..].trim_start_matches(' ').len();
        start += at + dropped;
    }
    pieces.push(start..text.len());
    pieces
}

/// The piece the caret stands on and its column there, for a cursor `col`
/// characters into `line`. A caret past a row's room takes the head of the next
/// piece, which the caller makes when none follows, and that covers the blanks
/// a break dropped: a break lands on one only past the room already.
fn caret_in(
    line: &str,
    pieces: &[std::ops::Range<usize>],
    col: usize,
    inner: usize,
) -> (usize, usize) {
    let at = line
        .char_indices()
        .nth(col)
        .map_or(line.len(), |(index, _)| index);
    let piece = pieces
        .iter()
        .rposition(|range| range.start <= at)
        .unwrap_or(0);
    let range = &pieces[piece];
    let column = crate::render::width_of(&line[range.start..at.max(range.start)]);
    if column >= inner {
        (piece + 1, 0)
    } else {
        (piece, column)
    }
}

/// The rows the box takes under a content width of `content`, and which of
/// them carries the caret, which the clamp keeps on screen. None at a width
/// that leaves the body no column between the box's two sides.
fn box_rows(pin: &BoxPin, content: usize) -> (Vec<Row>, usize) {
    if content <= BOX_FRAME {
        return (Vec::new(), 0);
    }
    let inner = content - BOX_FRAME;
    let (cursor_line, cursor_col) = pin.cursor;
    let mut body: Vec<(String, Option<usize>)> = Vec::new();
    let mut caret_row = 0;
    for (index, line) in pin.lines.iter().enumerate() {
        let pieces = prose_pieces(line, inner);
        let first = body.len();
        body.extend(
            pieces
                .iter()
                .map(|range| (line[range.clone()].to_owned(), None)),
        );
        if index == cursor_line {
            let (piece, column) = caret_in(line, &pieces, cursor_col, inner);
            if piece >= pieces.len() {
                body.push((String::new(), None));
            }
            caret_row = first + piece;
            body[caret_row].1 = Some(column);
        }
    }
    // The box holds `BOX_ROWS` rows and the caret's is always one of them: past
    // the cap the body scrolls so the caret rests on the last row drawn.
    let top = caret_row.saturating_sub(BOX_ROWS - 1);
    let mut rows = vec![Row::Box {
        part: BoxPart::Top {
            label: pin.label.clone(),
        },
    }];
    rows.extend(
        body.into_iter()
            .skip(top)
            .take(BOX_ROWS)
            .map(|(text, caret)| Row::Box {
                part: BoxPart::Body { text, caret },
            }),
    );
    rows.push(Row::Box {
        part: BoxPart::Bottom,
    });
    // The window above always holds the caret's row, since it begins at most
    // `BOX_ROWS` back from it, and the top edge stands before them all.
    (rows, 1 + caret_row - top)
}

/// One row of `pin`'s; `last` marks where its status word is drawn.
fn row(rows: &mut Vec<Row>, pin: &Pin, lead: NoteLead, text: String, last: bool) {
    rows.push(Row::Note {
        id: pin.id.clone(),
        lead,
        text,
        state: pin.word,
        last,
        faded: pin.faded,
    });
}

impl Pin {
    /// The display rows this note takes under a content width of `content`.
    fn rows(&self, content: usize) -> Vec<Row> {
        let room = content.saturating_sub(NOTE_LEAD);
        let boxed = content >= crate::render::edge_width(self.word);
        let inner = content.saturating_sub(BOX_FRAME);
        let pieces = |text: &str| prose_rows(text, room);
        let mut rows = Vec::new();
        if !self.resolved || self.reply.is_none() {
            if boxed {
                row(&mut rows, self, NoteLead::Top, String::new(), false);
                for text in prose_rows(&self.body, inner) {
                    row(&mut rows, self, NoteLead::Body, text, false);
                }
                row(
                    &mut rows,
                    self,
                    NoteLead::Bottom,
                    self.word.to_owned(),
                    true,
                );
            } else {
                let mut body = pieces(&self.body);
                // The word shares the last row, or takes one of its own where
                // that row has none: the reader's words are never cut to fit it.
                let last = body
                    .last()
                    .map_or(0, |piece| crate::render::width_of(piece));
                let gap = usize::from(last > 0);
                if self.word.len() <= room && last + gap + self.word.len() > room {
                    body.push(String::new());
                }
                let count = body.len();
                for (piece, text) in body.into_iter().enumerate() {
                    row(&mut rows, self, NoteLead::Bar, text, piece + 1 == count);
                }
            }
        }
        if let Some(reply) = &self.reply {
            for (piece, text) in prose_rows(reply, room).into_iter().enumerate() {
                let lead = if piece == 0 {
                    NoteLead::Reply
                } else {
                    NoteLead::Blank
                };
                row(&mut rows, self, lead, text, false);
            }
        }
        rows
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

/// The maps a file's mark is resolved through: every note by the path it was
/// written on, and the entry [`run_of`] gave each note whose path is in two runs.
struct Pinned<'n> {
    by_path: &'n HashMap<&'n str, Vec<&'n Note>>,
    chosen: &'n HashMap<&'n str, usize>,
}

/// Every note the entry at `index` holds, less the ones [`run_of`] gave to another
/// entry. Shared, because a rename answers to two paths and a run to two entries.
fn notes_at<'n>(
    change: &vigia_core::FileChange,
    index: usize,
    pinned: &Pinned<'n>,
) -> Vec<&'n Note> {
    if pinned.by_path.is_empty() {
        return Vec::new();
    }
    let mut held: Vec<&Note> = Vec::new();
    for path in change.paths() {
        if let Some(found) = pinned.by_path.get(path) {
            held.extend(found.iter().copied());
        }
    }
    if !pinned.chosen.is_empty() {
        held.retain(|note| {
            pinned
                .chosen
                .get(note.id.as_str())
                .is_none_or(|&at| at == index)
        });
    }
    held
}

/// What one file's notes say about it. Ordered weakest to strongest, so
/// [`Self::worse`] is a `max` and the ordering itself is the precedence rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum NoteMark {
    /// Drawn only for as long as the note's departure is.
    Resolved,
    /// Written, and the agent has not answered it.
    Waiting,
    /// The one state that asks the reader for something rather than the agent.
    Replied,
}

impl NoteMark {
    fn of(note: &Note) -> Self {
        match (note.status, note.reply.is_some()) {
            (Status::Resolved, _) => Self::Resolved,
            (_, true) => Self::Replied,
            (_, false) => Self::Waiting,
        }
    }

    /// What a file holding two of them draws.
    #[must_use]
    pub fn worse(self, other: Self) -> Self {
        self.max(other)
    }
}

/// What the reader's notes on one file put on its row and on its heat strip.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FileNotes {
    /// `None` where the file holds no note at all.
    pub mark: Option<NoteMark>,
    /// Which slices hold an unresolved one; presence and not state, per §11.1.
    pub at: [bool; HEAT_BUCKETS],
}

/// What `notes` put on `diff`'s file. The **stored** line is used and not the pane's
/// placement: a line that moved moved by at most `NEAR` rows, one slice at any rung.
fn notes_of(diff: &FileDiff, notes: &[&Note]) -> FileNotes {
    let mut held = FileNotes::default();
    for note in notes {
        let mark = NoteMark::of(note);
        held.mark = Some(held.mark.map_or(mark, |worst| worst.worse(mark)));
        // The strip answers where the outstanding conversation is, and this is not.
        if mark != NoteMark::Resolved
            && let Some(at) = slice_of(diff, note)
        {
            held.at[at] = true;
        }
    }
    held
}

/// The slice the line `note` is pinned to falls in. An old-side note is numbered by
/// the index and the strip is the working tree's, so it walks [`heat_of`]'s own.
/// The kind is part of the match: `positions` leaves `old` where it is on an
/// addition, so an addition carries the number of the index line it sits before.
fn slice_of(diff: &FileDiff, note: &Note) -> Option<usize> {
    let line = match note.side {
        Side::New => note.line,
        Side::Old => diff
            .hunks
            .iter()
            .flat_map(Hunk::positions)
            .find(|(old, _, line)| *old == note.line && line.kind == LineKind::Removed)
            .map(|(_, new, _)| new)?,
    };
    bucket_of(line, diff.lines)
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
        for (_, new, line) in hunk.positions() {
            let Some(at) = bucket_of(new, diff.lines) else {
                continue;
            };
            match line.kind {
                LineKind::Context => {}
                LineKind::Added => buckets[at].added = buckets[at].added.saturating_add(1),
                LineKind::Removed => {
                    buckets[at].removed = buckets[at].removed.saturating_add(1);
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
    /// A wrapped line's or a note's whole text, by the row its first piece sits on: the
    /// walk emits only the pieces that fit, so the rows cannot say what it was.
    pub whole: Vec<(usize, String)>,
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
    /// The reader's notes, as this screen placed them.
    pub notes: Noted,
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

/// The one-line stand-in for a file with no line-level diff, if it needs one.
fn note_for<'a>(kind: &ChangeKind, diff: &'a FileDiff) -> Option<&'a str> {
    match kind {
        ChangeKind::Conflict => Some("unresolved conflict"),
        ChangeKind::TypeChange => Some("type changed"),
        _ => diff
            .unreadable
            .as_deref()
            .or_else(|| diff.binary.then_some("binary")),
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
    /// The reader's notes on this file, by its path and by the path it was
    /// renamed from, as the rows to draw under their lines.
    notes: Vec<&'f Note>,
    /// The same, as the row's mark, resolved before [`Self::notes`] drops the
    /// one an open box holds.
    marks: FileNotes,
    /// The note box, as the anchor it is open on, when that anchor is in this
    /// file.
    boxed: Option<&'f crate::notes::Standing<'f>>,
}

/// Everything a row about this file needs, for either region.
fn entry_of(
    kind: &ChangeKind,
    origin: Origin,
    diff: &FileDiff,
    history: &History,
    notes: FileNotes,
) -> FileEntry {
    FileEntry {
        path: diff.path.clone(),
        origin,
        from: kind.source().map(str::to_owned),
        kind: letter(kind),
        churn: (note_for(kind, diff).is_none()).then_some((diff.added, diff.removed)),
        spark: history.level(&diff.path).unwrap_or([0; HISTORY_BUCKETS]),
        recency: history.recency(&diff.path),
        newest: history.newest(&diff.path),
        heat: heat_of(diff),
        notes,
    }
}

/// How many rows a file occupies, from its span rather than from its diff.
pub fn rows_of(change: &vigia_core::FileChange, span: &vigia_core::FileSpan) -> usize {
    // A note is a heading and one line saying why, which is exactly what
    // `note_for` produces for the same four cases.
    if matches!(change.kind, ChangeKind::Conflict | ChangeKind::TypeChange)
        || span.binary
        || span.unreadable
    {
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
        Self::collect_noted(frame, highlighter, history, viewport, &[], true, None)
    }

    /// [`View::collect`] with the reader's notes placed under the lines they are
    /// pinned to, as rows when `rows` is set and as marks alone otherwise.
    ///
    /// # Errors
    ///
    /// A file the window reaches cannot be read or measured.
    pub fn collect_noted(
        frame: &mut Frame,
        highlighter: &mut Highlighter,
        history: &History,
        viewport: Viewport,
        notes: &[Note],
        rows: bool,
        note_box: Option<&crate::notes::NoteBox>,
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
            whole: Vec::new(),
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
            notes: Noted::default(),
        };
        // Keyed by path, so a file the walk draws costs one probe for its notes
        // and a file with none costs nothing more. Empty when there are no notes,
        // and every branch below reads that emptiness first.
        let mut by_path: HashMap<&str, Vec<&Note>> = HashMap::new();
        for note in notes {
            by_path.entry(note.path.as_str()).or_default().push(note);
        }
        // The box is placed the way a note is, against the same anchor.
        let draft = note_box.map(crate::notes::NoteBox::stand_in);
        // Which entries answer to each anchored path, a fact about the changed set rather
        // than the screen: no entry means adrift, and two mean both runs hold the file.
        let mut runs_of: HashMap<&str, Vec<usize>> = HashMap::new();
        for path in by_path
            .keys()
            .copied()
            .chain(draft.as_ref().map(|standing| standing.note.path.as_str()))
        {
            runs_of.entry(path).or_default();
        }
        if !runs_of.is_empty() {
            for (index, change) in frame.files().iter().enumerate() {
                for path in change.paths() {
                    if let Some(runs) = runs_of.get_mut(path) {
                        runs.push(index);
                    }
                }
            }
            view.notes.adrift = notes
                .iter()
                .filter(|note| runs_of.get(note.path.as_str()).is_none_or(Vec::is_empty))
                .count();
        }
        if files == 0 {
            // Nothing to point at, so nothing to preserve either.
            view.top.row = 0;
            view.list_top = 0;
            return Ok(view);
        }
        // A file staged and then edited further is a diff in each run and the note
        // belongs under one: the run its line resolves best in, the earlier index on a
        // tie. Asked of every such path and not only one the walk reaches, because the
        // list draws entries the walk never will and an unresolved tie marks the file
        // twice for one note. Those diffs are I4's second exception.
        let mut chosen: HashMap<&str, usize> = HashMap::new();
        let mut boxed_run = None;
        for (path, indices) in &runs_of {
            if indices.len() < 2 {
                continue;
            }
            if let Some(here) = by_path.get(*path) {
                for (note, at) in here.iter().zip(run_of(frame, indices, here)) {
                    chosen.insert(note.id.as_str(), at);
                }
            }
            // The box belongs to the row the reader pressed, and both runs can draw that
            // line identically, so the note's own rule would answer the tie for the wrong
            // one. The run tells them apart: one run holds a path once.
            if let Some(standing) = draft.as_ref().filter(|held| held.note.path == **path) {
                let pressed = indices
                    .iter()
                    .copied()
                    .find(|&at| frame.files()[at].origin == standing.origin);
                let at = match pressed {
                    Some(at) => at,
                    // Left when the run the press was in is no longer in the changed set.
                    None => run_of(frame, indices, &[&standing.note])[0],
                };
                boxed_run = Some(at);
                // The box stands in for the note it holds, so that note goes where the box
                // is: an edit under an open box moves the rank without moving the box.
                if let Some(over) = standing.over {
                    chosen.insert(over, at);
                }
            }
        }

        let pinned = Pinned {
            by_path: &by_path,
            chosen: &chosen,
        };

        if height == 0 {
            // The list still resolves, and the tie above it already has.
            view.take_list(frame, history, list_rows, list_follows, &[], &pinned)?;
            return Ok(view);
        }

        let mut walked = Walked::default();

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
                let mut file_notes = notes_at(change, index, &pinned);
                // Before the retain below: a note being retyped is still one.
                let marks = notes_of(diff, &file_notes);
                let boxed = draft
                    .as_ref()
                    .filter(|standing| change.paths().any(|path| path == standing.note.path))
                    .filter(|_| boxed_run.is_none_or(|at| at == index));
                // The box holds this note's text, so its rows would say it twice.
                if let Some(over) = boxed.and_then(|standing| standing.over) {
                    file_notes.retain(|note| note.id != over);
                }
                view.take_file(
                    Changed {
                        kind: &change.kind,
                        origin: change.origin,
                        diff,
                        index,
                        closes: gap_rows(index, stop) > 0,
                        listed: list_rows > 0,
                        notes: file_notes,
                        marks,
                        boxed,
                    },
                    // The pass is taken whatever this frame does with it, so the sweep
                    // in its `Drop` still runs and the cache stays bounded the way I3
                    // needs.
                    highlight.then_some(&mut highlighter),
                    history,
                    skip,
                    height,
                    &mut walked,
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
                && view.display_rows(width, wrap, height, &walked, rows) < height
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
            // And what indexes into them.
            walked.pins.clear();
            walked.boxed = None;
            view.notes.segments.clear();
            // `walked.drawn` is deliberately kept.

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

        let trimmed = view.wrap_rows(width, wrap, height, at_bottom, &walked, rows);

        // After the walk, because only the walk knows where the diff landed.
        view.take_list(
            frame,
            history,
            list_rows,
            list_follows,
            &walked.drawn,
            &pinned,
        )?;
        view.measure(frame, measured, single, trimmed)?;

        Ok(view)
    }

    /// What a note written from row `offset` would pin to: the line's own row, or
    /// the head of a continuation. `None` off a content row, which is a heading,
    /// a hunk header, a note row and the blank rows.
    pub fn anchor_at(&self, offset: usize) -> Option<Anchor> {
        if offset >= self.rows.len()
            || matches!(self.rows[offset], Row::Note { .. } | Row::Box { .. })
        {
            return None;
        }
        let head = self.head_of(offset);
        let Row::Line { kind, number, .. } = &self.rows[head] else {
            return None;
        };
        let path = self
            .notes
            .segments
            .iter()
            .rev()
            .find(|(at, _, _)| *at <= head)
            .map(|(_, path, origin)| (path.clone(), *origin))?;
        Some(Anchor {
            path: path.0,
            side: Side::of(*kind),
            line: *number,
            text: self.line_at(head),
            origin: path.1,
        })
    }

    /// The notes marked on the line row `offset` is on, by id.
    pub fn marked_at(&self, offset: usize) -> Vec<&str> {
        if offset >= self.rows.len() {
            return Vec::new();
        }
        let head = self.head_of(offset);
        self.notes
            .marked
            .iter()
            .filter(|mark| mark.row == head)
            .map(|mark| mark.id.as_str())
            .collect()
    }

    /// Total the diff's rows, and how many are above this screen, the bottom
    /// clamp's `trimmed` rows counted in so this names the first row *drawn*.
    fn measure(
        &mut self,
        frame: &mut Frame,
        wanted: bool,
        single: bool,
        trimmed: usize,
    ) -> Result<()> {
        if !wanted || self.files == 0 {
            return Ok(());
        }
        if single {
            self.total_rows = self.current_span;
            self.rows_above = (self.top.row.min(self.current_span) + trimmed).min(self.total_rows);
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
        // Clamped, because a position past the end would invert the bar's travel.
        self.rows_above =
            (above + self.top.row.min(self.current_span) + trimmed).min(self.total_rows);
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
        pinned: &Pinned<'_>,
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
                    let notes = notes_of(diff, &notes_at(change, index, pinned));
                    let entry = entry_of(&change.kind, change.origin, diff, history, notes);
                    self.list.push(ListRow::from(entry));
                }
            }
        }
        Ok(())
    }

    /// How many rows of the terminal the rows this walk has collected would take.
    fn display_rows(
        &self,
        width: usize,
        wrap: bool,
        height: usize,
        walked: &Walked,
        drawn: bool,
    ) -> usize {
        let content = if width == 0 {
            0
        } else {
            let gutter = crate::render::gutter_width(&self.rows, width);
            crate::render::content_width(gutter, width)
        };
        let mut under: usize = if drawn {
            walked.pins.iter().map(|pin| pin.rows(content).len()).sum()
        } else {
            0
        };
        if let Some(boxed) = &walked.boxed {
            under += box_rows(boxed, content).0.len();
        }
        if !wrap || content == 0 {
            return self.rows.len() + under;
        }
        under
            + self
                .rows
                .iter()
                .map(|row| match row {
                    Row::Line { text, .. } => {
                        1 + crate::render::breaks_of(text, content, height).len()
                    }
                    _ => 1,
                })
                .sum::<usize>()
    }

    /// Turn logical rows into display rows, record the gutter, and answer the rows
    /// the bottom clamp trimmed off the front: [`Self::top`] still names the first.
    /// The display rows are a line's continuations and, when `drawn`, the rows of
    /// the notes pinned under it, and both are counted in the same unit.
    fn wrap_rows(
        &mut self,
        width: usize,
        wrap: bool,
        height: usize,
        at_bottom: bool,
        walked: &Walked,
        drawn: bool,
    ) -> usize {
        let pins = &walked.pins;
        // Only where a width was passed, so a caller that named none leaves
        // the decision where it has always been. See [`View::gutter`].
        self.gutter = (width > 0).then(|| crate::render::gutter_width(&self.rows, width));
        if height == 0 || self.rows.is_empty() {
            return 0;
        }
        let content = if width == 0 {
            0
        } else {
            crate::render::content_width(self.gutter.unwrap_or(0), width)
        };
        let wrapping = wrap && content > 0;

        // Where each collected row breaks, and how many rows of terminal it therefore
        // takes.
        let breaks: Vec<Vec<usize>> = self
            .rows
            .iter()
            .map(|row| match row {
                Row::Line { text, .. } if wrapping => {
                    crate::render::breaks_of(text, content, height.saturating_add(1))
                }
                _ => Vec::new(),
            })
            .collect();
        // Built once, so the clamp and the emit below agree on their count.
        let mut under: Vec<Vec<Row>> = vec![Vec::new(); breaks.len()];
        if drawn {
            for pin in pins {
                under[pin.row].extend(pin.rows(content));
            }
        }
        // The box stands first under its line, whatever else is pinned there,
        // and whether or not the rows are shown: a mode is not a toggle.
        let mut boxed_rows = 0usize;
        let mut caret_at = 0usize;
        if let Some(boxed) = &walked.boxed {
            let (rows, caret) = box_rows(boxed, content);
            boxed_rows = rows.len();
            caret_at = caret;
            under[boxed.row].splice(0..0, rows);
        }
        let cost = |at: usize| breaks[at].len() + 1 + under[at].len();
        let total: usize = (0..breaks.len()).map(cost).sum();

        // Nothing on this screen wraps and nothing sits under a row, so the rows
        // are the display rows and every index already names one.
        if total == breaks.len() {
            self.notes.marked = pins
                .iter()
                .filter(|pin| pin.marks)
                .map(|pin| Marked {
                    row: pin.row,
                    id: pin.id.clone(),
                    bare: pin.body.is_empty(),
                })
                .collect();
            self.notes.boxed = walked
                .boxed
                .as_ref()
                .filter(|boxed| boxed.marks)
                .map(|boxed| boxed.row);
            return 0;
        }

        // Both clamps as display rows dropped off the front, the only thing
        // either can move.
        let mut dropped = if at_bottom {
            total.saturating_sub(height)
        } else {
            0
        };
        // A box the reader cannot see is a mode they cannot leave on purpose,
        // so the window is pulled into the span that shows one.
        if boxed_rows > 0
            && let Some(boxed) = &walked.boxed
        {
            // The rows above the line, then the line with its continuations.
            let above_line = (0..boxed.row).map(cost).sum::<usize>();
            let opens = above_line + breaks[boxed.row].len() + 1;
            // Show as much of the box as the pane holds, from its last row up.
            let ends = opens + boxed_rows;
            // The caret's row has to be on screen or the reader cannot see what
            // they are typing. The anchored line is preferred, then the top
            // edge, and each gives way in that order on a pane too short.
            let caret = opens + caret_at;
            let floor = (caret + 1).saturating_sub(height);
            let ceiling = above_line.max(floor).min(opens.max(floor));
            dropped = dropped
                .max(ends.saturating_sub(height))
                .clamp(floor, ceiling);
        }
        let mut from = 0usize;
        let mut above = dropped;
        while from < breaks.len() && above >= cost(from) {
            above -= cost(from);
            from += 1;
        }

        // Each note's two texts by id, so the emission below need not rescan `pins`.
        let texts: HashMap<&str, (&str, Option<&str>)> = pins
            .iter()
            .map(|pin| (pin.id.as_str(), (pin.body.as_str(), pin.reply.as_deref())))
            .collect();
        // [`Self::top`] is not moved, and that is what makes the end of the
        // diff a place a reader can leave.
        let mut out: Vec<Row> = Vec::with_capacity(height);
        // One entry per wrapped line on this screen, which is what bounds the scan
        // `line_at` does over it.
        let mut whole: Vec<(usize, String)> = Vec::new();
        // The display row each logical row's own first piece landed on, so the
        // marks and the segments can be carried across the split.
        let mut landed: Vec<Option<usize>> = vec![None; breaks.len()];
        for (at, row) in self.rows.drain(..).enumerate() {
            if at < from {
                continue;
            }
            if out.len() >= height {
                break;
            }
            // Display rows of this logical row to pass over, which is the display
            // offset above: its own pieces first, then the note rows under it.
            let mut skip = if at == from { above } else { 0 };
            let head = out.len();
            match row {
                Row::Line {
                    kind,
                    number,
                    text,
                    spans,
                    emph,
                } if !breaks[at].is_empty() => {
                    // The last moment the whole line exists; the kept pieces are not it.
                    let indent = crate::render::indent_of(&text, content);
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
                        if skip > 0 {
                            skip -= 1;
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
                            landed[at] = Some(out.len());
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
                    whole.push((head, text));
                }
                row => {
                    if skip > 0 {
                        skip -= 1;
                    } else {
                        landed[at] = Some(out.len());
                        out.push(row);
                    }
                }
            }
            for note_row in std::mem::take(&mut under[at]) {
                if out.len() >= height {
                    break;
                }
                if skip > 0 {
                    skip -= 1;
                    continue;
                }
                // Recorded as a wrapped line's is, at the first row drawn. The
                // rows hold prose broken at blanks drawn on neither side, so
                // rejoining them spaces the note wrong in both directions.
                if !continues(&note_row, out.last())
                    && let Row::Note { id, lead, .. } = &note_row
                    && let Some(voice) = lead.voice()
                    && let Some((body, reply)) = texts.get(id.as_str())
                    && let Some(text) = match voice {
                        NoteVoice::Reader => Some((*body).to_owned()),
                        NoteVoice::Agent => reply.map(str::to_owned),
                    }
                {
                    whole.push((out.len(), text));
                }
                out.push(note_row);
            }
        }
        self.rows = out;
        self.whole = whole;
        self.notes.marked = pins
            .iter()
            .filter(|pin| pin.marks)
            .filter_map(|pin| {
                landed[pin.row].map(|row| Marked {
                    row,
                    id: pin.id.clone(),
                    bare: pin.body.is_empty(),
                })
            })
            .collect();
        self.notes.boxed = walked
            .boxed
            .as_ref()
            .filter(|boxed| boxed.marks)
            .and_then(|boxed| landed[boxed.row]);
        // A segment names the first row of its file still on screen, which after
        // the trim may be a later row than the one the walk recorded.
        let segments = std::mem::take(&mut self.notes.segments);
        self.notes.segments = segments
            .iter()
            .enumerate()
            .filter_map(|(at, (first, path, origin))| {
                let end = segments
                    .get(at + 1)
                    .map_or(landed.len(), |(next, _, _)| *next);
                landed[*first..end]
                    .iter()
                    .find_map(|row| *row)
                    .map(|row| (row, path.clone(), *origin))
            })
            .collect();
        from
    }

    /// Rows of the diff this screen holds: §11.1's *screenful*, the line a trimmed
    /// bottom opens inside counted so this is the trim's exact complement. Only a
    /// continuation counts: a note over the top edge leaves its line wholly above.
    pub fn shown(&self) -> usize {
        let opens_inside = self.rows.first().is_some_and(Row::is_wrap);
        self.rows.iter().filter(|row| !row.is_display()).count() + usize::from(opens_inside)
    }

    /// Whether `span` resolves to any line, which a standing wash asks every frame.
    pub fn resolves(&self, span: (usize, usize)) -> bool {
        let last = self.rows.len().checked_sub(1);
        last.is_some_and(|last| span.0 <= last)
    }

    /// The lines the rows `span` covers, inclusive: §11.2 B20's own strings, so a
    /// clipped line arrives whole and a note spanning rows arrives once.
    pub fn lines_in(&self, span: (usize, usize)) -> Option<Vec<String>> {
        let (from, to) = span;
        let last = self.rows.len().checked_sub(1)?;
        if from > last {
            return None;
        }
        let mut out: Vec<String> = Vec::new();
        let mut taken: Option<usize> = None;
        for at in from..=to.min(last) {
            // Passed over rather than sent as a blank, which would clear the
            // clipboard on a span that is all frame.
            let Some(head) = self.text_head_of(at) else {
                continue;
            };
            if taken == Some(head) {
                continue;
            }
            taken = Some(head);
            out.push(self.line_at(head));
        }
        Some(out)
    }

    /// The row whose text `at` is part of, `None` where it is part of none. Not
    /// [`Self::head_of`], which answers which line of the diff a row hangs under:
    /// a note's anchor and its mark want that, and the copy must not have it.
    fn text_head_of(&self, at: usize) -> Option<usize> {
        if !self.rows.get(at)?.owns_text() {
            return None;
        }
        let mut head = at;
        while head > 0 && continues(&self.rows[head], self.rows.get(head - 1)) {
            head -= 1;
        }
        Some(head)
    }

    /// The row a display row belongs to; none only above a scrolled head.
    pub(crate) fn head_of(&self, at: usize) -> usize {
        self.rows[..=at]
            .iter()
            .rposition(|row| !row.is_display())
            .unwrap_or(0)
    }

    /// One logical line: the walk's record where wrapping cut it, the row otherwise.
    fn line_at(&self, head: usize) -> String {
        if let Some((_, whole)) = self.whole.iter().find(|(row, _)| *row == head) {
            return whole.clone();
        }
        match &self.rows[head] {
            Row::Line { text, .. } | Row::Wrap { text, .. } => text.clone(),
            // The path and not the drawn label, which elides.
            Row::File(entry) => entry.path.clone(),
            Row::Hunk {
                old_start,
                old_lines,
                new_start,
                new_lines,
            } => {
                // The painter's own speller, which drops the `,1` git drops: spelled
                // the other way it is a header naming a different line.
                format!(
                    "@@ -{} +{} @@",
                    crate::render::span(*old_start, *old_lines),
                    crate::render::span(*new_start, *new_lines)
                )
            }
            Row::Reason(reason) => reason.clone(),
            Row::Note { text, .. } => text.clone(),
            // A draft is not a line of anything yet.
            Row::Box { .. } | Row::Gap => String::new(),
        }
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
        walked: &mut Walked,
    ) {
        let Walked {
            drawn,
            pins,
            boxed: box_pin,
        } = walked;
        let Changed {
            kind,
            origin,
            diff,
            index,
            closes,
            listed,
            notes,
            marks,
            boxed,
        } = file;
        let mut n = 0usize;
        let first = self.rows.len();
        // Where each content row landed, on its side and by its number, so a note
        // resolved against the file can be pinned to the row that draws its line.
        // Filled only for a file that carries notes.
        let mut placed: Vec<(Side, u32, usize)> = Vec::new();
        let mut heading: Option<usize> = None;

        // Built for the row when the heading fits, and recorded when it does not and a
        // list exists to read the record.
        if n >= skip {
            let entry = entry_of(kind, origin, diff, history, marks);
            drawn.push((index, entry.clone()));
            heading = Some(self.rows.len());
            self.rows.push(Row::file(entry));
        } else if listed {
            self.recorded += 1;
            // Moved rather than cloned, because there is no row to draw it in.
            drawn.push((index, entry_of(kind, origin, diff, history, marks)));
        }
        n += 1;

        // A labelled block so the block's closing gap has one push site.
        'block: {
            if let Some(reason) = note_for(kind, diff) {
                if n >= skip && self.rows.len() < height {
                    self.rows.push(Row::Reason(reason.to_owned()));
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
                // and counts both sides forward from the header in `numbered`.
                for (within, (number, line)) in hunk.numbered().enumerate() {
                    if n >= skip {
                        if self.rows.len() >= height {
                            break 'block;
                        }
                        if !notes.is_empty() || boxed.is_some() {
                            placed.push((Side::of(line.kind), number, self.rows.len()));
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

        if self.rows.len() > first {
            self.notes.segments.push((first, diff.path.clone(), origin));
        }
        if !notes.is_empty() {
            pin(pins, &notes, diff, heading, &placed);
        }
        if let Some(stand_in) = boxed {
            *box_pin = place_box(stand_in, diff, heading, &placed);
        }
    }
}

/// What the walk carries beside the rows.
#[derive(Default)]
struct Walked {
    /// Entries the body built, so the list can reuse rather than re-diff.
    /// Bounded by the viewport: one per file the walk reaches, which is one
    /// per heading that fits plus the file the viewport is sitting inside.
    drawn: Vec<(usize, FileEntry)>,
    /// Notes placed on logical rows, for the display pass to draw.
    pins: Vec<Pin>,
    /// The note box, placed on its logical row, when its line is on screen.
    boxed: Option<BoxPin>,
}

/// The logical row a note's line landed on, the word it carries there, whether
/// its rows dim, and whether the row is the line rather than the heading;
/// `None` off this screen, which is not a state.
fn placed_at(
    note: &Note,
    rows: &[(u32, &str)],
    heading: Option<usize>,
    placed: &[(Side, u32, usize)],
) -> Option<(usize, &'static str, bool, bool)> {
    let row_of = |number: u32| {
        placed
            .iter()
            .find(|(side, at, _)| *side == note.side && *at == number)
            .map(|(_, _, row)| *row)
    };
    let (row, word, faded, marks) = match resolve(note, rows) {
        Placement::At(number) | Placement::Moved(number) => {
            (row_of(number), note.status.name(), false, true)
        }
        Placement::Changed => (row_of(note.line), "changed", true, true),
        Placement::Gone => (heading, "gone", false, false),
    };
    row.map(|row| (row, word, faded, marks))
}

/// Place the box the way a note is placed, so it follows its line as one does.
fn place_box(
    stand_in: &crate::notes::Standing<'_>,
    diff: &FileDiff,
    heading: Option<usize>,
    placed: &[(Side, u32, usize)],
) -> Option<BoxPin> {
    let rows = diff.rows_on(stand_in.note.side);
    let (row, _, _, marks) = placed_at(&stand_in.note, &rows, heading, placed)?;
    Some(BoxPin {
        row,
        marks,
        label: format!("{}:{}", stand_in.note.path, stand_in.note.line),
        lines: stand_in.lines.to_vec(),
        cursor: stand_in.cursor,
    })
}

/// Place each of a file's notes on the logical row that draws its line, or on
/// the heading once the line is gone. Resolved against the whole diff in hand
/// rather than the rows on screen, because a line one row under the fold is not
/// gone, and the diff of a drawn file costs no read the frame did not already
/// make.
fn pin(
    pins: &mut Vec<Pin>,
    notes: &[&Note],
    diff: &FileDiff,
    heading: Option<usize>,
    placed: &[(Side, u32, usize)],
) {
    let mut on_new: Option<Vec<(u32, &str)>> = None;
    let mut on_old: Option<Vec<(u32, &str)>> = None;
    for note in notes {
        let rows = match note.side {
            Side::New => on_new.get_or_insert_with(|| diff.rows_on(Side::New)),
            Side::Old => on_old.get_or_insert_with(|| diff.rows_on(Side::Old)),
        };
        // Off screen this frame, which is not a state: the row it belongs on is
        // not drawn, so neither is it.
        let Some((row, word, faded, marks)) = placed_at(note, rows, heading, placed) else {
            continue;
        };
        pins.push(Pin {
            row,
            id: note.id.clone(),
            body: note.body.clone(),
            reply: note.reply.clone(),
            word,
            faded,
            marks,
            resolved: note.status == Status::Resolved,
        });
    }
}

#[cfg(test)]
mod tests {
    //! The heat projection, the follow landing and the box's own wrap, tested
    //! as the arithmetic they are.

    use vigia_core::Line;

    use super::*;

    /// A box holding `lines` with the caret at `cursor`, placed on row zero.
    fn boxed(lines: &[&str], cursor: (usize, usize)) -> BoxPin {
        BoxPin {
            row: 0,
            marks: true,
            label: "src/watch.rs:5".to_owned(),
            lines: lines.iter().map(|line| (*line).to_owned()).collect(),
            cursor,
        }
    }

    /// The body rows `pin` draws at `content` columns, and which one holds the
    /// caret.
    fn body_of(pin: &BoxPin, content: usize) -> (Vec<String>, Option<usize>) {
        let mut text = Vec::new();
        let mut caret = None;
        for row in box_rows(pin, content).0 {
            if let Row::Box {
                part:
                    BoxPart::Body {
                        text: piece,
                        caret: column,
                    },
            } = row
            {
                if column.is_some() {
                    caret = Some(text.len());
                }
                text.push(piece);
            }
        }
        (text, caret)
    }

    #[test]
    fn the_box_draws_its_cap_and_no_more_wherever_the_caret_sits() {
        // The rows below the caret are what a cap off by one adds, so a caret
        // at the end of the text cannot tell the two apart: there is nothing
        // under it left to draw.
        let lines = ["one", "two", "three", "four", "five", "six"];
        for (cursor, at) in [
            ((0, 0), Some(0)),
            ((2, 0), Some(2)),
            ((5, 3), Some(BOX_ROWS - 1)),
        ] {
            let (body, caret) = body_of(&boxed(&lines, cursor), 40);
            assert_eq!(
                body.len(),
                BOX_ROWS,
                "a caret at {cursor:?} drew {} body rows rather than the cap",
                body.len()
            );
            assert_eq!(
                caret, at,
                "the caret's row moved for a cursor at {cursor:?}"
            );
        }
    }

    #[test]
    fn the_caret_stands_inside_a_row_wherever_a_break_dropped_a_blank() {
        // A break drops the blanks it broke on, so a caret among them belongs
        // to no piece: it takes the head of the next row rather than a column
        // off the end of the row before, where no cell would draw it.
        let content = "aaaa  bbbb";
        let pieces = prose_pieces(content, 4);
        assert_eq!(pieces, vec![0..4, 6..10], "the fixture does not break here");
        assert_eq!(caret_in(content, &pieces, 4, 4), (1, 0));
        assert_eq!(caret_in(content, &pieces, 5, 4), (1, 0));
        assert_eq!(caret_in(content, &pieces, 6, 4), (1, 0));
        // And inside a piece it is where the characters put it.
        assert_eq!(caret_in(content, &pieces, 2, 4), (0, 2));
        assert_eq!(caret_in(content, &pieces, 8, 4), (1, 2));
        // Past the end of a piece that fills its row it takes the row after,
        // which is the row the caller makes when none follows.
        assert_eq!(caret_in("aaaa", &prose_pieces("aaaa", 4), 4, 4), (1, 0));
    }

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
            unreadable: None,
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
