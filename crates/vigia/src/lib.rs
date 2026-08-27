//! The `ratatui` + `crossterm` shell over [`vigia_core`].
//!
//! `SPEC.md` §6 asks this half of the workspace to be thin: the core produces
//! frames, the shell renders them, and the TUI stays swappable because nothing
//! it knows is load bearing. So there is no diff logic here, no caching and no
//! filtering. What is here is a terminal, a key map, a scroll position, and one
//! pure function from a screenful of rows to cells.
//!
//! **Coalescing is the one word in that list that needs two entries**, because
//! there are two of them with different subjects and they belong in different
//! crates. `vigia_core` coalesces **events**: which filesystem writes count as
//! one change, which is I1's, and the policy stays there because that is where
//! it is testable. `run` below coalesces **paints**: how many frames one burst
//! of wakes is worth, which is I9's, and it can only live here because a paint
//! is the shell's and because one of the four wake sources is the terminal. It
//! decides nothing about which events are real, which is what the sentence
//! above is protecting.
//!
//! It is a library with a five-line binary on top rather than a binary alone.
//! `SPEC.md` §7 makes the snapshot suite over `ratatui::backend::TestBackend` the
//! proof for I5 and I6, and a test cannot import a `main.rs`.
//!
//! ## Why four threads and two repositories
//!
//! I1 says an idle monitor does no work, and the core delivers that by blocking:
//! [`vigia_core::Watcher::next_tick`] waits on an untimed `recv`. `crossterm`
//! reads input the same way. **The first two** blocking sources need two threads
//! and one channel, or a poll loop, and a poll loop is the timer I1 forbids.
//!
//! The third is [`vigia_core::Highlighter::warm_ahead`], and **it is a sender on
//! that channel too** since [#129](https://github.com/breferrari/vigia/issues/129).
//! It compiles grammars ahead of the reader and ends by itself, and it now says
//! so when it does, because the frame path draws a hunk plain while its grammar
//! is uncompiled and a loop blocked on `recv` has no other reason to look again.
//!
//! **That does not make it a timer and I1 does not reach it.** I1's words are
//! *no filesystem event and no git index change means no work*, and its budget
//! is *0 wakeups while idle*: a warm exists only because something was written
//! or because a diff was already on screen, it is bounded by the number of
//! grammars a session meets rather than by time, and on a tree nobody touches
//! nothing is spawned and nothing is ever sent. The licence sentence about
//! clocks is not being stretched to cover it, because it is not one.
//!
//! What `warm_ahead` still does **not** report is which grammars it managed,
//! and that has not changed: there is no such thing as a warm grammar for the
//! frame path to believe in. What the frame path acts on is the opposite claim,
//! that a grammar has never been parsed under at all, which is exact.
//! `Highlighter::attempted` carries the distinction in full.
//!
//! The fourth is [`signal`]'s, and it is a **third wake source on the same
//! channel** rather than a new mechanism: an externally delivered signal ends
//! the loop, and the ordinary `Drop` restores the terminal (I8). It blocks the
//! way the first two do, on a self-pipe `signal-hook` drains, so it costs an idle
//! monitor nothing either. **On Windows it does not exist**: console control
//! events arrive on a thread the OS makes for the handler, and only while the
//! process is leaving, so there is nothing of ours to spawn.
//!
//! The watch thread opens its own [`vigia_core::Worktree`]. That is not
//! duplication for its own sake: a [`vigia_core::Watcher`] borrows the
//! `gix::Repository` it filters gitignore rules through, and `gix::Repository` is
//! `Send` but not `Sync`, so a borrow of one cannot cross a thread boundary. The
//! second open costs one repository discovery, paid after first paint, off the
//! path I7 measures.
//!
//! All of them are detached rather than scoped. Quitting means main returning,
//! and none of them can be woken to be joined: the warmer ends by itself, and
//! for the rest, `crossterm` has no portable
//! interruptible read, the one handle that unblocks the watcher makes its
//! `next_tick` return `None` permanently, which is a shutdown and not a nudge,
//! and there is no portable way to interrupt a blocked read of a self-pipe
//! either.

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

mod app;
mod colour;
/// What the pane starts as, which `SPEC.md` §11.2 B6 puts in a file.
///
/// `pub` for the reason [`theme`] gives in its own declaration below: the
/// functions that resolve a file take a lookup so a gate can place a home
/// directory without touching the process environment, and a gate can only call
/// them if it can name them.
pub mod config;
mod glyphs;
/// Public where its seven siblings are private, and for one reason: `soak.rs` is
/// an integration test, so it can only reach what the crate exports, and I3's
/// harness needs the same reader the shell uses. Two implementations of "read
/// this process's RSS" that could disagree is exactly what one of them existing
/// is meant to prevent.
pub mod icons;
mod input;
pub mod memory;
mod render;
mod signal;
mod terminal;
/// Public for the same reason [`memory`] is: a theme file is parsed by a pure
/// function over a string, `tests/palette.rs` is an integration test and can only
/// reach what the crate exports, and the alternative is re-exporting three free
/// functions into the crate root under names invented to avoid colliding with
/// [`colour`](Depth)'s.
pub mod theme;
mod view;

pub use app::App;
pub use colour::{DEPTH_VAR, Depth, DepthError};
pub use config::{CONFIG_FILE, Config, ConfigError};
pub use glyphs::{GLYPHS_VAR, Glyphs, GlyphsError};
pub use input::{
    Action, Grabbed, Held, Hovered, Pointing, Region, Regions, STEP_DELAY, STEP_REPEAT, Sheet,
    TRACK_SCALE, WHEEL_ROWS, action_for, drag_action, hover_after, hover_repainted, patience,
    scroll_mark, settled,
};
pub use render::{
    Areas, Band, Body, Chrome, HINT_SEPARATOR, Heat, LIST_SETTLED, Mode, PaintStats, body_layout,
    diff_height, regions, render,
};
pub use terminal::{Background, Screen, Session, background_of};
pub use theme::{THEME_FILE, THEME_VAR, Theme, ThemeError};
pub use view::{
    FileEntry, HEAT_BUCKETS, HeatBucket, ListRow, Position, Row, Scale, Slot, View, Viewport,
    block_rows, diff_rows, file_at, last_top, list_plan, list_rows_wanted, rows_in, rows_of,
    span_in,
};

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::Instant;

use ratatui::crossterm::event::{Event, MouseButton, MouseEventKind};
use ratatui::layout::Rect;
use vigia_core::{Highlighter, History, WatchOptions, Worktree};

/// Anything that stops the shell from starting or from drawing.
///
/// Deliberately not the type a *frame* failure uses. A repository that cannot be
/// opened is a reason to exit with a message; a file that cannot be read during
/// one frame is not, and goes to the footer instead. See [`App::warn`].
pub type Failure = Box<dyn std::error::Error>;

/// Why the shell woke up.
enum Wake {
    /// The terminal reported something.
    Input(ratatui::crossterm::event::Event),
    /// The working tree changed, coalesced into one signal by the core.
    ///
    /// Carries every file the burst wrote, with the one that landed last at the
    /// end. The tail is what follow mode moves to (I5) and the whole list is
    /// what the glance history samples (I10). Empty is ordinary: a staging write
    /// changes every diff's left-hand side and is nowhere to scroll to.
    Tick(Vec<String>),
    /// The watch stopped, so the shell is a still picture.
    ///
    /// Reported rather than fatal. A diff nobody is watching for changes is
    /// still a diff, and the reader should be told which of the two they have.
    WatchLost(String),
    /// Terminal input stopped, so nothing can reach the shell any more.
    ///
    /// Fatal, and it is the one wake-up that is. In raw mode every way out from
    /// this keyboard is a key event, because the terminal does not turn Ctrl-C
    /// into a signal. So a shell that kept drawing after this would hold the
    /// alternate screen with nothing the reader in front of it could do about
    /// it, and leaving is the smaller failure.
    ///
    /// **[`Signalled`](Wake::Signalled) changes the consequence, not the
    /// ruling.** Without it the only remaining exit is a kill, which runs
    /// neither the guard nor the panic hook and hands back a terminal in raw
    /// mode. A kill is caught and restores like any other exit, so the cost of
    /// staying is not a wrecked terminal. It is still a pane that
    /// cannot be closed from the pane, which is reason enough.
    InputLost,
    /// Something outside this process asked it to stop.
    ///
    /// The quit key's arm, reached without a key. It carries nothing because
    /// there is nothing left to decide: whichever signal it was, the answer is
    /// to leave, and the terminal goes back the way it does on every other exit,
    /// through the `Drop` on the way out. [`signal`] is where the reasoning
    /// lives, above all why the handler must restore nothing itself.
    Signalled,
    /// A warm finished, so a hunk that drew plain can draw in colour.
    ///
    /// **It carries nothing, and the arm below does nothing**, because the paint
    /// after the batch *is* the whole of the response: the highlighter already
    /// holds the compiled patterns, and what was missing was a reason for a loop
    /// blocked on `recv` to look again.
    ///
    /// **A fourth sender on the channel the other three share**, which is the
    /// shape [`signal`] already established as a third rather than a new
    /// mechanism. I1 does not reach it, and the row's own words are why: *no
    /// filesystem event and no git index change means no work*, budget **0
    /// wakeups while idle**. A warm happens only because something was written
    /// or because the reader is looking at a diff that already existed, it is
    /// bounded by the number of grammars a session meets rather than by time,
    /// and with nothing to warm nothing is ever spawned and nothing is ever
    /// sent. It is not a clock and the licence sentence about clocks does not
    /// have to be stretched to cover it.
    Warmed,
}

/// Whether a demand is worth handing to a warmer, given what the last one was
/// handed and whether the tree has changed since.
///
/// **The rule lives here rather than inside `Shell::request_warm` for the reason
/// `Held::ends` is a free function: that loop cannot be driven by a test, and a
/// rule written inline is a rule with no gate.**
///
/// Three answers. An empty demand is nothing to do. A demand **identical** to
/// the one the last warm was handed is nothing to do either, because that warm
/// has already had its turn at exactly these paths and finished: asking again
/// spawns a thread that will do the same nothing and send a wake for it, which
/// on a tree nobody is touching is I1's *0 wakeups while idle* breached. And
/// `written` overrides both, because a warm that could not be served a moment
/// ago may be servable now.
///
/// **It cannot stall a demand that is making progress**, and that is why the
/// comparison is on the whole list rather than on a flag. A warm that compiles
/// two of three grammars leaves a *shorter* demand, which is a different list,
/// which is offered immediately. Only a demand that moved not at all is held
/// back.
///
/// **`written` is a flag rather than a clearing of `served`, and that is a round
/// two correction.** Clearing on the tick itself wiped the record of a warm that
/// was still in flight, so when that warm finished there was nothing left to
/// compare its result against and the same unservable demand was spawned for
/// again. During active editing a tick lands inside a warm often, which is the
/// case this exists for. A flag cannot lose that record: it is set by the tick,
/// read here, and cleared by the spawn.
pub fn worth_warming(wanted: &[String], served: &[String], written: bool) -> bool {
    !wanted.is_empty() && (written || wanted != served)
}

/// The callback a warm ends with, wired to this shell's wake channel.
///
/// One line, and it is here rather than written out at its three call sites so
/// that what a warm does when it finishes is one decision. Failure is dropped:
/// a closed channel is the shell on its way out, and there is nothing to tell.
fn warmed(tx: &Sender<Wake>) -> vigia_core::Warmed {
    let tx = tx.clone();
    Box::new(move || {
        let _ = tx.send(Wake::Warmed);
    })
}

/// The version this binary reports, which is the package's.
///
/// Read from the manifest rather than written down, so the string a user quotes
/// in a report cannot drift from the tag the release was cut at. `SPEC.md` §9
/// makes the tag the one irreversible event in the release, and a version
/// constant maintained by hand is the obvious way for the binary to disagree
/// with it.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// What `vigia`'s argument list is asking for.
///
/// The CLI is one optional positional path and one flag (`SPEC.md` §11.1), so
/// this is the whole surface. It lives here rather than in `main.rs` because §7
/// makes the test suite the proof and a test cannot import a `main.rs`, which is
/// the split that file's own module docblock describes. What stays over there is
/// the dispatch: which stream each answer is written to, and the exit code, both
/// of which `tests/cli.rs` reaches by running the built binary.
///
/// **Deliberately not `#[non_exhaustive]`, and it was tried.** The argument for
/// it is that this crate is about to be published permanently and §11.1 leaves
/// `--help` open, so answering it adds a variant and breaks every downstream
/// `match`. Two things make it the wrong trade here. `main.rs` is a separate
/// crate from this library, so the attribute reaches it too and forces a `_`
/// arm on the one match that must never silently ignore a new variant, which is
/// exactly the exhaustiveness this enum exists to get. And at `0.x` the
/// protection is worth nothing anyway: cargo already treats every `0.x` minor
/// bump as breaking, so adding a variant costs `0.2.0` either way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    /// Watch the argument as a path.
    Watch,
    /// Print the version and exit successfully.
    Version,
    /// An argument beginning with `-` that is not a version query.
    ///
    /// Refused with one line naming the surface rather than being taken as a
    /// path, so `vigia --colour=never` is told what the options are instead of
    /// being told `--colour=never` is not a repository.
    NoSuchOption,
    /// More than one argument, when the surface is exactly one.
    ///
    /// **Refused rather than ignored.** Ungated, `vigia . --colour=never`
    /// watches `.` and drops the rest on the floor, so a reader who typed a flag
    /// alongside a path gets no signal that the flag does not exist. That is the
    /// same defect
    /// [`NoSuchOption`](Request::NoSuchOption) exists to prevent, reached from a
    /// position the old check never looked at, and it is worse there: the tool
    /// appears to accept the flag, because it starts and draws.
    TooManyArguments,
}

