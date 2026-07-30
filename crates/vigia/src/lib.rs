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
pub use render::{Chrome, HINT_SEPARATOR, body_height, render};
pub use terminal::{Screen, Session};
pub use theme::Theme;
pub use view::{Position, Row, View, rows_in};

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};

use ratatui::layout::Rect;
use vigia_core::{WatchOptions, Worktree};

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
    /// Carries the path of the write that landed last in the burst, when it
    /// named one, which is what follow mode moves to. `None` is ordinary: a
    /// staging write changes every diff's left-hand side and is nowhere to
    /// scroll to.
    Tick(Option<String>),
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
        theme: Theme::default(),
        name: short_name(worktree.workdir()),
        screen: View::default(),
    };
    shell.draw(&mut frame)?;

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
                let chrome = shell.app.chrome(&shell.name);
                let height = body_height(shell.area()?, &chrome, frame.files().len());
                match shell.app.apply(action, &mut frame, height) {
                    Ok(true) => {}
                    Ok(false) => break,
                    Err(e) => shell.app.warn(e.to_string()),
                }
            }
            Wake::Tick(newest) => {
                shell.app.clear_notice();
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
                        if let Some(path) = newest {
                            shell.app.follow(&path, &frame);
                        }
                    }
                    Err(e) => shell.app.warn(e.to_string()),
                }
            }
            Wake::WatchLost(message) => shell.app.warn(message),
        }

        shell.draw(&mut frame)?;
    }

    Ok(())
}

/// The terminal and everything drawn onto it that outlives one frame.
struct Shell {
    session: Session,
    app: App,
    theme: Theme,
    /// What the header calls the working tree.
    name: String,
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
    fn draw(&mut self, frame: &mut vigia_core::Frame) -> Result<(), Failure> {
        // The chrome is built before the height, not after, because the footer
        // takes a second line at narrow widths and `body_height` has to know
        // whether this frame is one of those. `frame.files().len()` is the same
        // number `View::collect` will report as `View::files`, which is what
        // keeps this row budget and the renderer's layout in agreement.
        let chrome = self.app.chrome(&self.name);
        let height = body_height(self.area()?, &chrome, frame.files().len());
        match self.app.view(frame, height) {
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
        let chrome = self.app.chrome(&self.name);
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
            if tx.send(Wake::Tick(tick.newest)).is_err() {
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
/// Falls back to the whole path. A worktree at a filesystem root has no last
/// component, and showing the root is better than showing nothing.
fn short_name(workdir: &Path) -> String {
    workdir
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| workdir.display().to_string())
}
