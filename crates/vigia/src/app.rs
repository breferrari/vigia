//! Everything the shell remembers between frames, and the arithmetic on it.

use std::time::{Duration, Instant};

use vigia_core::{Frame, Highlighter, History, Note, Result, Samples};

use crate::input::{Action, Pointing};
use crate::memory;
use crate::render::{Body, Chrome, Mode};
use crate::view::{Position, View, Viewport, rows_in};

/// Completed frames the status bar's p99 is taken over.
const FRAME_SAMPLES: usize = 128;

/// A track fraction resolved against a count, saturating at the last index.
fn scaled(at: u32, count: usize) -> usize {
    if count == 0 {
        return 0;
    }
    ((u64::from(at) * count as u64) / u64::from(crate::input::TRACK_SCALE)) as usize
}

/// The bytes a gesture asked to send, and what the footer calls them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sending {
    /// The bytes for the clipboard.
    pub text: String,
    /// What the footer names them.
    pub said: String,
}

impl Sending {
    /// Selected lines, named by how many. **Lines and not rows**: a wrapped line
    /// is several rows and one line, and the count names what was sent.
    fn lines(lines: &[String]) -> Self {
        Self {
            said: if lines.len() == 1 {
                "1 line".to_owned()
            } else {
                format!("{} lines", lines.len())
            },
            text: lines.join("\n"),
        }
    }
}

/// How far a shell has got through its opening two frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Paint {
    /// Nothing drawn yet; the next frame draws plain.
    Never,
    /// The plain frame is on screen and a coloured one is owed.
    Plain,
    /// Every frame from here parses.
    Coloured,
}

/// What a message on the footer is: its colour and how it arrives and leaves.
/// Carried rather than derived, since one call site produces two of these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Voice {
    /// An act of the reader's, answering.
    Said,
    /// Unasked-for, and nothing is wrong.
    Arrived,
    /// Something went wrong and the reader should know.
    Alert,
}

/// The shell's state.
#[derive(Debug, Clone)]
pub struct App {
    /// Top of the viewport.
    position: Position,
    /// What the footer should say instead of the key hints.
    notice: Option<String>,
    /// Shown over `notice` rather than into it: a confirmation must not be erased
    /// by the next tick, nor bury a warning with no expiry. Deadline and voice ride along.
    flash: Option<(String, Instant, Voice)>,
    /// Asked for and not sent yet.
    sending: Option<Sending>,
    /// Rows the drag has washed, as offsets into the collected rows.
    selecting: Option<(usize, usize)>,
    /// Whether the last collect resolved that span; the release resolves its own.
    resolved: bool,
    /// Whether the viewport moves itself to what just changed.
    following: bool,
    /// Whether the masthead is drawn, which `m` toggles.
    masthead: bool,
    /// Whether listed paths carry a file-type icon. Config only; no gesture.
    icons: bool,
    /// Whether listed paths are OSC 8 hyperlinks. Config only; on by default.
    links: bool,
    /// Whether the reader has asked for the list beside the diff. What the pane
    /// can give is [`Body::rail`].
    rail: bool,
    /// Whether the diff shows one file at a time (`s`). Which file is
    /// [`Self::position`]'s.
    single: bool,
    /// Whether a too-wide content line continues on the row below (`w`).
    wrap: bool,
    /// The reader's notes as the store last listed them, placed by every collect.
    notes: Vec<Note>,
    /// Whether the note rows are drawn under their lines (`c`); the marks stay.
    notes_shown: bool,
    /// Notes the last collect counted, and how many of them were adrift.
    note_count: (usize, usize),
    /// Logical rows the last frame drew, which a page step is measured in.
    /// Stepping by the display height instead walks over unwrapped content.
    shown: usize,
    /// Whether the reader has asked for the staged run (`a`).
    staged: bool,
    /// How many files the staged run held on the last collect.
    staged_files: usize,
    /// Which page of the gestures sheet is drawn, and `None` when it is not.
    /// Retained between frames, or an agent's write dismisses it.
    sheet: Option<usize>,
    /// Pages the sheet has on the pane last drawn for. One frame stale after a
    /// resize, which `sheet_plan` clamps.
    sheet_pages: usize,
    /// Whether the position was reached by scrolling rather than by a jump, which
    /// licenses the viewport to back up and fill the pane. Pinned `G` sets it true on
    /// purpose; see its arm.
    anchored: bool,
    /// The last path a tick named, kept while disengaged so `f` can jump to it.
    newest: Option<String>,
    /// Whether the next frame owes the position its row. Following may not diff
    /// or `stat` (I4), so it names the file and [`View::collect`] finds the row.
    landing: bool,
    /// First file the pinned list shows. Carried so `J` survives a redraw.
    list_top: usize,
    /// Whether the list's window is still the diff's to move. `J` takes it over
    /// and anything moving the diff hands it back.
    list_follows: bool,
    /// Rows the pinned list had on the frame last drawn, which `J` clamps to.
    list_rows: usize,
    /// Whether the watch is still live, which the header draws as a word.
    mode: Mode,
    /// What recent frames cost, which the status bar draws the p99 of.
    frames: Samples,
    /// How far this shell has got through its opening two frames.
    paint: Paint,
    /// Resident set size as of the last frame that sampled it. Stored because
    /// [`App::chrome`] is built more than once per frame.
    memory: Option<u64>,
}

