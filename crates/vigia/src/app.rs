//! Everything the shell remembers between frames, and the arithmetic on it.
//!
//! Which is very little on purpose. The diffs live in [`vigia_core::Frame`], the
//! cells live in the buffer, and what is left here is a scroll position and one
//! optional message. A monitor with more state than that has started becoming a
//! reviewer.

use std::time::Duration;

use vigia_core::{Frame, Highlighter, History, Result, Samples};

use crate::input::{Action, Pointing};
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
    /// Whether listed paths carry a file-type icon. Config only; no gesture.
    icons: bool,
    /// Whether listed paths are OSC 8 hyperlinks. Config only; on by default.
    links: bool,
    /// Whether the reader has asked for the pinned list beside the diff.
    ///
    /// **A request rather than a layout**, and the distinction is the one
    /// `Chrome::masthead` and `Body::graph` already draw one region over: this says
    /// what was asked for and `Body::rail` says what the pane could give. A pane
    /// under 134 columns has no room for a rail and this stays true through it, so
    /// narrowing and widening again returns the rail rather than the question.
    ///
    /// Off by default since `SPEC.md` §11.2 **B14**
    /// ([#295](https://github.com/breferrari/vigia/issues/295)). The rail arrived
    /// on its own before that, and the reader whose diff went from 129 planning
    /// columns to 60 had not asked for it.
    rail: bool,
    /// Whether the reader has asked for the diff to show one file at a time.
    ///
    /// `SPEC.md` §11.2 **B16**, from `s`
    /// ([#297](https://github.com/breferrari/vigia/issues/297)). Unlike
    /// [`Self::rail`] there is no pane that cannot honour it, so this is a
    /// request and an answer at once and nothing downstream has to distinguish
    /// them: every pane with a body has a file the viewport is inside, and that
    /// file is the whole of what the pin names.
    ///
    /// **Which file is not stored here**, and that is what keeps this a `bool`.
    /// The pinned file is [`Self::position`]'s, so `n`, `p`, a digit, a click and
    /// a follow move the pin by moving the thing that was already moving, and
    /// there is no second answer to *which file* for the two to disagree over.
    ///
    /// Off by default, which is the derived answer and also the ruled one: a
    /// reader who has pressed nothing gets the diff the tool has always drawn.
    single: bool,
    /// Whether a content line too wide for the pane continues on the row below.
    ///
    /// `SPEC.md` §11.2 **B19**, from `w`
    /// ([#272](https://github.com/breferrari/vigia/issues/272)). Off by default,
    /// and that is a ruling rather than the derived answer: every neighbour that
    /// wraps is given the whole terminal and this one is built for half of it, so
    /// the mode with a price is the one asked for
    /// ([#204](https://github.com/breferrari/vigia/issues/204)'s reasoning about
    /// the masthead, applied to the same reader).
    ///
    /// **Like [`Self::single`] and unlike [`Self::rail`] there is no pane that
    /// cannot honour it**, so this is a request and an answer at once: a pane too
    /// narrow to wrap is a pane too narrow to draw a diff on at all.
    wrap: bool,
    /// Logical rows the last frame actually drew, or zero before the first one.
    ///
    /// **The screenful a page step is measured in**
    /// ([#272](https://github.com/breferrari/vigia/issues/272)), and it exists
    /// because wrapping split one number into two. `height` is the region's
    /// **display** rows, and with `w` on a screen of that many display rows holds
    /// fewer rows of the diff, so `Space`, `d` and `u` stepped over content
    /// nobody had seen. It is the same units bug as the three in `View::collect`
    /// and it is in the one place the walk cannot reach, because a step is taken
    /// before the frame it moves to is built.
    ///
    /// **Read only while wrapping is on**, so every step with `w` off is the
    /// arithmetic every version before this one did, byte for byte.
    ///
    /// Zero before the first frame, which reads as *nobody has drawn yet* and
    /// falls back to the region's height. That is the honest answer: a step taken
    /// before anything is on screen has no drawn screenful to be measured in.
    shown: usize,
    /// Whether the reader has asked for the staged run beside the unstaged one.
    ///
    /// `SPEC.md` §11.2 **B17**, from `a`
    /// ([#313](https://github.com/breferrari/vigia/issues/313)). Off by default:
    /// a reader who has pressed nothing gets the comparison this tool has always
    /// drawn, and which way the toggle *starts* is still
    /// [#50](https://github.com/breferrari/vigia/issues/50)'s open question on
    /// [#306](https://github.com/breferrari/vigia/issues/306)'s file.
    ///
    /// **Held here and mirrored into the frame rather than read back from it.**
    /// The frame owns what it walks, and this owns what was *asked for*, which is
    /// the same split `Chrome::rail` and `Body::rail` already draw one region
    /// over. They cannot drift: `App::apply` is the only writer and it sets both
    /// in one arm.
    staged: bool,
    /// How many files the staged run held on the last collect.
    ///
    /// **Recorded rather than re-walked**, for the reason every other number here
    /// is: `Frame::advance` has already reported both runs, so this is a pass over
    /// a `Vec` the shell already holds. It is the *staged* total, where
    /// [`View::files`] is both runs together, and the header draws them as two
    /// facts because they answer two questions.
    staged_files: usize,
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
    /// that just changed there and `G` **unpinned** puts the last file there.
    /// Backing up to fill a short tail would move that file off the top row and
    /// make a reader hunt for what the jump was for. Scrolling makes no such
    /// claim, and running off the end of the diff into a half-blank pane is what
    /// [#59](https://github.com/breferrari/vigia/issues/59) reported.
    ///
    /// **`G` under `SPEC.md` §11.2 B16's pin is the exception, and it is
    /// deliberate rather than an oversight.** Pinned, `G` asks for the file's
    /// last row on the *bottom*, which is a claim about the bottom and not about
    /// the top, so it sets this **true** and takes the back-up on purpose. That
    /// is what corrects a height measured before `App::apply` turned follow off;
    /// [`Action::Bottom`]'s arm carries the whole of why. This sentence used to
    /// name `G` as the exemplar of a jump that must not anchor, which is exactly
    /// the shape a later session "restores" and reinstates a fixed defect with,
    /// so the qualifier is load-bearing rather than pedantic.
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
            rail: false,
            // Derived, and B16's ruling as well: an unpressed shell scrolls the
            // whole changed set, which is every version of this tool so far.
            single: false,
            // Derived, and B17's ruling as well: an unpressed shell draws the
            // working tree against the index, which is what §11.1's opening
            // contract has always said and what #50 has not yet reopened.
            staged: false,
            // Derived, and B19's ruling as well: an unpressed shell clips a long
            // line and marks it, which is what §11.1 has always said, and the
            // reader who wants the other mode asks for it once per session or
            // once in `~/.config/vigia/config`.
            wrap: false,
            shown: 0,
            // Derived: an unpressed, unconfigured shell draws no icons, and off
            // is byte-identical to every version before the key existed.
            icons: false,
            // On, which is `Config`'s own hand-written default and the one
            // field of this pane whose absence-of-a-file state is not "off":
            // OSC 8 degrades silently, so the link costs a reader nothing
            // anywhere it is not understood (#326).
            links: true,
            staged_files: 0,
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

    /// [`App::new`] with the view toggles a reader's config file asked for.
    ///
    /// `SPEC.md` §11.2 **B6** as amended by
    /// [#306](https://github.com/breferrari/vigia/issues/306). The three fields it
    /// sets are the three the file has keys for, and they are the *requests*
    /// rather than what a pane can honour: `rail` here is `Chrome::rail`, and a
    /// pane under 134 columns still draws none, which is B14 unchanged.
    ///
    /// **Beside [`App::new`] rather than replacing it**, and that is deliberate
    /// rather than shy. Every gate in this workspace builds an `App` and expects
    /// the shipped defaults; a constructor that read a file, or that took a
    /// `Config` everywhere, would put a reader's home directory inside several
    /// hundred tests. `Config::default()` is exactly `App::new`, which is what
    /// makes this additive: the one caller who has a file is `crate::run`.
    ///
    /// **`following` is not among the fields**, and that is I5 rather than an
    /// omission: *correct with zero interaction* is a promise about the program,
    /// so it is set here the way `App::new` sets it and no file can reach it.
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

    /// Whether the reader has asked for the staged run.
    ///
    /// Read by the loop to decide whether the empty state has another run to
    /// count, and by nothing else: with the run on, an empty frame means both
    /// comparisons are empty and there is nowhere for the work to have gone.
    pub fn staged(&self) -> bool {
        self.staged
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
        pointing: Pointing,
        elsewhere: usize,
        root: &str,
    ) -> Chrome {
        let Pointing {
            pressed,
            gripped,
            hovered,
            scrolling,
        } = pointing;
        Chrome {
            pressed,
            // `Some` whenever the reader has asked for the run, **including at
            // zero**: that zero is the only acknowledgment pressing `a` on a
            // worktree with nothing staged can give, and a key that does nothing
            // a reader can see is the failure B17 names in its own first line.
            // Counted from the frame's own changed set rather than walked again:
            // `App::view` records it, so this is a field read. The count the
            // *header* wants is the staged one alone, where `View::files` is both
            // runs together.
            staged: self.staged.then_some(self.staged_files),
            elsewhere,
            gripped,
            hovered,
            scrolling,
            worktree: worktree.to_owned(),
            branch: branch.map(str::to_owned),
            mode: self.mode,
            notice: self.notice.clone(),
            following: self.following,
            masthead: self.masthead,
            rail: self.rail,
            icons: self.icons,
            links: self.links,
            root: root.to_owned(),
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
            // **`Esc` leaves the frontmost thing, and the sheet is a thing**
            // ([#340](https://github.com/breferrari/vigia/issues/340)).
            // Reported from a real pane: a reader pressed `Esc` to put the
            // help away and the monitor exited. `SPEC.md` §11.2 B12's rule
            // that no key changes meaning while the sheet is up is intact,
            // because `input::key_action` still maps this key to one action
            // and is handed no state to branch on; what is frontmost is a
            // question about this struct, so this struct answers it.
            Action::Escape if self.sheet.is_some() => self.sheet = None,
            Action::Escape => return Ok(false),
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
            // **The same answer one region over.** The rail moves the map to the
            // side of the pane and takes the diff's columns to do it; the diff
            // keeps the row it was on, because a reader asking where the map goes
            // is not asking to be moved inside the diff.
            //
            // **Kept even where it cannot be honoured.** A pane under 134 columns
            // draws no rail whatever this says, and holding the request means a
            // reader who narrows such a pane and widens it again gets their rail
            // back rather than being asked twice. `Chrome::rail` is the request and
            // `Body::rail` is what the pane could give them.
            Action::ToggleRail => self.rail = !self.rail,
            // **No jump and no clamp here, which is the arm doing the least of
            // the four and is deliberate.** Every other toggle in this family
            // leaves the viewport exactly where it was; this one narrows what
            // the viewport is allowed to reach, and the position it already
            // holds may be outside that. Resolving it *here* would mean asking
            // how tall the pinned file is at the moment of the keystroke, which
            // is a second place that decides where a viewport lands.
            //
            // `View::collect` already has both answers and needs neither a new
            // branch nor a read to give them: a position past the pinned file's
            // end takes the same clamp the end of the diff takes, and a screen
            // left short takes the same back-up a short tail takes, both now
            // bounded at the file. So the screen after `s` is the pinned file's
            // last screenful with its final row on the bottom, which is what a
            // pager does and what the reader was already looking at most of.
            //
            // **The back-up is gated on `anchored || landed_inside || single`,
            // and the third term is the pin's own licence.** Two audit rounds
            // went into getting that right. It was left to whatever placed the
            // position first, which made this paragraph true only for a reader
            // who had arrived by scrolling: after a drag on the diff's bar, which
            // sets `anchored` false, the pinned screen came out short. Setting
            // `anchored` from the toggle's arm was the next attempt and it
            // leaked, because the flag outlives the pin: a jump onto a short
            // tail, then `s` and `s`, left an *unpinned* frame anchored and
            // backed the reader out of the file the jump was for. Licensing the
            // back-up from `single` itself has no state to leak.
            //
            // The cost is stated rather than hidden: a position that needed
            // clamping is **rewritten**, so pressing `s` twice from a screen
            // straddling two files does not restore the straddle. From every
            // screen already inside one file it is exactly identity, and from
            // any screen at all the second on-and-off pair is. `SPEC.md` §11.2
            // B16 says so out loud and `tests/single.rs` holds both halves.
            Action::ToggleSingle => self.single = !self.single,
            // **The bar's scale does not move.** The position is a logical row
            // and stays one, so a reader who presses `w` is looking at the same
            // line of the same file, drawn over more of the pane. That is
            // `SPEC.md` §11.2 B19's own claim about the scrollbar restated where a
            // reader can feel it. **The one place the row itself moves is the
            // diff's last screenful**, where wrapping means fewer of the diff's
            // rows fit and the clamp gives the difference back, which is the clamp
            // keeping the last row on the last row rather than an exception.
            Action::ToggleWrap => self.wrap = !self.wrap,
            // **The one toggle that changes what the frame *walks*.** The other
            // three rearrange rows the frame already holds; this one adds a second
            // status walk and a second set of diffs, so the frame is told rather
            // than the shell remembering on its own. `Frame::show_staged` drops
            // both caches when the answer actually moves and is a no-op when it
            // does not, so calling it here rather than once a frame costs nothing
            // and keeps one owner of the fact.
            //
            // **And the position goes to the top**, which is the deviation from
            // the three toggles above and is deliberate. They keep the reader's
            // row because the row still names the same file afterwards; here the
            // changed set itself grows or shrinks underneath the position, so
            // holding a row number would land the reader in a different file with
            // no gesture of theirs to explain it. `SPEC.md` §11.2 B17.
            Action::ToggleStaged => {
                self.staged = !self.staged;
                frame.show_staged(self.staged);
                self.position = Position::default();
                // **And the frame is walked here, which no other toggle needs.**
                // `Frame::advance` runs on a **tick**, and a keypress is not one:
                // the other three rearrange rows the frame already holds, so a
                // paint is the whole of what they owe. This one changes what the
                // frame *contains*, and without a walk the reader pressed `a`,
                // the header said `0 staged` over a worktree with two staged
                // files, and the pane went on drawing exactly what it drew before
                // — until something happened to be written, which on a tree an
                // agent has finished with may be never. Found in a live pane, and
                // it is the failure §11.2 B17 is named for one layer down: a key
                // that does nothing a reader can see.
                //
                // **I1 is untouched.** This is not a clock and not an unbidden
                // wake: it is work on a wake the reader caused, which is the same
                // licence every other key on this map already has.
                //
                // The error goes to the footer rather than out of `apply`, for the
                // reason the tick's own `advance` does: the core leaves the frame
                // exactly as it was on failure, so the previous run is still valid
                // to draw, and refusing the keystroke would blank a pane over a
                // walk that may succeed on the next one.
                if let Err(e) = frame.advance() {
                    self.warn(e.to_string());
                }
            }
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
                // The same ceiling `browse` clamps with, for the same reason: the
                // track maps onto **travel**, and travel is how far the window can
                // actually go rather than how many files there are. `View::list_span`
                // is that same number seen from the other end, so the drawn thumb's
                // travel and this one are one quantity.
                let travel = crate::view::last_top(frame.files(), self.list_rows.max(1));
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
                // **Resolved through the list's own plan, not by adding the offset
                // to the window's first file**
                // ([#313](https://github.com/breferrari/vigia/issues/313)). Those
                // are the same number only while every drawn row is a file, and
                // since B17 a grouped window opens each run with a separator. Added
                // blind, a click or a digit past the first separator names the file
                // *before* the one under the pointer, and it does it silently:
                // there is a file at that index, the jump lands, and the only sign
                // is that the reader ends up somewhere they did not point at.
                //
                // `None` is a separator, and a click on one does nothing at all
                // rather than resolving to a neighbour. That is the same answer a
                // digit naming no drawn row already gets, and it is the right one:
                // a separator is chrome, and chrome that teleported the diff would
                // be an affordance nothing announced.
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
            // **The bound is defence, and a mutation survives it.** Relaxing
            // `<` to `<=` puts `position.file` one past the last file and no gate
            // over the drawn output reddens, because `View::collect` clamps and
            // the screen is identical. So this is the same shape as `Body`'s own
            // unreachable guards: it is carried because nothing downstream should
            // have to be the only thing standing between a signed step and an
            // index, not because a test can see it. Measured 2026-08-24 with
            // [#296](https://github.com/breferrari/vigia/issues/296)'s battery.
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
            //
            // **Under a pin the same arithmetic runs over the pinned file**, and
            // this is the half of B16 that would have been easy to leave behind.
            // The thumb is drawn from `View::total_rows`, which under a pin is
            // the pinned file's own height, so a drag resolved against the whole
            // diff would be a gesture inverting a readout nobody drew. The two
            // agree at both ends of the track and nowhere in between, which is
            // exactly the shape that passes a test asserting the ends.
            Action::DiffTo(at) => self.diff_to(at, height, frame)?,
            // A page keeps one row of overlap, which is what stops a reader
            // losing their place at the seam between two screens.
            Action::Page(pages) => {
                self.step_by(pages, self.screenful(height).saturating_sub(1), frame)?;
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
                self.step_by(halves, self.screenful(height) / 2, frame)?;
            }
            // **The first row of what the reader can reach**, which is the
            // first changed file unpinned and the pinned file's own heading
            // under B16. One meaning over two subjects rather than two meanings:
            // `g` has always been *the top*, and the pin is what decides the top
            // of what. Jumping to file zero under a pin would change which file
            // is pinned, which is the one thing this gesture takes away from
            // everything that is not `n`, `p`, a digit or a click.
            Action::Top => {
                self.jump_to(if self.single { self.position.file } else { 0 });
            }
            // The last *file*, from its top, rather than the last row of the
            // whole diff. Finding that row would mean diffing every file to add
            // up their heights, which is the read I4 forbids.
            //
            // **Under a pin it is the last row, and that is affordable here and
            // nowhere else.** The subject is one file and that file has been
            // diffed by definition, so [`crate::view::span_in`] answers from the
            // diff the walk already holds. `G` therefore keeps meaning *the end
            // of what you can scroll to* while the thing it reaches gets better,
            // rather than meaning something different.
            //
            // What that costs is [`Self::diff_to`]'s docblock's to state and it
            // is not free in every case; two earlier spellings of this paragraph
            // said `span_in` was "a `stat` against a span this tick has already
            // proved", which is [`crate::view::block_rows`]' cost quoted under a
            // different call, and the sentence outlived both corrections because
            // nothing reads a comment. The resting row itself is the inner
            // comment below, and an earlier draft of this one contradicted it.
            Action::Bottom => {
                if let Some(file) = self.pinned_file(frame) {
                    // **`true`, unlike every other jump on this map, and it is
                    // what makes the resting row survive a stale height.**
                    // `anchored` means *reached by scrolling*, and it licenses
                    // `View::collect`'s back-up: a screen that came out short
                    // rests its last row on the bottom. `G` under a pin is asking
                    // for exactly that. It is not a claim about what belongs on
                    // the **top** row, which is what a jump is and why `jump_to`
                    // clears this.
                    //
                    // Without it the arm is sized against a chrome its own side
                    // effect invalidates. `Shell::diff_rows_for` builds the chrome
                    // *before* `apply` runs; `Bottom` is a manual scroll, so
                    // `apply` then turns follow off; and `Footer::plan` sizes its
                    // rungs from `Chrome::following`, where `follow ▶  N/M` is
                    // thirteen columns and `N/M` is three. On a pane narrow enough
                    // for that to decide between a one-line and a two-line footer,
                    // the region the frame actually draws is a row taller than the
                    // one this subtracted, the file's last row rests one line
                    // above the bottom, and `App::view` writes the same position
                    // back every frame: [#57](https://github.com/breferrari/vigia/issues/57)'s
                    // symptom, on the arm written to avoid it.
                    //
                    // The alternative is computing the height after `apply`, which
                    // cannot work: `apply` is what needs it. Letting the walk
                    // correct a short screen is the mechanism that already exists
                    // for exactly this, and it costs nothing when the height was
                    // right, because a full screen is not short.
                    self.anchored = true;
                    // **The resting row rather than the file's height, and the
                    // difference is a whole batch of keystrokes.** `View::collect`
                    // clamps an overrun to the last screenful either way, so
                    // writing the raw span draws the right screen; what it does
                    // not do is leave a *position* a later action in the same
                    // wake can move from. The shell drains actions in a batch and
                    // paints once at the end of it, so `G` and then a held `k`
                    // arrive together: the `k`s walk `span` down toward
                    // `span - height`, every one of them still clamps to the same
                    // row, and the reader presses a key up to `span - height`
                    // times before the screen moves. Nine on this file at this
                    // pane. Unpinned the case cannot arise, because `G` there is
                    // `jump_to`, which resolves to row zero.
                    //
                    // Clamping here costs the staleness correction `collect`'s own
                    // clamp gave for free: `span_in` reads the generation the bar
                    // was drawn from ([#84](https://github.com/breferrari/vigia/issues/84)),
                    // so a file that grew since then rests slightly short of its
                    // true bottom for one tick. That is invisible and
                    // self-correcting, where swallowed keystrokes are neither.
                    let span = crate::view::span_in(frame, file)?;
                    self.position = Position {
                        file,
                        // **A screenful rather than the region's height**
                        // ([#272](https://github.com/breferrari/vigia/issues/272)).
                        // This read `span.saturating_sub(height)`, a count of the
                        // file's own rows less a count of the terminal's, which
                        // are the same number only while nothing wraps: with `w`
                        // on it rested the top too early and the pinned file's
                        // last lines could not be reached by the gesture that
                        // names its end. [`Self::screenful`] is `height` exactly
                        // while `w` is off, so this is unchanged there.
                        row: span.saturating_sub(self.screenful(height)),
                    };
                } else {
                    self.jump_to(frame.files().len().saturating_sub(1));
                }
            }
        }
        Ok(true)
    }

    /// The file a pin is on, resolved against the files that actually exist.
    ///
    /// **`None` is *not pinned*, and it covers two different states on purpose**:
    /// the reader has not asked for a pin, and there is nothing to pin to. Both
    /// want the arm's unpinned branch, and both are states the arms below would
    /// otherwise have had to test for separately.
    ///
    /// **The clamp is what makes this a function rather than a field read**, and
    /// it is load-bearing rather than defensive. `SPEC.md` §11.2 B16 puts the
    /// pinned file in [`Self::position`], and a position is exactly the index
    /// that outlives the list it was resolved against:
    /// [`vigia_core::Frame::advance`] rebuilds the changed set from scratch, so
    /// the agent in the other pane committing its work, reverting an edit or
    /// switching branch leaves this naming a file that is gone.
    /// [`vigia_core::Frame::rows_of`] **panics** on that index by design, the same
    /// way [`vigia_core::Frame::diff`] does, and the two callers below reach the
    /// frame *before* [`View::collect`] has had a chance to clamp. **They are not
    /// the only ones, and an earlier draft of this sentence said they were**:
    /// [`Self::up`]'s walk back does too, it had the same latent panic, and the
    /// claim here is what kept anyone from looking. It carries its own clamp now,
    /// and this says *two of three* rather than *the only two*. A clean worktree is the whole
    /// of the second case and it is not an edge: it is the state a monitor sits in
    /// most of the time, so `s` and then `G` on a pane that has been left open is
    /// a panic in a tool whose job is to be left open.
    ///
    /// Clamped rather than refused, which is [`View::collect`]'s own answer to the
    /// same staleness: the reader asked for the end of the file they were on, and
    /// the nearest file that still exists is a better answer than nothing
    /// happening. `tests/single.rs::a_pinned_gesture_survives_the_diff_it_was_made_against`
    /// holds both shapes.
    ///
    /// [`Action::Top`] does not come through here, and that is not an omission:
    /// it writes an index and reads nothing, so a stale one is resolved by the
    /// same clamp every other jump gets.
    fn pinned_file(&self, frame: &Frame) -> Option<usize> {
        let files = frame.files().len();
        (self.single && files > 0).then(|| self.position.file.min(files - 1))
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
        // **The list's own ceiling, not `files - rows`**
        // ([#313](https://github.com/breferrari/vigia/issues/313)). The naive bound
        // compares a count of files against a count of drawn rows, and a grouped
        // window spends one or two of those on separators — so `J`, the wheel and
        // a drag to the bottom of the track all stopped one or two files short of
        // the end, with nothing on screen saying the map had more. Clamping in
        // `take_list` alone could not fix it: that only ever takes the *smaller*
        // of the two, so a bound already too low stays.
        let bound = crate::view::last_top(frame.files(), self.list_rows.max(1));
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

    /// Resolve a drag on the diff's bar into a position.
    ///
    /// **A method rather than an arm since the pin gave it a second branch**,
    /// which is the shape [`Self::up`] below and [`Self::step_by`] above already
    /// have: the unpinned walk is twenty lines and nesting it inside an `else`
    /// to add a three-line guard puts the arm's own body at a depth
    /// [`Self::apply`]'s match has nowhere else.
    ///
    /// **The pinned branch is a guard rather than a fork**, and the two read one
    /// quantity: `View::measure` under a pin reports the pinned file's span and
    /// [`crate::view::span_in`] reads the same generation of it, where the
    /// unpinned walk below sums [`crate::view::block_rows`] over the changed set
    /// against a bar drawn from `diff_rows`. Both arms therefore invert the bar
    /// the reader is actually looking at, which is the whole contract a drag has;
    /// see `span_in`'s own docblock for the route that gets it.
    ///
    /// **What it costs is a span lookup and not always a free one**, and the first
    /// two spellings of this sentence were both wrong in the cheap direction.
    /// [`vigia_core::Frame::fill_span`] answers from the cached diff for the file
    /// the viewport is inside, which is free and is the ordinary case, because
    /// that file is the one the walk drew. It is *not* guaranteed: after the
    /// changed set shrinks, [`Self::pinned_file`] clamps onto a file this frame
    /// has never drawn, and the cache then falls through to a measurement. One
    /// file, on a keypress rather than per frame, and it is the honest description
    /// rather than "a `stat` against a span this tick has already proved", which
    /// was `block_rows`' cost quoted under a different call.
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
            // **Written every iteration rather than only on the hit**, which is
            // what makes a target *past* the last row land past the last row
            // ([#272](https://github.com/breferrari/vigia/issues/272)). The
            // fall-through used to leave the initial `row: 0`, so a drag to the
            // very end of the track, which [`Self::dragged_to`] deliberately maps
            // past the end so the walk can clamp it in display rows, went to the
            // **top** of the last file instead of its bottom. On a one-file diff
            // that is the top of the diff, which is as wrong as a drag can be.
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

    fn up(&mut self, rows: usize, frame: &mut Frame) -> Result<()> {
        // **The upper clamp B16 needs, and the only one that cannot live in the
        // walk.** Scrolling *down* overruns into a row number `View::collect`
        // resolves, so the pin is enforced there by the walk simply not
        // advancing. Scrolling up is resolved here instead, because stepping off
        // the top of a file means knowing how tall the one above it is, and
        // under a pin there is no file above it to step into: the reader asked
        // for one file and the top of that file is where up stops.
        //
        // Above the loop rather than inside it, so the pin costs no `Frame::diff`
        // at all rather than one that is then discarded.
        if self.single {
            self.position.row = self.position.row.saturating_sub(rows);
            return Ok(());
        }
        // **The walk back reaches the frame before anything has clamped, and it
        // panicked on a stale index until [#297](https://github.com/breferrari/vigia/issues/297)'s
        // second audit round.** [`crate::view::rows_in`] is
        // [`vigia_core::Frame::diff`], which indexes `files` directly and panics
        // past the end; a position is exactly the index that outlives the list it
        // was resolved against, since [`vigia_core::Frame::advance`] rebuilds the
        // changed set whenever the worktree moves. So a reader scrolled deep into
        // the changed set, an agent committing in the other pane, and a wheel-up
        // batched into the same drain as that tick is a crash, with no paint in
        // between to clamp anything.
        //
        // Clamped rather than refused, which is [`View::collect`]'s own answer to
        // the same staleness: it opens with `position.file.min(files - 1)` for
        // exactly this reason, and a reader whose file is gone is better served by
        // the nearest one that exists than by a panic.
        //
        // **Pre-existing rather than this ruling's**, and found because
        // [`Self::pinned_file`]'s docblock claimed its two callers were the only
        // gestures that reach the frame ahead of the walk. They are not, this is
        // the third, and the sentence that was wrong is what kept anyone from
        // looking.
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
                // Passed through rather than resolved here, for the reason the
                // arm that sets it gives: the walk is where a position meets the
                // file it is inside, so the walk is where a pin can be enforced
                // without asking the frame anything twice.
                single: self.single,
                // **From the layout rather than from this method's idea of the
                // pane**, which is the reason `body` is a parameter at all: the
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
        // **The staged total, below the collect and for the reason `elsewhere` is.**
        // The header draws it beside `View::files`, and `Shell::screen` keeps the
        // previous view when a collect fails — so taken from the frame it could
        // pair this frame's staged count with last frame's changed count and read
        // `3 changed · 5 staged`, which cannot happen: staged is a subset. Round 4
        // fixed exactly this split on `elsewhere` and left its sibling standing.
        //
        // **From `Frame::staged_at` rather than a filter**, which is the boundary
        // the walk already recorded: `advance` concatenates unstaged then staged,
        // so the count is a subtraction rather than a pass over the changed set.
        self.staged_files = frame.files().len() - frame.staged_at();
        // Stored back for the reason the position is: resolution happens once,
        // in the code that knows where the diff landed, and a caller that kept
        // its own answer would be a second rule for the same fact.
        self.list_top = view.list_top;
        // **What a page step is measured in, recorded where the frame is built**
        // ([#272](https://github.com/breferrari/vigia/issues/272)). `View::rows`
        // is display rows since B19 and a step moves the position, which is
        // logical, so the two have to be told apart here rather than at the
        // stepping site: a continuation is a row of the terminal that is not a
        // row of the diff. See [`Self::shown`].
        self.shown = view
            .rows
            .iter()
            .filter(|row| !matches!(row, crate::view::Row::Wrap { .. }))
            .count();
        Ok(view)
    }

    /// Where a drag on the diff's bar lands, in rows of the diff.
    ///
    /// **The track maps onto travel and not onto the whole**, which is the
    /// arithmetic every scrollbar owes its reader and which this project has
    /// already been corrected on once: mapping onto the total and clamping leaves
    /// the last screenful of track dead.
    ///
    /// **And the far end of the track is the end of the diff, which only the walk
    /// can place** ([#272](https://github.com/breferrari/vigia/issues/272)).
    /// Travel is `total` less what one screen shows, and with `w` on what one
    /// screen shows is a property of *where* in the diff it is: a screenful of
    /// wrapped lines is fewer rows than a screenful that opens on a heading and a
    /// hunk header. So an estimate lands near the bottom rather than on it, and a
    /// reader who drags the thumb all the way down and cannot see the last line
    /// is the exact defect the paragraph above records. At the end of the track
    /// this therefore asks for a row **past** the end and lets `View::collect`
    /// clamp it, which is the same clamp scrolling off the end already takes and
    /// the only one measured in display rows. Everywhere else the mapping is
    /// unchanged.
    fn dragged_to(&self, at: u32, total: usize, height: usize) -> usize {
        // **Only where lines wrap, because only there is the estimate wrong.**
        // With `w` off a screenful is exactly `height` rows of the diff and the
        // mapping below is the one every version before B19 took, byte for byte.
        // Routing that case through the walk's clamp would land the same row by a
        // longer road and change what `App::position` holds between a drag and
        // the frame that answers it, which `tests/scroll.rs` reads directly.
        if self.wrap && at >= crate::input::TRACK_SCALE {
            return total;
        }
        scaled(at, total.saturating_sub(self.screenful(height)))
    }

    /// Rows of the **diff** one screenful holds, which is not `height` when
    /// lines wrap.
    ///
    /// **`height` where nothing has been drawn and where wrapping is off**, so
    /// every step this shell has ever taken is the step it took before B19. See
    /// [`Self::shown`] for why the number cannot be derived at the stepping site.
    fn screenful(&self, height: usize) -> usize {
        if self.wrap && self.shown > 0 {
            self.shown
        } else {
            height
        }
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
    // Named here rather than at the top of the file: since the four pointer facts
    // travel as one [`Pointing`], the module itself has no use for either type and
    // this is the only place that spells a mark out.
    use crate::input::{Grabbed, Hovered};

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
            Pointing {
                pressed: Some((79, 5)),
                gripped: Some(Grabbed::Diff),
                hovered: Some(Hovered::Button(79, 19)),
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

        // One way, and asserted rather than left implied by the absence of a
        // setter. Nothing can revive a watch: the one handle that unblocks the
        // watcher makes `next_tick` return `None` permanently. A later
        // convenience that reset this alongside a notice is exactly how a still
        // picture would start claiming to be live again, and the two are next to
        // each other precisely because they arrive from one event.
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
