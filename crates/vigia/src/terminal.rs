//! Taking the terminal, and giving it back.

use std::io::{self, IsTerminal, Stdout, Write, stdout};
use std::sync::Once;

use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::cursor::{Hide, Show};
use ratatui::crossterm::event::{
    DisableFocusChange, DisableMouseCapture, EnableFocusChange, EnableMouseCapture,
};
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
    /// Focus reporting, so a mark drawn from pointer state can be cleared when
    /// the reader looks away.
    FocusChange,
    /// The cursor, which a monitor never places anywhere meaningful.
    Cursor,
}

/// The order the terminal is taken in. Giving it back walks this backwards.
const TAKEOVER: [Step; 5] = [
    Step::RawMode,
    Step::AlternateScreen,
    Step::MouseCapture,
    Step::FocusChange,
    Step::Cursor,
];

/// The terminal underneath a [`Session`], as something that can be swapped.
trait Console {
    /// Take one step, or report why it could not be taken.
    fn take(&mut self, step: Step) -> io::Result<()>;

    /// Give one step back.
    fn give_back(&mut self, step: Step);
}

/// The real console, over `crossterm`.
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
            Step::FocusChange => execute!(self.out, EnableFocusChange),
            Step::Cursor => execute!(self.out, Hide),
        }
    }

    fn give_back(&mut self, step: Step) {
        let _ = match step {
            Step::RawMode => disable_raw_mode(),
            Step::AlternateScreen => execute!(self.out, LeaveAlternateScreen),
            Step::MouseCapture => execute!(self.out, DisableMouseCapture),
            Step::FocusChange => execute!(self.out, DisableFocusChange),
            Step::Cursor => execute!(self.out, Show),
        };
    }
}

/// The terminal, taken. Gives itself back on drop.
struct Takeover<C: Console> {
    console: C,
    taken: usize,
}

impl<C: Console> Takeover<C> {
    /// Take every step of [`TAKEOVER`], giving back what succeeded if one fails.
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
fn give_back_all<C: Console>(console: &mut C, taken: usize) {
    for step in TAKEOVER[..taken].iter().rev() {
        console.give_back(*step);
    }
}

/// A taken terminal that gives itself back.
pub struct Session {
    /// Declared first, so it drops first and no live `Terminal` outlives the
    /// screen it draws on, even for the length of a drop.
    screen: Screen,
    /// Second, so the terminal goes back after the screen is gone. Never read;
    /// its whole job is its `Drop`.
    _takeover: Takeover<Crossterm<Stdout>>,
}

impl Session {
    /// Enter the alternate screen, in raw mode, with the mouse reporting.
    ///
    /// # Errors
    ///
    /// A step of the takeover fails. Any step that already succeeded is undone first, so the
    /// terminal is never left half taken.
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

    /// Put `sequence` on the wire, outside the buffer.
    ///
    /// For an escape that draws nothing and so owns no cell. It could not go
    /// through the painter in any case: `ratatui`'s `set_stringn` drops a
    /// grapheme containing a control character, which is `SPEC.md` §11.2 B8's
    /// finding. Written through the backend rather than a second handle on
    /// stdout, so it cannot interleave with a frame mid-flush.
    ///
    /// # Errors
    ///
    /// The write or the flush fails.
    pub fn send(&mut self, sequence: &str) -> io::Result<()> {
        let out = self.screen.backend_mut();
        out.write_all(sequence.as_bytes())?;
        out.flush()
    }
}

/// Refuse a standard output that is not a terminal.
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
fn install_hook() {
    install_hook_in(&HOOK, || restore_everything(&mut Crossterm::on_stdout()));
}

/// Give back the whole of [`TAKEOVER`], whatever was actually taken.
fn restore_everything<C: Console>(console: &mut C) {
    give_back_all(console, TAKEOVER.len());
}

/// Chain `restore` onto the panic hook, at most once per `once`.
fn install_hook_in(once: &Once, restore: impl Fn() + Send + Sync + 'static) {
    once.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| on_panic(&restore, &previous, info)));
    });
}

