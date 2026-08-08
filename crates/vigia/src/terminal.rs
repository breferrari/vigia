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
//! ## Why this module is shaped as policy over a trait
//!
//! I8 is proven here rather than in `crates/vigia/tests/`, and the shape is
//! forced by two things that are not visible from reading the crossterm API.
//! Both were measured before this was written, not assumed.
//!
//! **Raw mode is not observable.** Under `cargo test` on Windows,
//! `is_raw_mode_enabled()` returns `Ok(true)` before anything has enabled it, and
//! `enable_raw_mode()` succeeds against a stdout that is not a terminal. A test
//! that asserted on real raw-mode state would pass against broken code, which is
//! worse than no test.
//!
//! **Mouse capture is not ANSI everywhere.** On Windows
//! `EnableMouseCapture::is_ansi_code_supported()` is `false`, so `execute!` routes
//! it through the console API and writes **zero bytes** into the sink. A
//! byte-exact assertion over the whole takeover would quietly mean one thing on
//! Unix and another on Windows.
//!
//! So the terminal itself cannot be the oracle. What can be asserted exactly is
//! the *policy*: which steps are taken, in what order, and that giving back is
//! their exact inverse. [`Step`] and [`TAKEOVER`] make that policy data, [`Console`]
//! makes the mechanism swappable, and [`Takeover`] is the guard that runs one over
//! the other. `SPEC.md` §7 licenses this directly: where a correctness rule has no
//! reachable integration path, the pure-function test *is* the gate.
//!
//! What that leaves untested is the mapping from a [`Step`] to the escape
//! sequence it really emits, since a recorder cannot see it. The last layer of
//! the test module closes that by driving the real [`Crossterm`] into a byte
//! sink, and the ANSI table beside it checks the commands against DEC's own mode
//! numbers rather than against crossterm restating itself.
//!
//! **A third way in, and deliberately not a third mechanism.** An externally
//! delivered signal used to be uncovered here: raw mode makes Ctrl-C a key event
//! and never a signal, so there is nothing to catch on the path a reader uses,
//! and a `kill` from elsewhere ran neither the guard nor the hook.
//! [`signal`](crate::signal) closes that
//! ([#24](https://github.com/breferrari/vigia/issues/24)) while adding nothing
//! to this module: the handler restores nothing, it ends the loop, and the guard
//! above then does what it already did. The number of ways to leave went up by
//! one and the number of ways to restore did not, which is the property worth
//! having: the three test layers below are unchanged, and what they gained is a
//! section beside them rather than among them.
//!
//! What is still out of reach is `SIGKILL` and `TerminateProcess`, on the same
//! footing on both platforms, because neither runs any code the process owns.

use std::io::{self, IsTerminal, Stdout, Write, stdout};
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

/// One thing a session takes from the terminal and has to give back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    /// No line buffering, no echo, and no signal translation.
    RawMode,
    /// The alternate screen, so the reader's scrollback survives being watched.
    AlternateScreen,
    /// Mouse reporting, which `SPEC.md` §4 puts in scope for the wheel.
    MouseCapture,
    /// The cursor, which a monitor never places anywhere meaningful.
    Cursor,
}

/// The order the terminal is taken in. Giving it back walks this **backwards**.
///
/// Raw mode is first, so it is the last thing given back. It has more side
/// effects than anything else here, and ratatui's own restore path orders it the
/// same way for that reason: the escape sequences that leave the alternate screen
/// should be written while the terminal is still in the mode they were written in.
const TAKEOVER: [Step; 4] = [
    Step::RawMode,
    Step::AlternateScreen,
    Step::MouseCapture,
    Step::Cursor,
];

/// The terminal underneath a [`Session`], as something that can be swapped.
///
/// Exists so the order in [`TAKEOVER`] and the unwinding in [`Takeover::take`] can
/// be asserted against a recorder. See this module's header for why the real
/// terminal cannot be the oracle.
trait Console {
    /// Take one step, or report why it could not be taken.
    fn take(&mut self, step: Step) -> io::Result<()>;

    /// Give one step back.
    ///
    /// Infallible on purpose. Nothing useful can be done about an error here: it
    /// runs while a process is already leaving, sometimes while it is panicking,
    /// and reporting it would mean writing to a terminal whose state is exactly
    /// what is in doubt.
    fn give_back(&mut self, step: Step);
}

/// The real console, over `crossterm`.
///
/// Generic over the sink only so a test can read back what it wrote. Production
/// is always [`stdout`], and [`Step::RawMode`] ignores the sink entirely because
/// both platforms change the mode through a syscall rather than an escape
/// sequence.
struct Crossterm<W: Write> {
    out: W,
}

impl Crossterm<Stdout> {
    /// The console the shell actually runs on.
    fn on_stdout() -> Self {
        Self { out: stdout() }
    }
}

impl<W: Write> Console for Crossterm<W> {
    fn take(&mut self, step: Step) -> io::Result<()> {
        match step {
            Step::RawMode => enable_raw_mode(),
            Step::AlternateScreen => execute!(self.out, EnterAlternateScreen),
            Step::MouseCapture => execute!(self.out, EnableMouseCapture),
            Step::Cursor => execute!(self.out, Hide),
        }
    }

