//! Everything the shell remembers between frames, and the arithmetic on it.
//!
//! Which is very little on purpose. The diffs live in [`vigia_core::Frame`], the
//! cells live in the buffer, and what is left here is a scroll position and one
//! optional message. A monitor with more state than that has started becoming a
//! reviewer.

use std::time::Duration;

use vigia_core::{Frame, Highlighter, History, Result, Samples};

use crate::input::Action;
use crate::memory;
use crate::render::{Body, Chrome, Mode};
use crate::view::{Position, View, Viewport, rows_in};

/// Completed frames the status bar's frame time is taken over.
///
/// **A hundred and twenty-eight, and the number is the statistic.** `SPEC.md`
/// §5.1 draws a p99 because I9 *is* a p99, and §7 records why a small buffer
/// cannot carry one: at 30 samples a nearest-rank p99 is just the maximum, so
/// the readout would report the worst frame in the window and never anything
/// else. At 128 the p99 is rank 127, which excludes exactly one outlier.
///
/// That is the behaviour a monitor wants rather than an arbitrary cutoff. §10
/// measures a first-touch parse at **60.97ms** against a 16ms budget, once, on a
/// path §7 puts outside I9 by definition. A window that let one such frame sit
/// on the readout would spend the next two minutes reporting a breach that
/// happened once and is not coming back; two of them in 128 frames is a real
/// problem and still shows.
///
/// Bounded on purpose, which is I3's business: at 128 durations this is two
/// kilobytes allocated once per session and never grown.
const FRAME_SAMPLES: usize = 128;

/// The shell's state.
#[derive(Debug, Clone)]
pub struct App {
    /// Top of the viewport.
    position: Position,
    /// What the footer should say instead of the key hints.
    notice: Option<String>,
    /// Whether the viewport moves itself to what just changed.
    following: bool,
    /// Whether the current position was reached by **scrolling** rather than by a
    /// jump, which is what decides whether the viewport may back up to fill the
    /// pane.
    ///
    /// A jump is a claim about what belongs at the *top*: follow puts the file
    /// that just changed there and `G` puts the last file there. Backing up to
    /// fill a short tail would move that file off the top row and make a reader
    /// hunt for what the jump was for. Scrolling makes no such claim, and running
    /// off the end of the diff into a half-blank pane is what
    /// [#59](https://github.com/breferrari/vigia/issues/59) reported.
    ///
    /// The two are indistinguishable from a [`Position`], so it is carried here
    /// rather than inferred in [`View::collect`].
    anchored: bool,
    /// The last path a tick named, whether or not it was followed.
    ///
    /// Kept while disengaged so `f` can jump to the newest change rather than
    /// waiting for the next one, which is what `less +F` does and what
    /// `SPEC.md` §11.1 rules. One path, replaced per tick: bounded by one
    /// string rather than by the session, so I3 never sees it.
    newest: Option<String>,
    /// First file the pinned list shows.
    ///
    /// A second window onto one file list, and deliberately **not** derived from
    /// [`Self::position`]. It tracks the diff on its own, so most of the time it
    /// is whatever [`View::collect`] resolved it to; what it carries between
    /// frames is the one thing that walk cannot know, which is that a reader
    /// moved it themselves with `J` and has not been overtaken yet.
    list_top: usize,
    /// Whether the watch is still live, which the header draws as a word.
    ///
    /// [`Mode::Watching`] is `Default`, and unlike `following` that is the right
    /// answer rather than an accident: the shell arms a watch on every path that
    /// reaches a screen, and a failure to arm arrives as its own wake and sets
    /// this. See [`Mode::Watching`] for why there is no third value for the
    /// microseconds before arming.
    mode: Mode,
    /// What recent frames cost, which the status bar draws the p99 of.
    ///
    /// Here rather than in [`crate::run`]'s loop, and that placement is the
    /// whole reason the readout is gated. `SPEC.md` §7's rule is that a stage
    /// left outside a gate is a stage nothing can regress you on, and this repo
    /// learned it by leaving `render` outside every budget for two phases. Every
    /// caller that builds a [`Chrome`] gets the readout for free by holding it
    /// here, so `tests/budgets.rs` and `tests/soak.rs` measure the shipped
    /// screen rather than a screen with the readout taken out.
    frames: Samples,
    /// Resident set size as of the last frame that sampled it.
    ///
    /// A stored value rather than a read inside [`App::chrome`], because chrome
    /// is built more than once per frame and on some input paths that draw
    /// nothing. One read per painted frame is the claim `SPEC.md` §5.1 makes and
    /// this is what makes it true.
    memory: Option<u64>,
}

