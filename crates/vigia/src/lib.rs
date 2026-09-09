//! The `ratatui` + `crossterm` shell over [`vigia_core`].

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

mod app;
mod clipboard;
mod colour;
/// What the pane starts as, which `SPEC.md` §11.2 B6 puts in a file.
pub mod config;
mod glyphs;
/// Public where its siblings are private: I3's harness in `soak.rs` is an
/// integration test and must measure through the same reader the shell uses.
pub mod icons;
mod input;
/// The agent's side of `SPEC.md` §11.2 B21: `vigia mcp`, the stdio server
/// over the notes store. Public because the suite drives it as the loop does.
pub mod mcp;
pub mod memory;
/// Public for [`theme`]'s reason, and because the pane's motions are its
/// sources: the suite compiles every one, which is what makes a text motion
/// safe to ship.
pub mod motion;
mod notes;
/// `SPEC.md` §11.2 B21's send rung: what Enter puts on the agent session's
/// socket. Public because the wire is the whole subject and no drawn cell shows
/// it, so the suite drives it as the pane does.
pub mod post;
mod render;
mod signal;
mod state;
mod terminal;
/// Public for [`memory`]'s reason: `tests/palette.rs` is an integration test and
/// can only reach what the crate exports.
pub mod theme;
/// Public for [`theme`]'s reason, and it is where `VIGIA_UPDATE` is read.
pub mod update;
mod view;

pub use app::{App, Sending, Voice};
pub use clipboard::{Carrier, Route, plan, put, remote, system_tools, tmux_command};
pub use colour::{DEPTH_VAR, Depth, DepthError};
pub use config::{CONFIG_FILE, Config, ConfigError};
pub use glyphs::{GLYPHS_VAR, Glyphs, GlyphsError};
pub use input::{
    Action, Deadlines, Grabbed, Held, Hovered, Pointing, Region, Regions, STEP_DELAY, STEP_REPEAT,
    Selection, Sheet, TRACK_SCALE, WHEEL_ROWS, action_for, drag_action, hover_after, patience,
    repainted, scroll_mark, selection_after, settled,
};
pub use motion::{
    ALERT_ARRIVING, ARRIVED_LINGER, ARRIVING, ARRIVING_FRAME, BOX_ARRIVING, LEAVING,
    NOTICE_ARRIVING, NOTICE_LINGER, RESOLVE_ARRIVING, RESOLVE_BEAT, RESOLVED_DEPARTURE,
    SAID_ARRIVING, Timed, effect_interval, length,
};
pub use notes::{
    Alerts, BoxRoute, Change, Committed, Ledger, NoteBox, NoteEffects, SWEEP, Settled, TRANSITION,
    box_entrance, box_exit, box_route, commit, edge_at, has_room, leaving, opening, press_at,
    resolve_arrival, withdraw, word_arrival,
};
pub use post::Posted;
pub use ratatui_textarea::{Input, Key};
pub use render::{
    Areas, Band, Body, Chrome, HINT_SEPARATOR, Heat, LIST_SETTLED, Mode, NoteCells, NoteCount,
    PaintStats, SHEET_PURPOSE, WORD_INSET, body_layout, box_cells, count_cell, diff_height,
    note_cells, notice_area, regions, render, voice_style,
};
pub use state::state_root;
pub use terminal::{Background, Screen, Session, background_of};
pub use theme::{THEME_FILE, THEME_VAR, Theme, ThemeError};
pub use update::{UPDATE_VAR, UpdateError};
pub use view::{
    Anchor, BOX_FRAME, BOX_ROWS, BoxPart, FileEntry, FileNotes, HEAT_BUCKETS, HeatBucket, ListRow,
    Marked, NoteLead, NoteMark, Noted, Position, Row, Scale, Slot, View, Viewport, block_rows,
    diff_rows, file_at, last_top, list_plan, list_rows_wanted, rows_in, rows_of, span_in,
};

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Instant, SystemTime};

use ratatui::crossterm::event::{Event, MouseButton, MouseEventKind};
use ratatui::layout::Rect;
use tachyonfx::EffectManager;
use tachyonfx::pattern::{AnyPattern, RadialPattern, SweepPattern};
use vigia_core::{Highlighter, History, Note, Registry, Store, StoreWatch, WatchOptions, Worktree};

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
    /// A newer version exists, so the footer can name it.
    Update(String),
    /// The notes store changed under another process's hand.
    Notes,
    /// Enter's post to the registered agent sessions came back.
    Posted(Posted),
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
    /// Serve the notes store to the agent over stdio, `SPEC.md` §11.2 B21.
    Mcp,
    /// Record this agent session's socket, or clear it, from a hook.
    McpRegister,
    /// Say how many notes are open, for a hook to put in front of the agent.
    McpPending,
    /// An argument beginning with `-` that is not a version query.
    NoSuchOption,
    /// A second word after `mcp` that is neither of the server's own.
    NoSuchWord,
    /// More than one argument, when the surface is exactly one, or two after
    /// `mcp`.
    TooManyArguments,
}

/// Classify the arguments `vigia` was given.
pub fn request_for(args: &[OsString]) -> Request {
    match args {
        [] => Request::Watch,
        [arg] => request_for_one(arg),
        // The server has words of its own, which are the server's and not the
        // pane's: they take no terminal and change no frame. `SPEC.md` §11.2 B6
        // as amended by B21.
        [first, second] if first == OsStr::new("mcp") => match second.to_str() {
            Some("register") => Request::McpRegister,
            Some("pending") => Request::McpPending,
            _ => Request::NoSuchWord,
        },
        _ => Request::TooManyArguments,
    }
}

/// Classify the one argument `vigia` takes.
fn request_for_one(arg: &OsStr) -> Request {
    if arg == OsStr::new("--version") || arg == OsStr::new("-V") {
        return Request::Version;
    }
    // The bare word and nothing else: `./mcp` is still a directory to watch.
    if arg == OsStr::new("mcp") {
        return Request::Mcp;
    }
    // The first byte rather than a decoded character: both encodings behind
    // `OsStr` are self-synchronising at ASCII, so a leading `b'-'` cannot be the
    // tail of another character.
    match arg.as_encoded_bytes().first() {
        Some(b'-') => Request::NoSuchOption,
        _ => Request::Watch,
    }
}

/// Remove the binary the last upgrade displaced, once nothing holds it.
///
/// Windows refuses to replace a running executable, so the upgrade `README.md`
/// documents renames the old one aside, and it cannot be deleted until the last
/// process that started before the upgrade exits. Nothing but a later `vigia` is
/// in a position to notice that moment, so one unlink at startup is the whole of
/// the cleanup: a file still held refuses, which is the ordinary case rather
/// than an error, and the next start tries again.
///
/// It appends to the running image's own path, so the one file it can reach is
/// that sibling, and never the binary a reader runs.
#[cfg(windows)]
pub fn sweep_displaced() {
    let Ok(me) = std::env::current_exe() else {
        return;
    };
    let mut displaced = me.into_os_string();
    displaced.push(".old");
    let _ = std::fs::remove_file(displaced);
}

/// Nothing to sweep: every other platform replaces a running binary in place,
/// so no upgrade ever displaces one.
#[cfg(not(windows))]
pub fn sweep_displaced() {}

/// Tell a frame what the shell's view defaults ask it to walk.
#[doc(hidden)]
pub fn arm_frame(frame: &mut vigia_core::Frame, config: crate::Config) {
    frame.show_staged(config.staged);
}