    fn give_back(&mut self, step: Step) {
        let _ = match step {
            Step::RawMode => disable_raw_mode(),
            Step::AlternateScreen => execute!(self.out, LeaveAlternateScreen),
            Step::MouseCapture => execute!(self.out, DisableMouseCapture),
            Step::Cursor => execute!(self.out, Show),
        };
    }
}

/// The terminal, taken. Gives itself back on drop.
///
/// `taken` is how far into [`TAKEOVER`] the takeover got, and it is the whole
/// reason a half-finished [`Session::enter`] can be undone exactly. A bare "did we
/// start?" flag would either give back a step that was never taken or leak one
/// that was.
struct Takeover<C: Console> {
    console: C,
    taken: usize,
}

impl<C: Console> Takeover<C> {
    /// Take every step of [`TAKEOVER`], giving back what succeeded if one fails.
    ///
    /// The unwinding cannot lean on `Drop`, because on the failing path there is
    /// no `Takeover` yet. A bare `?` here would hand an error to a caller that
    /// prints it into a terminal still in raw mode, with no echo and no line
    /// editing, which is a worse outcome than the failure it is reporting.
    fn take(console: C) -> io::Result<Self> {
        let mut takeover = Self { console, taken: 0 };
        for step in TAKEOVER {
            takeover.console.take(step)?;
            // After the call, never before: a step that failed was not taken, and
            // giving it back would write the undo for something that never
            // happened.
            takeover.taken += 1;
        }
        Ok(takeover)
    }
}

impl<C: Console> Drop for Takeover<C> {
    fn drop(&mut self) {
        give_back_all(&mut self.console, self.taken);
    }
}

/// Give the first `taken` steps of [`TAKEOVER`] back, in reverse.
///
/// One function rather than a loop at each call site, because the reverse walk
/// *is* the invariant: the guard and the panic hook are two different ways of
/// leaving, and while each wrote its own loop only one of them was covered by the
/// test that pins the order.
fn give_back_all<C: Console>(console: &mut C, taken: usize) {
    for step in TAKEOVER[..taken].iter().rev() {
        console.give_back(*step);
    }
}

/// A taken terminal that gives itself back.
///
/// Holding one is what makes the screen the shell's; dropping it is what makes it
/// the reader's again. There is deliberately no way to restore early and keep
/// drawing: [`Takeover`] is private, and a shell that could put the terminal back
/// and keep drawing would be drawing onto the reader's shell prompt.
pub struct Session {
    /// Declared **first**, so it drops first and no live `Terminal` outlives the
    /// screen it draws on, even for the length of a drop.
    ///
    /// Checked rather than assumed, because the intuitive reason is the wrong
    /// one: neither `ratatui::Terminal` nor `CrosstermBackend` implements `Drop`,
    /// so there is no buffered frame here that could flush itself into the
    /// reader's shell after the alternate screen is gone. The ordering is
    /// tidiness, not a fix for that.
    screen: Screen,
    /// Second, so the terminal goes back after the screen is gone. Never read;
    /// its whole job is its `Drop`.
    _takeover: Takeover<Crossterm<Stdout>>,
}

impl Session {
    /// Enter the alternate screen, in raw mode, with the mouse reporting.
    ///
    /// Fails rather than draws when standard output is not a terminal. A monitor
    /// whose output is being redirected has nothing useful to write there, and
    /// half a megabyte of escape sequences in a pipe is a worse answer than a
    /// sentence explaining it.
    pub fn enter() -> io::Result<Self> {
        check_drawable(stdout().is_terminal())?;

        // Before anything is changed, so a panic between here and the first
        // frame still restores.
        install_hook();

        let takeover = Takeover::take(Crossterm::on_stdout())?;
        // On the `?`, `takeover` drops and gives the terminal back. That is the
        // whole reason the guard is built before the screen rather than beside
        // it.
        let screen = Terminal::new(CrosstermBackend::new(stdout()))?;

        Ok(Self {
            screen,
            _takeover: takeover,
        })
    }

    /// The terminal to draw through.
    pub fn screen(&mut self) -> &mut Screen {
        &mut self.screen
    }
}

/// Refuse a standard output that is not a terminal.
///
/// Split out from [`Session::enter`] so the refusal is testable. `cargo test`
/// captures stdout and `--nocapture` does not, so a test driving `Session::enter`
/// directly would assert opposite things depending on how it was run, and would
/// take over the terminal of anyone who ran it the second way.
///
/// Takes the answer rather than something to ask, because `std::io::IsTerminal`
/// is a **sealed trait**: it cannot be implemented outside `std`, so there is no
/// fake terminal to hand this. What that leaves uncovered is the single
/// `stdout().is_terminal()` at the call site, which is one std call rather than a
/// derivation that could be subtly wrong.
fn check_drawable(is_terminal: bool) -> io::Result<()> {
    if is_terminal {
        Ok(())
    } else {
        Err(io::Error::other(
            "standard output is not a terminal, so there is nothing to draw on",
        ))
    }
}

/// Chain a restore onto the panic hook, once per process.
///
/// Builds its own console rather than borrowing a [`Session`]'s, because it has
/// to run when there is no session to borrow: it is installed before the first
/// step is taken, and a panic in that window is exactly the case `Drop` cannot
/// reach.
fn install_hook() {
    install_hook_in(&HOOK, || restore_everything(&mut Crossterm::on_stdout()));
}

