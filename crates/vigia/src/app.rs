//! Everything the shell remembers between frames, and the arithmetic on it.
//!
//! Which is very little on purpose. The diffs live in [`vigia_core::Frame`], the
//! cells live in the buffer, and what is left here is a scroll position and one
//! optional message. A monitor with more state than that has started becoming a
//! reviewer.

use vigia_core::{Frame, Result};

use crate::input::Action;
use crate::render::Chrome;
use crate::view::{Position, View, rows_in};

/// The shell's state.
#[derive(Debug, Clone, Default)]
pub struct App {
    /// Top of the viewport.
    position: Position,
    /// What the footer should say instead of the key hints.
    notice: Option<String>,
}

impl App {
    /// A shell looking at the top of the diff.
    pub fn new() -> Self {
        Self::default()
    }

    /// Where the viewport currently starts.
    pub fn position(&self) -> Position {
        self.position
    }

    /// The message the footer is carrying, if any.
    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    /// Record that something went wrong without giving up the screen.
    ///
    /// A monitor beside an agent sees a repository mid-`git gc` and files that
    /// vanish between being named and being read. `SPEC.md` §6 and the core's
    /// own error docs both call those ordinary, so they belong on the footer,
    /// not on the way out.
    pub fn warn(&mut self, message: impl Into<String>) {
        self.notice = Some(message.into());
    }

    /// Drop the current message, because the frame it described has passed.
    pub fn clear_notice(&mut self) {
        self.notice = None;
    }

    /// The chrome for this frame.
    pub fn chrome(&self, worktree: &str) -> Chrome {
        Chrome {
            worktree: worktree.to_owned(),
            notice: self.notice.clone(),
        }
    }

    /// Apply one intention.
    ///
    /// Takes the frame because scrolling is not arithmetic on a single number:
    /// the position names a file and an offset within it, so moving off either
    /// end of a file means asking how many rows the neighbouring one has. That
    /// question costs a `stat` for a file the frame already holds, and never a
    /// read. See [`crate::view::Position`] for why the cheaper-looking
    /// representation is the expensive one.
    ///
    /// Returns `false` when the action was to quit.
    pub fn apply(&mut self, action: Action, frame: &mut Frame, height: usize) -> Result<bool> {
        match action {
            Action::Quit => return Ok(false),
            Action::Redraw => {}
            Action::Scroll(rows) => self.scroll(rows, frame)?,
            // A page keeps one row of overlap, which is what stops a reader
            // losing their place at the seam between two screens.
            Action::Page(pages) => {
                let step = height.saturating_sub(1).max(1) as isize;
                self.scroll(pages.saturating_mul(step), frame)?;
            }
            Action::Top => self.position = Position::default(),
            // The last *file*, from its top, rather than the last row of the
            // whole diff. Finding that row would mean diffing every file to add
            // up their heights, which is the read I4 forbids.
            Action::Bottom => {
                self.position = Position {
                    file: frame.files().len().saturating_sub(1),
                    row: 0,
                };
            }
        }
        Ok(true)
    }

    fn scroll(&mut self, rows: isize, frame: &mut Frame) -> Result<()> {
        match rows.cmp(&0) {
            std::cmp::Ordering::Equal => Ok(()),
            std::cmp::Ordering::Greater => self.down(rows.unsigned_abs(), frame),
            std::cmp::Ordering::Less => self.up(rows.unsigned_abs(), frame),
        }
    }

    /// Scrolling down needs no arithmetic against the file at all.
    ///
    /// [`View::collect`] resolves a row offset that overruns its file by carrying
    /// it into the following ones, and reports where it landed. Doing it there
    /// rather than here is what keeps the resolve and the draw to one diff per
    /// file instead of two.
    fn down(&mut self, rows: usize, _frame: &mut Frame) -> Result<()> {
        self.position.row = self.position.row.saturating_add(rows);
        Ok(())
    }

    fn up(&mut self, rows: usize, frame: &mut Frame) -> Result<()> {
        let mut left = rows;
        loop {
            if left <= self.position.row {
                self.position.row -= left;
                return Ok(());
            }
            // Everything this file can absorb is absorbed; the rest comes out of
            // the ones above it.
            left -= self.position.row;
            if self.position.file == 0 {
                self.position.row = 0;
                return Ok(());
            }
            self.position.file -= 1;
            // One past the previous file's last row, so consuming the next step
            // lands on that last row rather than one before it.
            self.position.row = rows_in(frame, self.position.file)?;
        }
    }

    /// Collect the rows this screen needs, and keep where they came from.
    ///
    /// Storing [`View::top`] back is what keeps the next frame cheap. A position
    /// that overruns its file, or points past the end of a list the agent in the
    /// other pane has shortened, is resolved by walking the files it crosses; a
    /// resolved one starts on the file it draws. Writing the answer back means
    /// that walk is paid once per scroll rather than once per frame.
    pub fn view(&mut self, frame: &mut Frame, height: usize) -> Result<View> {
        let view = View::collect(frame, self.position, height)?;
        self.position = view.top;
        Ok(view)
    }
}
