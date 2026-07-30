//! Taking the terminal, and giving it back.
//!
//! Two mechanisms, because one is not enough.
//!
//! A [`Session`] restores on drop, which covers every ordinary return and every
//! `?` on the way out. It does **not** cover a panic: `Cargo.toml` sets
//! `panic = "abort"` for the release profile, and an aborting panic runs no
//! destructors at all. So the restore also runs from a panic hook, installed
//! before the screen is ever taken. Without it a panic leaves the reader in the
//! alternate screen with no prompt, no echo, and a mouse that reports every
//! movement as garbage.
//!
//! Proving both paths is [#8](https://github.com/breferrari/vigia/issues/8),
//! which is where the alternate-screen assertions and the panic-hook test land.
//! This module is the implementation those tests are written against.

use std::io::{self, IsTerminal, Stdout, stdout};
use std::sync::Once;

use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::cursor::{Hide, Show};
use ratatui::crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};

/// The terminal the shell draws on.
pub type Screen = Terminal<CrosstermBackend<Stdout>>;

static HOOK: Once = Once::new();

/// A taken terminal that gives itself back.
///
/// Holding one is what makes the screen the shell's; dropping it is what makes it
/// the reader's again. There is deliberately no way to restore early and keep
/// drawing.
pub struct Session {
    screen: Option<Screen>,
}

impl Session {
    /// Enter the alternate screen, in raw mode, with the mouse reporting.
    ///
    /// Fails rather than draws when standard output is not a terminal. A monitor
    /// whose output is being redirected has nothing useful to write there, and
    /// half a megabyte of escape sequences in a pipe is a worse answer than a
    /// sentence explaining it.
    pub fn enter() -> io::Result<Self> {
        if !stdout().is_terminal() {
            return Err(io::Error::other(
                "standard output is not a terminal, so there is nothing to draw on",
            ));
        }

        // Before anything is changed, so a panic between here and the first
        // frame still restores.
        install_hook();

        enable_raw_mode()?;

        // Every failure from here on has to undo what already succeeded, and
        // cannot lean on `Drop` to do it: there is no `Session` yet. A bare `?`
        // on either line below returns an error to a caller that prints it into
        // a terminal still in raw mode, with no echo and no line editing, which
        // is a worse outcome than the failure it is reporting.
        Self::or_restore(execute!(
            stdout(),
            EnterAlternateScreen,
            EnableMouseCapture,
            Hide
        ))?;

        Ok(Self {
            screen: Some(Self::or_restore(Terminal::new(CrosstermBackend::new(
                stdout(),
            )))?),
        })
    }

    /// Put the terminal back before handing a failure to the caller.
    fn or_restore<T>(result: io::Result<T>) -> io::Result<T> {
        if result.is_err() {
            restore();
        }
        result
    }

    /// The terminal to draw through.
    pub fn screen(&mut self) -> &mut Screen {
        self.screen
            .as_mut()
            .expect("the screen is only taken away by Drop")
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // Dropped before the restore so that no live `Terminal` outlives the
        // screen it draws on, even for the length of this function.
        //
        // Checked rather than assumed, because the intuitive reason is the wrong
        // one: neither `ratatui::Terminal` nor `CrosstermBackend` implements
        // `Drop`, so there is no buffered frame here that could flush itself into
        // the reader's shell after the alternate screen is gone. Ordering these
        // two lines is tidiness, not a fix for that.
        self.screen = None;
        restore();
    }
}

/// Put the terminal back the way it was, ignoring failures.
///
/// Nothing useful can be done about an error here. It runs while a process is
/// already leaving, sometimes while it is panicking, and reporting it would mean
/// writing to a terminal whose state is exactly what is in doubt.
///
/// Private, and the three callers are all in this file. Restoring is a
/// consequence of giving up a [`Session`] or of dying, never something a caller
/// asks for: a shell that could put the terminal back and keep drawing would be
/// drawing onto the reader's shell prompt.
fn restore() {
    // Raw mode first: it has more side effects than the alternate screen, and
    // ratatui's own restore path orders it this way for that reason.
    let _ = disable_raw_mode();
    let _ = execute!(stdout(), DisableMouseCapture, LeaveAlternateScreen, Show);
}

/// Chain a restore onto the panic hook, once per process.
///
/// Chained rather than replaced, so the panic message still reaches the reader
/// through whatever hook was already there. `Once` because a second install
/// would nest the restore inside itself, and every future panic would pay for
/// every session ever opened.
fn install_hook() {
    HOOK.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            restore();
            previous(info);
        }));
    });
}