/// Classify the arguments `vigia` was given.
///
/// A surface of at most one, so anything longer is [`TooManyArguments`](Request::TooManyArguments)
/// rather than a list to interpret.
///
/// Takes the whole list rather than one argument, because **arity is part of the
/// surface**. A classifier seeing only `args_os().nth(1)` lets
/// `vigia . --colour=never` watch `.` and discard the rest silently: the flag
/// that does not exist produces a running program instead of the one-line
/// refusal that a flag on its own produces. A function
/// handed one argument cannot notice a second, which is why the fix is the
/// signature rather than an extra check at the call site.
///
/// An empty list is [`Watch`](Request::Watch), and `main` supplies the default
/// path. That keeps the "optional positional" of §11.1 in one place instead of
/// splitting the default across both files.
pub fn request_for(args: &[OsString]) -> Request {
    match args {
        [] => Request::Watch,
        [arg] => request_for_one(arg),
        _ => Request::TooManyArguments,
    }
}

/// Classify the one argument `vigia` takes.
///
/// **B6 forbids flags that *configure*, and a version query is not one**, which
/// is the amendment `SPEC.md` §11 records: this prints a line and exits before a
/// terminal is taken, so there is no frame it can change and no state it can
/// leave. Both conventional spellings are accepted, because a user who tries one
/// tries the other, and refusing exactly one of them is a worse surface than
/// refusing both.
///
/// Everything else beginning with `-` is still
/// [`NoSuchOption`](Request::NoSuchOption). That includes `--help`, which §11.1
/// leaves open on purpose: help text describes a surface and has to be kept true
/// as the surface grows, where a version string comes from the manifest and
/// cannot drift.
///
/// **Compared against the raw [`OsStr`], and the reason is cost rather than
/// correctness.** The obvious claim to make here is that `to_string_lossy` (what
/// the old refusal used) would misclassify a path that is not valid Unicode, and
/// **that claim is false**: lossy decoding replaces what it cannot read with
/// `U+FFFD`, which is not `-` and is not `--version`, so it reaches the same
/// answer this does on every input. Mutation-tested rather than reasoned, and
/// the mutation *survived* the whole suite, which is how the overclaim was
/// caught.
///
/// What the raw comparison buys is that classifying an argument stops being
/// proportional to its length. `to_string_lossy` validates the entire string to
/// decide whether it can borrow, where `as_encoded_bytes().first()` reads one
/// byte, and this runs before anything else in the process on the I7 path.
fn request_for_one(arg: &OsStr) -> Request {
    if arg == OsStr::new("--version") || arg == OsStr::new("-V") {
        return Request::Version;
    }
    // The first byte, rather than a decoded first character. `-` is ASCII and
    // both encodings behind `OsStr` are self-synchronising there, so a leading
    // `b'-'` cannot be the tail of some other character however the rest of the
    // argument is spelled.
    match arg.as_encoded_bytes().first() {
        Some(b'-') => Request::NoSuchOption,
        _ => Request::Watch,
    }
}

/// Tell a frame what the shell's view defaults ask it to walk.
///
/// **One line, and it is a seam rather than a convenience.** `App` holds what was
/// *asked for* and `Frame` holds what is *walked*, which is the same split
/// `Chrome::rail` and `Body::rail` draw one region over, and everything that sets
/// the first has to set the second. Written twice they drifted immediately: the
/// keypress was wired and the file was not, so `staged = on` set a flag nothing
/// acted on.
///
/// **This is the startup half.** `Action::ToggleStaged` sets both in one arm of
/// `App::apply`, where the frame is already in hand and the pairing is one
/// statement; naming it there too would mean handing `apply` a borrow of the `App`
/// it is already mutating. Two call sites, one rule, and the rule is stated here
/// because this is the one a reader will not think to look for.
///
/// **Takes the [`Config`] rather than an [`App`]**, which is the whole of what it
/// needs: building an `App` to read one bool and then building the real one three
/// lines later said the seam was about the shell when it is about the file.
///
/// `pub` and `doc(hidden)` for `tests/config.rs`, which drives the startup path
/// without a terminal.
#[doc(hidden)]
pub fn arm_frame(frame: &mut vigia_core::Frame, config: crate::Config) {
    frame.show_staged(config.staged);
}