/// Watch the working tree at `path` and draw it until the reader quits.
///
/// # Errors
///
/// The path is not a repository, an input the reader controls does not parse, or the
/// terminal cannot be taken. Every one of them is reported before the takeover, on a
/// terminal the reader can still read.
pub fn run(path: &Path) -> Result<(), Failure> {
    let worktree = Worktree::discover(path)?;
    let mut frame = worktree.frame();

    // Same rule one input over: an error painted inside a TUI that then hands
    // the terminal back is an error nobody sees. `SPEC.md` §11.1.
    let detected = terminal::background(std::time::Duration::from_millis(150));
    let theme = theme::from_env(Depth::detect()?, |key| std::env::var(key).ok(), detected)?;

    // Resolved once before the screen is taken, so the frame path never asks the
    // environment anything. An unrecognised value is refused, not defaulted.
    let glyphs = Glyphs::detect()?;

    // Here rather than inside `App` for the palette's reason: a config file that
    // does not parse has to be reported on a terminal the reader can still read.
    // `SPEC.md` §11.2 B6.
    let config = config::from_env(|key| std::env::var(key).ok())?;

    // Read here for that same reason, and acted on after the first paint.
    let update = update::wanted(|key| std::env::var(key).ok())?;

    // Where the reader's notes live, resolved once with the rest of the
    // environment. `None` with no home to keep them under, which the first click
    // says rather than the launch: a pane with no notes is still a pane.
    let store = state::store_for(worktree.workdir(), |key| std::env::var(key).ok()).transpose()?;

    // Beside it, and read only on Enter: which agent sessions have registered a
    // socket against this worktree. A reader with no hook installed has none,
    // which is the common case and costs a directory read that finds nothing.
    let registry =
        state::registry_for(worktree.workdir(), |key| std::env::var(key).ok()).transpose()?;

    // The view defaults reach the frame before its first walk, not just the
    // shell. `staged` is the only key that decides what the frame *walks* rather
    // than how the rows it already holds are arranged, so it must be honoured here.
    arm_frame(&mut frame, config);
    frame.advance()?;

    // Inert until something sends, so I1 never sees it. Built here because the
    // handler on it is armed on the next line, before the terminal is taken.
    let (tx, rx) = mpsc::channel();

    // Before the terminal is taken, which is the whole point of it being here.
    let armed = signal::forward(tx.clone());

    let mut shell = Shell {
        session: Session::enter()?,
        app: App::configured(config),
        // Its 318µs of grammar *loading* lands before first paint, which is
        // where it belongs: I7 gives startup 50ms, so this is well under one
        // percent of it and deferring it would only move it onto the first frame
        // that draws something.
        effects: EffectManager::default(),
        notice_effects: EffectManager::default(),
        painted: Instant::now(),
        highlighter: Highlighter::new(),
        // Empty at startup, so every file in an already-dirty worktree draws cold until
        // something writes to it.
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
        selected: None,
        scrolling: None,
        scrolling_until: None,
        next: None,
        leaving: None,
        served: Vec::new(),
        written: false,
        warming: None,
        store,
        registry,
        store_watch: None,
        notes_stale: false,
        ledger: Ledger::default(),
        note_effects: NoteEffects::default(),
        box_effect: None,
        alerts: Alerts::default(),
        effects_ran: false,
    };

    // The arming from above, reported now that there is somewhere to report it. A
    // signal that arrived before this point is not lost: it waits in the channel
    // and the first `recv` below handles it.
    if let Err(e) = armed {
        shell.app.warn(format!(
            "not catching an external stop, so a kill may not restore the terminal: {e}"
        ));
    }

    // The notes a pane before this one left, so the first frame draws them.
    shell.reload_notes(Instant::now());

    // For a screen with rows on it, so a clean worktree spawns nothing.
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

    // One call, two frames. `Shell::draw` settles the repaint debt itself, so the
    // opening is one mechanism rather than two statements in a row that a future edit
    // can separate.
    shell.draw(&mut frame, &worktree, Instant::now())?;

    // Armed only now.
    spawn_watch(path.to_path_buf(), tx.clone());

    // And the store's own, an event source beside the tree's: what the agent
    // writes there is a wake, never a poll.
    shell.watch_store(&tx, Instant::now());

    // What the tree is made of, which the changed set cannot say on a tree nobody has
    // written to yet.
    shell
        .highlighter
        .warm_repository(worktree.workdir().to_path_buf(), Some(warmed(&tx)));

    if update {
        let tx = tx.clone();
        update::watch(
            || update::check(VERSION),
            move |version| {
                let _ = tx.send(Wake::Update(version));
            },
        );
    }

    // Demands the opening two frames raised, dispatched before the loop blocks.
    // Without this the screen keeps whatever the two paints managed until the
    // agent's next write, which on a tree nobody is touching is never.
    shell.request_warm(&worktree, &tx);

    // Cloned rather than moved, because the loop below keeps `tx` to hand a sender to
    // each warm it spawns.
    spawn_input(tx.clone());

    // Reused across iterations rather than allocated per wake. A monitor is left
    // open for days and I3 is the invariant that notices, so the one buffer the
    // loop needs is the one buffer it keeps.
    let mut batch = Vec::with_capacity(DRAIN_CAP);

    // Every clock the loop owns is folded in `Shell::patience`.
    'awake: loop {
        // Untimed with nothing held, which is the whole invariant. With something
        // held the wait is only as long as the next step is away, so the loop
        // still blocks rather than spinning.
        let wake = match shell.patience(&frame, Instant::now()) {
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

        // The step, folded to however many intervals actually elapsed. One `apply` and
        // one paint whatever the terminal has been doing, so the rate is a fact about
        // time rather than about paint speed.
        let repeat = shell.held.and_then(|hold| hold.fire(Instant::now()));
        if let Some((step, next)) = repeat {
            shell.held = Some(next);
            // The third of three, and it joined last. `Regions::step_at` yields only
            // `Scroll` and `ScrollList` today, neither of which reads a height, so the
            // literal zero this replaced was right by accident rather than by rule.
            let height = shell.diff_rows_for(step, frame.files())?;
            match shell.apply(step, &mut frame, height) {
                Ok(true) => {}
                Ok(false) => break 'awake,
                Err(e) => shell.app.warn(e.to_string()),
            }
        }

        let Some(wake) = wake else {
            // A timeout woke this, so there is nothing to drain and the paint below is
            // the whole of the frame.
            let began = Instant::now();
            shell.settle_scroll(began);
            shell.settle_footer(began);
            // A departure's end, which is a deadline this frame may be the one to find.
            shell.settle_notes(began);
            // The margin's end after a print that moved, which no filesystem event marks.
            shell.settle_heights(&mut frame);
            shell.app.sample_memory();
            shell.draw(&mut frame, &worktree, began)?;
            shell.request_warm(&worktree, &tx);
            // A timeout is a frame and belongs in the frame time the bar reports, which
            // `SPEC.md` §5.1 defines as the whole turn of this loop.
            shell.app.record_frame(began.elapsed());
            continue;
        };
        // Started before the drain, not after it, because the drain is part of what a
        // frame costs.
        let began = Instant::now();
        drain(&mut batch, wake, &rx, DRAIN_CAP);

        for wake in batch.drain(..) {
            match wake {
                // Returning rather than breaking, so the reason travels with the exit,
                // and `shell` drops on the way out to put the terminal back first.
                Wake::InputLost => {
                    // The one exit by `return`, so the flush after the loop misses it.
                    shell.settle_send(Instant::now());
                    return Err("terminal input ended, so there was no way left to quit".into());
                }
                // The quit key's arm without the key, so `break` and not `return`:
                // nothing failed, and a message printed after the terminal came back
                // would be a message the sender did not ask for.
                Wake::Signalled => break 'awake,
                Wake::Input(event) => {
                    // Checked before the event is interpreted, because a release is not
                    // an action and would otherwise fall through the `else` below with
                    // the repeat still armed.
                    let regions = shell.regions();
                    if shell.held.is_some_and(|hold| hold.ends(&event, regions)) {
                        shell.held = None;
                    }
                    // What the pointer is over, before anything asks what it meant.
                    shell.hovered = hover_after(&event, regions, shell.hovered);
                    // The mode: while the reader's hand is in the box every key is the
                    // box's and a press anywhere else closes it, and only what the box
                    // does not answer reaches the map below.
                    if shell.app.box_open() {
                        match notes::box_route(&event, shell.box_area()) {
                            BoxRoute::Send => {
                                shell.commit_box(&tx, Instant::now());
                                continue;
                            }
                            BoxRoute::Cancel => {
                                shell.cancel_box(Instant::now());
                                continue;
                            }
                            BoxRoute::Edit(input) => {
                                shell.app.box_edit(input);
                                continue;
                            }
                            BoxRoute::Paste(text) => {
                                shell.app.box_paste(&text);
                                continue;
                            }
                            BoxRoute::Inert => continue,
                            BoxRoute::Through => {}
                        }
                    }
                    // A press on a content row's gutter opens the box and never begins
                    // a selection, which is B20 and B21 sharing no cell: it is answered
                    // here and the wash below never sees it.
                    if let Some(offset) = notes::press_at(&shell.screen, regions, &event) {
                        shell.open_box(offset, Instant::now());
                        continue;
                    }
                    // And a press on a note's own left side takes that note back,
                    // answered here for the same reason and in the same place.
                    if let Some(id) = notes::edge_at(&shell.screen, regions, &event) {
                        shell.withdraw_note(&id, Instant::now());
                        continue;
                    }
                    // Before the event is interpreted, for the hold's reason: a press
                    // opening one is an action too, and the wash precedes its clearing.
                    let (standing, ended) = selection_after(&event, regions, shell.selected);
                    shell.selected = standing;
                    if let Some(span) = ended {
                        shell.send_wash(span);
                    }
                    // A drag under way answers before the column is consulted, and that
                    // ordering is the fix.
                    if let Some(on) = shell.grabbed {
                        if let Some(drag) = drag_action(&event, regions, on) {
                            // The height, because a drag on the diff's bar is a
                            // `DiffTo` and `DiffTo` reads one.
                            let height = shell.diff_rows_for(drag, frame.files())?;
                            match shell.apply(drag, &mut frame, height) {
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
                    // Armed from the same press that performs the first step, so a
                    // click is one step and a hold is that step continued.
                    if let Event::Mouse(mouse) = &event
                        && matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
                        && let Some(step) = regions.step_at(mouse.column, mouse.row)
                    {
                        shell.held =
                            Some(Held::new(step, (mouse.column, mouse.row), Instant::now()));
                    }
                    // A press on the track takes hold of that bar instead,
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
                    // Asked for only by the one action that reads it, and that is the
                    // drain's doing rather than tidiness.
                    let height = shell.diff_rows_for(action, frame.files())?;
                    shell.note_scroll(action, Instant::now());
                    match shell.apply(action, &mut frame, height) {
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
                    // Armed here rather than in `App::follow`, because a change
                    // arrives whether or not the viewport moves to it.
                    for path in &paths {
                        shell
                            .effects
                            .add_unique_effect(path.clone(), motion::coalescing(ARRIVING));
                    }
                    // A tick is the world changing, so a demand that could not be
                    // served a moment ago is worth offering again.
                    shell.written = true;
                    // Sampled here and nowhere else, which is the whole of I10's
                    // relationship with I1: the window is real time, and the only thing
                    // that moves it is a wake the loop was already having.
                    shell
                        .history
                        .record_sized(sized(worktree.workdir(), &paths), began);
                    // A walk that fails describes the whole tree rather than one
                    // path in it, so the previous frame is still the best thing to
                    // draw and the footer says why. One file's own failure never
                    // reaches here: `Frame::diff` draws it instead.
                    match frame.advance() {
                        // Advance first, follow second, and the order is the whole of
                        // it: the path is looked up in the file list, and before the
                        // walk that list is the previous frame's.
                        Ok(()) => {
                            if let Some(path) = paths.last() {
                                shell.app.follow(path, &frame);
                            }
                        }
                        Err(e) => shell.app.warn(e.to_string()),
                    }
                }
                // Both halves, and they are not the same half twice. The mode is
                // durable and goes to the header; the message says which failure it was
                // and goes to the footer, where a notice belongs.
                Wake::WatchLost(message) => {
                    shell.app.watch_lost();
                    shell.app.warn(message);
                }
                // Deliberately nothing.
                Wake::Warmed => {}
                Wake::Update(version) => {
                    shell.say(
                        format!("vigia {version} is available"),
                        Voice::Arrived,
                        began,
                    );
                }
                // Marked rather than read here, so a burst of writes in one batch
                // is one listing.
                Wake::Notes => shell.notes_stale = true,
                Wake::Posted(posted) => {
                    if let Some(word) = post::word(posted) {
                        shell.say(word.to_owned(), Voice::Said, began);
                    }
                }
            }
        }

        // Before the paint: a notice either of them raises has to reach this frame.
        shell.settle_footer(began);
        // The store, read back once for the batch, and the departures due.
        shell.settle_notes(began);
        // And the settle deadline, which can fall due under a drained batch too.
        shell.settle_heights(&mut frame);

        // Before the paint, so the cell drawn below carries this frame's number rather
        // than the previous one's, and inside the timed region, so the read's own cost
        // lands in the frame time it sits beside.
        shell.app.sample_memory();

        // Once per batch, not once per wake. That is the whole of the
        // coalescing: every wake above was handled, in arrival order, and only
        // the paint is shared. See `drain`.
        shell.draw(&mut frame, &worktree, began)?;

        // After the paint, because the paint is what raises the demand.
        // `Highlighter::wanted` describes the frame that just drew, so asking before it
        // would be acting on the previous screen's answer.
        shell.request_warm(&worktree, &tx);

        // After the paint, because the paint is the last third of what a frame costs.
        shell.app.record_frame(began.elapsed());
    }

    // A release then `q` copies and leaves, and every arm ending the loop does so before
    // the batch reaches its send. `InputLost` returns, and carries its own.
    shell.settle_send(Instant::now());

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
        // A directory is not a write with a size, and on the platforms where it has one
        // it is a lie: `relative` admits directory events, and `symlink_metadata`
        // reports 0 for a directory on Windows but 4096 and rising on Linux and macOS.
        Ok(meta) if meta.is_dir() => None,
        Ok(meta) => Some(meta.len()),
        // A file that is gone weighs zero bytes, not "no size", and the difference is
        // the largest edit a reader can make.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Some(0),
        // Anything else is a size that could not be read rather than a file that is not
        // there: a permission change, a path that stopped being valid Unicode under us.
        Err(_) => None,
    }
}

/// How long the direction arrows stay lit after the last scroll.
pub const SCROLL_LINGER: std::time::Duration = std::time::Duration::from_millis(220);

/// How long a message in `voice` holds the footer, both transitions included.
#[must_use]
pub fn linger_for(voice: Voice) -> std::time::Duration {
    match voice {
        Voice::Said | Voice::Alert => NOTICE_LINGER,
        Voice::Arrived => ARRIVED_LINGER,
    }
}

/// How each voice arrives: the text crossfading into place, glyphs never moving,
/// because revealing characters leaves the line unreadable while it runs. Which
/// colour travels which way, and `None`'s meaning, are §11.1.
#[doc(hidden)]
pub fn arrival(voice: Voice, theme: &Theme) -> Option<tachyonfx::Effect> {
    let from = travels_from(voice, theme)?;
    Some(motion::fading(
        from,
        duration_for(voice),
        travelling(voice),
        false,
    ))
}

/// How each voice leaves: back to the hints' colour, by the road it came. The
/// message stays drawn throughout, or this lands on the hints instead.
#[doc(hidden)]
pub fn departure(voice: Voice, theme: &Theme) -> Option<tachyonfx::Effect> {
    let to = travels_from(voice, theme)?;
    Some(motion::fading(
        to,
        duration_for(voice),
        travelling(voice),
        true,
    ))
}

/// The road each voice's colour takes across the message: a receipt's from the
/// end it was typed at, a warning's from both ends at once, an announcement's
/// all at once, because it is the one nobody is watching arrive.
fn travelling(voice: Voice) -> AnyPattern {
    match voice {
        Voice::Said => SweepPattern::right_to_left(TRAVEL).into(),
        Voice::Arrived => AnyPattern::default(),
        Voice::Alert => RadialPattern::center()
            .with_transition_width(TRAVEL_IN)
            .into(),
    }
}

/// Columns the colour's leading edge is soft over as it crosses the message.
const TRAVEL: u16 = 12;

/// The same, for the warning that resolves from both ends at once.
const TRAVEL_IN: f32 = 10.0;

/// The colour a message travels from, and back to: the hints it replaces.
fn travels_from(voice: Voice, theme: &Theme) -> Option<ratatui::style::Color> {
    theme::contrast(theme.chrome_dim, voice_style(voice, theme))
}

/// How long a voice takes, arriving or leaving. One table: copies drift.
const fn duration_for(voice: Voice) -> std::time::Duration {
    match voice {
        Voice::Said => SAID_ARRIVING,
        Voice::Arrived => NOTICE_ARRIVING,
        Voice::Alert => ALERT_ARRIVING,
    }
}

const NOTICE_KEY: &str = "notice";

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
    /// Keyed by path, so a second write replaces an effect rather than stacking.
    effects: EffectManager<String>,
    /// The footer's own: the diff's are clipped to it, and one manager processed
    /// twice advances every effect in it twice a frame.
    notice_effects: EffectManager<String>,
    /// When the previous frame painted. The time since it is what an effect is
    /// told, through `effect_interval`, which may answer none of it.
    painted: Instant,
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
    /// How many changes the run this pane is not drawing holds.
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
    /// The diff rows a drag has selected, in screen rows of the last paint.
    selected: Option<Selection>,
    /// Which way the viewport is currently being moved, and until when.
    scrolling: Option<(Grabbed, isize)>,
    /// When the mark above stops being true.
    scrolling_until: Option<Instant>,
    /// What replaces the message once it has finished leaving. One slot, not a
    /// queue: a newer message supersedes one that has not started.
    next: Option<(String, Voice)>,
    /// When a spent message has finished leaving. It is still drawn until then,
    /// because an outro over a line the hints have already taken back animates
    /// the wrong thing.
    leaving: Option<Instant>,
    /// The demand the last warm was handed, so a demand nothing can serve is
    /// asked for once rather than on every frame.
    served: Vec<String>,
    /// Whether the tree has changed since the last warm was spawned.
    written: bool,
    /// The warm this shell last asked for, if any.
    warming: Option<std::thread::JoinHandle<vigia_core::WarmReport>>,
    /// The reader's notes on this worktree, or `None` when the environment names
    /// no directory to keep them in.
    store: Option<Store>,
    /// The agent sessions that have registered a socket against this worktree,
    /// which Enter posts into. `None` alongside `store`, for the same reason.
    registry: Option<Registry>,
    /// The watch over the store, or `None` until something has written there.
    store_watch: Option<StoreWatch>,
    /// A wake said the store changed and no paint has read it back yet.
    notes_stale: bool,
    /// What the pane holds of the store between wakes: the notes as listed and
    /// the ones on their way off the screen.
    ledger: Ledger,
    /// The effects running over notes' rows.
    note_effects: NoteEffects,
    /// The effect over the note box's rows while it arrives or leaves.
    box_effect: Option<Timed>,
    /// What the last listing had to say, so the alert is said when that changes
    /// and not on every wake.
    alerts: Alerts,
    /// Whether an effect was drawing at the previous paint, which decides what the
    /// next one is told passed.
    effects_ran: bool,
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
            selected: self.selected,
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

    /// Whether anything drawn is still moving. Asked by the clock that offers
    /// the next frame and by the record of whether one drew, so the two cannot
    /// disagree about what counts as an effect.
    fn effects_running(&self) -> bool {
        self.effects.is_running()
            || self.notice_effects.is_running()
            || self.note_effects.is_running()
            || self.box_effect.as_ref().is_some_and(Timed::is_running)
    }

    /// How long the loop may block before something here has to act.
    fn patience(&self, frame: &vigia_core::Frame, now: Instant) -> Option<std::time::Duration> {
        // Every deadline is folded here rather than at the receive, so `patience`
        // stays the one place that decides whether this program owns a timer.
        input::patience(
            input::Deadlines {
                held: self.held,
                linger: self.scrolling_until,
                // The departure's deadline outranks the message's own, which
                // is spent by then: `patience` folds a past deadline to zero,
                // so offering it would spin the loop flat out until the
                // departure finished instead of asking for frames.
                notice: self.leaving.or_else(|| self.app.flash_until()),
                ageing: self.history.ages_in(now),
                // The frame owns the instant its last waiting file settles, and is
                // asked at decide time, as the window is.
                settling: frame.settles_in(SystemTime::now()),
                // The effect says when it is finished, so the clock is asked for a
                // frame only while one is running and goes untimed the moment none is.
                arriving: self.effects_running().then(|| now + ARRIVING_FRAME),
                // The frame after a departure ends is the one that drops its rows.
                departing: self.ledger.ends_in(),
                // And the frame after the box has left is the one that drops its.
                closing: self.app.box_ends_in(),
            },
            now,
        )
    }

    /// The cells the box drew on the last paint, which a press is judged against.
    fn box_area(&self) -> Option<Rect> {
        render::box_cells(&self.regions, &self.screen)
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

    /// Walk status once the frame's wait on a moved file has run out, so the
    /// heights it kept are asked again. On every path to a paint, so a spent
    /// deadline is consumed on the turn that finds it and not on a timeout.
    fn settle_heights(&mut self, frame: &mut vigia_core::Frame) {
        if let Err(e) = frame.advance_if_settled(SystemTime::now()) {
            self.app.warn(e.to_string());
        }
    }

    /// Clear the direction mark once its burst has stopped.
    fn settle_scroll(&mut self, now: Instant) {
        if input::settled(self.scrolling_until, now) {
            self.scrolling = None;
            self.scrolling_until = None;
        }
    }

    /// Apply `action`, taking any wash with it. A wash belongs to a gesture the
    /// pointer is still performing, so anything else arriving ends it, and `Esc`
    /// cancels that gesture rather than leaving. A resize is the exception: it is a
    /// redraw and no state change, and [`repainted`] retires the mark if the regions
    /// really moved.
    fn apply(
        &mut self,
        action: Action,
        frame: &mut vigia_core::Frame,
        height: usize,
    ) -> vigia_core::Result<bool> {
        // The rung `Esc` climbs over quitting, reachable only while the button is
        // down. Without it a tap mid-drag ends the program.
        if action == Action::Escape && self.selected.is_some() {
            self.deselect();
            return Ok(true);
        }
        if action != Action::Redraw {
            self.deselect();
        }
        self.app.apply(action, frame, height)
    }

    /// Drop the wash and the lines it stood for together: clearing one and not the
    /// other leaves a send carrying rows nothing on screen is claiming.
    fn deselect(&mut self) {
        self.selected = None;
        self.app.select(None);
    }

    /// End a drag on the clipboard: `span` is what the last painted frame washed.
    ///
    /// Resolved here rather than read off `App`, whose own resolve runs only on a
    /// frame that collects: the loop paints once per drained batch, so a press and its
    /// release can share one and leave nothing collected in between. `Shell::regions`
    /// is the layout those screen rows were read against.
    fn send_wash(&mut self, span: Selection) {
        if let Some(lines) = self.screen.lines_in(span.offsets(self.regions.diff.top)) {
            self.app.send(&lines);
        }
    }

    /// Open the box under the line a press landed on, with the text of the open
    /// note already there when there is one, and arm its entrance. With no store
    /// to write to, and on a pane too narrow to draw the box, there is nothing
    /// to open: the footer says so instead.
    fn open_box(&mut self, offset: usize, now: Instant) {
        if self.store.is_none() {
            self.say(state::no_home(), Voice::Alert, now);
            return;
        }
        if !notes::has_room(self.regions) {
            self.say(
                "no room for a note on this pane".to_owned(),
                Voice::Alert,
                now,
            );
            return;
        }
        let Some((anchor, existing)) = notes::opening(&self.screen, offset, self.app.notes())
        else {
            return;
        };
        // A box still leaving stands in for the note it holds, so the screen
        // this press landed on no longer marks that line: without asking the
        // box, the press would open an empty one and Enter would write a second
        // note beside the first.
        let existing = existing.or_else(|| self.app.box_over(&anchor).cloned());
        self.app.open_box(anchor, existing.as_ref());
        self.box_effect = Some(Timed::armed(notes::box_entrance(&self.theme), now));
    }

    /// Enter: write what the box holds, close it at once, and read the store
    /// back so the next frame draws the note arriving under its line. A write
    /// the store refuses is a footer alert and the box stays with its text,
    /// which is B7's rule for a monitor's own writes and the reader's words kept.
    fn commit_box(&mut self, tx: &Sender<Wake>, now: Instant) {
        let Some(store) = &self.store else {
            return;
        };
        let Some(open) = self.app.note_box() else {
            return;
        };
        let arrived = match notes::commit(store, open) {
            Ok(Committed::Written(id) | Committed::Rewritten(id)) => Some(id),
            Ok(Committed::Withdrawn(_) | Committed::Nothing) => None,
            Err(e) => {
                self.say(format!("could not write the note: {e}"), Voice::Alert, now);
                return;
            }
        };
        // What goes to the agent is the note as the store took it, never the
        // box's own copy, so the two rungs cannot disagree. A note simply gone
        // was withdrawn by another hand between the two acts, which is a race
        // rather than a fault and says nothing.
        let posting = match arrived.as_ref().map(|id| store.get(id)) {
            Some(Err(e)) => {
                self.say(
                    format!("wrote the note but could not read it back: {e}"),
                    Voice::Alert,
                    now,
                );
                None
            }
            Some(Ok(note)) => note,
            None => None,
        };
        self.app.take_box();
        self.box_effect = None;
        // The first write made the directory, so there is something to watch.
        if arrived.is_some() && self.store_watch.is_none() {
            self.watch_store(tx, now);
        }
        self.reload_notes(now);
        if let Some(id) = arrived {
            self.note_effects
                .arm(vec![Change::Written(id)], &self.theme, now);
        }
        if let Some(note) = posting {
            self.post_note(tx, &note);
        }
    }

    /// Hand the note to every agent session registered against this worktree,
    /// which is `SPEC.md` §11.2 B21 ruling 6's second half: the store is written
    /// first either way, and this runs after it.
    ///
    /// On a thread of its own, because opening a socket is the one act here that
    /// waits on something outside this process and a session that has ended must
    /// never be something the pane learns about by waiting. The footer's word
    /// comes back as a wake, and the note is already drawn under its line by
    /// then, so nothing on screen is waiting for it either.
    fn post_note(&self, tx: &Sender<Wake>, note: &Note) {
        let Some(registry) = self.registry.clone() else {
            return;
        };
        let (root, note, tx) = (self.root.clone(), note.clone(), tx.clone());
        post::spawn(
            registry,
            move || post::content(&note, &post::context_for(Path::new(&root), &note)),
            move |posted| {
                let _ = tx.send(Wake::Posted(posted));
            },
        );
    }

    /// Esc, or a press anywhere outside the box: the keys are the pane's again
    /// now, and the rows stay drawn while the entrance plays backwards.
    fn cancel_box(&mut self, now: Instant) {
        // The rows stand aside for as long as the sweep runs, which is the
        // motion's own length rather than a constant read twice.
        let exit = notes::box_exit();
        self.app.close_box(now + length(&exit));
        self.box_effect = Some(Timed::armed(exit, now));
    }

    /// Arm the watch over the store, so a write by the agent or another pane is
    /// a wake. With nothing on disk to arm on yet it stays unarmed until this
    /// pane's first write makes the directory; a watch that cannot be made is one
    /// footer alert, and the store still reads back on this pane's own writes.
    fn watch_store(&mut self, tx: &Sender<Wake>, now: Instant) {
        let Some(store) = &self.store else {
            return;
        };
        let tx = tx.clone();
        match store.watch(move || {
            let _ = tx.send(Wake::Notes);
        }) {
            Ok(watch) => self.store_watch = watch,
            Err(e) => self.say(format!("not watching the notes: {e}"), Voice::Alert, now),
        }
    }

    /// Take a note back: remove its file and read the store back, so the next
    /// frame draws it leaving. A note the agent resolved between the frame this
    /// press landed on and the press itself is left alone and nothing is said,
    /// since the reader is being answered rather than refused. A removal the
    /// store refuses is one footer alert, as every other write of B21's is.
    fn withdraw_note(&mut self, id: &str, now: Instant) {
        let Some(store) = &self.store else {
            return;
        };
        match notes::withdraw(store, id) {
            Ok(true) => self.reload_notes(now),
            Ok(false) => {}
            Err(e) => self.say(
                format!("could not take the note back: {e}"),
                Voice::Alert,
                now,
            ),
        }
    }

    /// Read the store, arm an effect for whatever moved, and hand the notes to
    /// the next collect. A file the store cannot read is skipped, and a store
    /// that cannot be read is left as it was; either is said when it changes,
    /// rather than on every wake the agent causes.
    fn reload_notes(&mut self, now: Instant) {
        let Some(store) = &self.store else {
            return;
        };
        let listing = store.list();
        if let Some(told) = self.alerts.of(&listing) {
            self.say(told, Voice::Alert, now);
        }
        if let Ok(listing) = listing {
            let changes = self.ledger.reload(listing.notes, now);
            self.note_effects.arm(changes, &self.theme, now);
            self.publish_notes();
        }
    }

    /// Take the notes through one frame: read the store back if a wake said it
    /// changed, sweep the resolves whose beat has run, drop the departures that
    /// have ended, and retire the effects that have run their length. On every
    /// path to a paint, so a departure's end is consumed on the turn that finds
    /// it and not on a timeout.
    fn settle_notes(&mut self, now: Instant) {
        if std::mem::take(&mut self.notes_stale) {
            self.reload_notes(now);
        }
        let settled = self.ledger.settle(now);
        if settled.changed {
            self.publish_notes();
        }
        // A wake later than the one that read the resolve, because `RESOLVE_BEAT`
        // separates them and the ledger is what holds the clock across it.
        self.note_effects.arm(
            settled.sweeping.into_iter().map(Change::Swept).collect(),
            &self.theme,
            now,
        );
        self.prune_departed(&settled.prune, now);
        self.note_effects.settle(now);
        // A pane narrowed under an open box can no longer draw it, and the rule
        // that a mode is never invisible has to hold after the resize and not
        // only at the press: the box leaves the way Esc sends it, and the
        // footer says why, since the reader did not ask for either.
        if self.app.box_open() && !notes::has_room(self.regions) {
            self.cancel_box(now);
            self.say(
                "no room for a note on this pane".to_owned(),
                Voice::Alert,
                now,
            );
        }
        // The box's own end, on the same terms: dropped on the turn that finds it.
        self.app.settle_box(now);
        self.box_effect.take_if(|armed| armed.spent(now));
    }

    /// Hand the next collect what the ledger says is drawn.
    fn publish_notes(&mut self) {
        self.app.set_notes(self.ledger.drawn());
    }

    /// Remove the files of the resolved notes whose departure has just run.
    ///
    /// The pane's, because only the pane knows the reader has watched the line
    /// arrive, which the server cannot see from the other end. A removal that
    /// fails is one footer alert and no more: the id is already remembered as
    /// departed, so the file is never drawn or reached for again.
    fn prune_departed(&mut self, ids: &[String], now: Instant) {
        let Some(store) = &self.store else {
            return;
        };
        let refused: Vec<String> = ids
            .iter()
            .filter_map(|id| store.remove(id).err().map(|e| e.to_string()))
            .collect();
        for why in refused {
            self.say(format!("could not remove a note: {why}"), Voice::Alert, now);
        }
    }

    /// Write what a gesture asked for, and say so for `NOTICE_LINGER`, which is
    /// what a receipt gets.
    ///
    /// It says **sent** rather than copied, which is honest and not modest on the
    /// escape's route: OSC 52 has no reply and several terminals ship it disabled.
    /// The tmux route has an exit status, so there the word is checked.
    /// A failed write is reported rather than propagated: a draw that fails has taken
    /// the pane with it, but a copy is one a reader can go on watching without.
    fn settle_send(&mut self, now: Instant) {
        if let Some(sending) = self.app.take_sending() {
            let said = sending.said;
            let plan = clipboard::plan(
                std::env::var_os("TMUX").as_deref(),
                clipboard::remote(|name| std::env::var_os(name).is_some()),
            );
            let (told, voice) = match clipboard::put(&mut self.session, &sending.text, &plan) {
                Ok(()) => (format!("sent {said} to the clipboard"), Voice::Said),
                Err(e) => (format!("could not send {said}: {e}"), Voice::Alert),
            };
            self.say(told, voice, now);
        }
    }

    /// Take the footer through one frame: retire what is spent, start whatever
    /// was waiting behind it, then write what a gesture asked for. Stating that
    /// order once is what stops the two paths to a paint disagreeing about it.
    fn settle_footer(&mut self, now: Instant) {
        self.settle_leaving(now);
        self.settle_send(now);
    }

    /// Retire a spent message and start what replaces it, in the same frame so
    /// there is no gap. The message stays drawn until its departure has run.
    fn settle_leaving(&mut self, now: Instant) {
        if let Some(gone) = self.leaving {
            if now < gone {
                return;
            }
            self.app.clear_flash();
            self.leaving = None;
            if let Some((message, voice)) = self.next.take() {
                self.show(message, voice, now);
            }
            return;
        }
        if !input::settled(self.app.flash_until(), now) {
            return;
        }
        if let Some(voice) = self.app.voice() {
            self.begin_leaving(voice, now);
        }
    }

    /// Say `message`, replacing what the line holds without cutting it off.
    ///
    /// One already leaving keeps going; one still settled is sent away early.
    /// Either way `next` is what the line takes when that finishes, so a change
    /// is one thing fading into another rather than a swap between frames.
    fn say(&mut self, message: String, voice: Voice, now: Instant) {
        if self.leaving.is_some() {
            self.next = Some((message, voice));
            return;
        }
        if let Some(showing) = self.app.voice() {
            self.next = Some((message, voice));
            self.begin_leaving(showing, now);
            return;
        }
        self.show(message, voice, now);
    }

    /// Send what is on the line away, over the frames its voice takes.
    fn begin_leaving(&mut self, voice: Voice, now: Instant) {
        if let Some(effect) = departure(voice, &self.theme) {
            self.notice_effects
                .add_unique_effect(NOTICE_KEY.to_owned(), effect);
        }
        self.leaving = Some(now + duration_for(voice));
    }

    /// Put `message` on the footer now, in `voice`, and arm its arrival.
    ///
    /// The one place a notice is armed, so the one place its effect is.
    fn show(&mut self, message: String, voice: Voice, now: Instant) {
        self.leaving = None;
        // The departure comes out of the linger, so it is the whole of the time.
        let spent = now + linger_for(voice).saturating_sub(duration_for(voice));
        self.app.flash(message, spent, voice);
        if let Some(effect) = arrival(voice, &self.theme) {
            self.notice_effects
                .add_unique_effect(NOTICE_KEY.to_owned(), effect);
        }
    }

    /// The drawable area of the terminal right now.
    fn area(&mut self) -> Result<Rect, Failure> {
        let screen = self.session.screen();
        screen.autoresize()?;
        Ok(screen.get_frame().area())
    }

    /// Where the regions of the last painted screen were.
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
        // The window is rolled here because this is where every frame passes.
        self.history.record_sized([], now);
        self.paint(frame, worktree, now)?;
        if self.app.owes_repaint() {
            self.paint(frame, worktree, now)?;
        }
        Ok(())
    }

    /// One collect and one paint, with no view of what it leaves owed.
    fn paint(
        &mut self,
        frame: &mut vigia_core::Frame,
        worktree: &Worktree,
        now: Instant,
    ) -> Result<(), Failure> {
        // Before the chrome, because the chrome carries it, and from the frame's own
        // file count so the read happens on exactly the frames that draw the answer.
        // That is the whole of I4 for this read.
        self.branch = worktree.branch();

        // The chrome is built before the layout, not after, because the footer takes a
        // second line at narrow widths and `body_layout` has to know whether this frame
        // is one of those.
        let chrome = self.app.chrome(
            &self.name,
            self.branch.as_deref(),
            self.pointing(),
            self.elsewhere,
            &self.root,
        );
        let area = self.area()?;
        let body = body_layout(
            area,
            &chrome,
            frame.files().len(),
            view::list_rows_wanted(frame.files()),
        );
        // Against this frame's own layout rather than the last paint's, so a wash is
        // judged on the screen it is drawn over.
        self.app.select(
            self.selected
                .map(|had| had.offsets(body.areas(area).diff.y)),
        );
        match self
            .app
            .view(frame, &mut self.highlighter, &self.history, body)
        {
            Ok(view) => self.screen = view,
            // The wash goes too: this frame resolved nothing, and keeping the last
            // frame's answer would leave one standing over rows it no longer covers.
            Err(e) => {
                self.app.warn(e.to_string());
                self.app.select(None);
            }
        }
        // A span the collect resolved to nothing is not a selection, whatever the
        // pointer did: it draws nothing, and left standing it would take `Esc`.
        if !self.app.holds_a_selection() {
            self.deselect();
        }

        // On a frame with nothing to draw, where the work went.
        self.elsewhere = if self.screen.files == 0 && !self.app.staged() {
            worktree.count_of(vigia_core::Origin::Staged).unwrap_or(0)
        } else {
            0
        };

        // Rebuilt so a notice raised by the collect above, and the notes it
        // counted, reach this frame rather than the next one. Safe to differ from
        // the chrome the height came from: neither can change how many rows the
        // footer takes, by construction.
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
        // What an effect is told passed. Taken before the draw so it and the frame
        // agree on the interval.
        let since = effect_interval(
            self.effects_ran,
            now.saturating_duration_since(self.painted),
        );
        let effects = &mut self.effects;
        let notice_effects = &mut self.notice_effects;
        let note_effects = &mut self.note_effects;
        let box_effect = &mut self.box_effect;
        let mut painted = Regions::default();
        let was = self.regions;
        self.session.screen().draw(|f| {
            let area = f.area();
            // Captured from inside the draw, because `Frame::area` is the size the
            // paint actually used: `Shell::area` reads it again and a resize between
            // the two would leave a pointer told about a screen nobody saw.
            painted = render::regions(area, &chrome, screen);
            // A screen-anchored mark does not outlive a relayout, and both have to be
            // retired here, between the layout and the paint that uses them.
            chrome.hovered = repainted(chrome.hovered, was, painted);
            chrome.selected = repainted(chrome.selected, was, painted);
            render(f.buffer_mut(), area, screen, theme, glyphs, &chrome);
            // After the widgets, because an effect works on the cells they drew. The
            // diff's own region only: a heading arriving is not a reason to disturb
            // the header, the footer or the map.
            let over = Rect::new(
                painted.diff.left,
                painted.diff.top,
                painted.diff.width,
                painted.diff.rows,
            );
            effects.process_effects(since.into(), f.buffer_mut(), over);
            // The notes' own, each over the cells its note drew this frame. A note
            // off screen draws no cells and its effect waits, and `settle_notes`
            // retires it at its own end whether it ever drew or not.
            if note_effects.is_running() {
                let cells = render::note_cells(&painted, screen);
                note_effects.draw(since, f.buffer_mut(), &cells);
            }
            // The box's, over the cells it drew; off screen it waits the same way.
            if let Some(armed) = box_effect.as_mut()
                && let Some(over) = render::box_cells(&painted, screen)
            {
                armed.draw(since, f.buffer_mut(), over);
            }
            // Separate from the pass above, which is clipped to the diff: a
            // notice effect in that manager would be clipped away unseen.
            if let Some(notice) = render::notice_area(area, &chrome, screen) {
                notice_effects.process_effects(since.into(), f.buffer_mut(), notice);
            }
        })?;
        self.painted = now;
        self.effects_ran = self.effects_running();
        self.hovered = chrome.hovered;
        if chrome.selected.is_none() {
            self.deselect();
        }
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

        // The tick says only that something changed, which is all the shell needs:
        // every tick triggers one status walk, and a walk finds whatever the events
        // missed.
        while let Some(tick) = watcher.next_tick() {
            if tx.send(Wake::Tick(tick.paths)).is_err() {
                return;
            }
        }

        // Falling out of that loop should be unreachable: the only thing that ends it
        // is a `Stop`, and nothing here holds one.
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
        // `vigia .` is the invocation the tool is named after, and it headered the
        // screen `.` for a whole phase.
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
        // The other direction, and the one the fix could quietly break: a resolved path
        // must not start reporting a whole path, a drive prefix or a `\\?\`
        // extended-length form.
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
        // Coalescing is about the paint and not about the events.
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
        // The guard, and it is not a tuning knob. An event source faster than the shell
        // would otherwise keep the queue non-empty forever and the screen would never
        // be painted again: a stuck key, or a build touching thousands of files.
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
        // complete.
        let (tx, rx) = mpsc::channel();
        tx.send(tick(1)).expect("send");
        drop(tx);

        let mut batch = Vec::new();
        drain(&mut batch, tick(0), &rx, DRAIN_CAP);
        assert_eq!(batch.len(), 2, "the queued wake was lost with the sender");
    }

    #[test]
    fn the_wash_is_dropped_on_every_route_that_ends_it() {
        // `Shell` is private and holds a terminal, so its rules are read here rather
        // than driven. Each of these was a defect: a span the collect resolved to
        // nothing took `Esc`, and a cleared span left its answer standing behind it.
        let source = include_str!("lib.rs");
        let shipped = source.split("#[cfg(test)]").next().expect("split");
        for rule in [
            "if !self.app.holds_a_selection() {",
            "let (standing, ended) = selection_after(",
            "shell.send_wash(span);",
            "self.app.select(None);",
            // Neither is reachable from a test, and both shipped once.
            "if action == Action::Escape && self.selected.is_some() {",
            "if action != Action::Redraw {",
            "if chrome.selected.is_none() {",
            "self.screen.lines_in(span.offsets(self.regions.diff.top))",
        ] {
            assert!(
                shipped.contains(rule),
                "`{rule}` is gone, so a wash outlives something that ends it"
            );
        }
        // And the one route that must not end it. Every other arm that touches state
        // deselects, so the tick arm reads like an omission and would survive being
        // "tidied": an agent's write is what this pane exists to watch, and it may not
        // cancel a selection the reader is still making.
        let tick = shipped
            .split("Wake::Tick(paths) => {")
            .nth(1)
            .and_then(|rest| rest.split("Wake::WatchLost").next())
            .expect("the loop no longer has a tick arm");
        for gone in ["deselect()", "select(None)"] {
            assert!(
                !tick.contains(gone),
                "the tick arm calls `{gone}`, so an agent's write cancels a drag the \
                 reader is in the middle of"
            );
        }

        let collect = shipped
            .find(".view(frame,")
            .expect("`paint` no longer collects");
        let retire = shipped
            .find("if !self.app.holds_a_selection() {")
            .expect("checked above");
        assert!(
            collect < retire,
            "the wash is retired before the collect that decides whether it resolved \
             to anything, so it is judged on the frame before this one"
        );
    }

    /// The footer says **sent** rather than copied, because OSC 52 has no reply. Both
    /// spellings live inside a method that owns a terminal, so they are read here.
    #[test]
    fn the_footer_says_sent_rather_than_copied() {
        let source = include_str!("lib.rs");
        let shipped = source.split("#[cfg(test)]").next().expect("split");
        for said in ["sent {said} to the clipboard", "could not send {said}: {e}"] {
            assert!(
                shipped.contains(said),
                "`{said}` is gone, so the footer claims something OSC 52 cannot promise"
            );
        }
    }

    /// A press on a note's left side takes the note back before the wash can see
    /// it, and acts on the store rather than on the screen it landed on. Both live
    /// inside methods that own a terminal, so they are read here.
    #[test]
    fn a_press_on_a_notes_side_withdraws_before_the_wash_and_goes_through_the_store() {
        let source = include_str!("lib.rs");
        let shipped = source.split("#[cfg(test)]").next().expect("split");
        let press = shipped
            .find("notes::edge_at(&shell.screen, regions, &event)")
            .expect("the input arm no longer routes a press on a note's left side");
        let wash = shipped
            .find("selection_after(&event, regions, shell.selected)")
            .expect("the input arm no longer opens a wash");
        assert!(
            press < wash,
            "a press on a note's left side also begins a selection, so B20 and B21              share a cell"
        );
        let withdraw = shipped
            .split("fn withdraw_note(&mut self, id: &str, now: Instant) {")
            .nth(1)
            .and_then(|rest| {
                rest.split(
                    "
    }
",
                )
                .next()
            })
            .expect("`withdraw_note` is gone");
        assert!(
            withdraw.contains("notes::withdraw(store, id)"),
            "`withdraw_note` no longer goes through the store's own read-back, so a              resolve that landed since the frame is deleted with the note"
        );
        assert!(
            withdraw.contains("Ok(true) => self.reload_notes(now)"),
            "`withdraw_note` no longer reads the store back, so the rows of a note              it took stay drawn"
        );
    }

    /// A press on a content row's gutter goes to the store before the wash can
    /// see it, and the store is read back whatever the write answered. Both live
    /// inside methods that own a terminal, so they are read here.
    #[test]
    fn a_gutter_press_opens_the_box_before_the_wash_and_enter_reads_the_store_back() {
        let source = include_str!("lib.rs");
        let shipped = source.split("#[cfg(test)]").next().expect("split");
        // The mode first, then the gutter press, then the wash: a key while the
        // box is open never reaches the map, and a press that opens the box
        // never begins a selection.
        let mode = shipped
            .find("if shell.app.box_open() {")
            .expect("the input arm no longer asks whether the box owns the keys");
        let press = shipped
            .find("notes::press_at(&shell.screen, regions, &event)")
            .expect("the input arm no longer routes a gutter press to the box");
        let wash = shipped
            .find("selection_after(&event, regions, shell.selected)")
            .expect("the input arm no longer opens a wash");
        assert!(
            mode < press && press < wash,
            "the input arm consults the box, the gutter press and the wash out of \
             order, so a key reaches the pane through an open box or a press that \
             opens it also begins a selection"
        );
        let commit = shipped
            .split("fn commit_box(&mut self, tx: &Sender<Wake>, now: Instant) {")
            .nth(1)
            .and_then(|rest| rest.split("\n    }\n").next())
            .expect("`commit_box` is gone");
        // The box is taken only once the store has answered, after the arm that
        // reports a refusal returns, so a refused write keeps the reader's words.
        let refused = commit
            .find("Err(e) =>")
            .expect("`commit_box` no longer has a failure arm");
        let taken = commit
            .find("self.app.take_box()")
            .expect("`commit_box` no longer takes the box");
        let read_back = commit
            .find("self.reload_notes(now)")
            .expect("`commit_box` no longer reads the store back");
        let arrived = commit
            .find("Change::Written(id)")
            .expect("`commit_box` no longer arms the rows' arrival");
        assert!(
            refused < taken && taken < read_back && read_back < arrived,
            "`commit_box` takes the box, reads the store back and arms the arrival \
             out of order, so a refused write loses the text or the rows arrive \
             before the collect has them"
        );
        let cancel = shipped
            .split("fn cancel_box(&mut self, now: Instant) {")
            .nth(1)
            .and_then(|rest| rest.split("\n    }\n").next())
            .expect("`cancel_box` is gone");
        assert!(
            cancel.contains("self.app.close_box(now + length(&exit))")
                && cancel.contains("notes::box_exit()"),
            "`cancel_box` no longer stands the rows aside for exactly as long              as the sweep it armed runs"
        );
        // Both refusals sit ahead of the open, and the pane's own is the one no
        // drawn screen can catch: without it a press on a pane too narrow to
        // draw the box still takes every key.
        let open = shipped
            .split("fn open_box(&mut self, offset: usize, now: Instant) {")
            .nth(1)
            .and_then(|rest| rest.split("\n    }\n").next())
            .expect("`open_box` is gone");
        let opened = open
            .find("self.app.open_box(")
            .expect("`open_box` no longer opens one");
        assert!(
            open.contains("self.app.box_over(&anchor)"),
            "`open_box` no longer asks the box still leaving which note it holds, \
             so a press inside its exit opens an empty one and Enter writes a \
             second note on a line that already has one"
        );
        for refusal in ["self.store.is_none()", "!notes::has_room(self.regions)"] {
            let at = open
                .find(refusal)
                .unwrap_or_else(|| panic!("`open_box` no longer refuses on {refusal}"));
            assert!(
                at < opened,
                "`open_box` opens the box before it asks {refusal}, so the reader \
                 is put in a mode the pane cannot show them"
            );
        }
    }

    /// An announcement outlasts a receipt, and the one place a notice is armed
    /// reads the table rather than the receipt's constant. The minute itself is
    /// `tests/update.rs`'s to hold.
    #[test]
    fn an_announcement_outlasts_a_receipt_and_show_reads_the_table() {
        assert!(linger_for(Voice::Arrived) > linger_for(Voice::Said));
        assert_eq!(linger_for(Voice::Alert), NOTICE_LINGER);

        let source = include_str!("lib.rs");
        let shipped = source.split("#[cfg(test)]").next().expect("split");
        let show = shipped
            .split("fn show(&mut self, message: String, voice: Voice, now: Instant) {")
            .nth(1)
            .and_then(|rest| rest.split("\n    }\n").next())
            .expect("`show` is gone");
        assert!(
            show.contains("linger_for(voice)"),
            "`show` no longer asks how long this voice lingers, so every message \
             gets the receipt's four and a half seconds"
        );
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

        // Comments stripped, because both names appear in prose in this file and a
        // check that reads prose is a check on prose.
        let code: String = shipped
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");

        // Order. The handler has to be armed before the first step of the takeover, not
        // after the fourth: a signal arriving mid-takeover is otherwise still an
        // uncaught kill.
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

        // Every input a reader can get wrong is read before the takeover too, and the
        // same scan is what says so.
        for reader in [
            "Worktree::discover(",
            "frame.advance()",
            "theme::from_env(",
            "Glyphs::detect(",
            "config::from_env(",
            "update::wanted(",
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

        // Form. `signal`'s escalation latches after one ask: the second goes to the
        // default disposition and kills the process. That is only safe because this arm
        // leaves the loop unconditionally, so one ask is always enough.
        assert!(
            code.contains("Wake::Signalled => break 'awake"),
            "the signalled wake no longer unconditionally leaves the loop, which is \
             what makes `signal`'s one-way escalation latch safe"
        );

        // Order, again, and this one fixes the state and not the screen when it is
        // wrong.
        let layout = code
            .find("render::regions(area, &chrome, screen)")
            .expect("`draw` no longer computes the layout it is about to paint");
        let paint = code
            .find("render(f.buffer_mut()")
            .expect("`draw` no longer paints");
        // Both marks: either one outside the window is drawn against stale geometry.
        for mark in ["chrome.hovered", "chrome.selected"] {
            let retire = code
                .find(&format!("repainted({mark}"))
                .unwrap_or_else(|| panic!("`draw` no longer retires {mark}"));
            assert!(
                layout < retire && retire < paint,
                "{mark} is retired outside the window between the layout and the \
                 paint, so a relayout draws a mark against geometry it was never \
                 resolved against and nothing repaints to correct it"
            );
        }

        // Every frame rolls the window before it paints, and `Shell::draw` is where
        // every frame passes.
        let drawer = &code[code.find("\n    fn draw(").expect("`Shell::draw` is gone")..];
        let signature = &drawer[..drawer
            .find("-> Result<(), Failure>")
            .expect("`Shell::draw` no longer returns a `Result`")];
        assert!(
            signature.contains("now: Instant"),
            "`Shell::draw` no longer takes the turn's instant, so it is back to \
             rolling on a clock of its own"
        );

        // And every caller inside the loop hands it the turn's own instant, which the
        // two checks above cannot see.
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
            .find("self.paint(frame, worktree")
            .expect("`Shell::draw` no longer paints");
        assert!(
            rolled < painted,
            "the draw paints before it rolls the window, so a frame shows the \
             picture it was woken to change and the next one corrects it a beat \
             late"
        );

        // And it rolls on the caller's clock, not its own.
        assert!(
            drawer[..painted].contains("self.history.record_sized([], now)"),
            "`Shell::draw` rolls on a clock of its own rather than the turn's, so \
             a sample boundary falling between a tick and its paint erases the \
             pulse of the burst that caused the frame"
        );

        // The timeout arm walks status only through the frame's own settle check: a
        // walk on the ageing path is the difference `SPEC.md` §11.1 prices on.
        let arm = &code[code
            .find("let Some(wake) = wake else {")
            .expect("the loop no longer has a timeout arm")..];
        let arm = &arm[..arm
            .find("continue;")
            .expect("the timeout arm no longer continues")];
        assert!(
            !arm.contains("frame.advance("),
            "the timeout arm walks status on every timeout, so an ageing wake now \
             costs a tick and the measurement I1's amendment was granted on no \
             longer holds"
        );

        // The arm draws, and that is a liveness gate rather than a tidiness one.
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
        // Position, not presence.
        assert!(
            drew < timed,
            "the timeout arm records its frame time before it paints, so the \
             number the bar draws for the most common frame on a quiet tree \
             excludes the paint that frame exists to do"
        );

        // The idle receive is untimed, and that is I1's budget as a structure rather
        // than as an observation.
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
        // One function answers for every clock. A deadline asked separately would
        // be another chance to leave one armed on an idle monitor, and the gate
        // above can only see the branch, not what fed it.
        let asked = code.find("input::patience(").expect(
            "`Shell::patience` is gone, so nothing decides *is there a timer at all* in one place",
        );
        // Bounded on `patience`'s closing brace rather than a byte count, so the
        // scan cannot reach a clock named by the next function.
        let sources = &code[asked..];
        let sources = &sources[..sources
            .find("\n    }\n")
            .expect("`Shell::patience` never closes")];
        for clock in [
            "held: self.held",
            "linger: self.scrolling_until",
            "notice: self.leaving.or_else(|| self.app.flash_until())",
            "ageing: self.history.ages_in",
            "settling: frame.settles_in(",
            "arriving: self.effects_running()",
            "departing: self.ledger.ends_in()",
            "closing: self.app.box_ends_in()",
        ] {
            assert!(
                sources.contains(clock),
                "`{clock}` is no longer among the deadlines `patience` is given, so \
                 that clock is either armed somewhere else or has stopped: {sources}"
            );
        }
        // And every manager is inside the one predicate the clock above asks,
        // since a manager dropped from it is an effect the loop stops offering
        // frames to and no gate over a drawn screen can see that.
        let running = code
            .split("fn effects_running(&self) -> bool {")
            .nth(1)
            .and_then(|rest| rest.split("\n    }\n").next())
            .expect("`Shell::effects_running` is gone");
        for manager in [
            "self.effects.is_running()",
            "self.notice_effects.is_running()",
            "self.note_effects.is_running()",
            "self.box_effect.as_ref().is_some_and(Timed::is_running)",
        ] {
            assert!(
                running.contains(manager),
                "`{manager}` is no longer one of the effects `effects_running` \
                 answers for, so the loop stops offering it frames: {running}"
            );
        }
        // Each path to a paint settles the footer on the way: an announcement
        // never taken is one this run never says.
        let paints: Vec<usize> = code
            .match_indices("shell.draw(&mut frame, &worktree, began)?")
            .map(|(at, _)| at)
            .collect();
        assert_eq!(
            paints.len(),
            2,
            "the loop no longer has exactly two paints, so this gate is checking a shape that moved"
        );
        let mut previous = 0;
        for paint in paints {
            let settled = code[previous..paint]
                .rfind("shell.settle_footer(began)")
                .map(|at| previous + at)
                .expect("a paint with no `settle_footer` before it in the same arm");
            assert!(settled < paint);
            let walked = code[previous..paint]
                .rfind("shell.settle_heights(&mut frame)")
                .map(|at| previous + at)
                .expect(
                    "a paint with no `settle_heights` before it in the same arm, so a \
                     settled height on that path waits for the next event",
                );
            assert!(walked < paint);
            let noted = code[previous..paint]
                .rfind("shell.settle_notes(began)")
                .map(|at| previous + at)
                .expect(
                    "a paint with no `settle_notes` before it in the same arm, so a \
                     departure that ended on that path keeps its rows until the next \
                     event",
                );
            assert!(noted < paint);
            previous = paint;
        }

        assert!(
            code.contains("match shell.patience(&frame, Instant::now())"),
            "the loop no longer decides how long to wait through `Held::wait`, so \
             the one function that can answer *is there a timer at all* is not the \
             one being asked"
        );
    }

    /// `SPEC.md` §11.2 B21 ruling 6: Enter writes the store first either way, so
    /// a note no socket takes is still the reader's.
    ///
    /// The order is inside a private method, which the suite cannot drive, so it
    /// is held by reading the source the way the loop's own ordering is below.
    #[test]
    fn the_note_reaches_the_store_before_it_reaches_a_socket() {
        let source = include_str!("lib.rs");
        let shipped = source.split("#[cfg(test)]").next().expect("split");
        let tail = shipped
            .split_once("fn commit_box(")
            .expect("the shell no longer has `commit_box`")
            .1;
        // Bounded at the next method, or a call moved out of `commit_box` into
        // anything defined below it would still be found and still read as in
        // order.
        let body = tail.split_once("\n    fn ").map_or(tail, |(body, _)| body);

        let wrote = body
            .find("notes::commit(store, open)")
            .expect("`commit_box` no longer writes the store");
        let posted = body
            .find("self.post_note(")
            .expect("`commit_box` no longer posts the note");
        assert!(
            wrote < posted,
            "`commit_box` posts before it writes, so a socket could take a note \
             the store then refuses"
        );
    }

    /// The store is an event source beside the tree's: its watch is armed once
    /// the first frame is up, its wake marks the store stale rather than reading
    /// it, and the read happens where every path to a paint passes.
    #[test]
    fn the_store_watch_is_armed_after_the_first_paint_and_its_wake_is_read_before_the_paint() {
        let source = include_str!("lib.rs");
        let shipped = source.split("#[cfg(test)]").next().expect("split");
        let code: String = shipped
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");

        let first_paint = code
            .find("shell.draw(&mut frame, &worktree, Instant::now())?")
            .expect("`run` no longer paints before the loop");
        let armed = code
            .find("shell.watch_store(&tx, Instant::now())")
            .expect("`run` no longer arms the store watch");
        let input = code
            .find("spawn_input(tx.clone())")
            .expect("`run` no longer spawns the input thread");
        assert!(
            first_paint < armed && armed < input,
            "the store watch is armed before the first paint or after input is \
             live, so either a wake precedes the screen it redraws or a write \
             between the two is missed"
        );

        let arm = code
            .find("Wake::Notes => shell.notes_stale = true")
            .expect("the loop no longer marks the store stale on its wake");
        let paints: Vec<usize> = code
            .match_indices("shell.draw(&mut frame, &worktree, began)?")
            .map(|(at, _)| at)
            .collect();
        assert!(
            paints.iter().any(|paint| arm < *paint),
            "the wake's arm sits after the batch's paint, so the listing it marks \
             stale is read one frame late"
        );

        let settle = code
            .find("fn settle_notes(&mut self, now: Instant)")
            .expect("`Shell::settle_notes` is gone");
        let body = &code[settle..];
        let body = &body[..body.find("\n    }\n").expect("`settle_notes` never closes")];
        for step in [
            "if std::mem::take(&mut self.notes_stale) {",
            "self.reload_notes(now)",
            "self.ledger.settle(now)",
            // Whole to the `collect`, so an adapter dropping ids on the way is a
            // red rather than a sweep nothing on screen ever runs.
            "settled.sweeping.into_iter().map(Change::Swept).collect(),",
            "self.note_effects.settle(now)",
            "self.app.settle_box(now)",
            "self.box_effect.take_if(|armed| armed.spent(now))",
            "if self.app.box_open() && !notes::has_room(self.regions) {",
        ] {
            assert!(
                body.contains(step),
                "`settle_notes` no longer runs `{step}`, so one of a stale store, a \
                 beat that has run, an ended departure, a spent effect and the box's \
                 own end outlives the frame that should have settled it"
            );
        }

        // The list above is presence and says nothing about order, which is right
        // for steps that do not depend on each other. These two do: the settle is
        // what names the beats that have run, so arming ahead of it arms nothing
        // and the sweep waits a frame that never comes.
        let ran = body.find("self.ledger.settle(now)").expect("checked above");
        let armed = body
            .find("settled.sweeping.into_iter()")
            .expect("checked above");
        assert!(
            ran < armed,
            "`settle_notes` arms the sweep before the settle that names it"
        );

        // And the paint asks the interval rule with what the previous paint
        // recorded, and records for the next one after the effects have drawn.
        let paint = code.find("fn paint(\n").expect("`Shell::paint` is gone");
        let paint = &code[paint..];
        let paint = &paint[..paint.find("\n    }\n").expect("`paint` never closes")];
        let asked = paint
            .find("effect_interval(\n            self.effects_ran,")
            .expect("`paint` no longer asks `effect_interval` with the previous paint's answer");
        let drawn = paint
            .find("process_effects(")
            .expect("`paint` no longer processes effects");
        let recorded = paint
            .find("self.effects_ran = self.effects_running()")
            .expect("`paint` no longer records whether an effect drew");
        assert!(
            asked < drawn && drawn < recorded,
            "the interval is asked or recorded on the wrong side of the draw, so an \
             effect armed after a quiet spell is told the whole of it"
        );
    }
}