impl Default for App {
    /// Hand-written because [`Samples`] takes its capacity at construction.
    fn default() -> Self {
        Self {
            position: Position::default(),
            notice: None,
            flash: None,
            sending: None,
            selecting: None,
            resolved: false,
            following: false,
            masthead: false,
            rail: false,
            single: false,
            staged: false,
            wrap: false,
            notes: Vec::new(),
            notes_shown: true,
            note_count: (0, 0),
            shown: 0,
            icons: false,
            // OSC 8 degrades silently, so it costs nothing where unsupported.
            links: true,
            staged_files: 0,
            sheet: None,
            sheet_pages: 1,
            anchored: false,
            list_top: 0,
            list_follows: true,
            list_rows: 0,
            newest: None,
            landing: false,
            mode: Mode::default(),
            paint: Paint::Never,
            frames: Samples::new(FRAME_SAMPLES),
            memory: None,
        }
    }
}

impl App {
    /// A shell looking at the top of the diff, and following (I5).
    pub fn new() -> Self {
        Self {
            following: true,
            ..Self::default()
        }
    }

    /// [`App::new`] with the view toggles a reader's config file asked for.
    pub fn configured(config: crate::Config) -> Self {
        Self {
            masthead: config.masthead,
            rail: config.rail,
            single: config.single,
            wrap: config.wrap,
            shown: 0,
            staged: config.staged,
            icons: config.icons,
            links: config.links,
            ..Self::new()
        }
    }

    /// A shell already past its opening two frames, so the next one colours.
    #[doc(hidden)]
    pub fn past_first_paint() -> Self {
        Self {
            paint: Paint::Coloured,
            ..Self::new()
        }
    }

    /// Whether a coloured frame is owed for the plain one already on screen.
    pub fn owes_repaint(&self) -> bool {
        self.paint == Paint::Plain
    }

    /// Record what one whole frame cost, once it is on screen.
    pub fn record_frame(&mut self, cost: Duration) {
        self.frames.push(cost);
    }

    /// Read this process's resident set size for the frame about to be drawn.
    pub fn sample_memory(&mut self) {
        self.memory = memory::resident();
    }

    /// Where the viewport currently starts.
    pub fn position(&self) -> Position {
        self.position
    }

    /// Whether the viewport is moving itself to what just changed.
    pub fn following(&self) -> bool {
        self.following
    }

    /// The message the footer is carrying, if any.
    pub fn notice(&self) -> Option<&str> {
        self.flash
            .as_ref()
            .map(|(message, _, _)| message.as_str())
            .or(self.notice.as_deref())
    }

    /// The voice it is carrying that in.
    pub fn voice(&self) -> Option<Voice> {
        self.flash
            .as_ref()
            .map(|(_, _, voice)| *voice)
            .or_else(|| self.notice.as_ref().map(|_| Voice::Alert))
    }

    /// Show `message` over what the footer holds, until `until`.
    pub fn flash(&mut self, message: impl Into<String>, until: Instant, voice: Voice) {
        self.flash = Some((message.into(), until, voice));
    }

    /// When the message on the footer stops being owed the line, if there is one.
    pub fn flash_until(&self) -> Option<Instant> {
        self.flash.as_ref().map(|(_, until, _)| *until)
    }