/// Give back the whole of [`TAKEOVER`], whatever was actually taken.
///
/// Named rather than inlined into the hook so the *decision* is assertable: the
/// hook itself can only be observed by panicking a real process against a real
/// terminal, so inlined, neither the count nor the direction would be reached by
/// any test, and the panic path is the one that matters most when it is wrong.
///
/// Deliberately not symmetrical with [`Takeover`], which gives back only the
/// prefix it took. At panic time there is no prefix to read, and the two
/// directions are not equally bad: giving back a step never taken is a no-op or
/// an escape sequence the terminal ignores, while failing to give back one that
/// was leaves the reader with no echo.
fn restore_everything<C: Console>(console: &mut C) {
    give_back_all(console, TAKEOVER.len());
}

/// Chain `restore` onto the panic hook, at most once per `once`.
///
/// Chained rather than replaced, so the panic message still reaches the reader
/// through whatever hook was already there. `Once` because a second install would
/// nest the restore inside itself, and every future panic would pay for every
/// session ever opened.
///
/// Takes the `Once` rather than reaching for the static, so a test can install
/// into one of its own instead of racing the process-global gate.
fn install_hook_in(once: &Once, restore: impl Fn() + Send + Sync + 'static) {
    once.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| on_panic(&restore, &previous, info)));
    });
}

/// The order the panic path runs in: restore, then whatever hook was there.
///
/// A free function because `PanicHookInfo` cannot be constructed outside `std`,
/// so a test cannot call a real hook, but it can call this. The order is the
/// point: the previous hook prints the panic message, and it has to land on a
/// terminal that has already been given back or the reader cannot read it.
fn on_panic<T>(restore: impl Fn(), previous: impl Fn(T), info: T) {
    restore();
    previous(info);
}

#[cfg(test)]
mod tests {
    //! I8, as the policy it is.
    //!
    //! The module header says why none of this can be asserted against a real
    //! terminal. What follows is in three layers, and the layers are not
    //! interchangeable:
    //!
    //! 1. The **policy**, against a recorder: order, the exact inverse, and what
    //!    a half-finished takeover gives back. This is most of I8.
    //! 2. The **panic path**, which is a separate mechanism because
    //!    `panic = "abort"` runs no destructors.
    //! 3. The **adapter**, because a recorder cannot see that
    //!    `Step::AlternateScreen` is wired to `LeaveAlternateScreen` rather than
    //!    to `EnterAlternateScreen`. Layers 1 and 2 would both pass with that
    //!    wire crossed.
    //!
    //! A fourth section follows them and is deliberately **not** a fourth layer.
    //! The three above prove the restore *policy*, each from an angle the others
    //! cannot see. What section 4 proves is that an externally delivered signal
    //! reaches that policy at all: it needs a second process, and it asserts over
    //! the real bytes a real guard wrote on its way out. Adding a way to *leave*
    //! did not add a way to *restore*, which is why the three layers are
    //! unchanged and why the new section sits beside them rather than among them.

    use std::sync::{Arc, Mutex};

    use ratatui::crossterm::Command;

    use super::*;

