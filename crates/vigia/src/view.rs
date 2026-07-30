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

use vigia_core::{ChangeKind, FileDiff, Frame, Highlighter, LineKind, Result, Span};

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
    /// Collect a screenful, bracketing the highlighter's frame around it.
    ///
    /// The bracket lives here rather than in [`crate::App`] or in the shell's
    /// draw loop because it is what bounds the highlight cache by the viewport,
    /// and a bound that every caller has to remember to apply is a bound that
    /// one of them will not. Sweeping runs even when the walk fails: a frame
    /// that could not be collected drew nothing, so there is nothing it should
    /// be holding.
    pub fn collect(
        frame: &mut Frame,
        highlighter: &mut Highlighter,
        position: Position,
        height: usize,
    ) -> Result<Self> {
        highlighter.begin();
        let view = Self::gather(frame, highlighter, position, height);
        highlighter.sweep();
        view
    }

    fn gather(
        frame: &mut Frame,
        highlighter: &mut Highlighter,
        position: Position,
        height: usize,
    ) -> Result<Self> {
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

        while index < files && view.rows.len() < height {
            view.read += 1;
            let (change, diff) = frame.diff(index)?;
            // Both halves of the tuple are immutable borrows of the same frame,
            // so the kind needs no clone to be read alongside the diff.
            let span = span_of(&change.kind, diff);

            if !placed {
                if skip >= span {
                    if index + 1 < files {
                        // Wholly above the window. Carrying the remainder into
                        // the next file rather than clamping is what makes
                        // scrolling off the end of a short file continue into the
                        // one below instead of stopping there.
                        skip -= span;
                        index += 1;
                        continue;
                    }
                    // Past the end of the last file. Rest on its final row, so
                    // the bottom of the diff is content rather than blank.
                    skip = span - 1;
                }
                view.top = Position {
                    file: index,
                    row: skip,
                };
                placed = true;
            }

            view.take_file(&change.kind, diff, highlighter, skip, height);
            skip = 0;
            index += 1;
        }

        Ok(view)
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
        highlighter: &mut Highlighter,
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