/// Watch the working tree at `path` and draw it until the reader quits.
pub fn run(path: &Path) -> Result<(), Failure> {
    let worktree = Worktree::discover(path)?;
    let mut frame = worktree.frame();

    // Same rule, same reason, one input over: a `VIGIA_THEME` that names nothing
    // or a file that does not parse has to be said on a terminal the reader can
    // still read. `SPEC.md` §11.1 states it for a path that is not a repository,
    // and an error painted inside a TUI that then hands the terminal back is an
    // error nobody sees.
    //
    // Resolved to the depth here as well, so the palette the renderer holds is
    // already in colours this terminal can show and the frame path never
    // quantises. I9 therefore sees none of it.
    // **The one question the shell asks its terminal** (`SPEC.md` §11.2 B18,
    // [#325](https://github.com/breferrari/vigia/issues/325)): the background,
    // OSC 11, before the event reader exists, because crossterm's parser
    // destroys any reply it does not recognise once the takeover is reading.
    // At most 150ms, and only where nothing has already chosen: the variable
    // and the theme file still win inside `from_env`, and a terminal that
    // stays silent costs the wait once and keeps the fallback.
    let detected = terminal::background(std::time::Duration::from_millis(150));
    let theme = theme::from_env(Depth::detect()?, |key| std::env::var(key).ok(), detected)?;

    // Beside the depth and for the same reason: a property of the terminal,
    // resolved once before the screen is taken, so the frame path never asks the
    // environment anything. Refused rather than defaulted when the variable is
    // set to something this does not recognise, which is `VIGIA_COLOR`'s rule and
    // reaches the reader on a terminal they can still see, exactly as the line
    // above does. **Not** when the variable is unreadable: `env::var` drops a
    // non-UTF-8 value, so that case falls through to detection rather than
    // refusing, which is the same behaviour the depth ladder has and is stated
    // here because the two are easy to conflate.
    let glyphs = Glyphs::detect()?;

    // **Beside the palette and for the palette's reason**, which is the whole of
    // why it is here rather than inside `App`: a config file that does not parse
    // has to be said on a terminal the reader can still read, and an error painted
    // inside a TUI that then hands the terminal back is an error nobody sees.
    // `SPEC.md` §11.2 B6 as amended by
    // [#306](https://github.com/breferrari/vigia/issues/306).
    //
    // Read once, before the screen is taken, so the frame path never asks the
    // filesystem what the reader prefers. Absent is not an error; unreadable is.
    let config = config::from_env(|key| std::env::var(key).ok())?;

    // **The view defaults reach the frame before its first walk, not just the
    // shell**. Three of the four keys decide how rows the frame already holds
    // are arranged, so setting them on `App` is all they need. `staged` decides
    // what the frame *walks*, so a reader whose file says `staged = on` has to
    // be honoured here or the key sets a flag nothing acts on and the opening
    // pane is the one they were trying to change. Same defect
    // `Action::ToggleStaged` had against the keypress, on the path that has no
    // keypress to trigger it.
    //
    // **Which is why the first walk sits below the config read.** Run three
    // lines after `Worktree::discover`, before this file has been looked at, no
    // amount of telling the frame afterwards reaches it without walking twice.
    // The rule that puts it early is untouched and is why it is
    // still above `Session::enter`: a repository that fails on its first walk
    // reports on a terminal the reader can still see, and so does a config file
    // that does not parse. The two now happen in the order they are needed in.
    arm_frame(&mut frame, config);
    frame.advance()?;

    // Inert until something sends: it costs nothing, wakes nobody, and I1 never
    // sees it. Built here because the handler on it is armed on the next line,
    // before the terminal is taken, and the other two senders are armed after the
    // first paint.
    let (tx, rx) = mpsc::channel();

    // **Before the terminal is taken, which is the whole point of it being here.**
    // `Session::enter` installs the panic hook before its first step for exactly
    // this reason: the window a safety net has to cover starts at the *first* step
    // of the takeover, not after the fourth. Armed afterwards, a signal arriving
    // between raw mode and the cursor was still an uncaught kill, and two earlier
    // versions of this line got that wrong in two different ways: inside the
    // struct literal it also sat after `Highlighter::new`'s 318µs grammar load,
    // because fields evaluate in written order.
    //
    // A signal that arrives before the loop exists is not lost. It waits in the
    // channel, and the first `recv` below acts on it, by which point there is a
    // `Session` to drop and a terminal to give back.
    //
    // Reported rather than fatal. A monitor that refused to open because it could
    // not arm a safety net would be a worse answer than one that opens and says
    // so, so the outcome is carried to where there is an `App` to warn through.
    let armed = signal::forward(tx.clone());

    let mut shell = Shell {
        session: Session::enter()?,
        app: App::configured(config),
        // Its 318µs of grammar *loading* lands before first paint, which is
        // where it belongs: I7 gives startup 50ms, so this is well under one
        // percent of it and deferring it would only move it onto the first frame
        // that draws something.
        //
        // **Loading is not compiling, and only the small half is this line.**
        // `syntect` hands each pattern to `fancy_regex` on first use, which is
        // 74-362ms per grammar, and that is why the first frame below draws
        // plain. The earlier version of this comment cited I7 "measuring 20ms",
        // which was the core's frame path from an example that builds no
        // highlighter at all; the shipped first paint measured 105.03ms.
        //
        // Not "before the screen is taken": struct fields evaluate in written
        // order and `Session::enter` is written above, so the alternate screen
        // is already ours by the time this runs. The placement is right and
        // that reason is
        // wrong.
        //
        // That same evaluation order is why the signal handler is armed *above*
        // this literal rather than as a field beside `session`: a field here
        // would run after this load, and the window it exists to cover opens
        // before the takeover's first step.
        highlighter: Highlighter::new(),
        // Empty at startup, so every file in an already-dirty worktree draws
        // cold until something writes to it. That is the honest first frame: a
        // monitor has no way to know what happened before it was looking, and
        // inventing a recency for it would light up rows nothing has touched.
        history: History::new(),
        theme,
        glyphs,
        name: short_name(worktree.workdir()),
        root: worktree.workdir().to_string_lossy().into_owned(),
        branch: None,
        elsewhere: 0,
        screen: View::default(),
        regions: Regions::default(),
        held: None,
        grabbed: None,
        hovered: None,
        scrolling: None,
        scrolling_until: None,
        served: Vec::new(),
        written: false,
        warming: None,
    };

    // The arming from above, reported now that there is somewhere to report it. A
    // signal that arrived before this point is not lost: it waits in the channel
    // and the first `recv` below handles it.
    //
    // **A notice rather than the durable header mode `WatchLost` also sets**, and
    // the two are not the same shape. A lost watch changes what every later frame
    // *is*, so a reader looking at a stale diff has to keep being told. An
    // unarmed handler changes nothing on screen and matters only at the moment
    // somebody kills the process, so being told once, on the first frame, is
    // being told. Stated here because the transience is a choice.
    //
    // The wording says *stop* rather than *signal*, because I8 is one guarantee
    // over two mechanisms and half the readers of this line are on a platform
    // with no signals at all.
    if let Err(e) = armed {
        shell.app.warn(format!(
            "not catching an external stop, so a kill may not restore the terminal: {e}"
        ));
    }

    // **For a screen with rows on it, so a clean worktree spawns nothing.**
    // Starting a monitor on a tree nobody has touched is an ordinary way to
    // start one, and there is no grammar to compile for an empty state.
    //
    // `take` before `map`, not after: `warm_ahead` considers at most
    // `WARM_FILES` paths, so cloning the rest would be ten thousand `String`
    // allocations at the scale
    // [#48](https://github.com/breferrari/vigia/issues/48) contemplates, every
    // one of them dropped unread.
    //
    // **The handle is kept now rather than dropped**, and it is `Shell::warming`
    // for the reason that field exists: a warm in flight is what stops the next
    // one being spawned. Dropped, the `request_warm` below saw no warm running
    // and started a second thread over the same paths the moment the opening
    // frames raised their demand. Nothing broke, because the compiled patterns
    // land in one shared `SyntaxSet` either way, but it is two threads and two
    // wakes for one grammar and the bound that field documents was not being
    // held. Detached still: it is never joined, only asked whether it is done.
    //
    // **Above the draw, so it overlaps the frame that compiles.** Below it,
    // the warm starts only once paint two has finished paying the 74-362ms
    // for what is on screen, which turns `max(paint, warm)` into their sum
    // and widens the window in which a first scroll below the fold meets a
    // cold grammar. That window is the whole reason this thread exists.
    //
    // **It sends a wake now, where before it reported to nobody.** A hunk whose
    // grammar nothing has compiled draws plain (`Highlighter::wanted`), so the
    // frame that finishes this warm is the frame that has colour to add, and a
    // loop blocked on `recv` has no other reason to look.
    if !frame.files().is_empty() {
        shell.warming = Some(
            shell.highlighter.warm_ahead(
                worktree.workdir().to_path_buf(),
                frame
                    .files()
                    .iter()
                    .take(vigia_core::WARM_FILES)
                    .map(|change| change.path.clone())
                    .collect(),
                Some(warmed(&tx)),
            ),
        );
    }

    // **One call, two frames.** `Shell::draw` settles the repaint debt itself,
    // so the opening is one mechanism rather than two statements in a row that a
    // future edit can separate. `App::paint` records why that matters: deleting
    // the second statement left the whole suite green while the product sat on a
    // permanently uncoloured screen.
    //
    // The alternate screen is already taken by the line above, so before this
    // the reader watched the whole 74-362ms grammar compile happen on a blank
    // one. Measured over the hundred-file fixture: 105.03ms to first paint
    // before, 13.26ms now.
    shell.draw(&mut frame, &worktree, Instant::now())?;

    // Armed only now. Everything above read `.git/index` and the gitignore
    // files, and a watch armed before those reads observes them: inotify and
    // FSEvents both report reads and attribute touches, so the shell would wake
    // itself up once for free at startup. The channel they send on is older than
    // they are: the signal handler above shares it, and has the opposite
    // requirement about when it is armed.
    spawn_watch(path.to_path_buf(), tx.clone());

    // **What the tree is made of, which the changed set cannot say on a tree
    // nobody has written to yet.** A pane is opened beside an agent *before* the
    // agent writes, so the warm above very often has nothing to warm, and then
    // the agent's first write arrives under a grammar nothing has compiled.
    //
    // Below the draw rather than above it, unlike the warm over the changed set:
    // that one is racing the frame that draws those very files and has to
    // overlap it, while this one reads `.git/index` and would put that read on
    // the path I7 measures. Nothing on screen is waiting for it.
    shell
        .highlighter
        .warm_repository(worktree.workdir().to_path_buf(), Some(warmed(&tx)));

    // Demands the opening two frames raised, dispatched before the loop blocks.
    // Without this the screen keeps whatever the two paints managed until the
    // agent's next write, which on a tree nobody is touching is never.
    shell.request_warm(&worktree, &tx);

    // Cloned rather than moved, because the loop below keeps `tx` to hand a
    // sender to each warm it spawns. That the channel can therefore never
    // disconnect is not a change: `spawn_input` reports the one end that
    // matters as `Wake::InputLost` before its thread returns, and the
    // disconnect arm below was already unreachable through it.
    spawn_input(tx.clone());

    // Reused across iterations rather than allocated per wake. A monitor is left
    // open for days and I3 is the invariant that notices, so the one buffer the
    // loop needs is the one buffer it keeps.
    let mut batch = Vec::with_capacity(DRAIN_CAP);

    // **Three clocks now, and `Shell::patience` is the seam that keeps every one
    // of them honest.** With `None` from it the receive below is untimed, which
    // is I1's *0 wakeups while idle* unchanged and unmeasurable-away; the
    // invariant is that nothing can arm a clock without going through that
    // function. `SPEC.md` §11.1 carries all three rulings.
    //
    // A press on a bar's end arms the repeat and it dies with the release. A
    // scroll arms the direction mark and it dies `SCROLL_LINGER` later. And a
    // write arms the ageing roll, which dies when the history window empties
    // `HISTORY_WINDOW` after the last one. **This comment said "the only clock"
    // through two of those three**, which is what a header describing a
    // mechanism costs when the mechanism grows underneath it.
    'awake: loop {
        // Untimed with nothing held, which is the whole invariant. With something
        // held the wait is only as long as the next step is away, so the loop
        // still blocks rather than spinning.
        let wake = match shell.patience(Instant::now()) {
            None => match rx.recv() {
                Ok(wake) => Some(wake),
                Err(_) => break 'awake,
            },
            Some(patience) => match rx.recv_timeout(patience) {
                Ok(wake) => Some(wake),
                // The repeat fell due with nothing else to do, which is the
                // ordinary case while a button is down.
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => break 'awake,
            },
        };

        // **The step, folded to however many intervals actually elapsed.** One
        // `apply` and one paint whatever the terminal has been doing, so the rate
        // is a fact about time rather than about paint speed. Taken before the
        // drain so a repeat that coincides with a filesystem tick still lands in
        // that tick's frame rather than in one of its own.
        let repeat = shell.held.and_then(|hold| hold.fire(Instant::now()));
        if let Some((step, next)) = repeat {
            shell.held = Some(next);
            // **The third of three, and it joined last.** `Regions::step_at`
            // yields only `Scroll` and `ScrollList` today, neither of which reads
            // a height, so the literal zero this replaced was right by accident
            // rather than by rule. It is the identical shape to the drag site
            // [`Shell::diff_rows_for`] was extracted for, and the identical shape
            // to `Action::Bottom` being classified wrongly: a call site that
            // decides a height instead of asking for one.
            //
            // No test drives this loop, so what protects it is that there is
            // one function every site calls rather than three answers that can
            // drift apart.
            let height = shell.diff_rows_for(step, frame.files())?;
            match shell.app.apply(step, &mut frame, height) {
                Ok(true) => {}
                Ok(false) => break 'awake,
                Err(e) => shell.app.warn(e.to_string()),
            }
        }

        let Some(wake) = wake else {
            // A timeout woke this, so there is nothing to drain and the paint
            // below is the whole of the frame. Either a step fell due, which the
            // block above has already applied, or a scroll burst went quiet and
            // the arrows stop claiming a direction.
            // **No `Frame::advance` on this path**, deliberately: an ageing wake
            // is not a filesystem event, nothing on disk changed, and walking
            // status anyway is the difference between an ageing wake and a tick,
            // which `SPEC.md` §11.1 prices. The window is rolled by `Shell::draw`
            // rather than here, for the reason that function's own comment gives.
            let began = Instant::now();
            shell.settle_scroll(began);
            shell.app.sample_memory();
            shell.draw(&mut frame, &worktree, began)?;
            shell.request_warm(&worktree, &tx);
            // **A timeout is a frame and belongs in the frame time the bar
            // reports**, which `SPEC.md` §5.1 defines as the whole turn of this
            // loop. It was absent while the only timeouts were a held step's,
            // and [#243](https://github.com/breferrari/vigia/issues/243) makes
            // this the most common frame on a quiet tree: a p99 that could not
            // see it would be reporting on the frames a reader is *not* looking
            // at.
            //
            // **And recording them cannot flatter the number I9 rests on**,
            // which is the obvious objection and it does not hold. A burst is
            // followed by up to `HISTORY_SAMPLES` cheap ageing wakes, and
            // `FRAME_SAMPLES` is 128, so on a quiet tree they do come to own most
            // of the ring. But I9 claims a *p99*, and p99 of 128 samples is the
            // worst or second worst of them: one expensive frame among a hundred
            // and twenty cheap ones still sets it. What moves is the p50, and it
            // moves to the truth, because on a tree that nothing is writing to
            // these genuinely are the frames. Dropping them to keep the median
            // describing something else would be curating the sample to protect
            // the claim, which is the failure `SPEC.md` §7 is about.
            shell.app.record_frame(began.elapsed());
            continue;
        };
        // Started before the drain, not after it, because the drain is part of
        // what a frame costs. `SPEC.md` §5.1 rules that the number on the status
        // bar is the whole turn of this loop, and the batch that a flick of a
        // trackpad produces is exactly the case where a narrower definition
        // would flatter the readout: sixty-four wakes handled and one paint is
        // one frame to a reader, whatever it is to the channel.
        let began = Instant::now();
        drain(&mut batch, wake, &rx, DRAIN_CAP);

        for wake in batch.drain(..) {
            match wake {
                // Returning rather than breaking, so the reason travels with the
                // exit. `shell` drops on the way out, which puts the terminal back
                // before `main` prints this where the reader will see it.
                Wake::InputLost => {
                    return Err("terminal input ended, so there was no way left to quit".into());
                }
                // The quit key's arm without the key, so `break` and not
                // `return`: nothing failed, and a message printed after the
                // terminal came back would be a message the sender did not ask
                // for. Breaking drops `shell`, which drops `Session`, which is
                // what puts the terminal back — the same three steps every other
                // exit takes, which is the whole reason the handler in `signal`
                // restores nothing itself.
                Wake::Signalled => break 'awake,
                Wake::Input(event) => {
                    // **Checked before the event is interpreted**, because a
                    // release is not an action and would otherwise fall through
                    // the `else` below with the repeat still armed. `Held::ends`
                    // carries the five ways a hold finishes and why.
                    // **The regions the last paint actually drew.** A pointer is
                    // told what it is over by asking the same function `render`
                    // asks, against the view that is on screen, so the wheel can
                    // never scroll the region beside the one under it. Free: no
                    // syscall and no allocation, unlike the height below, and a
                    // gesture that arrives before the first paint sees
                    // `Regions::default()`, which is a screen with no region and
                    // no bars.
                    let regions = shell.regions();
                    if shell.held.is_some_and(|hold| hold.ends(&event, regions)) {
                        shell.held = None;
                    }
                    // **What the pointer is over, before anything asks what it
                    // meant.** A mark is not an action and never becomes one
                    // (`SPEC.md` §11.1), so this sits outside the `action_for`
                    // path entirely: the `continue` below drops events that
                    // request nothing, and a motion is exactly such an event, so
                    // resolving after it would leave the mark answering the
                    // pointer's last *click* rather than its position.
                    //
                    // **The position is load-bearing against the drag block too,
                    // and that is the less obvious half.** A `Drag` that
                    // `drag_action` accepts `continue`s from inside that block,
                    // so moving this below it would skip resolution on every
                    // applied drag and leave the mark lit for the whole gesture,
                    // which is exactly what `hover_after`'s drag arm exists to
                    // prevent. Order against `Held::ends` above is free; order
                    // against the block below is not.
                    //
                    // The whole rule is in `hover_after` rather than here, for
                    // the reason `Held::ends` is a free function: this loop
                    // cannot be driven by a test, and a rule written inline is a
                    // rule with no gate.
                    shell.hovered = hover_after(&event, regions, shell.hovered);
                    // **A drag under way answers before the column is consulted,
                    // and that ordering is the fix.** `action_for` asks what is
                    // under the pointer, which is the right question for a press
                    // and the wrong one for a hand already moving: a reader
                    // dragging a one-column bar leaves that column immediately
                    // and the gesture ends there. The grip decides instead, and
                    // the row is clamped so pulling past either end holds that
                    // end.
                    if let Some(on) = shell.grabbed {
                        if let Some(drag) = drag_action(&event, regions, on) {
                            // **The height, because a drag on the diff's bar is a
                            // `DiffTo` and `DiffTo` reads one.** This block passed
                            // zero from the day it was written, and the ordinary
                            // path forty lines down has always asked
                            // `Action::needs_height`. So the **first** event of a
                            // drag resolved through `action_for` with a real
                            // height and every motion after it resolved here
                            // without one, which maps the track onto the whole
                            // rather than onto travel: the thumb runs past its own
                            // bottom and the last screenful of track is dead.
                            // That is the arithmetic
                            // `tests/scroll.rs::dragging_the_diff_bar_resolves_to_a_row_and_reaches_the_end`
                            // pins for the press, one gesture over from where the
                            // reader spends the rest of the drag.
                            //
                            // Found on this branch and not caused by it
                            // ([#297](https://github.com/breferrari/vigia/issues/297)'s
                            // audit): the pinned arm inherits the same parameter,
                            // so fixing it here fixes both.
                            let height = shell.diff_rows_for(drag, frame.files())?;
                            match shell.app.apply(drag, &mut frame, height) {
                                Ok(true) => continue,
                                Ok(false) => break 'awake,
                                Err(e) => {
                                    shell.app.warn(e.to_string());
                                    continue;
                                }
                            }
                        }
                        // Anything that is not a motion ends it: a release, a
                        // key, a pointer that moved with nothing down.
                        if Grabbed::ends(&event) {
                            shell.grabbed = None;
                        }
                    }
                    // **Armed from the same press that performs the first step**,
                    // so a click is one step and a hold is that step continued.
                    // `Regions::step_at` is the geometry both halves read, which
                    // is what stops the repeat and the press disagreeing about
                    // where a button is.
                    if let Event::Mouse(mouse) = &event
                        && matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
                        && let Some(step) = regions.step_at(mouse.column, mouse.row)
                    {
                        shell.held =
                            Some(Held::new(step, (mouse.column, mouse.row), Instant::now()));
                    }
                    // A press on the **track** takes hold of that bar instead,
                    // and keeps it until the button comes up.
                    if let Event::Mouse(mouse) = &event
                        && matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
                    {
                        shell.grabbed = regions.grab_at(mouse.column, mouse.row);
                    }
                    let Some(action) = action_for(&event, regions) else {
                        // Not every event is a request. Redrawing for a key release
                        // or a mouse move would make the idle cost non-zero for a
                        // reason nobody asked for.
                        continue;
                    };
                    // Asked for only by the one action that reads it, and that is
                    // the drain's doing rather than tidiness. `Shell::area` is an
                    // uncached terminal-size syscall and `chrome` allocates,
                    // and a full repaint per event amortises both. With
                    // sixty-four notches arriving between two paints, computing
                    // a height none of them but `Page` reads would be sixty-four
                    // syscalls and several hundred discarded
                    // allocations inside one batch.
                    //
                    // The branch it carries is whatever the last draw settled on,
                    // which is right rather than merely cheap: it feeds
                    // `diff_height` alone, and neither the branch nor the mode can
                    // change how many rows the footer takes. See `Footer::plan`.
                    let height = shell.diff_rows_for(action, frame.files())?;
                    shell.note_scroll(action, Instant::now());
                    match shell.app.apply(action, &mut frame, height) {
                        Ok(true) => {}
                        // Out of the batch *and* out of the loop, without the draw
                        // below: leaving was asked for, and painting one more
                        // frame on the way out is a frame nobody asked for.
                        Ok(false) => break 'awake,
                        Err(e) => shell.app.warn(e.to_string()),
                    }
                }
                Wake::Tick(paths) => {
                    shell.app.clear_notice();
                    // **A tick is the world changing, so a demand that could not
                    // be served a moment ago is worth offering again.** See
                    // `Shell::served`: without this, one unservable demand would
                    // keep its file plain for the rest of the session however
                    // many times it was rewritten.
                    //
                    // A flag rather than `served.clear()`: a tick lands inside
                    // a running warm often during active editing, and clearing
                    // there destroys the record of what that warm was handed, so
                    // its result has nothing to be compared against.
                    shell.written = true;
                    // Sampled here and nowhere else, which is the whole of I10's
                    // relationship with I1: the window is real time, and the only
                    // thing that moves it is a wake the loop was already having.
                    // An empty burst still rolls the window and still leaves the
                    // pulse where it was, which is the staging case.
                    // **Sized, and the `stat` is here on purpose**
                    // ([#232](https://github.com/breferrari/vigia/issues/232)).
                    // Counting files written instead lets a worktree where one
                    // file is saved over and over put a one in every sample,
                    // become its own peak, and draw every active column of the
                    // band at full height: the element says *when* and never
                    // *how much*. Weighing a write by the bytes it moved is what
                    // gives both glance elements the shape `SPEC.md` §5.1 names
                    // them for.
                    //
                    // One `symlink_metadata` per changed path per tick, taken
                    // where the budget gates can see it rather than inside the
                    // store. It does not follow links, for `fingerprint`'s
                    // reason one crate over: the thing weighed has to be the
                    // thing that was written.
                    shell
                        .history
                        .record_sized(sized(worktree.workdir(), &paths), began);
                    // The core leaves the frame exactly as it was on failure, so the
                    // previous diff is still valid to draw. Saying so on the footer
                    // beats blanking a pane for a reason the reader cannot see.
                    match frame.advance() {
                        // Advance first, follow second, and the order is the
                        // whole of it: the path is looked up in the file list,
                        // and before the walk that list is the previous frame's.
                        // Following into it would jump to wherever that file used
                        // to sit, which is the shape of a bug that only appears
                        // when the list changes length.
                        Ok(()) => {
                            if let Some(path) = paths.last() {
                                shell.app.follow(path, &frame);
                            }
                        }
                        Err(e) => shell.app.warn(e.to_string()),
                    }
                }
                // Both halves, and they are not the same half twice. The mode is
                // durable and goes to the header; the message says which failure it
                // was and goes to the footer, where a notice belongs. See
                // `App::watch_lost`.
                Wake::WatchLost(message) => {
                    shell.app.watch_lost();
                    shell.app.warn(message);
                }
                // Deliberately nothing. The patterns are compiled and sitting in
                // the `SyntaxSet` the highlighter already holds, so the paint
                // below is the whole of the response and any state changed here
                // would be state two mechanisms could disagree about.
                Wake::Warmed => {}
            }
        }

        // Before the paint, so the cell drawn below carries this frame's number
        // rather than the previous one's, and inside the timed region, so the
        // read's own cost lands in the frame time it sits beside. A readout
        // measured outside the thing it reports is the omission `SPEC.md` §7
        // names, and this one had the opportunity to make it twice.
        shell.app.sample_memory();

        // **Once per batch, not once per wake.** That is the whole of the
        // coalescing: every wake above was handled, in arrival order, and only
        // the paint is shared. See `drain`.
        shell.draw(&mut frame, &worktree, began)?;

        // **After the paint, because the paint is what raises the demand.**
        // `Highlighter::wanted` describes the frame that just drew, so asking
        // before it would be acting on the previous screen's answer. Inside the
        // timed region deliberately: spawning a thread is part of what this
        // frame cost and a readout that excluded it would be measuring a frame
        // the product does not have.
        shell.request_warm(&worktree, &tx);

        // After the paint, because the paint is the last third of what a frame
        // costs. The consequence is that the p99 drawn above is always the
        // *previous* frames' and never this one's, which is the only thing a
        // frame can honestly say about itself.
        //
        // Not reached on the quit path, which breaks out above without drawing:
        // a frame nobody saw is not a frame, and recording it would put a
        // half-frame into the window a later session never uses anyway.
        shell.app.record_frame(began.elapsed());
    }

    Ok(())
}

