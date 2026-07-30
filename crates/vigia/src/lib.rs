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
pub use render::{Chrome, body_height, render};
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
    Tick,
    /// The watch could not be established, so the shell is a still picture.
    ///
    /// Reported rather than fatal. A diff nobody is watching for changes is
    /// still a diff, and the reader should be told which of the two they have.
    WatchLost(String),
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
            Wake::Input(event) => {
                let Some(action) = action_for(&event) else {
                    // Not every event is a request. Redrawing for a key release
                    // or a mouse move would make the idle cost non-zero for a
                    // reason nobody asked for.
                    continue;
                };
                let height = body_height(shell.area()?);
                match shell.app.apply(action, &mut frame, height) {
                    Ok(true) => {}
                    Ok(false) => break,
                    Err(e) => shell.app.warn(e.to_string()),
                }
            }
            Wake::Tick => {
                shell.app.clear_notice();
                // The core leaves the frame exactly as it was on failure, so the
                // previous diff is still valid to draw. Saying so on the footer
                // beats blanking a pane for a reason the reader cannot see.
                if let Err(e) = frame.advance() {
                    shell.app.warn(e.to_string());
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
        let height = body_height(self.area()?);
        match self.app.view(frame, height) {
            Ok(view) => self.screen = view,
            Err(e) => self.app.warn(e.to_string()),
        }

        let chrome = self.app.chrome(&self.name);
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
        while watcher.next_tick().is_some() {
            if tx.send(Wake::Tick).is_err() {
                return;
            }
        }
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