impl Default for App {
    /// Hand-written for one field, and only that field.
    ///
    /// [`Samples`] has no `Default` because a zero-capacity ring is a panic
    /// waiting to happen, so it takes its capacity at construction. Everything
    /// else here is genuinely the derived answer, and the one decision that is
    /// *not* a default stays in [`App::new`] where the comment can argue for it.
    fn default() -> Self {
        Self {
            position: Position::default(),
            notice: None,
            following: false,
            // The opening position is the top of the diff, which is where a jump
            // would have put it, so nothing is owed a back-up before the reader
            // has moved.
            anchored: false,
            list_top: 0,
            newest: None,
            mode: Mode::default(),
            frames: Samples::new(FRAME_SAMPLES),
            memory: None,
        }
    }
}

impl App {
    /// A shell looking at the top of the diff, and following.
    ///
    /// Not `Self::default()`, and the difference is I5 rather than style:
    /// follow is **on** before anything is touched, because a monitor that
    /// needs a keypress to show the current state is not a monitor. `Default`
    /// gives `false` for a bool and cannot be made to say otherwise without
    /// hand-writing the impl, so the honest place for the decision is here.
    pub fn new() -> Self {
        Self {
            following: true,
            ..Self::default()
        }
    }

    /// Record what one whole frame cost, once it is on screen.
    ///
    /// **The whole turn of the loop**, which `SPEC.md` §5.1 rules and which is
    /// wider than it first looks: the wake, the drain, every
    /// [`vigia_core::Frame::advance`] in the batch, the collect, and the paint.
    /// A number that timed only the diff would be reporting the cheapest third
    /// of the work while sitting on a status bar that says `frame`.
    ///
    /// Called *after* the paint, so what it records is complete. The consequence
    /// is that the drawn p99 is always the **previous** frames' and never this
    /// one's, which is not a rounding error but the only thing a frame can
    /// honestly say about itself: it cannot include its own paint in what that
    /// paint draws.
    pub fn record_frame(&mut self, cost: Duration) {
        self.frames.push(cost);
    }