/// The order the panic path runs in: restore, then whatever hook was there.
fn on_panic<T>(restore: impl Fn(), previous: impl Fn(T), info: T) {
    restore();
    previous(info);
}

/// Which side of the luminance line the terminal's background sits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Background {
    /// Luminance at or below the line: the showcase dark palette fits.
    Dark,
    /// Above it: the light palette does.
    Light,
}

/// Classify an OSC 11 reply, or say it is not one.
pub fn background_of(reply: &[u8]) -> Option<Background> {
    let text = std::str::from_utf8(reply).ok()?;
    let at = text.find("]11;")?;
    let body = &text[at + 4..];
    let body = body.strip_prefix("rgb:")?;
    let end = body
        .find('\u{7}')
        .or_else(|| body.find('\u{1b}'))
        .unwrap_or(body.len());
    let mut channels = body[..end].split('/');
    let mut channel = || -> Option<f32> {
        let raw = channels.next()?.trim();
        if raw.is_empty() || raw.len() > 4 || !raw.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let value = u32::from_str_radix(raw, 16).ok()?;
        let ceiling = (16u32.pow(raw.len() as u32) - 1) as f32;
        Some(value as f32 / ceiling)
    };
    let (r, g, b) = (channel()?, channel()?, channel()?);
    let luminance = 0.299 * r + 0.587 * g + 0.114 * b;
    Some(if luminance > 0.5 {
        Background::Light
    } else {
        Background::Dark
    })
}

/// Ask the terminal its background colour, waiting at most `timeout`.
#[cfg(unix)]
pub fn background(timeout: std::time::Duration) -> Option<Background> {
    use std::io::{Read, Write};
    use std::os::fd::AsFd;

    let mut tty = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .ok()?;
    let raw = ratatui::crossterm::terminal::is_raw_mode_enabled().unwrap_or(false);
    if !raw {
        ratatui::crossterm::terminal::enable_raw_mode().ok()?;
    }
    let answer = (|| {
        tty.write_all(b"\x1b]11;?\x1b\\").ok()?;
        tty.flush().ok()?;
        let deadline = std::time::Instant::now() + timeout;
        let mut reply = Vec::with_capacity(64);
        loop {
            let now = std::time::Instant::now();
            if now >= deadline {
                return background_of(&reply);
            }
            let fd = tty.as_fd();
            let mut fds = [rustix::event::PollFd::new(
                &fd,
                rustix::event::PollFlags::IN,
            )];
            let waited =
                rustix::event::poll(&mut fds, Some(&(deadline - now).try_into().ok()?)).ok()?;
            if waited == 0 {
                return background_of(&reply);
            }
            let mut buffer = [0u8; 64];
            let read = tty.read(&mut buffer).ok()?;
            if read == 0 {
                return background_of(&reply);
            }
            reply.extend_from_slice(&buffer[..read]);
            if reply.contains(&0x07) || reply.windows(2).any(|w| w == b"\x1b\\") {
                return background_of(&reply);
            }
        }
    })();
    if !raw {
        let _ = ratatui::crossterm::terminal::disable_raw_mode();
    }
    answer
}

/// The Windows half: no query, no answer, today's posture.
#[cfg(not(unix))]
pub fn background(_timeout: std::time::Duration) -> Option<Background> {
    None
}

#[cfg(test)]
mod background_tests {
    use super::{Background, background_of};

    #[test]
    fn every_reply_shape_the_matrix_saw_classifies() {
        // Four-digit channels, ESC-backslash terminated: the common shape.
        assert_eq!(
            background_of(b"\x1b]11;rgb:0d0d/1111/1717\x1b\\"),
            Some(Background::Dark)
        );
        // BEL terminated, light.
        assert_eq!(
            background_of(b"\x1b]11;rgb:ffff/ffff/eeee\x07"),
            Some(Background::Light)
        );
        // Two-digit channels scale by their own width.
        assert_eq!(
            background_of(b"\x1b]11;rgb:ff/ff/ff\x07"),
            Some(Background::Light)
        );
        assert_eq!(
            background_of(b"\x1b]11;rgb:00/00/00\x07"),
            Some(Background::Dark)
        );
        // The boundary leans dark: a gray at exactly one half is not light.
        assert_eq!(
            background_of(b"\x1b]11;rgb:8000/8000/8000\x07"),
            Some(Background::Light),
            "0x8000 of 0xffff is just past one half"
        );
    }

