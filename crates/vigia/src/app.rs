//! Everything the shell remembers between frames, and the arithmetic on it.
//!
//! Which is very little on purpose. The diffs live in [`vigia_core::Frame`], the
//! cells live in the buffer, and what is left here is a scroll position and one
//! optional message. A monitor with more state than that has started becoming a
//! reviewer.

use std::time::Duration;

use vigia_core::{Frame, Highlighter, History, Result, Samples};

use crate::input::{Action, Grabbed, Hovered};
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

/// A track fraction resolved against a count.
///
/// [`crate::input::TRACK_SCALE`] is the denominator the input layer reports in,
/// because it has no frame to ask how many files there are. Saturating at the
/// last index rather than wrapping: a drag to the very bottom of a track is a
/// request for the end, not for one past it.
fn scaled(at: u32, count: usize) -> usize {
    if count == 0 {
        return 0;
    }
    ((u64::from(at) * count as u64) / u64::from(crate::input::TRACK_SCALE)) as usize
}

/// How far a shell has got through its opening two frames.
///
/// See [`App::paint`]. The order is the lifecycle and the middle value is the
/// one that carries work: it is a *debt*, and [`App::owes_repaint`] is what
/// collects it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Paint {
    /// Nothing drawn yet. The next frame draws plain, which is what I7 buys.
    Never,
    /// The plain frame is on screen and a coloured one is owed.
    Plain,
    /// Steady state: every frame from here parses.
    Coloured,
}

