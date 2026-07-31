//! The `ratatui` + `crossterm` shell over [`vigia_core`].
//!
//! `SPEC.md` §6 asks this half of the workspace to be thin: the core produces
//! frames, the shell renders them, and the TUI stays swappable because nothing
//! it knows is load bearing. So there is no diff logic here, no caching, no
//! filtering and no coalescing. What is here is a terminal, a key map, a scroll
//! position, and one pure function from a screenful of rows to cells.
//!
//! It is a library with a five-line binary on top rather than a binary alone.
//! `SPEC.md` §7 makes the snapshot suite over `ratatui::backend::TestBackend` the
//! proof for I5 and I6, and a test cannot import a `main.rs`.
//!
//! ## Why two threads and two repositories
//!
//! I1 says an idle monitor does no work, and the core delivers that by blocking:
//! [`vigia_core::Watcher::next_tick`] waits on an untimed `recv`. `crossterm`
//! reads input the same way. Two blocking sources need two threads and one
//! channel, or a poll loop, and a poll loop is the timer I1 forbids.
//!
//! The watch thread opens its own [`vigia_core::Worktree`]. That is not
//! duplication for its own sake: a [`vigia_core::Watcher`] borrows the
//! `gix::Repository` it filters gitignore rules through, and `gix::Repository` is
//! `Send` but not `Sync`, so a borrow of one cannot cross a thread boundary. The
//! second open costs one repository discovery, paid after first paint, off the
//! path I7 measures.
//!
//! Both threads are detached rather than scoped. Quitting means main returning,
//! and neither thread can be woken to be joined: `crossterm` has no portable
//! interruptible read, and the one handle that unblocks the watcher makes its
//! `next_tick` return `None` permanently, which is a shutdown and not a nudge.

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

mod app;
mod input;
mod render;
mod terminal;
mod theme;
mod view;

pub use app::App;
pub use input::{Action, WHEEL_ROWS, action_for};
pub use render::{Chrome, HINT_SEPARATOR, Heat, Mode, body_height, render};
pub use terminal::{Screen, Session};
pub use theme::Theme;
pub use view::{HEAT_BUCKETS, HeatBucket, Position, Row, View, rows_in};

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::time::Instant;

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
    /// Fatal, and it is the one wake-up that is. In raw mode every way out is a
    /// key event: the terminal does not turn Ctrl-C into a signal, and there is
    /// no handler that would catch one. So a shell that kept drawing after this
    /// would hold the alternate screen with no way to leave it, and the only
    /// remaining exit is a kill, which runs neither the guard nor the panic hook
    /// and hands back a terminal in raw mode. Leaving is the smaller failure.
    InputLost,
}