    /// Stop showing it. The shell decides when, since a spent message is still
    /// drawn while it leaves.
    pub fn clear_flash(&mut self) {
        self.flash = None;
    }

    /// Record that the watch has stopped, so the header stops claiming
    /// otherwise. Called *with* [`App::warn`], which carries the cause: state
    /// belongs on the header and advice on the footer. One direction only.
    pub fn watch_lost(&mut self) {
        self.mode = Mode::Lost;
    }

    /// Record that something went wrong without giving up the screen. A runtime
    /// measured in days makes every transient failure a certainty.
    pub fn warn(&mut self, message: impl Into<String>) {
        self.notice = Some(message.into());
    }

    /// Which rows the next collect resolves, from the loop that holds the pointer.
    pub fn select(&mut self, span: Option<(usize, usize)>) {
        self.selecting = span;
        if span.is_none() {
            // The answer goes with the span: the resolve below only runs on a collect.
            self.resolved = false;
        }
    }

    /// Whether the last collect resolved the span it was given to any lines.
    pub fn holds_a_selection(&self) -> bool {
        self.resolved
    }

    /// Queue `lines`, replacing rather than adding: one send leaves a batch, so a
    /// release over blank rows retires the one before it. An empty write clears.
    pub fn send(&mut self, lines: &[String]) {
        self.sending = lines
            .iter()
            .any(|line| !line.is_empty())
            .then(|| Sending::lines(lines));
    }

    /// Taken, so what was asked for is sent once.
    pub fn take_sending(&mut self) -> Option<Sending> {
        self.sending.take()
    }

    /// Drop the current message, because the frame it described has passed.
    pub fn clear_notice(&mut self) {
        self.notice = None;
    }

    /// Whether the reader has asked for the staged run.
    pub fn staged(&self) -> bool {
        self.staged
    }

    /// Replace the notes the next collect places, with what the store lists.
    pub fn set_notes(&mut self, notes: Vec<Note>) {
        self.notes = notes;
    }

    /// The chrome for this frame.
    pub fn chrome(
        &self,
        worktree: &str,
        branch: Option<&str>,
        pointing: Pointing,
        elsewhere: usize,
        root: &str,
    ) -> Chrome {
        let Pointing {
            pressed,
            gripped,
            hovered,
            scrolling,
            selected,
        } = pointing;
        Chrome {
            pressed,
            selected,
            // `Some` even at zero: that is the only acknowledgment pressing
            // `a` on a worktree with nothing staged can give.
            staged: self.staged.then_some(self.staged_files),
            elsewhere,
            gripped,
            hovered,
            scrolling,
            notes: self.note_count,
            worktree: worktree.to_owned(),
            branch: branch.map(str::to_owned),
            mode: self.mode,
            notice: self.notice().map(str::to_owned),
            voice: self.voice(),
            following: self.following,
            masthead: self.masthead,
            rail: self.rail,
            icons: self.icons,
            links: self.links,
            root: root.to_owned(),
            sheet: self.sheet,
            frame: self.frames.percentile(0.99),
            memory: self.memory,
        }
    }

    /// Record what changed most recently, and move to it if following (I5).
    pub fn follow(&mut self, path: &str, frame: &Frame) -> bool {
        // Stored even while disengaged, so `f` has somewhere to jump to.
        self.newest = Some(path.to_owned());
        self.following && self.jump_to_newest(frame)
    }

    /// Move the viewport to the newest changed file, if it is still one.
    fn jump_to_newest(&mut self, frame: &Frame) -> bool {
        let Some(newest) = self.newest.as_deref() else {
            return false;
        };
        // Linear over the changed files rather than the worktree, once per
        // tick: string comparison against a list already in memory.
        let Some(file) = frame
            .files()
            .iter()
            .position(|change| change.path == newest)
        else {
            return false;
        };
        self.jump_to(file);
        // On a diff running to several screens the heading and the change are
        // not the same place, and I5 promises the change.
        self.landing = true;
        // A jump moves the diff, so the map follows it again.
        self.list_follows = true;
        true
    }

