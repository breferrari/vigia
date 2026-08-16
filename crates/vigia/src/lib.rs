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
//! is the shell's and because one of the three wake sources is the terminal. It
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
//! The third is [`vigia_core::Highlighter::warm_ahead`], and it is not on that
//! channel at all: it sends no wake, so it cannot make an idle monitor do work
//! and I1 never sees it. It compiles grammars ahead of the reader and ends by
//! itself. That it reports nothing back is the design rather than a shortcut —
//! there is no such thing as a warm grammar for the frame path to believe in,
//! which `warm_ahead` documents in full.
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
mod input;
/// Public where its seven siblings are private, and for one reason: `soak.rs` is
/// an integration test, so it can only reach what the crate exports, and I3's
/// harness needs the same reader the shell uses. Two implementations of "read
/// this process's RSS" that could disagree is exactly what one of them existing
/// is meant to prevent.
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
pub use input::{
    Action, Grabbed, Held, Hovered, Region, Regions, STEP_DELAY, STEP_REPEAT, TRACK_SCALE,
    WHEEL_ROWS, action_for, drag_action, hover_after, patience, scroll_mark,
};
pub use render::{
    Band, Body, Chrome, HINT_SEPARATOR, Heat, LIST_ROWS, Mode, PaintStats, body_layout,
    diff_height, regions, render,
};
pub use terminal::{Screen, Session};
pub use theme::{THEME_FILE, THEME_VAR, Theme, ThemeError};
pub use view::{
    FileEntry, HEAT_BUCKETS, HeatBucket, Position, Row, View, Viewport, block_rows, diff_rows,
    rows_in, rows_of,
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
    /// **What changed with [`Signalled`](Wake::Signalled) is the consequence,
    /// not the ruling.** This used to say the only remaining exit was a kill,
    /// which ran neither the guard nor the panic hook and handed back a terminal
    /// in raw mode. A kill is caught now and restores like any other exit, so the
    /// cost of staying is no longer a wrecked terminal. It is still a pane that
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
    /// **Refused rather than ignored**, which is a change from how this behaved
    /// before the surface was gated: `vigia . --colour=never` used to watch `.`
    /// and drop the rest on the floor, so a reader who typed a flag alongside a
    /// path got no signal that the flag does not exist. That is the same defect
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
/// surface and nothing was checking it**. The classifier used to see only
/// `args_os().nth(1)`, so `vigia . --colour=never` watched `.` and discarded the
/// rest silently: the flag that does not exist produced a running program
/// instead of the one-line refusal that a flag on its own produces. A function
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

/// Watch the working tree at `path` and draw it until the reader quits.
pub fn run(path: &Path) -> Result<(), Failure> {
    let worktree = Worktree::discover(path)?;
    let mut frame = worktree.frame();

    // Before the screen is taken, so a repository that fails on its first walk
    // reports on a terminal the reader can still see.
    frame.advance()?;

    // Same rule, same reason, one input over: a `VIGIA_THEME` that names nothing
    // or a file that does not parse has to be said on a terminal the reader can
    // still read. `SPEC.md` §11.1 states it for a path that is not a repository,
    // and an error painted inside a TUI that then hands the terminal back is an
    // error nobody sees.
    //
    // Resolved to the depth here as well, so the palette the renderer holds is
    // already in colours this terminal can show and the frame path never
    // quantises. I9 therefore sees none of it.
    let theme = theme::from_env(Depth::detect()?, |key| std::env::var(key).ok())?;

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
        app: App::new(),
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
        // Not "before the screen is taken", which an earlier version of this
        // comment claimed: struct fields evaluate in written order and
        // `Session::enter` is written above, so the alternate screen is already
        // ours by the time this runs. The placement is right and the reason was
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
        name: short_name(worktree.workdir()),
        branch: None,
        screen: View::default(),
        regions: Regions::default(),
        held: None,
        grabbed: None,
        hovered: None,
        scrolling: None,
        scrolling_until: None,
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
    // Detached by dropping the handle, like the two threads below and for a
    // simpler reason: it ends by itself, and nothing waits for a result that
    // only ever makes a later frame cheaper.
    //
    // **Above the draw, so it overlaps the frame that compiles.** Below it,
    // the warm starts only once paint two has finished paying the 74-362ms
    // for what is on screen, which turns `max(paint, warm)` into their sum
    // and widens the window in which a first scroll below the fold meets a
    // cold grammar. That window is the whole reason this thread exists.
    if !frame.files().is_empty() {
        shell.highlighter.warm_ahead(
            worktree.workdir().to_path_buf(),
            frame
                .files()
                .iter()
                .take(vigia_core::WARM_FILES)
                .map(|change| change.path.clone())
                .collect(),
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
    shell.draw(&mut frame, &worktree)?;

    // Armed only now. Everything above read `.git/index` and the gitignore
    // files, and a watch armed before those reads observes them: inotify and
    // FSEvents both report reads and attribute touches, so the shell would wake
    // itself up once for free at startup. The channel they send on is older than
    // they are: the signal handler above shares it, and has the opposite
    // requirement about when it is armed.
    spawn_watch(path.to_path_buf(), tx.clone());
    spawn_input(tx);

    // Reused across iterations rather than allocated per wake. A monitor is left
    // open for days and I3 is the invariant that notices, so the one buffer the
    // loop needs is the one buffer it keeps.
    let mut batch = Vec::with_capacity(DRAIN_CAP);

    // **The only clock this program owns, and it exists only between a press on
    // a step button and its release.** `SPEC.md` §11.1 carries the ruling and
    // `Held::wait` is the seam that keeps it honest: with `None` here the receive
    // below is untimed, which is I1's *0 wakeups while idle* unchanged and
    // unmeasurable-away. Nothing arms this but a press on a bar's end.
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
            match shell.app.apply(step, &mut frame, 0) {
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
            shell.settle_scroll(Instant::now());
            shell.app.sample_memory();
            shell.draw(&mut frame, &worktree)?;
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
                    // carries the four ways a hold finishes and why.
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
                    // The whole rule is in `hover_after` rather than here, for
                    // the reason `Held::ends` is a free function: this loop
                    // cannot be driven by a test, and a rule written inline is a
                    // rule with no gate.
                    shell.hovered = hover_after(&event, regions, shell.hovered);
                    // **A drag under way answers before the column is consulted,
                    // and that ordering is the fix.** `action_for` asks what is
                    // under the pointer, which is the right question for a press
                    // and the wrong one for a hand already moving: a reader
                    // dragging a one-column bar leaves that column immediately,
                    // and the gesture used to end there. Now the grip decides,
                    // and the row is clamped so pulling past either end holds
                    // that end.
                    if let Some(on) = shell.grabbed {
                        if let Some(drag) = drag_action(&event, regions, on) {
                            match shell.app.apply(drag, &mut frame, 0) {
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
                        if !matches!(
                            &event,
                            Event::Mouse(mouse)
                                if matches!(mouse.kind, MouseEventKind::Drag(MouseButton::Left))
                        ) {
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
                    // uncached terminal-size syscall and `chrome` allocates, and
                    // both used to be amortised against the full repaint each
                    // event caused. With sixty-four notches now arriving between
                    // two paints, computing a height none of them but `Page` reads
                    // would be sixty-four syscalls and several hundred discarded
                    // allocations inside one batch.
                    //
                    // The branch it carries is whatever the last draw settled on,
                    // which is right rather than merely cheap: it feeds
                    // `diff_height` alone, and neither the branch nor the mode can
                    // change how many rows the footer takes. See `Footer::plan`.
                    let height = if action.needs_height() {
                        let chrome = shell.app.chrome(
                            &shell.name,
                            shell.branch.as_deref(),
                            shell.pressed(),
                            shell.gripped(),
                            shell.hovered(),
                            shell.scrolling,
                        );
                        diff_height(shell.area()?, &chrome, frame.files().len())
                    } else {
                        0
                    };
                    shell.note_scroll(action, Instant::now());
                    match shell.app.apply(action, &mut frame, height) {
                        Ok(true) => {}
                        // Out of the batch *and* out of the loop, without the draw
                        // below: the reader asked to leave, and painting one more
                        // frame on the way out is a frame they did not ask for.
                        Ok(false) => break 'awake,
                        Err(e) => shell.app.warn(e.to_string()),
                    }
                }
                Wake::Tick(paths) => {
                    shell.app.clear_notice();
                    // Sampled here and nowhere else, which is the whole of I10's
                    // relationship with I1: the window is real time, and the only
                    // thing that moves it is a wake the loop was already having.
                    // An empty burst still rolls the window and still leaves the
                    // pulse where it was, which is the staging case.
                    shell
                        .history
                        .record(paths.iter().map(String::as_str), Instant::now());
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
        shell.draw(&mut frame, &worktree)?;

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
/// reports a flick as a stream of scroll events rather than as one, and every one
/// of them used to be a full redraw: the pane then renders each notch in turn and
/// falls behind the thumb, which is what *"if it gets too fast, it struggles"*
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
/// reason `branch_for` is one: `run` owns a terminal and cannot be driven from a
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
    /// What the header calls the working tree.
    name: String,
    /// What the empty state calls the branch, or `None` when it will not draw
    /// one.
    ///
    /// Refreshed per draw rather than held for the session, because an agent in
    /// the other pane can check out a branch and a name cached at startup would
    /// then be a confident lie. `None` whenever the diff is not empty, so a
    /// populated frame never carries a stale answer it would not have drawn
    /// anyway. See [`branch_for`].
    branch: Option<String>,
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
    /// It needs neither an expiry nor an end, which is the whole of §11.1's
    /// three-mark rule arriving at its third case: a hold ends with an `Up`, a
    /// key burst has no end and takes a clock, and this is retired by its
    /// **replacement**, because the next mouse event says where the pointer is
    /// now. [`hover_after`] is that rule and is where it is gated.
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
    scrolling: Option<(u16, isize)>,
    /// When the mark above stops being true.
    scrolling_until: Option<Instant>,
}

impl Shell {
    /// The cell a step button is being held on, for the frame that draws it lit.
    fn pressed(&self) -> Option<(u16, u16)> {
        self.held.map(Held::at)
    }

    /// The first row of the region whose bar is being dragged, for the frame that
    /// draws its thumb lit.
    fn gripped(&self) -> Option<u16> {
        self.grabbed.map(|on| on.top(self.regions))
    }

    /// What the pointer is over, for the frame that marks it.
    ///
    /// Stored resolved rather than as a position, unlike [`Shell::pressed`],
    /// because the resolution is against the regions of the paint the pointer
    /// was actually over. Re-resolving here would answer against the *next*
    /// layout, so a frame that changed the bar's geometry would move a mark the
    /// reader had not moved.
    fn hovered(&self) -> Option<Hovered> {
        self.hovered
    }

    /// How long the loop may block before something here has to act.
    ///
    /// **`None` is the whole invariant, and it is why both clocks are asked
    /// through one function.** With nothing held and nothing lingering this
    /// returns `None`, the receive below is untimed, and I1's *0 wakeups while
    /// idle* is a fact about the structure rather than about care taken
    /// elsewhere. Two clocks asked separately is two chances to leave a deadline
    /// armed on an idle monitor; asked together there is one place to be wrong
    /// and one place to gate.
    ///
    /// Where both are armed it is the nearer of the two, because the loop has to
    /// wake for whichever comes first.
    fn patience(&self, now: Instant) -> Option<std::time::Duration> {
        input::patience(self.held, self.scrolling_until, now)
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
    /// Returns whether anything changed, so the caller repaints only on the frame
    /// that actually turns it off.
    fn settle_scroll(&mut self, now: Instant) -> bool {
        if self.scrolling_until.is_some_and(|until| now >= until) {
            self.scrolling = None;
            self.scrolling_until = None;
            return true;
        }
        false
    }

    /// The drawable area of the terminal right now.
    ///
    /// **Taken from the terminal's own resized state rather than from a second
    /// size syscall**, so the area a frame is *planned* for is the area it is
    /// *painted* into. This used to call `Backend::size` directly, which meant two
    /// independent reads per frame: this one, deciding how many rows
    /// [`View::collect`] was asked for, and the one `Terminal::draw` makes inside
    /// its own `autoresize`. A resize landing between them left the collect sized
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
    fn draw(&mut self, frame: &mut vigia_core::Frame, worktree: &Worktree) -> Result<(), Failure> {
        self.paint(frame, worktree)?;
        if self.app.owes_repaint() {
            self.paint(frame, worktree)?;
        }
        Ok(())
    }

    /// One collect and one paint, with no view of what it leaves owed.
    fn paint(&mut self, frame: &mut vigia_core::Frame, worktree: &Worktree) -> Result<(), Failure> {
        // Before the chrome, because the chrome carries it, and from the frame's
        // own file count so the read happens on exactly the frames that draw the
        // answer. That is the whole of I4 for this read.
        self.branch = branch_for(frame, || worktree.branch());

        // The chrome is built before the layout, not after, because the footer
        // takes a second line at narrow widths and `body_layout` has to know
        // whether this frame is one of those. `frame.files().len()` is the same
        // number `View::collect` will report as `View::files`, which is what
        // keeps this row budget and the renderer's layout in agreement: `render`
        // recomputes the same split from the same two inputs.
        let chrome = self.app.chrome(
            &self.name,
            self.branch.as_deref(),
            self.pressed(),
            self.gripped(),
            self.hovered(),
            self.scrolling,
        );
        let body = body_layout(self.area()?, &chrome, frame.files().len());
        match self
            .app
            .view(frame, &mut self.highlighter, &self.history, body)
        {
            Ok(view) => self.screen = view,
            Err(e) => self.app.warn(e.to_string()),
        }

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
        // rather than merely inconsistent. It was decided from the *frame's*
        // count above, while the empty state is drawn from `view.files`, so a
        // collect that fails on the way from a clean tree to a dirty one draws
        // last frame's empty state with no branch on it. One line loses four
        // words for one frame, on a path that has already reported a failure to
        // the footer, and the alternative is deciding it twice from two counts
        // that can disagree. Reading the branch from the stale view instead
        // would mean holding a name across frames, which is the confident lie
        // `branch_for` refuses.
        let chrome = self.app.chrome(
            &self.name,
            self.branch.as_deref(),
            self.pressed(),
            self.gripped(),
            self.hovered(),
            self.scrolling,
        );
        // Borrowed out of `self` before the draw, not for style: the closure would
        // otherwise hold `&self` while `self.session` is borrowed mutably to reach
        // the terminal.
        let (theme, screen) = (&self.theme, &self.screen);
        let mut painted = Regions::default();
        self.session.screen().draw(|f| {
            let area = f.area();
            // Captured from inside the draw, because `Frame::area` is the size the
            // paint actually used: `Shell::area` reads it again and a resize
            // between the two would leave a pointer told about a screen nobody
            // saw. Same seam #59 found on the other side.
            painted = render::regions(area, &chrome, screen);
            render(f.buffer_mut(), area, screen, theme, &chrome);
        })?;
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

/// The branch to draw, and whether to go and look for one at all.
///
/// **This is I4 for the empty state's branch**, expressed as a function so it can
/// be gated rather than inspected. Only the empty state names a branch, so only a
/// frame with nothing to diff may pay for reading `.git/HEAD`, and a populated
/// frame must not read a file it is not going to draw.
///
/// `read` is a closure rather than a [`Worktree`] so a test can count the calls.
/// Reaching the same assurance through a real repository would mean observing a
/// read the frame path does not account for, which no counter here can see.
///
/// **It takes the frame rather than its file count, and that is what makes the
/// rule gateable at all.** With a count, the expression deciding it lived at the
/// call site inside `Shell::draw`, which owns a terminal and which no test can
/// drive: hardcoding that argument to `0` and to `1` both passed the **entire
/// suite**, in both directions, while every unit test of this function stayed
/// green. The mutations killed the consumer and never touched the producer.
/// Moving the count inside the boundary leaves nothing outside it to get wrong,
/// and lets `tests/reads.rs` drive this with a real [`vigia_core::Frame`], so
/// what is asserted is what production computes rather than a number someone
/// typed.
///
/// Public for the reason [`rows_in`] and [`diff_height`] are: `SPEC.md` §7 makes
/// the test suite the proof, and a rule reachable only from inside the crate is
/// one the suite cannot hold against a real repository.
///
/// The read is not cached across frames on purpose: an agent in the other pane
/// can check out a branch, and a name held from startup would be a confident lie
/// on exactly the screen that exists to orient the reader.
pub fn branch_for(
    frame: &vigia_core::Frame,
    read: impl FnOnce() -> Option<String>,
) -> Option<String> {
    if !frame.files().is_empty() {
        return None;
    }
    read()
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
    //! [`branch_for`] used to be tested here too and deliberately is not any
    //! more. Its rule is about a **frame**, and driving it from a number typed
    //! into a unit test proved only that the function reads its own argument:
    //! the call site producing that argument was untestable, and mutating it
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
        // reports one flick as a stream of events, and every one of them used to
        // be a full redraw. One batch is one paint.
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
        // **Two clocks now, and one function that answers for both.** A second
        // deadline asked separately would be a second chance to leave one armed
        // on an idle monitor, and the gate above can only see the branch, not
        // what fed it.
        assert!(
            code.contains("input::patience(self.held, self.scrolling_until, now)"),
            "`Shell::patience` is gone, so the two clocks are being asked              separately and nothing decides *is there a timer at all* in one place"
        );
        assert!(
            code.contains("match shell.patience(Instant::now())"),
            "the loop no longer decides how long to wait through `Held::wait`, so \
             the one function that can answer *is there a timer at all* is not the \
             one being asked"
        );
    }
}