/// What each path in one wake's burst now holds, for [`vigia_core::History::record_sized`].
///
/// **Every path, and the cap that was here twice is the finding.** A burst
/// reports up to `HISTORY_PATHS` paths on one coalesced wake, and sizing all of
/// them measures **2.60ms of thread CPU** in isolation against I9's 16ms. That
/// number is real and it is not the cost, which is the whole lesson: measured in
/// the frame it actually sits in, interleaved over thirty rounds of a hundred-file
/// bulk rewrite, the frame costs **18.43ms sizing nothing, 17.39ms sizing
/// sixty-four paths and 17.93ms sizing all 256**. The unsized run is the slowest
/// of the three. `Frame::advance` walks status on the same wake and stats every
/// one of these paths to decide they changed at all, so by the time this runs the
/// metadata is warm and the marginal syscall costs nothing measurable.
///
/// **A component measured alone is not a budget.** `CLAUDE.md` asks for the
/// headroom to be quoted beside any cost that refuses something, and this was
/// refused twice on a number taken outside the frame: once on a wall figure
/// inflated by a loaded machine, and once on a CPU figure that was real and
/// answered a question nobody was asking. Both times the thing the cap protected
/// went unmeasured.
///
/// The duplicated work is still real and still worth removing: the walk already
/// has the size and `gix` does not surface it, which is
/// [#233](https://github.com/breferrari/vigia/issues/233). That is an efficiency
/// row, not a reason to draw a worse graph.
pub fn sized<'p>(
    workdir: &'p Path,
    paths: &'p [String],
) -> impl Iterator<Item = (&'p str, Option<u64>)> + 'p {
    paths
        .iter()
        .map(move |path| (path.as_str(), weigh(workdir, path)))
}

/// What a written path now holds on disk, for [`vigia_core::History::record_sized`].
///
/// **Private, because [`sized`] is what a caller wants.** The budget gates price
/// this syscall inside the frame they measure, and they reach it through `sized`
/// so a gate cannot end up sampling a shape the product does not have: what a
/// gate leaves out gets *cheaper*, so that drift would never fail.
///
/// **Does not follow links**, for `Frame`'s fingerprint rule one crate over: the
/// thing weighed has to be the thing that was written, and a link's own bytes are
/// what a write to it changes.
///
/// `None` for a path that cannot be read, which is a file that vanished between
/// the watch naming it and this call. The store weighs that at its floor rather
/// than at nothing, because it was still written.
fn weigh(workdir: &Path, path: &str) -> Option<u64> {
    match std::fs::symlink_metadata(workdir.join(path)) {
        // **A directory is not a write with a size**, and on the platforms where
        // it has one it is a lie: `relative` admits directory events, and
        // `symlink_metadata` reports 0 for a directory on Windows but 4096 and
        // rising on Linux and macOS. Weighing those would charge `mkdir` churn as
        // kilobytes against a path that is in no diff at all.
        Ok(meta) if meta.is_dir() => None,
        Ok(meta) => Some(meta.len()),
        // **A file that is gone weighs zero bytes, not "no size"**, and the
        // difference is the largest edit a reader can make. Mapping both to
        // `None` kept the store's baseline at whatever the file last held, so a
        // deletion weighed the floor of one and the *recreation* after it weighed
        // the dead file's whole size: the biggest change drawn as the smallest
        // and a trivial one drawn as a spike. Measured at two million bytes: the
        // delete scored 1 and the fifty-byte file that replaced it scored
        // 1,999,950.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Some(0),
        // Anything else is a size that could not be read rather than a file that
        // is not there: a permission change, a path that stopped being valid
        // Unicode under us. The store weighs those at its floor, because the
        // write happened and its magnitude is genuinely unknown.
        Err(_) => None,
    }
}

/// How long the direction arrows stay lit after the last scroll.
///
/// **Long enough to survive the gap between two key repeats, short enough that a
/// reader who stopped does not see a claim about the past.** A terminal's own key
/// repeat runs near 30ms once it gets going and its first gap is far longer, so
/// this covers the steady stream and expires on the pause. It is the only number
/// here that is a feel judgement rather than a measurement, and it is stated as
/// one.
pub const SCROLL_LINGER: std::time::Duration = std::time::Duration::from_millis(220);

/// Wakes taken in one go, so one gesture costs one paint.
///
/// **Sixty-four**, and the number matters in one direction only. A trackpad
/// reports a flick as a stream of scroll events rather than as one, and a full
/// redraw per event renders each notch in turn and falls behind the thumb, which
/// is what *"if it gets too fast, it struggles"*
/// describes. Draining removes that whole class, because the shell moves the
/// viewport by the flick and draws where it ended up.
///
/// The cap is not tuning. It is the guard against an event source faster than the
/// shell: without one, a stuck key or a build touching thousands of files could
/// keep the queue non-empty forever and the screen would never be painted again.
/// Sixty-four notches is far more than any one gesture and still a bounded amount
/// of work between two frames.
const DRAIN_CAP: usize = 64;

/// Take the wake that woke the loop, plus everything already queued behind it.
///
/// A pure function over the channel rather than a loop inline in [`run`], for the
/// reason [`drain`] is one: `run` owns a terminal and cannot be driven from a
/// test, so a rule left inside it is a rule nothing can gate.
///
/// **Nothing is dropped.** Coalescing here is about the *paint*, not about the
/// events: every wake is handed back and handled in arrival order, so a tick still
/// records its paths for I10, a scroll still moves the viewport by its own rows,
/// and `Quit` still arrives. A version that kept only the last wake would be
/// shorter and would lose the history the glance strip is drawn from.
///
/// `batch` is passed in rather than returned so the caller can keep one buffer for
/// the life of the process. `try_recv` fails on empty and on a hung-up sender
/// alike, and both mean the same thing here: there is nothing more to take right
/// now. A disconnect is then reported by the `recv` that follows.
fn drain(batch: &mut Vec<Wake>, first: Wake, rx: &Receiver<Wake>, cap: usize) {
    batch.clear();
    batch.push(first);
    while batch.len() < cap {
        match rx.try_recv() {
            Ok(wake) => batch.push(wake),
            Err(_) => break,
        }
    }
}

/// The terminal and everything drawn onto it that outlives one frame.
struct Shell {
    session: Session,
    app: App,
    /// The syntax classes of whatever is on screen, kept between frames.
    ///
    /// Held here rather than in [`App`] so that type stays cheap to clone; see
    /// [`App::view`]. Bounded by the viewport rather than by the diff, so a day
    /// of scrolling leaves it the size of one screen.
    highlighter: Highlighter,
    /// What changed recently: the source for the sparkline, the recency gradient
    /// and the pulse.
    ///
    /// The one thing the shell keeps that deliberately outlives the diff. A file
    /// that settles leaves [`vigia_core::Frame`]'s cache and must not leave this,
    /// or the strip would empty exactly when a reader glances over to ask what
    /// was busy. I10 bounds it instead, by a window and a path cap rather than by
    /// the session.
    history: History,
    theme: Theme,
    /// Which glyphs the sparkline may draw from, resolved once at startup.
    ///
    /// Held beside [`Shell::theme`] because it is the same kind of value: a
    /// property of the terminal, decided before the screen was taken and
    /// unchanged for the session. It reaches the renderer the way `theme` does,
    /// as its own [`render`] parameter, rather than riding [`Chrome`]: nothing
    /// in the layout reads it, so a per-frame field meant two of its three
    /// callers stamping something the function they fed never looked at.
    glyphs: Glyphs,
    /// What the header calls the working tree.
    name: String,
    /// The worktree's absolute path, spelled once for the links' `file://`
    /// targets.
    root: String,
    /// What the header calls the branch, or `None` when there is none to call.
    ///
    /// Refreshed per draw rather than held for the session, because an agent in
    /// the other pane can check out a branch and a name cached at startup would
    /// then be a confident lie.
    ///
    /// **Every frame since [#158](https://github.com/breferrari/vigia/issues/158)**,
    /// where it was the empty state's alone. `None` now means a detached HEAD
    /// rather than a populated frame, which is the only case that draws no
    /// branch anywhere.
    branch: Option<String>,
    /// How many changes the run this pane is **not** drawing holds.
    ///
    /// Zero on every frame but the one that draws the empty state, which is what
    /// keeps I4 true for the walk behind it: a count costs a status walk, and it
    /// is spent only where it is drawn. Same rule [`Shell::branch`] follows one
    /// field up.
    ///
    /// **What it buys is the difference between a clean tree and a fully staged
    /// one**, which `no unstaged changes` says nothing about and which is the
    /// whole of [#313](https://github.com/breferrari/vigia/issues/313)'s report.
    elsewhere: usize,
    /// The last view collected successfully.
    ///
    /// Painted again when collecting a new one fails, which is why it is kept at
    /// all. A monitor showing a stale diff with the reason on its footer is more
    /// use than one showing an empty pane, and it is the same promise the core
    /// makes about a failed [`vigia_core::Frame::advance`]. Bounded by the screen,
    /// not by the diff, so keeping it costs nothing I3 would notice.
    screen: View,
    /// Where the last painted screen's regions and scrollbars were.
    ///
    /// Held so a mouse gesture can be told what it is over without a terminal
    /// syscall per event, and so the answer describes the screen a reader is
    /// actually pointing at rather than the one the next paint will make.
    regions: Regions,
    /// What a mouse button is currently being held down on, if anything.
    ///
    /// **Here rather than in [`App`] because it is a fact about the terminal, not
    /// about the viewport.** A hold begins and ends on input events, and `App`
    /// owns where the reader is looking; giving it a field it never reads would
    /// put a gesture's lifetime inside the thing the gesture moves. The paint
    /// reads it to light the pressed button and nothing else does.
    held: Option<Held>,
    /// The bar a drag is currently moving, if one is.
    ///
    /// **Separate from [`Shell::held`] because they are different gestures on the
    /// same column.** A press on a step button repeats on a clock; a press on the
    /// track seeks, and keeps seeking wherever the pointer goes until it is let
    /// go. Only one can be armed at a time, because only one press starts them.
    grabbed: Option<Grabbed>,
    /// What the pointer is resting on, when it is on something a click acts on.
    ///
    /// **The mark `SPEC.md` §11.2 B10 adopts, and it is here for
    /// [`Shell::held`]'s reason**: where a pointer is, is a fact about the
    /// terminal rather than about the viewport, and `App` owns the viewport.
    ///
    /// It needs no clock, which is §11.1's three-mark rule arriving at its third
    /// case: a hold ends with an `Up`, a key burst has no end and takes a clock,
    /// and this is retired by its **replacement**, because the next mouse event
    /// says where the pointer is now.
    ///
    /// **The rule is in two functions, and both are free functions so that both
    /// have gates.** [`hover_after`] answers *what does this event do to the
    /// mark*, and [`hover_repainted`] answers *what does a repaint do to it*,
    /// which is a different question with a different subject: the first is
    /// about the pointer moving, the second about the screen moving underneath a
    /// pointer that did not.
    ///
    /// `None` is both *not over anything* and *the window is not focused*, which
    /// are the same drawn result and are deliberately not distinguished: the
    /// mark says the pointer is here, and it has nothing to say about why it is
    /// not.
    hovered: Option<Hovered>,
    /// Which way the viewport is currently being moved, and until when.
    ///
    /// **The one lit thing on this screen that nobody is touching.** A reader
    /// scrolling with `j` or `d` gets the matching arrow lit, because the arrows
    /// are the element whose whole job is *which way* and there is no reason the
    /// keyboard should not reach them.
    ///
    /// It needs an expiry where the other two gestures do not, and that is the
    /// honest cost: a release ends a hold and a key burst simply stops, sending
    /// nothing. Without `scrolling_until` the last arrow of a burst would stay
    /// lit forever on an idle tree, as a claim about the past. The clock that
    /// clears it is bounded by the burst that armed it, fires once, and is the
    /// same one `Held` uses.
    ///
    /// **Why this field and not the other marks**: §11.1 states the rule, which
    /// is that a mark is retired by whatever the program can still observe about
    /// its subject — its end, its replacement, or, failing both, a clock. A hold
    /// has an end. A burst has neither, which is why the expiry lives here and
    /// nowhere else. The rule is at one address on purpose, because the same
    /// reasoning restated per field is what left `Regions::step`'s doc claiming
    /// a held button cannot repeat for a day after it could.
    scrolling: Option<(Grabbed, isize)>,
    /// When the mark above stops being true.
    scrolling_until: Option<Instant>,
    /// The demand the last warm was handed, so a demand nothing can serve is
    /// asked for once rather than on every frame.
    ///
    /// **The backstop for a demand the core cannot mark**, which is a narrower
    /// set than it sounds and is not empty. `warm_run` marks the grammar it had
    /// a run at, so an ordinary vanished file stops being asked for; what it
    /// cannot do is mark the *right* grammar for a file it never opened whose
    /// extension lies about its language. A Qt `.ts` translation file draws as
    /// XML because the frame has its first line, and if it is deleted before the
    /// warmer opens it the warmer can only mark TypeScript, which the frame was
    /// never waiting on. Without this that hunk is demanded on every frame, and
    /// every demand spawns a thread and sends a wake.
    ///
    /// Overridden by [`Self::written`] rather than cleared, because a tick can
    /// land while the warm this describes is still running. See
    /// [`worth_warming`], which is where the rule lives so that it can be gated.
    served: Vec<String>,
    /// Whether the tree has changed since the last warm was spawned.
    ///
    /// A tick is the world changing, so a demand that could not be served a
    /// moment ago is worth offering once more. Set by the tick and spent by the
    /// spawn, so one tick buys exactly one re-offer.
    written: bool,
    /// The warm this shell last asked for, if any.
    ///
    /// **Held to be asked whether it has finished, never to be joined.** See
    /// [`Shell::request_warm`]: it is the one bound keeping the thread count to
    /// the grammars a session meets, and joining it would be a frame waiting on
    /// the compile the whole deferral exists to get off the frame.
    ///
    /// Detached on the way out like every other thread here, which
    /// [`vigia_core::Highlighter::warm_ahead`] is written for: it ends on its
    /// own and holds nothing but an `Arc` to the grammars.
    warming: Option<std::thread::JoinHandle<vigia_core::WarmReport>>,
}

impl Shell {
    /// The diff region's height for `action`, or zero where it reads none.
    ///
    /// **One expression, because there were two and they disagreed.** The
    /// ordinary path has asked [`Action::needs_height`] since the predicate
    /// existed; the drag-under-way path passed a literal `0` from the day it was
    /// written. A drag on the diff's bar is a [`Action::DiffTo`], which reads the
    /// height to map the track onto **travel** rather than onto the whole, so the
    /// first event of a gesture resolved correctly and every motion after it
    /// resolved a screenful short. The two ends of the track agree under both
    /// arithmetics and only the middle does not, which is why nothing noticed.
    ///
    /// **The classification is gated and this wiring is not**: it lives inside
    /// the takeover loop, which no test drives, so the answer is to leave
    /// one place that can be wrong rather than two that can disagree. A mutation
    /// that empties this function survives the suite and is recorded as such.
    ///
    /// Cheap by construction: a drained trackpad flick is up to sixty-four
    /// actions between two paints, and only the four that read a height pay for
    /// the terminal-size syscall and the `Chrome` this builds. **Four rather than
    /// three since `Action::Bottom` joined them**, which is B16's doing: pinned,
    /// `G` rests a file's last row on the bottom, and *the bottom* is a height.
    ///
    /// **Takes the changed set rather than its length**, since
    /// [#313](https://github.com/breferrari/vigia/issues/313). The body's split
    /// needs two numbers now — how many files the footer counts, and how many
    /// *rows* the list wants once its run separators are in — and they are equal
    /// only while one comparison is drawn. Passed as a pair at this seam they were
    /// got wrong immediately: `files` went to both, and a scroll step was measured
    /// in a diff up to two rows taller than the paint laid out. Handed the slice,
    /// neither can be supplied without the other.
    fn diff_rows_for(
        &mut self,
        action: Action,
        files: &[vigia_core::FileChange],
    ) -> Result<usize, Failure> {
        if !action.needs_height() {
            return Ok(0);
        }
        let chrome = self.app.chrome(
            &self.name,
            self.branch.as_deref(),
            self.pointing(),
            self.elsewhere,
            &self.root,
        );
        let area = self.area()?;
        Ok(diff_height(
            area,
            &chrome,
            files.len(),
            view::list_rows_wanted(files),
        ))
    }

    /// The cell a step button is being held on, for the frame that draws it lit.
    /// What the pointer is doing this frame, as the one value the chrome takes.
    ///
    /// Gathered here rather than at each call site because the chrome is built
    /// twice per paint and a second spelling of this list is a second chance to
    /// transpose two `Option`s that compile either way.
    fn pointing(&self) -> Pointing {
        Pointing {
            pressed: self.pressed(),
            gripped: self.gripped(),
            hovered: self.hovered(),
            scrolling: self.scrolling,
        }
    }

    fn pressed(&self) -> Option<(u16, u16)> {
        self.held.map(Held::at)
    }

    /// Which region's bar is being dragged, for the frame that draws its thumb
    /// lit.
    ///
    /// A plain read: converting the [`Grabbed`] it holds into that region's
    /// first row is an identity only while the regions are stacked.
    fn gripped(&self) -> Option<Grabbed> {
        self.grabbed
    }

    /// What the pointer is over, for the frame that marks it.
    ///
    /// A plain read. The staleness this would otherwise have to carry is handled
    /// where the layout actually moves: [`hover_repainted`] retires the mark
    /// inside the paint that changes the geometry, so anything reaching this
    /// accessor was resolved against the screen that is on show.
    ///
    /// **Two narrower rules were tried in this function first and both were
    /// wrong.** The reasoning is recorded on `hover_repainted` rather than here,
    /// because the second of them was a tautology that read as a fix, and a
    /// reader who finds this accessor plain deserves to know it was not always.
    fn hovered(&self) -> Option<Hovered> {
        self.hovered
    }

    /// How long the loop may block before something here has to act.
    ///
    /// **`None` is the whole invariant, and it is why every clock is asked
    /// through one function.** With nothing held, nothing lingering and nothing
    /// left in the history window this returns `None`, the receive below is
    /// untimed, and I1's *0 wakeups while idle* is a fact about the structure
    /// rather than about care taken elsewhere. Clocks asked separately is that
    /// many chances to leave a deadline armed on an idle monitor; asked together
    /// there is one place to be wrong and one place to gate.
    ///
    /// Where several are armed it is the nearest, because the loop has to wake
    /// for whichever comes first.
    ///
    /// **There were two of these until
    /// [#243](https://github.com/breferrari/vigia/issues/243) and this docblock
    /// said so through the commit that added the third**, which is what a comment
    /// counting the things below it costs. It is written without a number now.
    fn patience(&self, now: Instant) -> Option<std::time::Duration> {
        // The history's own deadline joins the two gesture clocks here rather
        // than at the receive, so `patience` stays the one place that decides
        // whether this program owns a timer at all.
        input::patience(
            self.held,
            self.scrolling_until,
            self.history.ages_in(now),
            now,
        )
    }

    /// Note which way an action is moving the viewport, so the bar can say so.
    ///
    /// Only the actions that move it, and only their direction: a jump to a file
    /// or a drag of the thumb is not a direction the arrows can honestly draw.
    fn note_scroll(&mut self, action: Action, now: Instant) {
        // The routing lives in `input::scroll_mark`, beside the key map it is a
        // fact about, and is driven directly by a test there. What is left here
        // is arming the clock that expires it.
        if let Some(mark) = input::scroll_mark(action, self.regions) {
            self.scrolling = Some(mark);
            self.scrolling_until = Some(now + SCROLL_LINGER);
        }
    }

    /// Clear the direction mark once its burst has stopped.
    ///
    /// **Returns nothing.** Returning whether anything changed, *"so the caller
    /// repaints only on the frame that actually turns it off"*, is a value no
    /// caller reads: the timeout arm this sits on draws unconditionally,
    /// because it is also where a held step and an ageing wake land. So the
    /// sentence described a caller that has never existed, and the value was free
    /// to be wrong.
    ///
    /// The comparison lives in [`input::settled`] for the reason that module's
    /// `patience` gives: what stays here owns a terminal and three threads and
    /// cannot be driven by a test. It was here, and inverting it passed the whole
    /// suite while the arrows never claimed a direction.
    fn settle_scroll(&mut self, now: Instant) {
        if input::settled(self.scrolling_until, now) {
            self.scrolling = None;
            self.scrolling_until = None;
        }
    }

    /// The drawable area of the terminal right now.
    ///
    /// **Taken from the terminal's own resized state rather than from a second
    /// size syscall**, so the area a frame is *planned* for is the area it is
    /// *painted* into. Calling `Backend::size` directly is two independent reads
    /// per frame: this one, deciding how many rows [`View::collect`] is asked
    /// for, and the one `Terminal::draw` makes inside its own `autoresize`. A
    /// resize landing between them leaves the collect sized
    /// for a screen the paint no longer had.
    ///
    /// That is not hypothetical on Windows. Entering the alternate screen under
    /// Warp changes the reported size (measured: 195x77 before, 199x75 after), so
    /// the two reads genuinely disagree, and they disagree on the very first frame
    /// where a monitor may then sit idle for minutes before anything wakes it.
    ///
    /// `autoresize` is the same call `draw` makes, so doing it here does not add a
    /// read: it moves the one that decides the layout to before the decision
    /// instead of after it, and `draw`'s own call then finds nothing to change.
    /// A resize that lands in the remaining window is repainted by the `Resize`
    /// event, which `input.rs` maps to [`Action::Redraw`].
    fn area(&mut self) -> Result<Rect, Failure> {
        let screen = self.session.screen();
        screen.autoresize()?;
        Ok(screen.get_frame().area())
    }

    /// Where the regions of the **last painted** screen were.
    ///
    /// Stored rather than recomputed, because recomputing needs the terminal's
    /// size, and that is an uncached syscall the drain deliberately does not make
    /// per event. It is also the honest answer: a pointer is over the screen a
    /// reader can see, which is the one that was last drawn, not the one the next
    /// paint will produce.
    fn regions(&self) -> Regions {
        self.regions
    }

    /// Hand the warmer whatever the last paint drew plain, and let it wake us.
    ///
    /// **The other half of the deferral.** `Highlighter::wanted` lists the paths
    /// whose grammar nothing has compiled, one per grammar, as of the frame that
    /// just drew; those hunks are on screen in plain text right now, and this is
    /// what turns them into colour.
    ///
    /// **One warm in flight at a time**, which is what keeps the thread count
    /// bounded by grammars rather than by frames. A demand that arrives while
    /// one is running is not lost and is not queued either: the running warm
    /// ends with a wake, that wake paints, that paint raises the demand again if
    /// it is still true, and this spawns the next. The loop is self-driving and
    /// it terminates, because the warmer marks every grammar it had a run at
    /// even when the file it meant to read has gone.
    ///
    /// **A finished handle is not the same as no handle.** Checking only for
    /// `Some` would spawn once and never again, since the shell detaches rather
    /// than joining; `is_finished` is what says the previous run is over without
    /// blocking a frame on it.
    fn request_warm(&mut self, worktree: &Worktree, tx: &Sender<Wake>) {
        if self
            .warming
            .as_ref()
            .is_some_and(|handle| !handle.is_finished())
        {
            return;
        }
        if !worth_warming(self.highlighter.wanted(), &self.served, self.written) {
            self.warming = None;
            if self.highlighter.wanted().is_empty() {
                self.served.clear();
                self.written = false;
            }
            return;
        }
        self.served = self.highlighter.wanted().to_vec();
        // Spent here, so one tick buys one re-offer of a demand that has not
        // moved. Left set, every frame after the tick would spawn again.
        self.written = false;
        self.warming = Some(self.highlighter.warm_ahead(
            worktree.workdir().to_path_buf(),
            self.served.clone(),
            Some(warmed(tx)),
        ));
    }

    /// Collect a screenful and paint it, settling any repaint it leaves owed.
    ///
    /// **The opening two frames are one call, and that is the point.** The first
    /// frame of a process draws plain, because a grammar's patterns compile on
    /// first use at 74-362ms and I7 gives the whole of startup 50ms; the frame
    /// after it colours. Written as two `draw` statements in [`run`] that held
    /// only by statement order, deleting the second left the entire suite green
    /// while the product sat on a permanently uncoloured screen for any tree
    /// nobody was writing to — an I5 failure, since a monitor is meant to be
    /// correct untouched. Found by mutation.
    ///
    /// So [`App::owes_repaint`] carries the debt and this collects it, which
    /// makes the pair impossible to separate by editing one line. It costs one
    /// extra frame once per process: measured at ~1.8ms on the hundred-file
    /// fixture, against the 91.51ms compile it exists to hide.
    ///
    /// **At most one repaint, and the bound is structural rather than argued.**
    /// Written as a `while` this spun forever: [`Self::paint`] swallows a failed
    /// collect into [`App::warn`] and returns `Ok`, while [`App::view`] advances
    /// its state only past the `?`, so a collect that keeps failing leaves the
    /// debt standing and the condition can never clear — 100% CPU with the
    /// alternate screen held and the quit key unreachable, which is the terminal
    /// this shell refuses to leave a reader in. It is reachable rather than
    /// exotic: `Frame::diff` re-reads any file written in the last two seconds,
    /// so a `git checkout` landing between the two paints does it.
    ///
    /// An `if` cannot spin, and a debt that survives it is simply carried to the
    /// next wake, where the reader is looking at the plain frame rather than at
    /// nothing. That is the same "report and keep the previous screen" rule the
    /// rest of the frame path already follows.
    fn draw(
        &mut self,
        frame: &mut vigia_core::Frame,
        worktree: &Worktree,
        now: Instant,
    ) -> Result<(), Failure> {
        // **The window is rolled here because this is where every frame
        // passes**. It sat on the loop's timeout arm first, which is where the
        // ageing clock's own wake lands, and that was wrong in a way only an
        // audit found: `recv_timeout` returns `Ok` whenever anything is queued,
        // so a stream of wakes that are not ticks never reaches that arm at
        // all. Pointer motion is exactly such a stream, and the takeover asks
        // for it, so moving the mouse over a quiet pane redrew a window that
        // never rolled: the freeze this row exists to fix, for as long as the
        // motion continued.
        //
        // Rolling *here* makes "the drawn window is current" true by
        // construction rather than by remembering it at each of the paths that
        // draw. It costs nothing on the frames that do not need it:
        // `History::record_sized` skips its projection when no sample boundary
        // was crossed and nothing was named, which is 20ns at the path cap.
        //
        // Before the paint, not after, or the frame draws the picture it was
        // woken to change and the next one corrects it a beat late.
        //
        // **`now` is the caller's, not this function's, and the two are not
        // interchangeable.** A tick records its burst at the top of the turn and
        // reaches here only after `Frame::advance` has walked status, so with a
        // fresh `Instant::now()` a sample boundary landing inside that gap rolled
        // away the very sample the burst had just written. `Track::shift` zeroes
        // the newest sample, so `recency` fell to `Live` and the batch drew with
        // no pulse on the one frame it caused, which is `SPEC.md` §11.2's B2 mark
        // failing exactly when it has something to say. One instant per turn of
        // the loop makes the roll and the tick agree about when *now* is, and the
        // gap closes by construction rather than by being small.
        self.history.record_sized([], now);
        self.paint(frame, worktree)?;
        if self.app.owes_repaint() {
            self.paint(frame, worktree)?;
        }
        Ok(())
    }

    /// One collect and one paint, with no view of what it leaves owed.
    fn paint(&mut self, frame: &mut vigia_core::Frame, worktree: &Worktree) -> Result<(), Failure> {
        // Before the chrome, because the chrome carries it, and from the
        // frame's own file count so the read happens on exactly the frames that
        // draw the answer. That is the whole of I4 for this read. **Read on
        // every draw, because every frame draws it**. This went through a
        // `branch_for` seam whose whole job was the guard *only the empty state
        // names a branch, so a populated frame must not read a file it will not
        // draw*. The **header** draws it always, so that premise is false and
        // the wrapper became the identity over its closure: a function taking a
        // `&Frame` it did not read, with a docblock arguing for a parameter
        // that no longer decided anything, and a gate asserting that `read()`
        // calls `read`.
        //
        // **The header rather than the masthead**, which
        // [#204](https://github.com/breferrari/vigia/issues/204) makes load
        // bearing rather than a nicety of wording: the band is hidden until a
        // reader presses `m`, so a branch that lived up there would put this back
        // to reading a file most frames do not draw. The header's ladder draws it
        // on every frame whatever the masthead is doing.
        //
        // The **rule** survives untouched and is now satisfied rather than
        // guarded: this is a read the frame is going to draw. Reading `.git/HEAD`
        // measures 56 to 69us against I9's 16ms, and it is not cached across
        // frames on purpose, because an agent in the other pane can check out a
        // branch and a name held from startup would be a confident lie on
        // exactly the screen that exists to orient a reader.
        self.branch = worktree.branch();

        // The chrome is built before the layout, not after, because the footer
        // takes a second line at narrow widths and `body_layout` has to know
        // whether this frame is one of those. `frame.files().len()` is the same
        // number `View::collect` will report as `View::files`, which is what
        // keeps this row budget and the renderer's layout in agreement: `render`
        // recomputes the same split from the same two inputs.
        let chrome = self.app.chrome(
            &self.name,
            self.branch.as_deref(),
            self.pointing(),
            self.elsewhere,
            &self.root,
        );
        let body = body_layout(
            self.area()?,
            &chrome,
            frame.files().len(),
            view::list_rows_wanted(frame.files()),
        );
        match self
            .app
            .view(frame, &mut self.highlighter, &self.history, body)
        {
            Ok(view) => self.screen = view,
            Err(e) => self.app.warn(e.to_string()),
        }

        // **On a frame with nothing to draw, where the work went.**
        // [#313](https://github.com/breferrari/vigia/issues/313) was reported as an
        // agent that stages its own work emptying the pane: `no unstaged changes`
        // is true of a clean tree and of a fully staged one alike, and the reader
        // had no way to tell them apart. Counting the other run turns that line
        // into `no unstaged changes · 3 staged`.
        //
        // **Decided from the screen that will be drawn, not from the frame**, and
        // the difference is a failed collect. `Shell::screen` keeps the previous
        // view when a collect fails, so a frame whose walk succeeded and whose
        // collect did not draws *last* frame's rows — and asked of `frame` this
        // would say "there are files, so no signpost" over an empty state that is
        // still on screen, taking the `· 3 staged` off the one line that explains
        // it. That is exactly the split [#158](https://github.com/breferrari/vigia/issues/158)
        // removed for the branch name, restated for this field, and it is why the
        // read sits below the collect rather than above it.
        //
        // **The rule that put it here is unchanged and is now satisfied rather
        // than guarded**: a count costs a status walk, and this is a walk the
        // frame is going to draw. A screen with a diff on it never asks. One
        // without has no diff to compute and reads nothing else, and the walk it
        // pays for is the cheaper of the two — the staged comparison touches no
        // file at all, measured at 430us against 2.15ms on a 200-file fixture.
        //
        // Asked only while the staged run is **off**, because with it on the pane
        // already holds both comparisons and an empty screen means both are empty.
        // A failed walk is not worth a notice: it costs the reader a hint on one
        // frame, and saying so on the footer would push a real notice off it.
        self.elsewhere = if self.screen.files == 0 && !self.app.staged() {
            worktree.count_of(vigia_core::Origin::Staged).unwrap_or(0)
        } else {
            0
        };

        // Rebuilt so a notice raised by the collect above reaches this frame
        // rather than the next one. Safe to differ from the chrome the height
        // came from: a notice cannot change how many rows the footer takes, by
        // construction. See `Footer::plan`.
        //
        // The *file count* can, and on a failed collect the screen drawn below
        // is the previous one, whose count may differ from the frame's. That
        // costs nothing worse than a row budget that was one out for a collect
        // which failed anyway: the renderer plans and draws from the same
        // `view.files`, so what reaches the screen is self-consistent either way.
        //
        // The **branch** has that shape too, and one direction of it is visible
        // rather than merely inconsistent. **That discrepancy is gone with
        // [#158](https://github.com/breferrari/vigia/issues/158)**: it existed
        // because the branch was decided from the *frame's* count while the
        // empty state was drawn from `view.files`, so a collect failing on the
        // way from a clean tree to a dirty one drew last frame's empty state
        // with no branch on it. The branch is not decided from a count any more.
        // What stands is the reason it is re-read rather than held: a name kept
        // across frames is a confident lie the moment the other pane checks
        // something out.
        let mut chrome = self.app.chrome(
            &self.name,
            self.branch.as_deref(),
            self.pointing(),
            self.elsewhere,
            &self.root,
        );
        // Borrowed out of `self` before the draw, not for style: the closure would
        // otherwise hold `&self` while `self.session` is borrowed mutably to reach
        // the terminal.
        let (theme, screen, glyphs) = (&self.theme, &self.screen, self.glyphs);
        let mut painted = Regions::default();
        let was = self.regions;
        self.session.screen().draw(|f| {
            let area = f.area();
            // Captured from inside the draw, because `Frame::area` is the size the
            // paint actually used: `Shell::area` reads it again and a resize
            // between the two would leave a pointer told about a screen nobody
            // saw. Same seam #59 found on the other side.
            painted = render::regions(area, &chrome, screen);
            // **A hover mark does not outlive a relayout, and it has to be
            // retired here, between the layout and the paint that uses it.** The
            // obvious placement is after the `draw`, and it does not work: the
            // frame has already gone to the screen carrying the stale mark, and
            // correcting the field only reaches the *next* paint. On an idle
            // tree there is no next paint (I1), so a mark "wrong for one frame"
            // is wrong until somebody writes to the worktree.
            //
            // The ordering here is not circular even though it looks it:
            // `render::regions` reads the chrome's *heights* (the notice, the
            // mode, the branch) and never its marks, so the layout is settled
            // before the mark is judged against it, and the paint below sees the
            // judged one.
            chrome.hovered = hover_repainted(chrome.hovered, was, painted);
            render(f.buffer_mut(), area, screen, theme, glyphs, &chrome);
        })?;
        self.hovered = chrome.hovered;
        self.regions = painted;
        Ok(())
    }
}

/// Forward coalesced working-tree changes onto the shell's channel.
fn spawn_watch(path: PathBuf, tx: Sender<Wake>) {
    std::thread::spawn(move || {
        let worktree = match Worktree::discover(&path) {
            Ok(worktree) => worktree,
            Err(e) => {
                let _ = tx.send(Wake::WatchLost(format!("not watching: {e}")));
                return;
            }
        };
        let mut watcher = match worktree.watch(WatchOptions::default()) {
            Ok(watcher) => watcher,
            Err(e) => {
                let _ = tx.send(Wake::WatchLost(format!("not watching: {e}")));
                return;
            }
        };

        // The tick says only that something changed, which is all the shell
        // needs: every tick triggers one status walk, and a walk finds whatever
        // the events missed. That is what makes a recursive watch's blind spot
        // over freshly created directories harmless here.
        while let Some(tick) = watcher.next_tick() {
            if tx.send(Wake::Tick(tick.paths)).is_err() {
                return;
            }
        }

        // Falling out of that loop should be unreachable: the only thing that
        // ends it is a `Stop`, and nothing here holds one. Saying so out loud
        // costs a line and turns "the pane quietly stopped updating" into
        // something the reader can see, which is the difference between a bug
        // they report and one they work around for a week.
        let _ = tx.send(Wake::WatchLost(
            "the watch ended; this diff is no longer live".to_owned(),
        ));
    });
}

/// Forward terminal events onto the shell's channel.
fn spawn_input(tx: Sender<Wake>) {
    std::thread::spawn(move || {
        while let Ok(event) = ratatui::crossterm::event::read() {
            if tx.send(Wake::Input(event)).is_err() {
                return;
            }
        }
        let _ = tx.send(Wake::InputLost);
    });
}

/// The last component of the worktree path, which is what a reader recognises.
///
/// Three steps, and the order is the whole of it.
///
/// **The path as given comes first**, so the name a reader typed is the name they
/// see. A worktree reached through a symlink keeps the link's name rather than
/// its target's, and the common case costs no syscall at all.
///
/// **Then the resolved path**, because `.` and `..` have no final component and
/// `vigia .` is the invocation this tool is named after. `gix` hands back the
/// workdir exactly as it was given, so that spelling headered the screen `.` for
/// a whole phase: the one thing the header exists to say was the one thing it
/// could not. Resolving is for **display only** and the result is thrown away
/// after its last component is taken. It must not reach anything that compares
/// paths, because it is `\\?\C:\…` on Windows and `/private/var/…` on macOS, and
/// [#30](https://github.com/breferrari/vigia/issues/30) is the record of what a
/// root matching no event path costs.
///
/// **Then the path itself.** A worktree at a filesystem root has no last
/// component by any route, and showing the root beats showing nothing.
fn short_name(workdir: &Path) -> String {
    if let Some(name) = workdir.file_name() {
        return name.to_string_lossy().into_owned();
    }
    if let Ok(resolved) = workdir.canonicalize()
        && let Some(name) = resolved.file_name()
    {
        return name.to_string_lossy().into_owned();
    }
    workdir.display().to_string()
}

#[cfg(test)]
mod tests {
    //! The one rule in this file that is arithmetic rather than plumbing.
    //!
    //! [`short_name`] is private and `run` owns a terminal, so nothing outside
    //! can reach it. `terminal.rs` already keeps its unit tests beside the code
    //! for the same reason.
    //!
    //! [`branch_for`] is deliberately not tested here. Its rule is about a
    //! **frame**, and driving it from a number typed into a unit test proves
    //! only that the function reads its own argument: the call site producing
    //! that argument is untestable there, and mutating it
    //! passed the whole suite. It is exported and gated against real frames in
    //! `tests/reads.rs` instead.

    use super::*;

    #[test]
    fn a_relative_worktree_root_still_names_the_folder() {
        // `vigia .` is the invocation the tool is named after, and it headered
        // the screen `.` for a whole phase. `gix` hands back the workdir as it
        // was given, `Path::new(".")` has no final component, and the fallback
        // then printed the path itself. The header's whole job is saying which
        // tree this is, and this was the one input where it could not.
        //
        // Both sides come from the process's own directory, so the assertion
        // holds wherever the suite is run from.
        let here = std::env::current_dir().expect("a current directory");
        let expected = here
            .file_name()
            .expect("the current directory has a name")
            .to_string_lossy()
            .into_owned();

        let drawn = short_name(Path::new("."));
        assert_eq!(drawn, expected);
        // Stated separately rather than left implied by the equality above. A
        // `file_name` is never `"."`, so the two say the same thing today, and
        // this one keeps saying it if the fixture ever changes.
        assert_ne!(drawn, ".", "the header named the path instead of the tree");
    }

    #[test]
    fn an_absolute_worktree_root_still_names_its_last_component() {
        // The other direction, and the one the fix could quietly break: a
        // resolved path must not start reporting a whole path, a drive prefix or
        // a `\\?\` extended-length form. This root exists, so resolution
        // succeeds and the answer has to come from the same place either way.
        let here = std::env::current_dir().expect("a current directory");
        let expected = here
            .file_name()
            .expect("the current directory has a name")
            .to_string_lossy()
            .into_owned();

        assert_eq!(short_name(&here), expected);
    }

    /// A tick naming one path, which is the cheapest [`Wake`] to build.
    fn tick(at: usize) -> Wake {
        Wake::Tick(vec![format!("src/mod_{at}.rs")])
    }

    fn paths(batch: &[Wake]) -> Vec<String> {
        batch
            .iter()
            .map(|wake| match wake {
                Wake::Tick(paths) => paths.join(","),
                _ => "other".to_owned(),
            })
            .collect()
    }

    #[test]
    fn a_burst_of_wakes_arrives_as_one_batch() {
        // The reported symptom, in the only form a test can hold it: a trackpad
        // reports one flick as a stream of events, and a redraw per event is
        // what this rules out. One batch is one paint.
        let (tx, rx) = mpsc::channel();
        for at in 1..=5 {
            tx.send(tick(at)).expect("send");
        }

        let mut batch = Vec::new();
        drain(&mut batch, tick(0), &rx, DRAIN_CAP);

        assert_eq!(
            batch.len(),
            6,
            "the batch took {} of the 6 wakes queued, so the rest are still \
             waiting and will each cost their own frame",
            batch.len()
        );
    }

    #[test]
    fn a_batch_preserves_arrival_order() {
        // Coalescing is about the paint and not about the events. Follow mode
        // moves to the path a tick names *last*, and the glance history is a
        // window over when each one arrived, so a batch that reordered or
        // dropped wakes would take the reader to the wrong file and draw a
        // recency gradient for a sequence that never happened.
        let (tx, rx) = mpsc::channel();
        for at in 1..=3 {
            tx.send(tick(at)).expect("send");
        }

        let mut batch = Vec::new();
        drain(&mut batch, tick(0), &rx, DRAIN_CAP);

        assert_eq!(
            paths(&batch),
            vec![
                "src/mod_0.rs",
                "src/mod_1.rs",
                "src/mod_2.rs",
                "src/mod_3.rs"
            ],
            "the wake that woke the loop has to come first and the queue has to \
             follow it in order"
        );
    }

    #[test]
    fn a_batch_stops_at_the_cap_so_the_screen_cannot_be_starved() {
        // The guard, and it is not a tuning knob. An event source faster than
        // the shell would otherwise keep the queue non-empty forever and the
        // screen would never be painted again: a stuck key, or a build touching
        // thousands of files. The cap is what turns "drain the queue" into
        // "drain a bounded amount of it".
        let (tx, rx) = mpsc::channel();
        for at in 0..50 {
            tx.send(tick(at)).expect("send");
        }

        let mut batch = Vec::new();
        drain(&mut batch, tick(999), &rx, 4);

        assert_eq!(batch.len(), 4, "the cap did not bound the batch");
        // And what it left behind is still there, rather than dropped on the
        // floor: the next `recv` picks up exactly where this stopped.
        assert!(rx.try_recv().is_ok(), "the remainder was discarded");
    }

    #[test]
    fn a_batch_with_nothing_behind_it_is_the_wake_alone() {
        // The ordinary case, and the one a cap could break by waiting for more.
        // `try_recv` must not block: an idle monitor that woke for one keypress
        // has to draw for that keypress and go back to sleep, which is I1.
        let (_tx, rx) = mpsc::channel::<Wake>();
        let mut batch = Vec::new();
        drain(&mut batch, tick(0), &rx, DRAIN_CAP);
        assert_eq!(batch.len(), 1);
    }

    #[test]
    fn a_hung_up_sender_ends_the_batch_rather_than_the_process() {
        // Both `try_recv` failures mean the same thing here, and conflating them
        // deliberately is worth stating: empty means nothing more *yet*, and
        // disconnected means nothing more *ever*, and either way this batch is
        // complete. The disconnect is then reported by the `recv` that follows,
        // which is the one place that can act on it.
        let (tx, rx) = mpsc::channel();
        tx.send(tick(1)).expect("send");
        drop(tx);

        let mut batch = Vec::new();
        drain(&mut batch, tick(0), &rx, DRAIN_CAP);
        assert_eq!(batch.len(), 2, "the queued wake was lost with the sender");
    }

    /// Two properties of `run` that no test can execute, because `run` owns a
    /// terminal, and that are load bearing enough to gate by reading the source.
    ///
    /// This is the shape `no_exit_path_in_the_shell_skips_the_destructors` already
    /// uses in `terminal`, and it is a weak instrument used deliberately: both
    /// properties are single lines whose *position* and *form* are the whole of
    /// their correctness, and a mutation of either passes the entire suite. A gate
    /// that reads the file is worth more than a paragraph nobody re-checks.
    #[test]
    fn the_signal_arming_covers_the_takeover_and_the_wake_ends_the_loop() {
        // Only what ships, so the strings below cannot match this test itself.
        let source = include_str!("lib.rs");
        let shipped = source.split("#[cfg(test)]").next().expect("split");
        assert!(
            shipped.len() > 200,
            "lib.rs was not read, so scanning it proves nothing"
        );

        // **Comments stripped, because both names appear in prose in this file and
        // a check that reads prose is a check on prose.** Every comment above the
        // arming discusses `Session::enter`, so one sentence moved earlier would
        // either fail this test against correct code or pass it against wrong code,
        // depending on which name it mentioned. Both failure directions are the
        // gate proving nothing, which is the whole thing this test exists against.
        let code: String = shipped
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");

        // **Order.** The handler has to be armed before the first step of the
        // takeover, not after the fourth: a signal arriving mid-takeover is
        // otherwise still an uncaught kill. Two earlier versions of this line were
        // in the wrong place, one of them after a 318µs grammar load.
        let arming = code
            .find("signal::forward(")
            .expect("`run` no longer arms the signal handler at all");
        let takeover = code
            .find("Session::enter()")
            .expect("`run` no longer takes the terminal");
        assert!(
            arming < takeover,
            "the signal handler is armed after the terminal is taken, so a signal \
             arriving during the takeover is uncaught"
        );

        // **Every input a reader can get wrong is read before the takeover too**,
        // and the same scan is what says so. `SPEC.md` §11.1 rules that a path
        // that is not a repository, a `VIGIA_THEME` naming nothing, a variable
        // this does not recognise and a file that does not parse are all reported
        // on a terminal the reader can still see, because an error painted inside
        // a TUI that then hands the terminal back is an error nobody reads.
        //
        // **The config file joined them with [#309](https://github.com/breferrari/vigia/issues/309)
        // and nothing enforced the position**: the ordering was verified by
        // running the binary, which is evidence that dies with the shell it ran
        // in. Adding it here costs one line and covers the whole family, so the
        // next input added is a failing assertion rather than a hand check
        // somebody remembers to repeat.
        // **`frame.advance()` is in the list too**, and leaving it out was the
        // block's own claim about itself being wider than the block: its comment
        // in `run` makes exactly this promise, that a repository failing on its
        // first walk reports on a terminal the reader can still see.
        //
        // **Two of these appear twice in the shipped source**, `Worktree::discover(`
        // and `frame.advance()`, and in both the second occurrence is after the
        // takeover. `find` returns the first, which is what this wants: a moved
        // call cannot hide behind a *later* occurrence, only behind an earlier
        // one, and neither has one.
        for reader in [
            "Worktree::discover(",
            "frame.advance()",
            "theme::from_env(",
            "Glyphs::detect(",
            "config::from_env(",
        ] {
            let at = code
                .find(reader)
                .unwrap_or_else(|| panic!("`run` no longer calls {reader}"));
            assert!(
                at < takeover,
                "{reader} is read after the terminal is taken, so an error in it \
                 is painted inside a screen that is about to be handed back"
            );
        }

        // **Form.** `signal`'s escalation latches after one ask: the second goes to
        // the default disposition and kills the process. That is only safe because
        // this arm leaves the loop unconditionally, so one ask is always enough. An
        // arm that merely continued would make the first signal a no-op and the
        // second a hard kill, which is worse than either alone.
        assert!(
            code.contains("Wake::Signalled => break 'awake"),
            "the signalled wake no longer unconditionally leaves the loop, which is \
             what makes `signal`'s one-way escalation latch safe"
        );

        // **Order, again, and this one fixes the state and not the screen when it
        // is wrong.** `hover_repainted` retires a mark the new layout has
        // invalidated, and it has to run between `render::regions` and `render`.
        // Placed after the `draw` it still compiles, still passes every other
        // gate, and still leaves the stale mark on the glass: the frame carrying
        // it has already gone out, and the corrected field only reaches the next
        // paint, which on an idle tree never comes. That was the shipped
        // behaviour until an audit read the two lines in order.
        let layout = code
            .find("render::regions(area, &chrome, screen)")
            .expect("`draw` no longer computes the layout it is about to paint");
        let retire = code
            .find("hover_repainted(chrome.hovered")
            .expect("`draw` no longer retires a hover mark the new layout invalidated");
        let paint = code
            .find("render(f.buffer_mut()")
            .expect("`draw` no longer paints");
        assert!(
            layout < retire && retire < paint,
            "the hover mark is retired outside the window between the layout and \
             the paint, so a relayout draws a mark against geometry it was never \
             resolved against and nothing repaints to correct it"
        );

        // **Every frame rolls the window before it paints, and `Shell::draw` is
        // where every frame passes**. Position is the whole of it: a roll after
        // the paint draws the picture it was woken to change, and a roll on
        // only one of the loop's paths leaves the other redrawing a frozen
        // window. This lived on the timeout arm first and was exactly that
        // second bug, because `recv_timeout` returns `Ok` whenever anything is
        // queued and a stream of pointer motion never reaches that arm.
        // Anchored on the newline and the indentation rather than on the
        // argument list, because the argument list is the thing this row
        // changed: the previous anchor spelled the whole signature out and went
        // red the moment `now: Instant` pushed it onto five lines, reporting
        // "`Shell::draw` is gone" about a function three lines above it. An
        // anchor that has to be edited whenever the thing it guards is edited
        // is an anchor that gets edited to whatever makes the test pass.
        //
        // It cannot match this test's own strings, but that is not why it is
        // written this way and the first version of this comment claimed it was:
        // `code` is `shipped` above, which is everything before `#[cfg(test)]`,
        // so no literal in this module was ever reachable from here.
        let drawer = &code[code.find("\n    fn draw(").expect("`Shell::draw` is gone")..];
        let signature = &drawer[..drawer
            .find("-> Result<(), Failure>")
            .expect("`Shell::draw` no longer returns a `Result`")];
        assert!(
            signature.contains("now: Instant"),
            "`Shell::draw` no longer takes the turn's instant, so it is back to \
             rolling on a clock of its own"
        );

        // **And every caller inside the loop hands it the turn's own instant,
        // which the two checks above cannot see.** They gate the callee: that the
        // parameter exists and that the roll reads it. A caller passing
        // `Instant::now()` satisfies both and puts the defect straight back, one
        // token wide, compiling clean and green across the whole suite. That
        // mutation was found by review rather than by this file, which is the
        // reason it is written down here.
        //
        // The binding matters as much as the value. `began` is taken once at the
        // top of the turn, before the drain, so a tick's record and the paint
        // that draws it agree about when *now* is even though `Frame::advance`
        // runs between them. Two fresh clocks a status walk apart is exactly the
        // gap a sample boundary falls into.
        //
        // Matched on the *argument* rather than on the whole call, because a
        // literal spelling out the argument list is a gate that reddens against
        // correct code the next time a parameter is added: `rustfmt` explodes the
        // call across five lines and the string stops matching, reporting a lost
        // instant about a reflow. That is how the anchor above broke, one row ago.
        let turns = &code[code.find("'awake: loop {").expect("the loop is gone")..];
        let calls: Vec<&str> = turns
            .match_indices("shell.draw(")
            .map(|(at, _)| {
                let rest = &turns[at..];
                &rest[..rest.find(')').map_or(rest.len(), |end| end + 1)]
            })
            .collect();
        assert_eq!(calls.len(), 2, "the loop no longer has its two paints");
        for call in calls {
            assert!(
                call.contains("began") && !call.contains("Instant::now()"),
                "a draw inside the loop reads a clock of its own rather than the \
                 turn's, so a sample boundary landing between a tick and its \
                 paint erases the pulse of the burst that caused the frame"
            );
        }

        let recorded = &turns[turns
            .find(".record_sized(sized(")
            .expect("the tick no longer records its burst")..];
        let recorded = &recorded[..recorded
            .find(';')
            .expect("the tick's record is not a statement")];
        assert!(
            recorded.contains("began") && !recorded.contains("Instant::now()"),
            "a tick timestamps its burst on a clock of its own, so it and the \
             paint that draws it can straddle a sample boundary and the burst \
             loses its pulse on the one frame it caused"
        );
        let rolled = drawer.find("self.history.record_sized([], ").expect(
            "`Shell::draw` no longer rolls the window, so a frame can draw one that stopped moving",
        );
        let painted = drawer
            .find("self.paint(frame, worktree)")
            .expect("`Shell::draw` no longer paints");
        assert!(
            rolled < painted,
            "the draw paints before it rolls the window, so a frame shows the \
             picture it was woken to change and the next one corrects it a beat \
             late"
        );

        // **And it rolls on the caller's clock, not its own.** This is the
        // clock-discipline half of the same mechanism and nothing else in the
        // repo can see it: a tick records its burst at the top of the turn and
        // reaches the draw only after the status walk, so a `Instant::now()`
        // taken here rolls away the sample that burst just wrote whenever a
        // boundary falls in the gap. The store cannot catch it (both calls are
        // well formed and it has no view of the turn) and no rendering test can
        // (it needs a boundary inside one frame's cost), so the parameter is the
        // gate.
        assert!(
            drawer[..painted].contains("self.history.record_sized([], now)"),
            "`Shell::draw` rolls on a clock of its own rather than the turn's, so \
             a sample boundary falling between a tick and its paint erases the \
             pulse of the burst that caused the frame"
        );

        // And the ageing wake stays a paint rather than a tick: a status walk on
        // that path is the difference `SPEC.md` §11.1 prices the amendment on.
        let arm = &code[code
            .find("let Some(wake) = wake else {")
            .expect("the loop no longer has a timeout arm")..];
        let arm = &arm[..arm
            .find("continue;")
            .expect("the timeout arm no longer continues")];
        assert!(
            !arm.contains("frame.advance("),
            "the timeout arm walks status, so an ageing wake now costs a tick and \
             the measurement I1's amendment was granted on no longer holds"
        );

        // **The arm draws, and that is a liveness gate rather than a tidiness
        // one.** `Shell::draw` is now the only thing that advances `opened`, so
        // an arm that woke on the ageing deadline and returned without drawing
        // would leave `ages_in` pinned at zero and put the loop on
        // `recv_timeout(0)`: a spin, at full CPU, on an idle worktree, which is
        // the precise failure I1 exists to forbid and which every gate over
        // `ages_in` and `patience` would sit through green.
        let drew = arm.find("shell.draw(").expect(
            "the timeout arm no longer draws, so the ageing deadline never \
             advances and the loop spins on a zero timeout",
        );

        // And the frame it draws is one the bar counts. Without this, deleting
        // the call leaves the whole suite green, because a frame time nobody
        // asserts on is invisible to every test in the repo.
        let timed = arm.find("record_frame(").expect(
            "a timeout frame is not recorded, so the readout `SPEC.md` §5.1 \
             defines as the whole turn of the loop silently omits what is now the \
             most common frame on a quiet tree",
        );
        // **Position, not presence.** Containment alone let the call move *above*
        // the paint and stay green, which reports a frame time excluding the
        // paint: the most common frame on a quiet tree would then be measured on
        // everything except the work it does.
        assert!(
            drew < timed,
            "the timeout arm records its frame time before it paints, so the \
             number the bar draws for the most common frame on a quiet tree \
             excludes the paint that frame exists to do"
        );

        // **The idle receive is untimed, and that is I1's budget as a structure
        // rather than as an observation.** `Held::wait` returning `None` is gated
        // in `tests/input.rs`, and it buys nothing unless this loop actually
        // branches on it: a `recv_timeout` reached unconditionally would put an
        // idle monitor on a poll loop while every gate over `Held` stayed green,
        // because none of them can see which receive the loop calls.
        //
        // Read from the source for the reason the two assertions above are: the
        // loop owns a terminal and three threads, so there is nothing to drive it
        // with in a unit test, and the alternative is no gate at all.
        let untimed = code
            .find("None => match rx.recv()")
            .expect("the loop no longer has an untimed receive for the idle case");
        let timed = code
            .find("rx.recv_timeout(")
            .expect("the loop no longer has a bounded receive for a held step");
        assert!(
            untimed < timed,
            "the loop reaches `recv_timeout` before it has decided whether \
             anything is held, so an idle monitor is being given a deadline"
        );
        // **Three clocks now, and one function that answers for all of them.** A
        // deadline asked separately would be another chance to leave one armed on
        // an idle monitor, and the gate above can only see the branch, not what
        // fed it. Matched on the arguments rather than on the whole call, because
        // the call spans lines and a formatter deciding otherwise is not a defect
        // this should report.
        let asked = code.find("input::patience(").expect(
            "`Shell::patience` is gone, so nothing decides *is there a timer at all* in one place",
        );
        let sources = &code[asked..asked + 200.min(code.len() - asked)];
        for clock in ["self.held", "self.scrolling_until", "ages_in"] {
            assert!(
                sources.contains(clock),
                "`{clock}` is no longer among the deadlines `patience` is given, so \
                 that clock is either armed somewhere else or has stopped: {sources}"
            );
        }
        assert!(
            code.contains("match shell.patience(Instant::now())"),
            "the loop no longer decides how long to wait through `Held::wait`, so \
             the one function that can answer *is there a timer at all* is not the \
             one being asked"
        );
    }
}