    /// Whether the viewport still points at the file [`Self::newest`] names.
    /// The guard on an owed landing; see the call site in [`Self::view`].
    fn still_the_followed_file(&self, frame: &Frame) -> bool {
        let Some(newest) = self.newest.as_deref() else {
            return false;
        };
        frame
            .files()
            .get(self.position.file)
            .is_some_and(|change| change.path == newest)
    }

    /// Apply one intention.
    pub fn apply(&mut self, action: Action, frame: &mut Frame, height: usize) -> Result<bool> {
        // Once, above the match, rather than repeated in each arm that moves the view.
        if action.is_manual_scroll() {
            self.following = false;
            // And an owed landing is settled, which belongs here for the reason the
            // line above does.
            self.landing = false;
            // And the map is handed back.
            self.list_follows = true;
        }

        match action {
            Action::Quit => return Ok(false),
            // `Esc` leaves the frontmost thing, and the sheet is a thing. Reported from
            // a real pane: a reader pressed `Esc` to put the help away and the monitor
            // exited.
            Action::Escape if self.sheet.is_some() => self.sheet = None,
            Action::Escape => return Ok(false),
            Action::Redraw => {}
            // Re-engaging jumps rather than arming: `less +F` goes to the end when you
            // ask it to follow, and a reader who presses `f` is asking to see what
            // changed, not to wait for the next thing that does.
            Action::ToggleFollow => {
                self.following = !self.following;
                if self.following {
                    self.jump_to_newest(frame);
                } else {
                    // Disengaging settles an owed landing.
                    self.landing = false;
                }
            }
            // No jump, unlike follow. Re-engaging follow is a move as well as a state
            // change because a reader asking to follow is asking to see what changed.
            Action::ToggleMasthead => self.masthead = !self.masthead,
            // The same answer one region over.
            Action::ToggleRail => self.rail = !self.rail,
            // No jump and no clamp here, which is the arm doing the least of the four
            // and is deliberate.
            Action::ToggleSingle => self.single = !self.single,
            // The reflow changes what a screenful is; see [`Self::screenful`].
            Action::ToggleWrap => self.wrap = !self.wrap,
            // Display rows too, and the marks stay: a hidden note is still on its
            // line, and a click there still withdraws it.
            Action::ToggleNotes => self.notes_shown = !self.notes_shown,
            // The one toggle that changes what the frame *walks*.
            Action::ToggleStaged => {
                self.staged = !self.staged;
                frame.show_staged(self.staged);
                self.position = Position::default();
                // And the frame is walked here, which no other toggle needs.
                if let Err(e) = frame.advance() {
                    self.warn(e.to_string());
                }
            }
            // No jump and no move at all, which is one better than the masthead: that
            // toggle resizes the diff's region, and this one draws over rows the diff
            // keeps.
            Action::ToggleSheet => {
                self.sheet = match self.sheet {
                    None => Some(0),
                    Some(page) if page + 1 < self.sheet_pages => Some(page + 1),
                    Some(_) => None,
                };
            }
            // The control means close, where `?` means the sheet, which is why B13
            // needs a second variant.
            Action::CloseSheet => self.sheet = None,
            Action::Scroll(rows) => {
                self.scroll(rows, frame)?;
            }
            // Moves the window and nothing else, which is the whole of
            // `SPEC.md` §11.1's ruling: the diff does not move, follow is not
            // disengaged (see `Action::is_manual_scroll`), and `anchored` is
            // untouched because that word is about how the *diff's* position was
            // reached.
            Action::ScrollList(rows) => {
                self.browse(self.list_top.saturating_add_signed(rows), frame);
            }
            // Dragging the list's own bar. The fraction is resolved against the
            // changed-file count here rather than in `input`, which has no frame to
            // ask.
            Action::ListTo(at) => {
                // The same ceiling `browse` clamps with, for the same reason: the track
                // maps onto travel, and travel is how far the window can actually go
                // rather than how many files there are.
                let travel = crate::view::last_top(frame.files(), self.list_rows.max(1));
                self.browse(scaled(at, travel), frame);
            }
            // A click on a listed file, or one of the digits `1`-`6`.
            Action::ListRow(offset) => {
                // Resolved through the list's own plan, not by adding the offset to the
                // window's first file.
                let offset = usize::from(offset);
                if offset >= self.list_rows {
                    return Ok(true);
                }
                if let Some(file) =
                    crate::view::file_at(frame.files(), self.list_top, self.list_rows, offset)
                {
                    self.jump_to(file);
                }
            }
            // One rule: step the file index, land on the heading, do nothing when there
            // is no such file.
            Action::File(step) => {
                if let Some(file) = self.position.file.checked_add_signed(step)
                    && file < frame.files().len()
                {
                    self.jump_to(file);
                }
            }
            // Dragging the diff's bar, which counts rows, so this resolves a
            // row of the whole diff back into the file it falls inside and the
            // offset within it.
            Action::DiffTo(at) => self.diff_to(at, height, frame)?,
            // A page keeps one row of overlap, which is what stops a reader
            // losing their place at the seam between two screens.
            Action::Page(pages) => {
                self.step_by(pages, self.screenful(height).saturating_sub(1), frame)?;
            }
            // And a half page keeps none, which is not an inconsistency with the arm
            // above.
            Action::HalfPage(halves) => {
                self.step_by(halves, self.screenful(height) / 2, frame)?;
            }
            // The first row of what the reader can reach, which is the first changed
            // file unpinned and the pinned file's own heading under B16.
            Action::Top => {
                self.jump_to(if self.single { self.position.file } else { 0 });
            }
            // The last *file*, from its top, rather than the last row of the
            // whole diff. Finding that row would mean diffing every file to add
            // up their heights, which is the read I4 forbids.
            Action::Bottom => {
                if let Some(file) = self.pinned_file(frame) {
                    // `true`, unlike every other jump on this map, and it is what makes
                    // the resting row survive a stale height.
                    self.anchored = true;
                    // The resting row rather than the file's height, and the difference
                    // is a whole batch of keystrokes.
                    let span = crate::view::span_in(frame, file)?;
                    self.position = Position {
                        file,
                        // Unchanged by B19, and that is a finding rather than an
                        // oversight.
                        row: span.saturating_sub(height),
                    };
                } else {
                    self.jump_to(frame.files().len().saturating_sub(1));
                }
            }
        }
        Ok(true)
    }

