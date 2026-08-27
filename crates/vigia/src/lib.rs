//! The `ratatui` + `crossterm` shell over [`vigia_core`].

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

mod app;
mod colour;
/// What the pane starts as, which `SPEC.md` §11.2 B6 puts in a file.
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
pub type Failure = Box<dyn std::error::Error>;

/// Why the shell woke up.
enum Wake {
    /// The terminal reported something.
    Input(ratatui::crossterm::event::Event),
    /// The working tree changed, coalesced into one signal by the core.
    Tick(Vec<String>),
    /// The watch stopped, so the shell is a still picture.
    WatchLost(String),
    /// Terminal input stopped, so nothing can reach the shell any more.
    InputLost,
    /// Something outside this process asked it to stop.
    Signalled,
    /// A warm finished, so a hunk that drew plain can draw in colour.
    Warmed,
}

/// Whether a demand is worth handing to a warmer, given what the last one was
/// handed and whether the tree has changed since.
pub fn worth_warming(wanted: &[String], served: &[String], written: bool) -> bool {
    !wanted.is_empty() && (written || wanted != served)
}

/// The callback a warm ends with, wired to this shell's wake channel.
fn warmed(tx: &Sender<Wake>) -> vigia_core::Warmed {
    let tx = tx.clone();
    Box::new(move || {
        let _ = tx.send(Wake::Warmed);
    })
}

/// The version this binary reports, which is the package's.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// What `vigia`'s argument list is asking for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    /// Watch the argument as a path.
    Watch,
    /// Print the version and exit successfully.
    Version,
    /// An argument beginning with `-` that is not a version query.
    NoSuchOption,
    /// More than one argument, when the surface is exactly one.
    TooManyArguments,
}

/// Classify the arguments `vigia` was given.
pub fn request_for(args: &[OsString]) -> Request {
    match args {
        [] => Request::Watch,
        [arg] => request_for_one(arg),
        _ => Request::TooManyArguments,
    }
}

/// Classify the one argument `vigia` takes.
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
    let config = config::from_env(|key| std::env::var(key).ok())?;

    // **The view defaults reach the frame before its first walk, not just the
    // shell**. Three of the four keys decide how rows the frame already holds
    // are arranged, so setting them on `App` is all they need. `staged` decides
    // what the frame *walks*, so a reader whose file says `staged = on` has to
    // be honoured here or the key sets a flag nothing acts on and the opening
    // pane is the one they were trying to change. Same defect
    // `Action::ToggleStaged` had against the keypress, on the path that has no
    // keypress to trigger it.
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
    let armed = signal::forward(tx.clone());

    let mut shell = Shell {
        session: Session::enter()?,
        app: App::configured(config),
        // Its 318µs of grammar *loading* lands before first paint, which is
        // where it belongs: I7 gives startup 50ms, so this is well under one
        // percent of it and deferring it would only move it onto the first frame
        // that draws something.
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
    if let Err(e) = armed {
        shell.app.warn(format!(
            "not catching an external stop, so a kill may not restore the terminal: {e}"
        ));
    }

    // **For a screen with rows on it, so a clean worktree spawns nothing.**
    // Starting a monitor on a tree nobody has touched is an ordinary way to
    // start one, and there is no grammar to compile for an empty state.
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
        shell.app.record_frame(began.elapsed());
    }

    Ok(())
}

/// What each path in one wake's burst now holds, for [`vigia_core::History::record_sized`].
pub fn sized<'p>(
    workdir: &'p Path,
    paths: &'p [String],
) -> impl Iterator<Item = (&'p str, Option<u64>)> + 'p {
    paths
        .iter()
        .map(move |path| (path.as_str(), weigh(workdir, path)))
}

/// What a written path now holds on disk, for [`vigia_core::History::record_sized`].
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
pub const SCROLL_LINGER: std::time::Duration = std::time::Duration::from_millis(220);

/// Wakes taken in one go, so one gesture costs one paint.
const DRAIN_CAP: usize = 64;

/// Take the wake that woke the loop, plus everything already queued behind it.
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
    highlighter: Highlighter,
    /// What changed recently: the source for the sparkline, the recency gradient
    /// and the pulse.
    history: History,
    theme: Theme,
    /// Which glyphs the sparkline may draw from, resolved once at startup.
    glyphs: Glyphs,
    /// What the header calls the working tree.
    name: String,
    /// The worktree's absolute path, spelled once for the links' `file://`
    /// targets.
    root: String,
    /// What the header calls the branch, or `None` when there is none to call.
    branch: Option<String>,
    /// How many changes the run this pane is **not** drawing holds.
    elsewhere: usize,
    /// The last view collected successfully.
    screen: View,
    /// Where the last painted screen's regions and scrollbars were.
    regions: Regions,
    /// What a mouse button is currently being held down on, if anything.
    held: Option<Held>,
    /// The bar a drag is currently moving, if one is.
    grabbed: Option<Grabbed>,
    /// What the pointer is resting on, when it is on something a click acts on.
    hovered: Option<Hovered>,
    /// Which way the viewport is currently being moved, and until when.
    scrolling: Option<(Grabbed, isize)>,
    /// When the mark above stops being true.
    scrolling_until: Option<Instant>,
    /// The demand the last warm was handed, so a demand nothing can serve is
    /// asked for once rather than on every frame.
    served: Vec<String>,
    /// Whether the tree has changed since the last warm was spawned.
    written: bool,
    /// The warm this shell last asked for, if any.
    warming: Option<std::thread::JoinHandle<vigia_core::WarmReport>>,
}

impl Shell {
    /// The diff region's height for `action`, or zero where it reads none.
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
    fn gripped(&self) -> Option<Grabbed> {
        self.grabbed
    }

    /// What the pointer is over, for the frame that marks it.
    fn hovered(&self) -> Option<Hovered> {
        self.hovered
    }

    /// How long the loop may block before something here has to act.
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
    fn settle_scroll(&mut self, now: Instant) {
        if input::settled(self.scrolling_until, now) {
            self.scrolling = None;
            self.scrolling_until = None;
        }
    }

    /// The drawable area of the terminal right now.
    fn area(&mut self) -> Result<Rect, Failure> {
        let screen = self.session.screen();
        screen.autoresize()?;
        Ok(screen.get_frame().area())
    }

    /// Where the regions of the **last painted** screen were.
    fn regions(&self) -> Regions {
        self.regions
    }

    /// Hand the warmer whatever the last paint drew plain, and let it wake us.
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
        self.elsewhere = if self.screen.files == 0 && !self.app.staged() {
            worktree.count_of(vigia_core::Origin::Staged).unwrap_or(0)
        } else {
            0
        };

        // Rebuilt so a notice raised by the collect above reaches this frame
        // rather than the next one. Safe to differ from the chrome the height
        // came from: a notice cannot change how many rows the footer takes, by
        // construction. See `Footer::plan`.
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

    use super::*;

    #[test]
    fn a_relative_worktree_root_still_names_the_folder() {
        // `vigia .` is the invocation the tool is named after, and it headered
        // the screen `.` for a whole phase. `gix` hands back the workdir as it
        // was given, `Path::new(".")` has no final component, and the fallback
        // then printed the path itself. The header's whole job is saying which
        // tree this is, and this was the one input where it could not.
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