    /// Read this process's resident set size for the frame about to be drawn.
    ///
    /// Called *before* the paint and inside the timed region, which is both
    /// halves of the seam this readout has: the drawn number is current rather
    /// than one frame old, and the read's own cost lands in the frame time that
    /// [`App::record_frame`] measures and the budget gates assert on. A readout
    /// measured outside the thing it reports is the failure `SPEC.md` §7 names.
    ///
    /// `None` on a platform with no cheap answer, which draws nothing. See
    /// [`crate::memory`] for what "cheap" cost to establish.
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
        self.notice.as_deref()
    }

    /// Record that the watch has stopped, so the header stops claiming otherwise.
    ///
    /// Separate from [`App::warn`], and called **with** it rather than instead of
    /// it. The two carry different halves of one event: this one is durable state
    /// and belongs on the header, the notice is which failure caused it and
    /// belongs on the footer, which is `SPEC.md` §11.1's split between state and
    /// advice applied one line up.
    ///
    /// Before they were split, the durable half rode the notice alone and
    /// survived only because the tick that clears a notice can never arrive again
    /// once the watch is gone. That is correct by coincidence rather than by
    /// construction, which is a bug waiting for the coincidence to change.
    ///
    /// One direction only, deliberately. Nothing turns the watch back on: the one
    /// handle that unblocks the watcher makes its `next_tick` return `None`
    /// permanently, so a lost watch is a lost session.
    pub fn watch_lost(&mut self) {
        self.mode = Mode::Lost;
    }

    /// Record that something went wrong without giving up the screen.
    ///
    /// A monitor beside an agent sees a repository mid-`git gc` and files that
    /// vanish between being named and being read. The core calls both ordinary
    /// and says so where it matters: [`vigia_core::Error::MissingBlob`] is
    /// documented as something a monitor must survive rather than exit on, and
    /// [`vigia_core::Frame::advance`] leaves the previous frame intact rather
    /// than blanking a pane. `SPEC.md` §2 is why: a runtime measured in days
    /// makes every transient failure a certainty rather than a possibility.
    pub fn warn(&mut self, message: impl Into<String>) {
        self.notice = Some(message.into());
    }

    /// Drop the current message, because the frame it described has passed.
    pub fn clear_notice(&mut self) {
        self.notice = None;
    }

    /// The chrome for this frame.
    ///
    /// `branch` is a parameter rather than a field because it is not this type's
    /// state: it is read from the repository on the frames that draw it, and
    /// holding it here would give [`App`] a second answer to "which branch"
    /// that nothing keeps in step with `.git/HEAD`. The same reasoning keeps the
    /// highlighter and the history out of here; see [`App::view`].
    pub fn chrome(&self, worktree: &str, branch: Option<&str>) -> Chrome {
        Chrome {
            worktree: worktree.to_owned(),
            branch: branch.map(str::to_owned),
            mode: self.mode,
            notice: self.notice.clone(),
            following: self.following,
            // `None` until a frame has completed, which is the honest first
            // paint: there is no p99 of nothing. The status bar simply has no
            // frame cell on the very first screen, and `Footer::plan` is written
            // so that its arrival cannot move a row underneath the reader.
            frame: self.frames.percentile(0.99),
            memory: self.memory,
        }
    }

    /// Record what changed most recently, and move to it if following.
    ///
    /// This is I5. `path` is [`vigia_core::Tick::newest`], and the whole of
    /// "auto-follows the newest change" is the jump below happening with no
    /// input on the reader's side.
    ///
    /// Takes `&Frame` rather than `&mut Frame` deliberately, and the signature
    /// is the guarantee: following reads the file *list*, which the frame
    /// already holds, so it cannot diff, cannot read and cannot `stat`. A
    /// `&mut` here would make it possible for a later change to start paying
    /// for a jump, which is what I4 forbids and what
    /// `tests/follow.rs::following_a_file_costs_no_diff_and_no_read` gates.
    ///
    /// Returns whether the viewport moved.
    pub fn follow(&mut self, path: &str, frame: &Frame) -> bool {
        // Stored even while disengaged, so `f` has somewhere to jump to.
        self.newest = Some(path.to_owned());
        self.following && self.jump_to_newest(frame)
    }

    /// Move the viewport to the newest changed file, if it is still one.
    ///
    /// The path can name nothing in the diff, and that is ordinary rather than
    /// exceptional: an edit reverted before the tick landed, or a file written
    /// to the bytes the index already holds. There is no newest *change* then,
    /// so the view stays where it is instead of jumping somewhere arbitrary.
    ///
    /// Row zero rather than a computed offset. The heading is the top of what
    /// changed, and finding any other row would mean asking how tall the file
    /// is, which costs the diff this method exists to avoid.
    fn jump_to_newest(&mut self, frame: &Frame) -> bool {
        let Some(newest) = self.newest.as_deref() else {
            return false;
        };
        // Linear over the changed files, not over the worktree, and once per
        // tick rather than once per frame. At the 2000-file shape #19 measures
        // this is string comparison against a list already in memory, where
        // the rejected alternative was 2000 syscalls.
        let Some(file) = frame
            .files()
            .iter()
            .position(|change| change.path == newest)
        else {
            return false;
        };
        self.anchored = false;
        self.position = Position { file, row: 0 };
        true
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
        // Once, above the match, rather than repeated in each arm that moves
        // the view. A rule spelled out four times is a rule that is eventually
        // spelled out three times, and the arm that forgot it would fail
        // silently: follow mode would simply keep dragging the reader back.
        if action.is_manual_scroll() {
            self.following = false;
        }

        match action {
            Action::Quit => return Ok(false),
            Action::Redraw => {}
            // Re-engaging jumps rather than arming: `less +F` goes to the end
            // when you ask it to follow, and a reader who presses `f` is
            // asking to see what changed, not to wait for the next thing that
            // does. `SPEC.md` §11.1.
            Action::ToggleFollow => {
                self.following = !self.following;
                if self.following {
                    self.jump_to_newest(frame);
                }
            }
            Action::Scroll(rows) => {
                self.anchored = true;
                self.scroll(rows, frame)?;
            }
            // A page keeps one row of overlap, which is what stops a reader
            // losing their place at the seam between two screens.
            Action::Page(pages) => {
                self.anchored = true;
                let rows = height.saturating_sub(1).max(1);
                // `as isize` would turn an absurd height into a negative step and
                // send a page-down upwards. A terminal cannot be that tall, which
                // is a reason to convert rather than to rely on it.
                let step = isize::try_from(rows).unwrap_or(isize::MAX);
                self.scroll(pages.saturating_mul(step), frame)?;
            }
            Action::Top => {
                self.anchored = false;
                self.position = Position::default();
            }
            // The last *file*, from its top, rather than the last row of the
            // whole diff. Finding that row would mean diffing every file to add
            // up their heights, which is the read I4 forbids.
            Action::Bottom => {
                self.anchored = false;
                self.position = Position {
                    file: frame.files().len().saturating_sub(1),
                    row: 0,
                };
            }
        }
        Ok(true)
    }

    /// The two directions are deliberately not symmetrical, and the signatures
    /// say so rather than hiding it.
    ///
    /// Down needs neither the frame nor a way to fail: it adds to the offset and
    /// lets [`View::collect`] carry the overrun into the following files, which is
    /// what keeps resolving and drawing to one diff per file instead of two. Up
    /// cannot do that. Stepping off the top of a file means knowing how tall the
    /// one above it is, which is a question only the frame can answer and which
    /// can fail.
    fn scroll(&mut self, rows: isize, frame: &mut Frame) -> Result<()> {
        match rows.cmp(&0) {
            std::cmp::Ordering::Equal => Ok(()),
            std::cmp::Ordering::Greater => {
                self.position.row = self.position.row.saturating_add(rows.unsigned_abs());
                Ok(())
            }
            std::cmp::Ordering::Less => self.up(rows.unsigned_abs(), frame),
        }
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
    /// The highlighter is passed in rather than held here, and that is not an
    /// accident of plumbing. [`App`] is `Clone` and `Default` because it is a
    /// scroll position and a message, which is all `SPEC.md` §6 wants the shell
    /// to remember; a [`Highlighter`] owns seventy-five compiled grammars, and
    /// putting one behind a derived `Clone` leaves a two-megabyte copy one
    /// keystroke away from being made by accident.
    ///
    /// The history is passed in for the same reason and with the same force.
    /// It is `vigia_core`'s, it is bounded by I10 rather than by anything here,
    /// and a copy of it behind [`App`]'s derived `Clone` would be a second
    /// answer to "what changed recently" that nothing keeps in step with the
    /// first.
    /// `body` is [`crate::render::body_layout`]'s answer, so the two regions are
    /// sized by one rule rather than by this method's idea of one.
    pub fn view(
        &mut self,
        frame: &mut Frame,
        highlighter: &mut Highlighter,
        history: &History,
        body: Body,
    ) -> Result<View> {
        let view = View::collect(
            frame,
            highlighter,
            history,
            Viewport {
                position: self.position,
                anchored: self.anchored,
                diff_rows: body.diff,
                list_top: self.list_top,
                list_rows: body.list,
            },
        )?;
        self.position = view.top;
        // Stored back for the reason the position is: resolution happens once,
        // in the code that knows where the diff landed, and a caller that kept
        // its own answer would be a second rule for the same fact.
        self.list_top = view.list_top;
        Ok(view)
    }
}