    /// The file a pin is on, resolved against the files that actually exist.
    /// Clamped rather than refused: [`vigia_core::Frame::rows_of`] panics on a
    /// stale index, and a monitor left open sits on a changed set that moves.
    fn pinned_file(&self, frame: &Frame) -> Option<usize> {
        let files = frame.files().len();
        (self.single && files > 0).then(|| self.position.file.min(files - 1))
    }

    /// Put the viewport at the top of `file`, which is what a jump means.
    fn jump_to(&mut self, file: usize) {
        self.anchored = false;
        self.position = Position { file, row: 0 };
    }

    /// Move the list's window, and take the map over only if it moved.
    fn browse(&mut self, to: usize, frame: &Frame) {
        // The list's own ceiling, not `files - rows`.
        let bound = crate::view::last_top(frame.files(), self.list_rows.max(1));
        let moved = to.min(bound);
        if moved != self.list_top {
            self.list_top = moved;
            self.list_follows = false;
        }
    }

    /// Move `count` steps of `rows` each, for the actions measured in screens
    /// rather than in rows.
    fn step_by(&mut self, count: isize, rows: usize, frame: &mut Frame) -> Result<()> {
        let step = isize::try_from(rows.max(1)).unwrap_or(isize::MAX);
        self.scroll(count.saturating_mul(step), frame)
    }

    /// The two directions are deliberately not symmetrical, and the signatures
    /// say so rather than hiding it.
    fn scroll(&mut self, rows: isize, frame: &mut Frame) -> Result<()> {
        self.anchored = true;
        match rows.cmp(&0) {
            std::cmp::Ordering::Equal => Ok(()),
            std::cmp::Ordering::Greater => {
                self.position.row = self.position.row.saturating_add(rows.unsigned_abs());
                Ok(())
            }
            std::cmp::Ordering::Less => self.up(rows.unsigned_abs(), frame),
        }
    }