/// The shell's state.
#[derive(Debug, Clone)]
pub struct App {
    /// Top of the viewport.
    position: Position,
    /// What the footer should say instead of the key hints.
    notice: Option<String>,
    /// Whether the viewport moves itself to what just changed.
    following: bool,
    /// Whether the masthead is drawn, which `m` toggles.
    ///
    /// **Hidden by default**, which is
    /// [#204](https://github.com/breferrari/vigia/issues/204) reversing the
    /// ruling the toggle shipped under, from use and by the reader who asked for
    /// the toggle. The band costs four rows of the thing this tool exists to
    /// show, and an element that costs them should be asked for rather than
    /// dismissed. On is a keystroke rather than a setting, which is `SPEC.md`
    /// §11.2 B6 keeping this tool configured by gesture rather than by file.
    ///
    /// What the first ruling was right about is that an element nobody discovers
    /// may as well not exist. That is paid in the README's key table, which named
    /// neither the band nor `m`, rather than in the hint bar, which
    /// [#121](https://github.com/breferrari/vigia/issues/121) and
    /// [#147](https://github.com/breferrari/vigia/issues/147) leave where it is.
    masthead: bool,
    /// Which page of the gestures sheet is drawn, and `None` when it is not.
    ///
    /// **Retained here rather than lived for one frame, and that is the whole
    /// reason it is a field.** The pane wakes on filesystem events, so an agent's
    /// write redraws the frame underneath the sheet; a sheet that were not carried
    /// between frames would be dismissed at random by somebody else's build,
    /// which `SPEC.md` §11.2's B12 names as the constraint most likely to be
    /// missed. Off by default for the reason every reader starts with the diff
    /// rather than with instructions about it.
    ///
    /// **A page rather than a flag since `SPEC.md` §11.2 B13**
    /// ([#286](https://github.com/breferrari/vigia/issues/286)). On every pane
    /// whose sheet is one page the two are the same type wearing different names,
    /// which is the point: `?` opens `Some(0)` and closes from it, exactly as the
    /// flag did.
    sheet: Option<usize>,
    /// Pages the sheet has on the pane the last frame was drawn for.
    ///
    /// **The one thing `?` cannot answer on its own.** Advancing needs to know
    /// which page is the last, that is a fact about the pane's layout rather than
    /// about this state, and [`Action`] carries no pane. So the layout hands it
    /// over: [`crate::body_layout`] measures it every frame and [`App::view`]
    /// records it, which is the same frame the reader is looking at when they
    /// press.
    ///
    /// **One frame stale in exactly one case, and that case is handled where it
    /// lands.** A pane resized between the last paint and the next press can be
    /// asked for a page it no longer has, and `sheet_plan` clamps to the last one
    /// rather than panicking or closing a sheet nobody dismissed.
    ///
    /// One by default, so an [`App`] nobody has painted yet still opens and closes
    /// a sheet on two presses.
    sheet_pages: usize,
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
    /// Whether the next frame owes the position its row, because follow placed
    /// it.
    ///
    /// **A debt, the way [`Self::paint`] is one**, and for the same reason:
    /// [`Self::follow`] takes `&Frame` so that following cannot diff, cannot
    /// read and cannot `stat` (I4), which leaves it able to name the file and
    /// nothing else. Where in that file the change sits is a question only a
    /// diff answers, and the frame's diff for the one file that just changed is
    /// the *previous* tick's until the draw re-reads it. So the jump names the
    /// file here and [`View::collect`] resolves the row while it has the fresh
    /// diff in hand, for nothing.
    ///
    /// Set by [`Self::jump_to_newest`] alone. The digits, a list click and
    /// `n`/`p` all go through [`Self::jump_to`] and keep the heading, which is
    /// `SPEC.md` §11.1's ruling for them and is unchanged.
    landing: bool,
    /// First file the pinned list shows.
    ///
    /// A second window onto one file list, and deliberately **not** derived from
    /// [`Self::position`]. It tracks the diff on its own, so most of the time it
    /// is whatever [`View::collect`] resolved it to; what it carries between
    /// frames is the one thing that walk cannot know, which is that a reader
    /// moved it themselves with `J` and has not been overtaken yet.
    list_top: usize,
    /// Whether the list's window is still the diff's to move.
    ///
    /// **The list's own `following`, and it exists for the same reason.** The
    /// window tracks the diff by default, so the region is correct untouched the
    /// way I5 requires; `J` takes it over, and anything that moves the *diff*
    /// hands it back. Without the second half a reader who browsed once would
    /// have a map that never agreed with the diff again; without the first, `J`
    /// does nothing at all, because the next frame drags the window straight
    /// back onto the current file.
    ///
    /// True at rest, which `Default` gives, and which is the right answer here
    /// rather than an accident: a monitor's map follows what it is a map of.
    list_follows: bool,
    /// Rows the pinned list had on the frame that was last drawn.
    ///
    /// Carried only so a list gesture can be clamped against the window's real
    /// bound rather than against the file count. Without it, `J` at the last
    /// window moves `list_top` past where `View::take_list` will put it, so a
    /// keypress that changes nothing on screen still takes the map over. Zero
    /// until the first frame, which is the honest answer before anything has
    /// been laid out.
    list_rows: usize,
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
    /// How far this shell has got through its opening two frames.
    ///
    /// **The whole of I7's fix.** `syntect` compiles a grammar's patterns on
    /// first use at 74-362ms, and I7 gives the whole of startup 50ms, so a first
    /// frame that parsed was 105.03ms measured over the hundred-file fixture.
    /// The reader spent all of it looking at a blank alternate screen, because
    /// `Session::enter` runs before the draw. So the first frame draws plain and
    /// the next one colours.
    ///
    /// **Three states rather than a bool, because the middle one is a debt.**
    /// A bool can say "has drawn", which is enough to decide what *this* frame
    /// does and not enough to say that another frame is owed. Without
    /// [`Paint::Plain`] the second draw is a convention held up by statement
    /// order in [`crate::run`], and deleting that statement left the entire
    /// suite green while the product sat on a permanently uncoloured screen for
    /// any tree nobody was writing to — an I5 failure, since a monitor is meant
    /// to be correct untouched. Found by mutation.
    ///
    /// Here rather than in [`crate::run`]'s loop for the reason
    /// [`Self::frames`] gives further up: every caller that builds a view gets
    /// the behaviour, so `tests/soak.rs` and the budget gates drive the shipped
    /// opening rather than one with the rule taken out.
    ///
    /// One direction only, and never reset. A second plain frame mid-session
    /// would be a screen losing its colour for no reason a reader could see.
    paint: Paint,
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
            // Genuinely the derived answer since
            // [#204](https://github.com/breferrari/vigia/issues/204), unlike
            // `following`: a shell nobody has pressed `m` on draws no band.
            masthead: false,
            // Derived, and for once trivially so: nobody has pressed `?`.
            sheet: None,
            sheet_pages: 1,
            // The opening position is the top of the diff, which is where a jump
            // would have put it, so nothing is owed a back-up before the reader
            // has moved.
            anchored: false,
            list_top: 0,
            list_follows: true,
            list_rows: 0,
            newest: None,
            // Genuinely the derived answer: nothing has been followed, so
            // nothing is owed a row.
            landing: false,
            mode: Mode::default(),
            // Genuinely the derived answer, unlike `following`: a shell that
            // has drawn nothing has drawn nothing.
            paint: Paint::Never,
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