#[cfg(test)]
mod tests {
    //! What this type turns state into, which no rendering test can reach.
    //!
    //! `tests/render.rs` proves a chrome carrying [`Mode::Lost`] draws
    //! `not watching`, and it builds that chrome **by hand**. So nothing there
    //! says the shell ever produces one, and nothing there says these fields
    //! survive the trip. Mutating [`App::watch_lost`] into an empty body left
    //! the entire suite green, which is what this closes.
    //!
    //! Beside the code rather than in `tests/`, the way `terminal.rs` and
    //! `view.rs` already keep theirs: this is arithmetic on state and needs no
    //! repository, no terminal and no fixture.

    use super::*;

    #[test]
    fn a_shell_starts_watching_and_a_lost_watch_is_one_way() {
        // Asserted through `chrome`, which is the only way the mode leaves this
        // type and therefore the only path that can be wrong. A bare accessor
        // beside it would let this pass while the chrome dropped the field.
        let mut app = App::new();
        assert_eq!(app.chrome("fixture", None).mode, Mode::Watching);

        app.watch_lost();
        assert_eq!(app.chrome("fixture", None).mode, Mode::Lost);

        // One way, and asserted rather than left implied by the absence of a
        // setter. Nothing can revive a watch: the one handle that unblocks the
        // watcher makes `next_tick` return `None` permanently. A later
        // convenience that reset this alongside a notice is exactly how a still
        // picture would start claiming to be live again, and the two are next to
        // each other precisely because they arrive from one event.
        app.clear_notice();
        app.warn("a file vanished between being named and being read");
        assert_eq!(app.chrome("fixture", None).mode, Mode::Lost);
    }

    #[test]
    fn the_chrome_carries_the_branch_it_was_handed() {
        // The branch is deliberately not this type's state: it is read per frame
        // and passed in, so the only thing here is that it travels unchanged and
        // that nothing invents one when there is none.
        let app = App::new();
        assert_eq!(
            app.chrome("fixture", Some("main")).branch.as_deref(),
            Some("main")
        );
        assert_eq!(app.chrome("fixture", None).branch, None);
    }
}