    #[test]
    fn garbage_and_truncation_answer_nothing() {
        assert_eq!(background_of(b""), None);
        assert_eq!(background_of(b"\x1b]11;rgb:"), None);
        assert_eq!(background_of(b"\x1b]11;rgb:zz/zz/zz\x07"), None);
        assert_eq!(
            background_of(b"\x1b]11;rgb:ff/ff\x07"),
            None,
            "two channels are not three"
        );
        assert_eq!(
            background_of(b"\x1b]10;rgb:ff/ff/ff\x07"),
            None,
            "OSC 10 is the foreground"
        );
        assert_eq!(background_of(b"hello"), None);
        assert_eq!(
            background_of(b"\x1b]11;rgb:fffff/ffff/ffff\x07"),
            None,
            "five digits is no channel the protocol has"
        );
    }
}

#[cfg(test)]
mod tests {
    //! I8, as the policy it is.

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
    #[derive(Clone)]
    struct Recorder {
        log: Arc<Mutex<Vec<Event>>>,
        /// The index into [`TAKEOVER`] whose `take` fails, if any.
        fail_at: Option<usize>,
        /// Calls to [`Console::take`] so far, including the one that fails.
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
        // What the hook does, minus which console it writes to.
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

        // Silence the hook that is already there *before* chaining onto it. The chain
        // defers to whatever it replaced, and what it replaced would print a backtrace
        // for a panic this test causes deliberately.
        let real = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));

        // Counted per thread, not per process. The hook is global for as long as this
        // test holds it, so a *different* test failing in that window would run this
        // restore too and report a second failure here, pointing at code that is fine.
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

        // Read out, then assert.
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
        // The oracle is DEC's own private mode numbers, not crossterm restating itself:
        // 1049 is the alternate screen buffer, 25 is cursor visibility,
        // 1000/1002/1003/1015/1006 are the mouse reporting modes, and 1004 is focus
        // reporting.
        assert_eq!(ansi(&EnterAlternateScreen), "\x1b[?1049h");
        assert_eq!(ansi(&LeaveAlternateScreen), "\x1b[?1049l");
        assert_eq!(ansi(&Hide), "\x1b[?25l");
        assert_eq!(ansi(&Show), "\x1b[?25h");
        assert_eq!(
            ansi(&EnableMouseCapture),
            "\x1b[?1000h\x1b[?1002h\x1b[?1003h\x1b[?1015h\x1b[?1006h"
        );
        assert_eq!(ansi(&EnableFocusChange), "\x1b[?1004h");
        assert_eq!(ansi(&DisableFocusChange), "\x1b[?1004l");
    }

    #[test]
    fn the_takeover_takes_every_step_there_is() {
        // The assertion no other gate here can make, and it is general on purpose.
        const EVERY: [Step; 5] = [
            Step::RawMode,
            Step::AlternateScreen,
            Step::MouseCapture,
            Step::FocusChange,
            Step::Cursor,
        ];
        for step in EVERY {
            // Exhaustive by construction: adding a variant to `Step` without
            // adding it to `EVERY` is a non-exhaustive-match error right here.
            let named = match step {
                Step::RawMode => "raw mode, without which keys arrive line-buffered",
                Step::AlternateScreen => {
                    "the alternate screen, without which a reader loses scrollback"
                }
                Step::MouseCapture => {
                    "mouse reporting, which SPEC.md 4 puts in scope for the wheel"
                }
                Step::FocusChange => "focus reporting, the middle rung of 11.1's clearing ladder",
                Step::Cursor => "the cursor, which a monitor never places anywhere meaningful",
            };
            assert!(
                TAKEOVER.contains(&step),
                "the takeover no longer takes {named}"
            );
        }
        assert_eq!(
            TAKEOVER.len(),
            EVERY.len(),
            "TAKEOVER takes a step twice, or takes something not in `Step`"
        );

        // Focus reporting sits with the mouse rather than after the cursor, so
        // the two modes that change how *input* is reported are taken together
        // and given back together.
        let at = |step| {
            TAKEOVER
                .iter()
                .position(|s| *s == step)
                .expect("asserted present above")
        };
        assert!(
            at(Step::MouseCapture) < at(Step::FocusChange),
            "focus reporting is no longer taken beside the mouse"
        );
        assert!(
            at(Step::FocusChange) < at(Step::Cursor),
            "focus reporting is taken after the cursor rather than with the \
             other input modes"
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

        // Non-vacuity. A platform that routed every one of these through a console API
        // would leave both streams empty, and every assertion below would hold over
        // nothing.
        assert!(
            !took.is_empty(),
            "the real console wrote no escape sequences at all, so this proves nothing"
        );

        // The wiring, which is the one thing neither of the other two layers can see.
        assert!(
            took.contains(&(1004, true)),
            "the takeover wrote no `?1004h`, so `Step::FocusChange` is wired to \
             something other than focus reporting: {took:?}"
        );

        // The inverse, derived from what was actually written rather than listed
        // again. Polarity flipped and order reversed, per mode.
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
        // The structural half of "the terminal is restored on every exit". What makes
        // that true is that every exit drops `Shell`, which owns the `Session`.
        const SOURCES: [(&str, &str); 16] = [
            ("lib.rs", include_str!("lib.rs")),
            ("main.rs", include_str!("main.rs")),
            ("app.rs", include_str!("app.rs")),
            ("clipboard.rs", include_str!("clipboard.rs")),
            ("colour.rs", include_str!("colour.rs")),
            ("config.rs", include_str!("config.rs")),
            ("glyphs.rs", include_str!("glyphs.rs")),
            ("icons.rs", include_str!("icons.rs")),
            ("input.rs", include_str!("input.rs")),
            ("memory.rs", include_str!("memory.rs")),
            ("render.rs", include_str!("render.rs")),
            ("signal.rs", include_str!("signal.rs")),
            ("terminal.rs", include_str!("terminal.rs")),
            ("theme.rs", include_str!("theme.rs")),
            ("update.rs", include_str!("update.rs")),
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
            // Only what ships.
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

    /// Where the parent tells the child to leave its evidence.
    const CHILD: &str = "VIGIA_SIGNAL_DIR";

    /// The child, by the name the harness selects it with.
    const CHILD_TEST: &str = "terminal::tests::signal_child";

    /// Tells the child to take the wake and then do nothing with it.
    const WEDGE: &str = "VIGIA_SIGNAL_WEDGE";

    /// The child gets a process group of its own, so a control event addressed
    /// to its process id reaches it and nothing else, above all not the runner.
    #[cfg(windows)]
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

    /// A process that arms the real handler, blocks the way the shell blocks, and
    /// leaves the real guard to restore.
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

        // The expectation is derived here, in the child, and that placement is the
        // whole of it.
        let mut owed = Crossterm { out: Vec::new() };
        give_back_all(&mut owed, TAKEOVER.len());

        // Non-vacuity, asserted where the fact lives rather than inferred across a
        // process boundary.
        assert!(
            !owed.out.is_empty(),
            "this console emits no escape sequences for the takeover, so the sink \
             would prove nothing"
        );
        std::fs::write(dir.join("owed"), &owed.out).expect("write what is owed");

        let (tx, rx) = std::sync::mpsc::channel();
        crate::signal::forward(tx).expect("arm the signal handler");

        {
            // Built rather than taken, and that is the one departure from the
            // production path. `Takeover::take` would enable raw mode on the console
            // running this test, which layer 3 above rules out in so many words.
            let _takeover = Takeover {
                console: Crossterm { out: sink },
                taken: TAKEOVER.len(),
            };

            // Last of all, so the parent can never signal a process that is not
            // listening yet, and after the guard exists so that what the signal
            // finds is a terminal already owed back.
            std::fs::write(dir.join("ready"), b"armed").expect("write the ready marker");

            // What this borrows from `run`'s loop, and all it borrows: block on the
            // channel, recognise the wake, leave. Everything else there is about
            // frames, and `run` owns a terminal so no test can drive the real one.
            match rx.recv() {
                Ok(crate::Wake::Signalled) => {}
                Ok(_) => panic!("the forwarder sent a wake that was not the signal"),
                Err(e) => panic!("the forwarder hung up before any signal arrived: {e}"),
            }

            if std::env::var_os(WEDGE).is_some() {
                std::fs::write(dir.join("woke"), b"and did nothing").expect("write woke");
                // Never leaves this scope, so the guard never drops and the
                // terminal is never given back. Exactly the shell the second ask
                // is the floor under.
                loop {
                    std::thread::park();
                }
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
        #[cfg(windows)]
        ensure_console();

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
            // A printed skip rather than a pass.
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_dir_all(&dir);
            println!("skipped: {why}");
            return;
        }

        // Bounded, rather than `child.wait()` straight away, which has no timeout: a
        // defect that stopped the wake from ever arriving would leave the child blocked
        // on its `recv` forever and hang this test instead of failing it, and a hang
        // reports the runner rather than the defect.
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

        // Both facts in one message, in this order and deliberately.
        assert!(
            dropped,
            "the child was signalled and left no marker, so the guard never dropped. \
             It exited {status}, which says whether it hung or failed on its way out"
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

    #[test]
    fn a_second_external_signal_kills_a_shell_that_ignored_the_first() {
        // The failing test I8's new by-choice exclusion needs. `SPEC.md` §11.1 rules
        // that the second ask takes the default disposition: it kills the process and
        // restores nothing.
        let dir = std::env::temp_dir().join(format!("vigia-wedged-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create the child's directory");

        #[cfg(windows)]
        ensure_console();

        let exe = std::env::current_exe().expect("this test binary's own path");
        let mut command = std::process::Command::new(&exe);
        command
            .args([CHILD_TEST, "--exact", "--ignored", "--nocapture"])
            .args(["--test-threads", "1"])
            .env(CHILD, &dir)
            .env(WEDGE, "1");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(CREATE_NEW_PROCESS_GROUP);
        }
        let mut child = command.spawn().expect("spawn the wedged child");

        if !wait_for(&dir.join("ready")) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("the child never armed, so nothing could be delivered to it");
        }

        if let Err(why) = deliver(child.id()) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_dir_all(&dir);
            println!("skipped: {why}");
            return;
        }

        // The first ask has to be *consumed* before the second is sent, or this
        // measures two racing first asks rather than an escalation.
        if !wait_for(&dir.join("woke")) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("the child never took the first wake, so there was nothing to ignore");
        }

        if let Err(why) = deliver(child.id()) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_dir_all(&dir);
            println!("skipped on the second ask: {why}");
            return;
        }

        // Bounded, because a process that ignores the escalation is the defect this
        // is looking for and `wait()` has no timeout to catch it with.
        let died = wait_for_exit(&mut child);
        if !died {
            let _ = child.kill();
            let _ = child.wait();
            panic!(
                "the child survived a second delivery, so an armed `vigia` is \
                 unkillable short of SIGKILL and the escalation is not working"
            );
        }

        let status = child.wait().expect("wait for the wedged child");
        assert!(
            !status.success(),
            "the child exited cleanly ({status}), so the second ask was answered \
             gracefully rather than taken to the default disposition"
        );
        assert!(
            !dir.join("dropped").exists(),
            "the wedged child restored the terminal, which means it left its loop \
             after all and this test proved nothing about the escalation"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Whether the child is gone, within the same bound [`wait_for`] uses.
    fn wait_for_exit(child: &mut std::process::Child) -> bool {
        for _ in 0..600 {
            match child.try_wait() {
                Ok(Some(_)) => return true,
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(50)),
                Err(_) => return false,
            }
        }
        false
    }

    /// Wait for a marker the child writes.
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

    /// Make sure this process has a console.
    #[cfg(windows)]
    fn ensure_console() {
        // SAFETY: an FFI call taking nothing. Failure is the ordinary case and
        // means a console was already attached.
        unsafe {
            windows_sys::Win32::System::Console::AllocConsole();
        }
    }
}