/// Watch the working tree at `path` and draw it until the reader quits.
pub fn run(path: &Path) -> Result<(), Failure> {
    let worktree = Worktree::discover(path)?;
    let mut frame = worktree.frame();

    // Before the screen is taken, so a repository that fails on its first walk
    // reports on a terminal the reader can still see.
    frame.advance()?;

    let mut shell = Shell {
        session: Session::enter()?,
        app: App::new(),
        // Its 318µs of grammar loading lands before first paint, which is where
        // it belongs: I7 gives startup 50ms and measures 20ms, so this is 1.5%
        // of what starting already costs, and deferring it would only move it
        // onto the first frame that draws something.
        //
        // Not "before the screen is taken", which an earlier version of this
        // comment claimed: struct fields evaluate in written order and
        // `Session::enter` is written above, so the alternate screen is already
        // ours by the time this runs. The placement is right and the reason was
        // wrong.
        highlighter: Highlighter::new(),
        // Empty at startup, so every file in an already-dirty worktree draws
        // cold until something writes to it. That is the honest first frame: a
        // monitor has no way to know what happened before it was looking, and
        // inventing a recency for it would light up rows nothing has touched.
        history: History::new(),
        theme: Theme::default(),
        name: short_name(worktree.workdir()),
        branch: None,
        screen: View::default(),
    };
    shell.draw(&mut frame, &worktree)?;

    // Armed only now. Everything above read `.git/index` and the gitignore
    // files, and a watch armed before those reads observes them: inotify and
    // FSEvents both report reads and attribute touches, so the shell would wake
    // itself up once for free at startup.
    let (tx, rx) = mpsc::channel();
    spawn_watch(path.to_path_buf(), tx.clone());
    spawn_input(tx);

    while let Ok(wake) = rx.recv() {
        match wake {
            // Returning rather than breaking, so the reason travels with the
            // exit. `shell` drops on the way out, which puts the terminal back
            // before `main` prints this where the reader will see it.
            Wake::InputLost => {
                return Err("terminal input ended, so there was no way left to quit".into());
            }
            Wake::Input(event) => {
                let Some(action) = action_for(&event) else {
                    // Not every event is a request. Redrawing for a key release
                    // or a mouse move would make the idle cost non-zero for a
                    // reason nobody asked for.
                    continue;
                };
                // The branch here is whatever the last draw settled on, which is
                // right rather than merely cheap: it feeds `body_height` alone,
                // and neither the branch nor the mode can change how many rows
                // the footer takes. See `Footer::plan`.
                let chrome = shell.app.chrome(&shell.name, shell.branch.as_deref());
                let height = body_height(shell.area()?, &chrome, frame.files().len());
                match shell.app.apply(action, &mut frame, height) {
                    Ok(true) => {}
                    Ok(false) => break,
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

        shell.draw(&mut frame, &worktree)?;
    }

    Ok(())
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
}

impl Shell {
    /// The drawable area of the terminal right now.
    fn area(&mut self) -> Result<Rect, Failure> {
        let size = self.session.screen().size()?;
        Ok(Rect::new(0, 0, size.width, size.height))
    }

    /// Collect a screenful and paint it.
    fn draw(&mut self, frame: &mut vigia_core::Frame, worktree: &Worktree) -> Result<(), Failure> {
        // Before the chrome, because the chrome carries it, and from the frame's
        // own file count so the read happens on exactly the frames that draw the
        // answer. That is the whole of I4 for this read.
        self.branch = branch_for(frame, || worktree.branch());

        // The chrome is built before the height, not after, because the footer
        // takes a second line at narrow widths and `body_height` has to know
        // whether this frame is one of those. `frame.files().len()` is the same
        // number `View::collect` will report as `View::files`, which is what
        // keeps this row budget and the renderer's layout in agreement.
        let chrome = self.app.chrome(&self.name, self.branch.as_deref());
        let height = body_height(self.area()?, &chrome, frame.files().len());
        match self
            .app
            .view(frame, &mut self.highlighter, &self.history, height)
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
        let chrome = self.app.chrome(&self.name, self.branch.as_deref());
        // Borrowed out of `self` before the draw, not for style: the closure would
        // otherwise hold `&self` while `self.session` is borrowed mutably to reach
        // the terminal.
        let (theme, screen) = (&self.theme, &self.screen);
        self.session.screen().draw(|f| {
            let area = f.area();
            render(f.buffer_mut(), area, screen, theme, &chrome);
        })?;
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
/// call site inside [`Shell::draw`], which owns a terminal and which no test can
/// drive: hardcoding that argument to `0` and to `1` both passed the **entire
/// suite**, in both directions, while every unit test of this function stayed
/// green. The mutations killed the consumer and never touched the producer.
/// Moving the count inside the boundary leaves nothing outside it to get wrong,
/// and lets `tests/reads.rs` drive this with a real [`vigia_core::Frame`], so
/// what is asserted is what production computes rather than a number someone
/// typed.
///
/// Public for the reason [`rows_in`] and [`body_height`] are: `SPEC.md` §7 makes
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
    //! The two rules in this file that are arithmetic rather than plumbing.
    //!
    //! Both are unreachable from an integration test: `run` owns a terminal, and
    //! neither of these is exported. `terminal.rs` already keeps its unit tests
    //! beside the code for the same reason.

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
}