    /// What a [`Recorder`] saw, in the order it happened.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Event {
        Took(Step),
        GaveBack(Step),
    }

    /// A console that records instead of acting, and can be told to fail.
    ///
    /// The log is shared rather than owned because every one of these is handed
    /// to a [`Takeover`] that then owns it, and the assertions run after that
    /// takeover has been dropped. A clone the test keeps is the only way back to
    /// what the moved-away copy saw.
    #[derive(Clone)]
    struct Recorder {
        log: Arc<Mutex<Vec<Event>>>,
        /// The index into [`TAKEOVER`] whose `take` fails, if any.
        fail_at: Option<usize>,
        /// Calls to [`Console::take`] so far, **including** the one that fails.
        ///
        /// Not the same quantity as [`Takeover`]'s `taken`, which counts only
        /// steps that succeeded. Kept distinct on purpose: conflating the two is
        /// how a test stops being able to tell the off-by-one it exists to catch.
        attempts: usize,
    }

    impl Recorder {
        fn new() -> Self {
            Self {
                log: Arc::new(Mutex::new(Vec::new())),
                fail_at: None,
                attempts: 0,
            }
        }

        fn failing_at(index: usize) -> Self {
            Self {
                fail_at: Some(index),
                ..Self::new()
            }
        }

        fn events(&self) -> Vec<Event> {
            self.log.lock().expect("log").clone()
        }

        fn took(&self) -> Vec<Step> {
            self.events()
                .into_iter()
                .filter_map(|e| match e {
                    Event::Took(step) => Some(step),
                    Event::GaveBack(_) => None,
                })
                .collect()
        }

        fn gave_back(&self) -> Vec<Step> {
            self.events()
                .into_iter()
                .filter_map(|e| match e {
                    Event::GaveBack(step) => Some(step),
                    Event::Took(_) => None,
                })
                .collect()
        }
    }

    /// The message the injected failure carries, so a test can tell it apart from
    /// an error the code invented.
    const INJECTED: &str = "injected by the recorder";

    impl Console for Recorder {
        fn take(&mut self, step: Step) -> io::Result<()> {
            let at = self.attempts;
            self.attempts += 1;
            if self.fail_at == Some(at) {
                return Err(io::Error::other(INJECTED));
            }
            self.log.lock().expect("log").push(Event::Took(step));
            Ok(())
        }

        fn give_back(&mut self, step: Step) {
            self.log.lock().expect("log").push(Event::GaveBack(step));
        }
    }

    // ---- 1. The policy ----------------------------------------------------

    #[test]
    fn takeover_order_is_raw_mode_first_then_the_screen() {
        let recorder = Recorder::new();
        let takeover = Takeover::take(recorder.clone()).expect("take");

        assert_eq!(
            recorder.took(),
            TAKEOVER.to_vec(),
            "the takeover did not walk TAKEOVER in order"
        );
        assert_eq!(takeover.taken, TAKEOVER.len());
        assert!(
            recorder.gave_back().is_empty(),
            "a live takeover gave something back"
        );
    }

    #[test]
    fn giving_back_is_the_exact_reverse_of_taking() {
        let recorder = Recorder::new();
        let takeover = Takeover::take(recorder.clone()).expect("take");
        // Derived from what was actually taken rather than restated as a literal,
        // so adding a step to TAKEOVER cannot leave this test asserting the old
        // list while looking green.
        let mut expected = recorder.took();
        expected.reverse();
        assert!(!expected.is_empty(), "nothing was taken to give back");

        drop(takeover);

        assert_eq!(
            recorder.gave_back(),
            expected,
            "giving back is not the reverse of taking"
        );
    }

    #[test]
    fn a_failure_part_way_through_gives_back_only_what_was_taken() {
        // Every failure point, not one: the interesting bug is off by one, and a
        // single k can miss it in whichever direction k happens to sit.
        for k in 0..TAKEOVER.len() {
            let recorder = Recorder::failing_at(k);
            // `let ... else` rather than `expect_err`, which needs the `Ok` side
            // to be `Debug`: a `Takeover` owns a `Console`, and neither is.
            let Err(error) = Takeover::take(recorder.clone()) else {
                panic!("failure at {k} was not reported");
            };

            assert_eq!(
                error.to_string(),
                INJECTED,
                "failure at {k} reported an error the takeover invented"
            );
            assert_eq!(
                recorder.took(),
                TAKEOVER[..k].to_vec(),
                "failure at {k} took the wrong steps"
            );

            let mut expected = TAKEOVER[..k].to_vec();
            expected.reverse();
            assert_eq!(
                recorder.gave_back(),
                expected,
                "failure at {k} gave back the wrong steps"
            );
        }
    }

    #[test]
    fn nothing_is_given_back_when_nothing_was_taken() {
        let recorder = Recorder::failing_at(0);
        let Err(_) = Takeover::take(recorder.clone()) else {
            panic!("the first step was told to fail and did not");
        };

        assert!(
            recorder.events().is_empty(),
            "a takeover that never started still touched the terminal: {:?}",
            recorder.events()
        );
    }

    #[test]
    fn a_takeover_gives_the_terminal_back_exactly_once() {
        let recorder = Recorder::new();
        drop(Takeover::take(recorder.clone()).expect("take"));

        let gave_back = recorder.gave_back();
        assert_eq!(
            gave_back.len(),
            TAKEOVER.len(),
            "expected one give-back per step, got {gave_back:?}"
        );
        for step in TAKEOVER {
            assert_eq!(
                gave_back.iter().filter(|s| **s == step).count(),
                1,
                "{step:?} was not given back exactly once"
            );
        }
    }

    // ---- 2. The panic path ------------------------------------------------

    #[test]
    fn the_panic_hook_restores_before_it_defers_to_the_previous_hook() {
        let order = Arc::new(Mutex::new(Vec::new()));

        let restore = {
            let order = Arc::clone(&order);
            move || order.lock().expect("order").push("restore".to_owned())
        };
        let previous = {
            let order = Arc::clone(&order);
            move |what: &str| order.lock().expect("order").push(what.to_owned())
        };

        on_panic(restore, previous, "previous hook");

        // Restore first is not cosmetic: the previous hook is what prints the
        // panic message, and a message printed into the alternate screen in raw
        // mode is a message the reader never sees.
        assert_eq!(
            *order.lock().expect("order"),
            vec!["restore", "previous hook"]
        );
    }

    #[test]
    fn the_panic_hook_gives_back_everything_in_reverse() {
        // What the hook does, minus which console it writes to. Without this the
        // production panic path is reached by no test at all: the test below
        // installs its own counting closure, so it pins that the hook fires once
        // and says nothing about what the real one restores.
        let mut recorder = Recorder::new();
        restore_everything(&mut recorder);

        let mut expected = TAKEOVER.to_vec();
        expected.reverse();
        assert_eq!(recorder.gave_back(), expected);
        assert!(
            recorder.took().is_empty(),
            "restoring took something instead"
        );
    }

    #[test]
    fn two_installs_leave_one_restore_on_the_panic_path() {
        // The only test that touches the process-global panic hook, and it is one
        // test for that reason: `set_hook` is process-wide, so splitting this
        // across two tests would have them racing each other under the default
        // parallel harness.
        let restores = Arc::new(Mutex::new(0usize));

        // Silence the hook that is already there *before* chaining onto it. The
        // chain defers to whatever it replaced, and what it replaced would print
        // a backtrace for a panic this test causes deliberately. Doing it the
        // other way round, installing and then replacing, would take the chained
        // hook straight back out and leave nothing to count.
        let real = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));

        // Counted per thread, not per process. The hook is global for as long as
        // this test holds it, so a *different* test failing in that window would
        // run this restore too and report a second failure here, pointing at code
        // that is fine. The harness gives each test its own thread and a hook runs
        // on the thread that panicked, so this is exact rather than a narrowed
        // race.
        let me = std::thread::current().id();

        let once = Once::new();
        for _ in 0..2 {
            let restores = Arc::clone(&restores);
            install_hook_in(&once, move || {
                if std::thread::current().id() == me {
                    *restores.lock().expect("count") += 1;
                }
            });
        }

        let panicked = std::panic::catch_unwind(|| panic!("on purpose")).is_err();

        // The process's own hook goes back before anything is asserted. A failing
        // assertion below panics, and panicking while the chained hook is still
        // installed would run the restore counter again on the way out.
        std::panic::set_hook(real);

        // Read out, then assert. `assert_eq!(*m.lock(), ..)` holds the guard for
        // the whole macro, so a failure panics while holding it, and a panic hook
        // that touches the same mutex deadlocks against a test that was only
        // trying to report a wrong number.
        let count = *restores.lock().expect("count");

        assert!(panicked, "the panic did not happen, so nothing was proven");
        assert_eq!(
            count, 1,
            "a second install nested the restore inside itself"
        );
    }

    // ---- 3. The adapter ---------------------------------------------------

    fn ansi(command: &impl Command) -> String {
        let mut rendered = String::new();
        command.write_ansi(&mut rendered).expect("write_ansi");
        rendered
    }

    #[test]
    fn every_command_is_the_escape_sequence_it_is_named_for() {
        // The oracle is DEC's own private mode numbers, not crossterm restating
        // itself: 1049 is the alternate screen buffer, 25 is cursor visibility,
        // and 1000/1002/1003/1015/1006 are the mouse reporting modes. `h` sets a
        // mode and `l` resets it, so a swapped pair is visible here as a letter.
        assert_eq!(ansi(&EnterAlternateScreen), "\x1b[?1049h");
        assert_eq!(ansi(&LeaveAlternateScreen), "\x1b[?1049l");
        assert_eq!(ansi(&Hide), "\x1b[?25l");
        assert_eq!(ansi(&Show), "\x1b[?25h");
        assert_eq!(
            ansi(&EnableMouseCapture),
            "\x1b[?1000h\x1b[?1002h\x1b[?1003h\x1b[?1015h\x1b[?1006h"
        );
    }

    #[test]
    fn the_mouse_disable_is_the_derived_inverse_of_the_enable() {
        // Derived from the actual enable rather than written out again, so the
        // two cannot be wrong in the same way twice. Mouse capture is the one
        // step that is five modes, which is where an incomplete undo would hide.
        let mut expected: Vec<String> = modes(&ansi(&EnableMouseCapture))
            .into_iter()
            .map(|(number, set)| {
                assert!(set, "the enable reset mode {number} instead of setting it");
                format!("\x1b[?{number}l")
            })
            .collect();
        expected.reverse();

        assert_eq!(ansi(&DisableMouseCapture), expected.concat());
    }

    /// Every `ESC [ ? <number> (h|l)` in `stream`, as `(number, is_set)`.
    ///
    /// Deliberately ignores anything else: crossterm writes mode changes in this
    /// one shape, and a parser that tried to cover the whole grammar would be a
    /// second implementation to get wrong.
    fn modes(stream: &str) -> Vec<(u32, bool)> {
        stream
            .split('\x1b')
            .filter_map(|chunk| chunk.strip_prefix("[?"))
            .filter_map(|chunk| {
                let set = chunk.ends_with('h');
                if !set && !chunk.ends_with('l') {
                    return None;
                }
                chunk[..chunk.len() - 1].parse().ok().map(|n| (n, set))
            })
            .collect()
    }

    #[test]
    fn the_real_console_gives_back_every_mode_it_set() {
        // What layers 1 and 2 cannot see. A recorder is blind to which crossterm
        // command a step is wired to, so `Step::AlternateScreen` giving back
        // `EnterAlternateScreen` would pass every test above.
        //
        // `Step::RawMode` is skipped because it is a syscall against the real
        // console on both platforms rather than an escape sequence, and a unit
        // test has no business changing the mode of the terminal running it.
        let emitting = || TAKEOVER.iter().filter(|step| **step != Step::RawMode);

        let mut console = Crossterm { out: Vec::new() };
        for step in emitting() {
            console.take(*step).expect("take");
        }
        let taken = String::from_utf8(std::mem::take(&mut console.out)).expect("utf8");

        for step in emitting().rev() {
            console.give_back(*step);
        }
        let given_back = String::from_utf8(console.out).expect("utf8");

        let took = modes(&taken);

        // Non-vacuity. A platform that routed every one of these through a
        // console API would leave both streams empty, and every assertion below
        // would hold over nothing. That is a result worth failing on rather than
        // passing quietly.
        assert!(
            !took.is_empty(),
            "the real console wrote no escape sequences at all, so this proves nothing"
        );

        // The inverse, derived from what was actually written rather than listed
        // again. Polarity flipped and order reversed, per mode.
        //
        // Flipped, not "all resets": hiding the cursor is `?25l`, which *resets*
        // mode 25, and showing it again sets the same mode. A rule saying taking
        // sets and giving back resets would be wrong for the cursor in one
        // direction and wrong for everything else in the other.
        let expected: Vec<(u32, bool)> = took
            .iter()
            .rev()
            .map(|(number, set)| (*number, !set))
            .collect();

        assert_eq!(
            modes(&given_back),
            expected,
            "giving back is not the inverse of taking, mode for mode"
        );
    }

    // ---- The refusal, and the exits ---------------------------------------

    #[test]
    fn a_redirected_stdout_is_refused_and_the_message_says_why() {
        let error = check_drawable(false).expect_err("must refuse");
        assert!(
            error.to_string().contains("not a terminal"),
            "the message does not say why: {error}"
        );

        check_drawable(true).expect("a terminal is drawable");
    }

    #[test]
    fn no_exit_path_in_the_shell_skips_the_destructors() {
        // The structural half of "the terminal is restored on every exit". What
        // makes that true is that every exit drops `Shell`, which owns the
        // `Session`. Nothing in this crate may reach for a call that skips
        // destructors, because one of those anywhere in the shell would put the
        // reader back in raw mode with no message and nothing to report.
        //
        // `main` returns an `ExitCode` for exactly this reason.
        const SOURCES: [(&str, &str); 11] = [
            ("lib.rs", include_str!("lib.rs")),
            ("main.rs", include_str!("main.rs")),
            ("app.rs", include_str!("app.rs")),
            ("colour.rs", include_str!("colour.rs")),
            ("input.rs", include_str!("input.rs")),
            ("memory.rs", include_str!("memory.rs")),
            ("render.rs", include_str!("render.rs")),
            ("signal.rs", include_str!("signal.rs")),
            ("terminal.rs", include_str!("terminal.rs")),
            ("theme.rs", include_str!("theme.rs")),
            ("view.rs", include_str!("view.rs")),
        ];

        // The list above is checked against `lib.rs` rather than trusted, so a
        // module added later cannot be quietly exempt from the scan.
        let lib = SOURCES
            .iter()
            .find(|(name, _)| *name == "lib.rs")
            .map(|(_, source)| *source)
            .expect("lib.rs");
        let declared: Vec<&str> = lib
            .lines()
            .map(str::trim)
            // Visibility stripped first, or `pub mod foo;` slips past the check
            // that keeps this list honest and the module is silently exempt from
            // the scan below. That is the one way this gate rots quietly.
            .map(|line| {
                line.strip_prefix("pub(crate) ")
                    .or_else(|| line.strip_prefix("pub "))
                    .unwrap_or(line)
            })
            .filter_map(|line| line.strip_prefix("mod "))
            .filter_map(|rest| rest.strip_suffix(';'))
            .collect();
        assert!(!declared.is_empty(), "no modules found in lib.rs");
        for module in &declared {
            assert!(
                SOURCES
                    .iter()
                    .any(|(name, _)| name.strip_suffix(".rs") == Some(module)),
                "`mod {module};` is not scanned; add it to SOURCES"
            );
        }

        for (name, source) in SOURCES {
            // Only what ships. Cutting at the test module is what lets this file
            // scan itself rather than being exempt from its own rule: the names
            // being searched for appear literally below this line, and nowhere
            // above it.
            let shipped = source.split("#[cfg(test)]").next().expect("split");

            // Non-vacuity, per file rather than in aggregate: a path that read
            // nothing would satisfy every assertion below.
            assert!(
                shipped.contains("//!") && shipped.len() > 200,
                "{name} was not read, so scanning it proves nothing"
            );

            for skips in ["process::exit", "process::abort", "mem::forget"] {
                assert!(
                    !shipped.contains(skips),
                    "{name} calls {skips}, which skips the Drop that restores the terminal"
                );
            }
        }
    }

    // ---- Beside the layers: the signal, end to end ------------------------
    //
    // The one thing here that needs two processes. A signal cannot be delivered
    // to the process that would have to observe it arriving, a console control
    // handler cannot be uninstalled once a test is over, and the wake has to
    // cross a real blocked `recv` rather than a fake one. So the shape is the
    // `soak.rs` shape: this binary re-invokes itself by name.

    /// Where the parent tells the child to leave its evidence.
    const CHILD: &str = "VIGIA_SIGNAL_DIR";

    /// The child, by the name the harness selects it with.
    const CHILD_TEST: &str = "terminal::tests::signal_child";

    /// The child gets a process group of its own, so a control event addressed
    /// to its process id reaches it and nothing else, above all not the runner.
    ///
    /// It is also what decides which event the parent sends: a process created
    /// this way starts with Ctrl-C disabled, and `CTRL_BREAK` is the one a fresh
    /// group can always be given.
    #[cfg(windows)]
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

    /// A process that arms the real handler, blocks the way the shell blocks, and
    /// leaves the real guard to restore.
    ///
    /// Ignored because it is meaningless on its own, and it **fails** rather than
    /// returning green when run directly. That is `soak_child`'s rule and the
    /// right one: a test reporting `ok` having armed nothing is the green tick
    /// over a check that did not run, which is what the parent below refuses two
    /// screens further down. The assert fires before anything blocks, so running
    /// this directly reports rather than hangs.
    #[test]
    #[ignore = "the parent process delivers the signal this waits for"]
    fn signal_child() {
        let dir = std::env::var(CHILD).unwrap_or_else(|_| {
            panic!(
                "{CHILD} is not set, so this was run directly. It is the body of \
                 `an_external_signal_ends_the_loop_and_the_terminal_goes_back`, which \
                 delivers the signal it waits for; run that instead."
            )
        });
        let dir = std::path::PathBuf::from(dir);

        // The real adapter over a real file, so what lands in it is whatever this
        // platform actually emits rather than what a recorder was told to expect.
        let sink = std::fs::File::create(dir.join("restored")).expect("create the restore sink");

        // **The expectation is derived here, in the child, and that placement is
        // the whole of it.** `crossterm` decides once per process whether the
        // takeover is ANSI or console-API calls, and caches the answer. The parent
        // may have decided *before* the console this fixture allocates existed and
        // the child *after*, in which case a parent-side expectation is a
        // different platform's answer: zero bytes against real escape sequences,
        // reported as I8 having broken. Same process, same answer, no straddle.
        let mut owed = Crossterm { out: Vec::new() };
        give_back_all(&mut owed, TAKEOVER.len());

        // Non-vacuity, asserted where the fact lives rather than inferred across a
        // process boundary. This is layer 3's rule (`the_real_console_gives_back_
        // every_mode_it_set`) applied in the process that is about to be measured:
        // if this console emits nothing, the sink below can prove nothing and
        // saying so is the only honest outcome.
        assert!(
            !owed.out.is_empty(),
            "this console emits no escape sequences for the takeover, so the sink \
             would prove nothing"
        );
        std::fs::write(dir.join("owed"), &owed.out).expect("write what is owed");

        let (tx, rx) = std::sync::mpsc::channel();
        crate::signal::forward(tx).expect("arm the signal handler");

        {
            // **Built rather than taken**, and that is the one departure from the
            // production path. `Takeover::take` would enable raw mode on the
            // console running this test, which layer 3 above rules out in so many
            // words. Giving it back is the direction under test and the safe one:
            // `disable_raw_mode` does nothing on Unix when this process never
            // stored a prior mode, and on Windows it ORs the ordinary bits back
            // on, which is a restoration and never a takeover.
            let _takeover = Takeover {
                console: Crossterm { out: sink },
                taken: TAKEOVER.len(),
            };

            // Last of all, so the parent can never signal a process that is not
            // listening yet, and after the guard exists so that what the signal
            // finds is a terminal already owed back.
            std::fs::write(dir.join("ready"), b"armed").expect("write the ready marker");

            // What this borrows from `run`'s loop, and all it borrows: block on
            // the channel, recognise the wake, leave. Everything else there is
            // about frames, and `run` owns a terminal so no test can drive the
            // real one. Said out loud because a copy nobody names is a copy that
            // drifts.
            //
            // One `recv` rather than a loop, because `signal::forward` owns the
            // only sender on this channel and sends one kind of wake. The other
            // two arms are the ways this test could otherwise pass for the wrong
            // reason: a `recv` that ended because the forwarder died would drop
            // the guard exactly as a signal does, so what ended the wait is
            // checked rather than assumed.
            match rx.recv() {
                Ok(crate::Wake::Signalled) => {}
                Ok(_) => panic!("the forwarder sent a wake that was not the signal"),
                Err(e) => panic!("the forwarder hung up before any signal arrived: {e}"),
            }
        }

        // Reachable only once the guard above has dropped, which is what makes
        // this marker evidence about the *restore* rather than about a signal
        // merely arriving.
        std::fs::write(dir.join("dropped"), b"the takeover dropped").expect("write the marker");
    }

    #[test]
    fn an_external_signal_ends_the_loop_and_the_terminal_goes_back() {
        let dir = std::env::temp_dir().join(format!("vigia-signal-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create the child's directory");

        // Before the child is spawned, because a child inherits whatever console
        // its parent had at the moment it started. A process with none can be
        // sent no control event, and neither can anything it spawns.
        //
        // Held in a guard rather than freed at the end, because there are three
        // ways out of this test below it: a skip, a failing assert, and the
        // ordinary end. A cleanup written at the bottom runs on one of them. This
        // module is about exactly that mistake, so making it here would be a poor
        // joke.
        #[cfg(windows)]
        let _console = Allocated(ensure_console());

        let exe = std::env::current_exe().expect("this test binary's own path");
        let mut command = std::process::Command::new(&exe);
        command
            .args([CHILD_TEST, "--exact", "--ignored", "--nocapture"])
            .args(["--test-threads", "1"])
            .env(CHILD, &dir);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(CREATE_NEW_PROCESS_GROUP);
        }
        let mut child = command.spawn().expect("spawn the signal child");

        if !wait_for(&dir.join("ready")) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("the child never armed its handler, so nothing could have been delivered to it");
        }

        if let Err(why) = deliver(child.id()) {
            // A printed skip rather than a pass. On a runner with no console at
            // all there is no way to deliver a control event, and a green tick
            // over a check that did not run is the shape `SPEC.md` §7 exists to
            // refuse.
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_dir_all(&dir);
            println!("skipped: {why}");
            return;
        }

        // Bounded, rather than `child.wait()` straight away, which has no
        // timeout: a defect that stopped the wake from ever arriving would leave
        // the child blocked on its `recv` forever and **hang** this test instead
        // of failing it, and a hang reports the runner rather than the defect.
        // The marker is written after the guard's `Drop` has returned, so once it
        // exists the sink beside it is complete too.
        let dropped = wait_for(&dir.join("dropped"));
        if !dropped {
            let _ = child.kill();
        }
        let status = child.wait().expect("wait for the signal child");

        // Nothing to undo here, and that is worth one line rather than a
        // defensive `disable_raw_mode()`: the child *builds* its guard instead of
        // taking one, so no process in this test ever enables raw mode, and a
        // call putting the bits back would be the global mutation layer 3
        // refuses.

        assert!(
            dropped,
            "the child was signalled and never left its loop, so the guard never dropped"
        );
        assert!(
            status.success(),
            "the signal child exited {status}, so it did not reach the end of its restore"
        );

        // Both sides from the child, for the reason it states where it writes
        // them: the parent is a different process and `crossterm` answers this
        // question once per process.
        let owed = std::fs::read(dir.join("owed")).expect("read what the child owed");
        let written = std::fs::read(dir.join("restored")).expect("read the restore sink");

        // The child refuses to run at all on a console that emits nothing, so an
        // empty pair here is a walk that stopped walking rather than a platform
        // that never wrote.
        assert!(
            !owed.is_empty(),
            "the child recorded nothing owed, which it should have refused to do"
        );
        assert_eq!(
            written, owed,
            "the terminal was not given back the way every other exit gives it back"
        );

        // Left behind on failure on purpose, the way `soak.rs` leaves its
        // worktree: the markers and the sink are the evidence.
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Frees a console this test had to allocate, on every way out.
    ///
    /// `false` when the process already had one, in which case there is nothing
    /// to give back and this does nothing.
    #[cfg(windows)]
    struct Allocated(bool);

    #[cfg(windows)]
    impl Drop for Allocated {
        fn drop(&mut self) {
            if self.0 {
                // SAFETY: an FFI call taking nothing. Only reached when this
                // process had no console until a moment ago, so nothing of ours
                // is attached to the one being freed.
                unsafe {
                    windows_sys::Win32::System::Console::FreeConsole();
                }
            }
        }
    }

    /// Wait for a marker the child writes.
    ///
    /// Polling because the two processes share nothing else, and generous because
    /// a cold `cargo test` on a shared runner can take seconds to reach the
    /// child's first line. Slowness is not the failure this is guarding against.
    fn wait_for(marker: &std::path::Path) -> bool {
        for _ in 0..600 {
            if marker.exists() {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        marker.exists()
    }

    /// Deliver the signal, or say why it could not be.
    #[cfg(unix)]
    fn deliver(pid: u32) -> Result<(), String> {
        // Shelling out rather than taking `libc` as a dev-dependency, the same
        // choice the fixtures make with `git`, and for the same reason: it keeps
        // the dependency list exactly what `SPEC.md` names.
        let out = std::process::Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .output()
            .map_err(|e| format!("`kill` could not be run: {e}"))?;

        if out.status.success() {
            Ok(())
        } else {
            Err(format!(
                "`kill -TERM {pid}` failed ({}): {}",
                out.status,
                String::from_utf8_lossy(&out.stderr).trim()
            ))
        }
    }

    /// Deliver the signal, or say why it could not be.
    #[cfg(windows)]
    fn deliver(pid: u32) -> Result<(), String> {
        // SAFETY: an FFI call taking two integers. `pid` is a process group id
        // here because the child was created as a group of its own.
        let sent = unsafe {
            windows_sys::Win32::System::Console::GenerateConsoleCtrlEvent(
                windows_sys::Win32::System::Console::CTRL_BREAK_EVENT,
                pid,
            )
        };

        if sent != 0 {
            Ok(())
        } else {
            Err(format!(
                "GenerateConsoleCtrlEvent could not reach process group {pid}: {}",
                std::io::Error::last_os_error()
            ))
        }
    }

    /// Make sure this process has a console, and say whether one had to be made.
    ///
    /// Unconditional rather than detected, which is measured rather than lazy:
    /// `GetConsoleWindow` returns null in an ordinary redirected `cargo test` that
    /// delivers control events perfectly well, so there is no cheap question to
    /// ask first. `AllocConsole` answers it by doing. With a console already
    /// attached it fails with `ERROR_ACCESS_DENIED` and changes nothing; with none
    /// it makes one, which is what a runner started by a service needs. The three
    /// standard handles were measured unchanged across it, so cargo keeps
    /// capturing this test's output either way.
    #[cfg(windows)]
    fn ensure_console() -> bool {
        // SAFETY: an FFI call taking nothing.
        unsafe { windows_sys::Win32::System::Console::AllocConsole() != 0 }
    }
}
