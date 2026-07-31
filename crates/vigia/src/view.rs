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

/// What a row of the body is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    /// A changed file's heading.
    File {
        /// Repository-relative path.
        path: String,
        /// Where the content came from, for a rename or a copy.
        from: Option<String>,
        /// One letter naming what happened.
        kind: char,
        /// Lines added and removed, or `None` when there is no line-level diff.
        churn: Option<(u32, u32)>,
        /// This file's churn over the glance window, oldest bucket first.
        ///
        /// Raw counts rather than heights. Which glyph a count becomes is the
        /// renderer's, the same way a [`Row::Line`] carries its spans as classes
        /// and lets the renderer pick the colour: the scale is shared across the
        /// screen and lives on [`View::peak`].
        ///
        /// All zeroes for a file `vigia` has not seen change, which is the
        /// ordinary case for a worktree that was already dirty at startup.
        spark: [u16; HISTORY_BUCKETS],
        /// How recently this file changed, which is what dims a settled row and
        /// what puts the pulse on one that just moved.
        recency: Recency,
        /// Where in this file the change is, as counts per slice of its length.
        ///
        /// The finest resolution the strip is ever drawn at. A renderer with
        /// fewer columns sums adjacent buckets and classifies the sums, which is
        /// exact; it never draws a prefix of this array, because half a strip
        /// drawn as a whole one says the file's tail is unchanged.
        ///
        /// All zeroes when there is nothing to place: a binary file, a removal,
        /// a conflict.
        heat: [HeatBucket; HEAT_BUCKETS],
    },
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
}

/// Slices a file's length is divided into for the heat strip.
///
/// Twelve, which is not a taste call: `assets/preview.svg` draws exactly twelve
/// and `SPEC.md` §5.1 rules that a published artifact answering an open question
/// **is** the answer. The picture also draws an empty slice as a dark track
/// rather than as a gap, which is why [`Row::File::heat`] is always this long and
/// why the renderer draws a block for every bucket.
pub const HEAT_BUCKETS: usize = 12;

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

/// A screenful of rows, plus what the chrome needs to describe it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct View {
    /// The rows to draw, top to bottom.
    pub rows: Vec<Row>,
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
    /// Zero means nothing is tracked, and a renderer must read it as "draw no
    /// sparkline" rather than dividing by it.
    pub peak: u16,
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

/// How many rows the file at `index` would occupy.
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
    let (change, diff) = frame.diff(index)?;
    Ok(span_of(&change.kind, diff))
}

impl View {
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
        position: Position,
        height: usize,
    ) -> Result<Self> {
        // One pass, dropped at every exit including the `?`s below, which is
        // what keeps the highlight cache bounded by the viewport. The guard
        // rather than a pair of calls is `vigia_core::Highlighter::pass`'s
        // business and its doc says why.
        let mut highlighter = highlighter.pass();
        let files = frame.files().len();
        let mut view = Self {
            // Bounded by the screen, not by the diff. The cap keeps a caller
            // asking for an absurd height from allocating for it up front.
            rows: Vec::with_capacity(height.min(64)),
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
            peak: history.peak(),
        };
        if files == 0 {
            // Nothing to point at, so nothing to preserve either.
            view.top.row = 0;
            return Ok(view);
        }
        if height == 0 {
            return Ok(view);
        }

        let mut index = view.top.file;
        let mut skip = position.row;
        let mut placed = false;

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
                let span = span_of(&change.kind, diff);

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

                view.take_file(&change.kind, diff, &mut highlighter, history, skip, height);
                skip = 0;
                index += 1;
            }

            if !overshot {
                break;
            }
            // Nothing has been drawn yet: `placed` is still false, and
            // `take_file` runs only after placing. So restarting is a restart
            // rather than a second helping.
            view.top = Self::last_screenful(frame, files, height, &mut view.read)?;
            index = view.top.file;
            skip = view.top.row;
            placed = true;
        }

        Ok(view)
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
            have += span_of(&change.kind, diff);
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
        kind: &ChangeKind,
        diff: &FileDiff,
        highlighter: &mut Pass<'_>,
        history: &History,
        skip: usize,
        height: usize,
    ) {
        let mut n = 0usize;

        if n >= skip {
            self.rows.push(Row::File {
                path: diff.path.clone(),
                from: source_of(kind).map(str::to_owned),
                kind: letter(kind),
                churn: (note_for(kind, diff).is_none()).then_some((diff.added, diff.removed)),
                // Asked for only on a row actually pushed, the same rule the
                // highlighter follows below. A heading scrolled past above the
                // window costs two hash lookups it would never have drawn.
                spark: history.churn(&diff.path).unwrap_or([0; HISTORY_BUCKETS]),
                recency: history.recency(&diff.path),
                heat: heat_of(diff),
            });
        }
        n += 1;

        if let Some(note) = note_for(kind, diff) {
            if n >= skip && self.rows.len() < height {
                self.rows.push(Row::Note(note));
            }
            return;
        }

        for (ordinal, hunk) in diff.hunks.iter().enumerate() {
            if self.rows.len() >= height {
                return;
            }

            // A hunk entirely above the window costs one addition. The line
            // numbers restart from the next hunk's header, so nothing has to be
            // carried across the ones that are skipped.
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

            // The core carries line numbers per hunk rather than per line, so
            // both sides are counted forward from the header. Every line
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
                        return;
                    }
                    self.rows.push(Row::Line {
                        kind: line.kind,
                        number,
                        text: line.text.clone(),
                        spans: highlighter
                            .spans(&diff.path, ordinal, hunk, within)
                            .to_vec(),
                    });
                }
                n += 1;
            }
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

    /// A hundred and twenty lines over twelve buckets is ten lines each, so a
    /// change at line 1 is bucket 0 and a change at line 61 is bucket 6.
    #[test]
    fn a_hunk_lands_in_the_buckets_its_lines_fall_in() {
        let map = heat_of(&diff(
            120,
            vec![
                hunk(1, &[LineKind::Added]),
                hunk(61, &[LineKind::Added, LineKind::Added]),
            ],
        ));

        assert_eq!(touched(&map), vec![0, 6]);
        assert_eq!(map[0].added, 1);
        assert_eq!(map[6].added, 2);
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

        assert_eq!(touched(&map), vec![0, 4, 8]);
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
            vec![1],
            "the addition is on line 11, which is the second bucket of 120/12"
        );
    }
}