    /// Resolve a drag on the diff's bar into a position.
    fn diff_to(&mut self, at: u32, height: usize, frame: &mut Frame) -> Result<()> {
        self.anchored = false;
        if let Some(file) = self.pinned_file(frame) {
            let total = crate::view::span_in(frame, file)?;
            self.position = Position {
                file,
                row: self.dragged_to(at, total, height),
            };
            return Ok(());
        }
        let total = crate::view::diff_rows(frame)?;
        let target = self.dragged_to(at, total, height);
        let mut seen = 0;
        let files = frame.files().len();
        let mut position = Position {
            file: files.saturating_sub(1),
            row: 0,
        };
        for file in 0..files {
            let rows = crate::view::block_rows(frame, file)?;
            // Written every iteration rather than only on the hit, which is what makes
            // a target *past* the last row land past the last row.
            position = Position {
                file,
                row: target.saturating_sub(seen),
            };
            if seen + rows > target {
                break;
            }
            seen += rows;
        }
        self.position = position;
        Ok(())
    }

    /// Clamps the walk-back index: it reaches the frame before the collect can.
    fn up(&mut self, rows: usize, frame: &mut Frame) -> Result<()> {
        // The upper clamp B16 needs, and the only one that cannot live in the walk.
        // Scrolling *down* overruns into a row number `View::collect` resolves, so the
        // pin is enforced there by the walk simply not advancing.
        if self.single {
            self.position.row = self.position.row.saturating_sub(rows);
            return Ok(());
        }
        // The walk back reaches the frame before anything has clamped, so it panics on
        // a stale index without the clamp below.
        let files = frame.files().len();
        if files == 0 {
            self.position = Position::default();
            return Ok(());
        }
        self.position.file = self.position.file.min(files - 1);
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
    pub fn view(
        &mut self,
        frame: &mut Frame,
        highlighter: &mut Highlighter,
        history: &History,
        body: Body,
    ) -> Result<View> {
        // Refused is settled, not deferred, and taking it out of the clear below is
        // what makes that true.
        let owed = self.landing && self.still_the_followed_file(frame);
        // Recorded here because this is the one call every frame makes with the
        // pane's own layout in hand. `?` advancing needs to know which page is
        // the last, and `Action` carries no pane; see [`App::sheet_pages`].
        if let Some(pages) = body.sheet_pages {
            self.sheet_pages = pages;
            // And the page is clamped to what this pane has, so the state and the
            // screen agree about which page is up.
            if let (true, Some(page)) = (pages > 0, self.sheet) {
                self.sheet = Some(page.min(pages - 1));
            }
        }
        let view = View::collect_noted(
            frame,
            highlighter,
            history,
            Viewport {
                position: self.position,
                anchored: self.anchored,
                diff_rows: body.diff,
                list_top: self.list_top,
                list_rows: body.list,
                list_follows: self.list_follows,
                // Asked for whenever a bar could be drawn, which is what
                // `body_layout` already decided by giving the diff more than one
                // row. A pane too short for a bar pays nothing.
                measured: body.diff > 1,
                // Only for the file it was armed for, which is the whole of the
                // staleness rule and is one rule rather than a list of the ways an
                // index can go stale.
                landing: owed,
                // Passed through rather than resolved here, for the reason the
                // arm that sets it gives: the walk is where a position meets the
                // file it is inside, so the walk is where a pin can be enforced
                // without asking the frame anything twice.
                single: self.single,
                // From the layout rather than from this method's idea of the
                // pane, which is the reason `body` is a parameter at all: the
                // width a row is laid out against decides where a line breaks,
                // and a second derivation of it here is a pane whose rows were
                // counted against one width and drawn against another.
                width: body.diff_width,
                wrap: self.wrap,
                // Read before the advance below, so the first frame through
                // here is the plain one and every later frame colours. See
                // [`Self::paint`].
                highlight: self.paint != Paint::Never,
            },
            &self.notes,
            self.notes_shown,
        )?;
        self.note_count = (view.notes.placed + view.notes.adrift, view.notes.adrift);
        // Advanced here rather than by the caller, because this is the call
        // that *is* a frame: a shell that painted without coming through here
        // has not drawn a screen.
        self.paint = match self.paint {
            Paint::Never => Paint::Plain,
            Paint::Plain | Paint::Coloured => Paint::Coloured,
        };
        self.position = view.top;
        // A span the walk had no rows for is not a selection, whatever the pointer did.
        self.resolved = self.selecting.is_some_and(|span| view.resolves(span));
        // Cleared only once it was served. A pane with no diff region
        // resolves nothing, and forgetting the request there would leave a
        // reader on the heading for good: the tick that armed it is spent.
        self.landing = owed && !view.landed;
        self.list_rows = body.list;
        // The staged total, below the collect and for the reason `elsewhere` is.
        self.staged_files = frame.files().len() - frame.staged_at();
        // Stored back for the reason the position is: resolution happens once,
        // in the code that knows where the diff landed, and a caller that kept
        // its own answer would be a second rule for the same fact.
        self.list_top = view.list_top;
        self.shown = view.shown();
        Ok(view)
    }

    /// Where a drag on the diff's bar lands, in rows of the diff.
    fn dragged_to(&self, at: u32, total: usize, height: usize) -> usize {
        // Past the end, so the clamp answers and the wrapped bottom's trim runs. A
        // travel lands on a file's first row, where the clamp stands aside for a jump.
        if self.wrap && at >= crate::input::TRACK_SCALE {
            return total;
        }
        scaled(at, total.saturating_sub(self.screenful(height)))
    }

    /// Rows of the diff one screenful holds, which is not `height` when lines wrap,
    /// and is what the bar's travel and a page step are both measured in. Read off
    /// the last frame rather than [`Self::wrap`]: the loop paints once per batch, so
    /// turning wrap off would otherwise grow the step under a screen that held less.
    fn screenful(&self, height: usize) -> usize {
        if self.shown > 0 {
            // Clamped by the pane this step is being taken in.
            self.shown.min(height)
        } else {
            height
        }
    }
}

#[cfg(test)]
mod tests {
    //! What this type turns state into, which no rendering test can reach.