    /// A shell already past its opening two frames, so the next one colours.
    ///
    /// **A test affordance, and `doc(hidden)` because it is one.** [`App::new`]
    /// draws its first frame plain, which is I7's fix and is the shipped
    /// behaviour; a gate that wants a *coloured* screen out of a single
    /// [`App::view`] would otherwise be measuring the one frame that
    /// deliberately does not parse.
    ///
    /// It exists so a test can hold the frame **cold** — every file computed
    /// rather than reused — while the shell is past its first paint, which are
    /// independent axes that a priming frame would collapse into one.
    ///
    /// **Never reach for this in [`crate::run`].** It puts the 105.03ms compile
    /// back on the frame I7 gives 50ms to, and nothing would go red: the gate
    /// that would notice constructs its own `App::new`. Named for what it *is*
    /// rather than for what it is convenient for, so that misuse reads wrong at
    /// the call site.
    #[doc(hidden)]
    pub fn past_first_paint() -> Self {
        Self {
            paint: Paint::Coloured,
            ..Self::new()
        }
    }

    /// Whether a coloured frame is owed for the plain one already on screen.
    ///
    /// **The debt `Self::paint` exists to carry.** `Shell::draw` is
    /// the only caller and it settles it immediately, which is what keeps the
    /// opening two frames one mechanism rather than two statements a future
    /// edit can separate.
    pub fn owes_repaint(&self) -> bool {
        self.paint == Paint::Plain
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
    /// `pressed` is the cell a step button is being held on, which the loop owns
    /// rather than this type: a hold begins and ends on terminal events and has
    /// no bearing on the viewport, so putting it in [`App`] would give the
    /// viewport a field it never reads. It is passed through for the one frame
    /// that draws it.
    pub fn chrome(
        &self,
        worktree: &str,
        branch: Option<&str>,
        pressed: Option<(u16, u16)>,
        gripped: Option<Grabbed>,
        hovered: Option<Hovered>,
        scrolling: Option<(Grabbed, isize)>,
    ) -> Chrome {
        Chrome {
            pressed,
            gripped,
            hovered,
            scrolling,
            worktree: worktree.to_owned(),
            branch: branch.map(str::to_owned),
            mode: self.mode,
            notice: self.notice.clone(),
            following: self.following,
            masthead: self.masthead,
            sheet: self.sheet,
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
    /// **Names the file and owes the row**, which is
    /// [#257](https://github.com/breferrari/vigia/issues/257) and is the one
    /// place this map does not resolve a jump to row zero. The heading is the
    /// top of what changed and not always the change itself, and finding the
    /// row that is means asking how tall the file is, which costs the diff this
    /// method exists to avoid. So it is not asked here: [`Self::landing`] is set
    /// instead, and [`View::collect`] resolves the row from a diff it has
    /// already fetched. Until #257 this landed on the heading and said so.
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
        self.jump_to(file);
        // **And the row is owed.** `jump_to` puts the top of the block on the
        // top row, which is the whole of every other jump on this map and was
        // the whole of this one until
        // [#257](https://github.com/breferrari/vigia/issues/257): on a file
        // whose diff runs to several screens the heading and the change are not
        // the same place, and I5 promises the change. Nothing here can find that
        // row (see [`Self::landing`]), so the request is carried to the frame
        // that can.
        self.landing = true;
        // A jump moves the diff, so the map goes back to following it. Follow
        // mode dragging the view to a file the pinned list was not showing is
        // exactly when a reader most needs the two to agree.
        self.list_follows = true;
        true
    }

    /// Whether the viewport still points at the file [`Self::newest`] names.
    ///
    /// The guard on an owed landing, and the reason is on the call site in
    /// [`Self::view`]. False when the changed set has moved under the position,
    /// which is ordinary rather than exceptional on the pane this tool is for.
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
            // **And an owed landing is settled**, which belongs here for the
            // reason the line above does. A tick and a keystroke coalesce into
            // one batch, so a request armed by the follow can still be
            // unresolved when this runs, and resolving it afterwards draws over
            // the row the reader just asked for.
            //
            // **This predicate is the whole rule**, rather than a clear at each
            // site that moves the view, and the three routes below are why no
            // smaller site covers it. `n` and `p` do not go through
            // `Self::scroll`, and at either end of the changed set they reach no
            // jump either. A digit goes through `Self::jump_to`. A drag on the
            // diff's bar goes through neither and writes a position of its own.
            // `Action::is_manual_scroll` calls all of them a manual scroll, so
            // every gesture that moves the viewport by a reader's intent is one
            // of these.
            self.landing = false;
            // **And the map is handed back.** Every action that reaches here
            // moves the diff, and a reader who moves the diff is asking to see
            // where it went; a window left behind from an earlier `J` would be
            // showing somewhere else with no caret to say so. This is the exact
            // mirror of the line above it: manual scrolling takes the *diff*
            // away from follow mode and gives the *list* back to it.
            self.list_follows = true;
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
                } else {
                    // **Disengaging settles an owed landing.** A tick and the
                    // keystroke can arrive in one batch, so `f` can run with a
                    // request the frame has not resolved yet, and resolving it
                    // afterwards would move the viewport for a reader who has
                    // just asked the view to stop moving itself.
                    self.landing = false;
                }
            }
            // **No jump, unlike follow.** Re-engaging follow is a move as well
            // as a state change because a reader asking to follow is asking to
            // see what changed. Asking for the masthead back is asking for the
            // masthead back: the diff keeps the row it was on, and the region
            // above it grows or goes.
            Action::ToggleMasthead => self.masthead = !self.masthead,
            // **No jump and no move at all**, which is one better than the
            // masthead: that toggle resizes the diff's region, and this one draws
            // over rows the diff keeps. Nothing about the viewport changes, so a
            // reader who opens the sheet and closes it is looking at exactly the
            // screen they left.
            //
            // **`?` advances, and the last page is what closes it**, which is
            // `SPEC.md` §11.2 B13. On a pane whose sheet is one page that is the
            // toggle it has always been. `?` keeps exactly one meaning, *the
            // sheet*, and no other key changes meaning while it is up, which is
            // what keeps B12's reconciliation with B4 intact.
            Action::ToggleSheet => {
                self.sheet = match self.sheet {
                    None => Some(0),
                    Some(page) if page + 1 < self.sheet_pages => Some(page + 1),
                    Some(_) => None,
                };
            }
            // **The control means close, where `?` means the sheet**, which is why
            // B13 needs a second variant. Sending `ToggleSheet` from a click on
            // `✕` made the sheet's only pointer escape *advance*, so a reader on
            // page one of six needed six clicks to get out, and both `SPEC.md`
            // §11.1 and `Action::ToggleSheet`'s own docblock claimed otherwise
            // while it did. Found by #286's adversarial round.
            Action::CloseSheet => self.sheet = None,
            Action::Scroll(rows) => {
                self.scroll(rows, frame)?;
            }
            // **Moves the window and nothing else**, which is the whole of
            // `SPEC.md` §11.1's ruling: the diff does not move, follow is not
            // disengaged (see `Action::is_manual_scroll`), and `anchored` is
            // untouched because that word is about how the *diff's* position was
            // reached.
            //
            // Bounded here only against the file list's length, because the real
            // clamp needs the region's height and `View::take_list` is where that
            // is known. Same division of labour the diff's position already has:
            // this moves a number, the collect resolves it, and the resolved
            // answer comes back through `App::view`.
            // **Only a move that moves something takes the map over.** `K` at
            // the top, or `J` at the last window, changes not one cell on screen,
            // and detaching there leaves a reader with a map that has silently
            // stopped following and no readout saying so: `following` has
            // `follow ▶` in the footer and this has nothing.
            Action::ScrollList(rows) => {
                self.browse(self.list_top.saturating_add_signed(rows), frame);
            }
            // Dragging the list's own bar. The fraction is resolved against the
            // changed-file count here rather than in `input`, which has no frame
            // to ask.
            // Both bars map the track onto **travel** rather than onto the whole,
            // which is the arithmetic the thumb is drawn with. Mapping onto the
            // whole instead leaves the last screenful's worth of track dead: the
            // pointer reaches the bottom and the view is still short of the end.
            Action::ListTo(at) => {
                let travel = frame.files().len().saturating_sub(self.list_rows.max(1));
                self.browse(scaled(at, travel), frame);
            }
            // A click on a listed file, or one of the digits `1`-`6`. Out of
            // range is not a file and so is not a jump: silently doing nothing is
            // right where clamping to the last file would move the diff somewhere
            // nobody pointed at.
            //
            // **Two bounds, and they answer different questions.** The row bound
            // is about *this* screen: `Regions::over_list` already keeps a click
            // inside the region, so it changes nothing for the mouse, but a
            // keystroke has no such filter and a pane short enough to give the
            // list four rows still has a `5` and a `6` on the reader's keyboard.
            // `SPEC.md` §11.1 gives the digits the **drawn window** rather than
            // the changed set, so a digit naming a row that is not on screen
            // names nothing at all.
            //
            // The file bound is about the screen being **out of date**, which is
            // the part reading gets wrong. Given the row bound, no gesture against
            // a freshly drawn list can name a file past the end: the window is
            // never taller than what is left below it. `list_rows` is what the
            // *last frame* drew, though, and the changed set can shrink before the
            // next one: `git reset --hard`, a branch switch, the agent in the
            // other pane reverting its own work. Six rows drawn, two files left,
            // and `6` still passes the row bound. Deleting this line left the
            // whole suite green until `a_digit_after_the_diff_shrank_names_no_file`
            // was written for exactly that, which is how the rationale above it
            // was found to be the wrong one.
            Action::ListRow(offset) => {
                let offset = usize::from(offset);
                let file = self.list_top.saturating_add(offset);
                if offset < self.list_rows && file < frame.files().len() {
                    self.jump_to(file);
                }
            }
            // **One rule: step the file index, land on the heading, do nothing
            // when there is no such file.** Row zero is the heading, which is the
            // resolution a list click and `jump_to_newest` already use, and it
            // costs no diff: nothing here asks how tall anything is, so I4 never
            // sees this.
            //
            // `p` from inside a file goes to the **previous** file rather than to
            // the top of the current one. The pager reflex of "this section
            // first" would make one key mean two things depending on where the
            // viewport happened to be, which `SPEC.md` §11.1 refuses across this
            // whole map, and `g` already reaches a top.
            //
            // Both ends are no-ops for the position, so neither key ever moves
            // the view in the direction opposite to itself. Follow is still
            // disengaged above, for the reason `Action::is_manual_scroll` gives.
            Action::File(step) => {
                if let Some(file) = self.position.file.checked_add_signed(step)
                    && file < frame.files().len()
                {
                    self.jump_to(file);
                }
            }
            // Dragging the diff's bar, which counts **rows**, so this resolves a
            // row of the whole diff back into the file it falls inside and the
            // offset within it.
            //
            // It counted files until 2026-08-02, from when the bar itself did.
            // The bar became row-exact and this did not, so a drag had one
            // landing spot per changed file while the thumb it followed had one
            // per row: on a worktree of three long files the lower bar moved
            // under the pointer and the diff jumped to a heading or did not move
            // at all. Reported from use, which is the fifth time.
            //
            // `Frame::height` is the count the bar already drew itself with, and
            // every span it needs was proved earlier in this same tick, so the
            // walk below reads nothing that this frame has not read already.
            Action::DiffTo(at) => {
                self.anchored = false;
                let total = crate::view::diff_rows(frame)?;
                let target = scaled(at, total.saturating_sub(height));
                let mut seen = 0;
                let files = frame.files().len();
                let mut position = Position {
                    file: files.saturating_sub(1),
                    row: 0,
                };
                for file in 0..files {
                    let rows = crate::view::block_rows(frame, file)?;
                    if seen + rows > target {
                        position = Position {
                            file,
                            row: target - seen,
                        };
                        break;
                    }
                    seen += rows;
                }
                self.position = position;
            }
            // A page keeps one row of overlap, which is what stops a reader
            // losing their place at the seam between two screens.
            Action::Page(pages) => {
                self.step_by(pages, height.saturating_sub(1), frame)?;
            }
            // **And a half page keeps none, which is not an inconsistency with
            // the arm above.** The overlap row exists to leave a reader something
            // shared across the seam; a half page already leaves half the screen
            // standing, so a row taken off the step would be paying twice for one
            // anchor. `less` and vim both move exactly half a window and this is
            // their binding.
            //
            // Floored in both directions rather than rounded, so `d` and then `u`
            // land back where they started on an odd body as well as an even one.
            // The floor under the step itself is [`App::step_by`]'s, and it is
            // what keeps a body two rows tall moving at all.
            Action::HalfPage(halves) => {
                self.step_by(halves, height / 2, frame)?;
            }
            Action::Top => {
                self.jump_to(0);
            }
            // The last *file*, from its top, rather than the last row of the
            // whole diff. Finding that row would mean diffing every file to add
            // up their heights, which is the read I4 forbids.
            Action::Bottom => {
                self.jump_to(frame.files().len().saturating_sub(1));
            }
        }
        Ok(true)
    }

    /// Put the viewport at the top of `file`, which is what a **jump** means.
    ///
    /// **The rule was written out at five call sites and this is the fifth's
    /// doing.** [`App::scroll`] one method down already argues this case for the
    /// other half of the pair: anchoring lives there rather than in each arm
    /// that scrolls, because *"a rule spelled out three times is one an arm
    /// eventually forgets"*. The jump side never got the same treatment, and
    /// adding a sixth arm that restates it is what made the omission worth
    /// closing rather than noting.
    ///
    /// Row zero because a jump lands on the file's **heading**. Finding any
    /// other row means asking how tall the file is, which costs the diff I4
    /// forbids, so this is the resolution every jump on this map shares: `g`,
    /// `G`, a click on a listed file, a digit, and `n`/`p`. **Follow starts
    /// here too and is then moved off it**, which is [`Self::landing`] and
    /// #257; every one of the keys named above keeps the heading, because
    /// `SPEC.md` §11.1 gives them the *file* as their unit.
    ///
    /// `anchored` is cleared because that word means "reached by scrolling", and
    /// a jump is the other thing. See [`App::anchored`] for what it costs to get
    /// wrong: a viewport free to back up and fill a short tail moves the file the
    /// jump was *for* off the top row.
    ///
    /// **[`App::landing`] is not cleared here, and a draft of this cleared it.**
    /// The debt has to be settled by every gesture that moves the viewport, or a
    /// `G` or a digit inherits it and lands mid-file, which `SPEC.md` §11.1 rules
    /// against by name for exactly those keys. That is one predicate rather than
    /// a list of sites, and `Action::is_manual_scroll` is it: every caller of
    /// this but [`App::jump_to_newest`] is one, so a clear here would be a second
    /// statement of a rule already made and could never fire. Mutation testing is
    /// what said so, by leaving the whole suite green without it. (A drag on the
    /// diff's bar is a manual scroll too and settles the debt the same way, but
    /// it does not come through here: it writes a position of its own.)
    ///
    /// The caller picks the index, and that is the whole of what the arms differ
    /// by. Nothing here bounds it: an arm that cannot say which file it means has
    /// nothing to jump to and does not call this.
    fn jump_to(&mut self, file: usize) {
        self.anchored = false;
        self.position = Position { file, row: 0 };
    }

    /// Move the list's window, and take the map over only if it moved.
    ///
    /// **Clamped against the window's real bound**, which is the file count less
    /// the region's height, not less one. `View::take_list` clamps there anyway,
    /// so a gesture past it changes nothing on screen — and detaching the map for
    /// a keypress a reader cannot see is worse than ignoring it, because
    /// `following` has `follow ▶` in the footer to say what it is doing and this
    /// has nothing.
    fn browse(&mut self, to: usize, frame: &Frame) {
        let bound = frame.files().len().saturating_sub(self.list_rows.max(1));
        let moved = to.min(bound);
        if moved != self.list_top {
            self.list_top = moved;
            self.list_follows = false;
        }
    }

    /// Move `count` steps of `rows` each, for the actions measured in screens
    /// rather than in rows.
    ///
    /// The two of them differ in the row count alone, so that is all their arms
    /// state and everything else is here. **Floored at one row**, which is what
    /// keeps a body two rows tall moving under either of them.
    ///
    /// `as isize` would turn an absurd height into a negative step and send a
    /// page-down upwards. A terminal cannot be that tall, which is a reason to
    /// convert rather than to rely on it. That argument covers both callers from
    /// here; it used to sit on one of them and be copied to the other.
    fn step_by(&mut self, count: isize, rows: usize, frame: &mut Frame) -> Result<()> {
        let step = isize::try_from(rows.max(1)).unwrap_or(isize::MAX);
        self.scroll(count.saturating_mul(step), frame)
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
    ///
    /// **Anchoring happens here rather than in each arm that scrolls**, because
    /// [`App::anchored`] is defined as "reached by scrolling rather than by a
    /// jump" and reaching here *is* what scrolling means. It was written out at
    /// three call sites, which is the shape [`App::apply`]'s own header argues
    /// against: a rule spelled out three times is one an arm eventually forgets,
    /// and the arm that forgot it would leave the viewport free to back up and
    /// fill the pane with nothing on screen to say so, which is
    /// [#59](https://github.com/breferrari/vigia/issues/59) returning.
    ///
    /// Above the zero case rather than inside the two that move, so a step that
    /// resolves to no rows still counts as scrolling, exactly as it did when the
    /// callers set it.
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
    /// to remember; a [`Highlighter`] owns a couple of hundred compiled grammars, and
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
        // **Refused is settled, not deferred**, and taking it out of the clear
        // below is what makes that true. A debt the guard refuses is a debt for a
        // file the viewport is no longer on, and the guard is re-read every
        // frame: kept, it fires the moment an index happens to name that path
        // again, on a frame no tick armed. Reachable by opening the sheet, which
        // moves no viewport and whose own ruling says a reader who opens it and
        // closes it sees the screen they left.
        let owed = self.landing && self.still_the_followed_file(frame);
        // **Recorded here because this is the one call every frame makes with the
        // pane's own layout in hand.** `?` advancing needs to know which page is
        // the last, and `Action` carries no pane; see [`App::sheet_pages`].
        //
        // **Whenever the layout measured one, which is every frame.**
        // [`crate::body_layout`] answers with the sheet up or down, and its own
        // docblock carries the reason: the shell drains actions in a batch and
        // paints once at the end of it, so a second `?` in the same wake is
        // measured against the last *draw*. The `Option` is about the three `Body`
        // constructors that have no pane to measure, not about when it is taken.
        if let Some(pages) = body.sheet_pages {
            self.sheet_pages = pages;
            // **And the page is clamped to what this pane has**, so the state and
            // the screen agree about which page is up. `sheet_plan` clamps too, and
            // has to, because it is the last thing standing between a stale index
            // and the tables; what it cannot do is write the answer back. Without
            // this, a pane that shrank its page count left `self.sheet` pointing
            // past the end and every `?` after it walked the gap one dead press at
            // a time while the screen showed the same page.
            // **Not on a pane with no pages at all**, which is a pane below the
            // sheet's own floor: there `pages` is zero, a saturating subtraction
            // makes the clamp read `Some(0)`, and a reader who dragged a pane
            // narrow and back would find themselves on page one of a sheet they
            // had left on page four. Nothing was drawn in between, so nothing
            // asked for the move.
            if let (true, Some(page)) = (pages > 0, self.sheet) {
                self.sheet = Some(page.min(pages - 1));
            }
        }
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
                list_follows: self.list_follows,
                // Asked for whenever a bar could be drawn, which is what
                // `body_layout` already decided by giving the diff more than one
                // row. A pane too short for a bar pays nothing.
                measured: body.diff > 1,
                // **Only for the file it was armed for**, which is the whole of
                // the staleness rule and is one rule rather than a list of the
                // ways an index can go stale. A landing names a *row inside a
                // file* and the position holds an index, and
                // [`vigia_core::Frame::advance`] renumbers every index whenever
                // the changed set moves: a file committed, an edit reverted, a
                // branch switched. Ticks coalesce and only the paint is shared,
                // so an advance can happen between the follow that armed this
                // and the frame that would resolve it, including on a tick that
                // names no path at all and so never reaches [`Self::follow`].
                // A `.git/index` write is exactly that. Resolved against the
                // renumbered index, the viewport lands deep inside whichever
                // file inherited the number, which is worse than the heading it
                // replaced.
                //
                // [`Self::newest`] is the path the last tick named, which the
                // jump was made for whenever there was one, so this is a string
                // compare against a list the frame already holds: no read, no
                // `stat`, no diff, exactly as `follow` itself is. A tick that
                // reached no jump has overwritten it, and the debt is dropped on
                // that alone, which is the conservative direction and what
                // `a_tick_that_follows_nothing_drops_the_landing_the_one_before_it_armed`
                // rules.
                landing: owed,
                // Read before the advance below, so the first frame through
                // here is the plain one and every later frame colours. See
                // [`Self::paint`].
                highlight: self.paint != Paint::Never,
            },
        )?;
        // Advanced here rather than by the caller, because this is the call
        // that *is* a frame: a shell that painted without coming through here
        // has not drawn a screen.
        //
        // **After the `?` on purpose.** A collect that failed drew nothing, so
        // it was not the first paint and the next successful one still is. The
        // shell redraws its previous screen on that path, which on the very
        // first frame is the empty one.
        self.paint = match self.paint {
            Paint::Never => Paint::Plain,
            Paint::Plain | Paint::Coloured => Paint::Coloured,
        };
        self.position = view.top;
        // **Cleared only once it was served.** A pane with no diff region
        // resolves nothing, and forgetting the request there would leave a
        // reader on the heading for good: the tick that armed it is spent.
        self.landing = owed && !view.landed;
        self.list_rows = body.list;
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
    fn the_chrome_carries_every_gesture_mark_it_is_handed() {
        // **The wire nothing else covers, and it is invisible from both ends.**
        // Every one of the thirty-odd `App::chrome` call sites in the suite
        // passes `None` for all four marks, and every render gate builds a
        // `Chrome` literal directly rather than going through here, so dropping
        // a mark on the floor in this function leaves the whole workspace green
        // while the feature does nothing on screen.
        //
        // All four rather than the one #186 added, for the reason
        // `the_takeover_takes_every_step_there_is` covers every `Step`: a gate
        // written for the mark in front of it is a gate the next mark has to
        // remember to extend, and `scrolling` and `gripped` were each added
        // without one.
        let app = App::new();
        let chrome = app.chrome(
            "fixture",
            None,
            Some((79, 5)),
            Some(Grabbed::Diff),
            Some(Hovered::Button(79, 19)),
            Some((Grabbed::List, -1)),
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
            app.chrome("fixture", None, None, None, None, None).mode,
            Mode::Watching
        );

        app.watch_lost();
        assert_eq!(
            app.chrome("fixture", None, None, None, None, None).mode,
            Mode::Lost
        );

        // One way, and asserted rather than left implied by the absence of a
        // setter. Nothing can revive a watch: the one handle that unblocks the
        // watcher makes `next_tick` return `None` permanently. A later
        // convenience that reset this alongside a notice is exactly how a still
        // picture would start claiming to be live again, and the two are next to
        // each other precisely because they arrive from one event.
        app.clear_notice();
        app.warn("a file vanished between being named and being read");
        assert_eq!(
            app.chrome("fixture", None, None, None, None, None).mode,
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
            app.chrome("fixture", Some("main"), None, None, None, None)
                .branch
                .as_deref(),
            Some("main")
        );
        assert_eq!(
            app.chrome("fixture", None, None, None, None, None).branch,
            None
        );
    }
}