    use super::*;
    // Named here rather than at the top of the file: since the four pointer facts
    // travel as one [`Pointing`], the module itself has no use for either type and
    // this is the only place that spells a mark out.
    use crate::input::{Grabbed, Hovered};

    #[test]
    fn the_chrome_carries_every_gesture_mark_it_is_handed() {
        // The wire nothing else covers, and it is invisible from both ends.
        let app = App::new();
        let chrome = app.chrome(
            "fixture",
            None,
            Pointing {
                pressed: Some((79, 5)),
                gripped: Some(Grabbed::Diff),
                hovered: Some(Hovered::Button(79, 19)),
                selected: None,
                scrolling: Some((Grabbed::List, -1)),
            },
            0,
            "",
        );

        assert_eq!(chrome.pressed, Some((79, 5)), "the pressed cell");
        assert_eq!(chrome.gripped, Some(Grabbed::Diff), "the dragged bar");
        assert_eq!(
            chrome.hovered,
            Some(Hovered::Button(79, 19)),
            "the hover mark"
        );
        assert_eq!(
            chrome.scrolling,
            Some((Grabbed::List, -1)),
            "the scrolled bar"
        );
    }

    #[test]
    fn a_shell_starts_watching_and_a_lost_watch_is_one_way() {
        // Asserted through `chrome`, which is the only way the mode leaves this
        // type and therefore the only path that can be wrong. A bare accessor
        // beside it would let this pass while the chrome dropped the field.
        let mut app = App::new();
        assert_eq!(
            app.chrome("fixture", None, Pointing::default(), 0, "").mode,
            Mode::Watching
        );

        app.watch_lost();
        assert_eq!(
            app.chrome("fixture", None, Pointing::default(), 0, "").mode,
            Mode::Lost
        );

        // One way, and asserted rather than left implied by the absence of a setter.
        // Nothing can revive a watch: the one handle that unblocks the watcher makes
        // `next_tick` return `None` permanently.
        app.clear_notice();
        app.warn("a file vanished between being named and being read");
        assert_eq!(
            app.chrome("fixture", None, Pointing::default(), 0, "").mode,
            Mode::Lost
        );
    }

    #[test]
    fn the_chrome_carries_the_branch_it_was_handed() {
        // The branch is deliberately not this type's state: it is read per frame
        // and passed in, so the only thing here is that it travels unchanged and
        // that nothing invents one when there is none.
        let app = App::new();
        assert_eq!(
            app.chrome("fixture", Some("main"), Pointing::default(), 0, "")
                .branch
                .as_deref(),
            Some("main")
        );
        assert_eq!(
            app.chrome("fixture", None, Pointing::default(), 0, "")
                .branch,
            None
        );
    }
}
