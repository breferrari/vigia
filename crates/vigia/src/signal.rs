//! Catching the exit nobody at this keyboard asked for.

use std::io;
use std::sync::OnceLock;
use std::sync::mpsc::Sender;

use crate::Wake;

/// The process-wide right to arm, claimed once.
#[cfg(any(unix, windows))]
static ARMED: OnceLock<()> = OnceLock::new();

/// What a delivery is answered with, given whether one came before it.
#[cfg(any(unix, windows))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Answer {
    /// Wake the shell and let it leave under its own power, restoring on the way.
    Wake,
    /// Hand this one to the default disposition. The shell was asked already and
    /// is still here.
    Escalate,
}

/// Answer one delivery.
#[cfg(any(unix, windows))]
fn answer(asked_already: bool) -> Answer {
    if asked_already {
        Answer::Escalate
    } else {
        Answer::Wake
    }
}

/// Claim that right, or refuse, and only then arm.
#[cfg(any(unix, windows))]
fn claim(armed: &OnceLock<()>, arm: impl FnOnce() -> io::Result<()>) -> io::Result<()> {
    armed
        .set(())
        .map_err(|()| io::Error::other("signals are already armed for this process"))?;
    arm()
}

/// The signals this catches, by their POSIX numbers.
#[cfg(unix)]
const CAUGHT: [i32; 3] = [
    signal_hook::consts::signal::SIGINT,
    signal_hook::consts::signal::SIGTERM,
    signal_hook::consts::signal::SIGHUP,
];

/// Send one [`Wake::Signalled`] per delivery, until the shell has hung up.
#[cfg(unix)]
fn pump(source: impl Iterator<Item = i32>, tx: &Sender<Wake>, escalate: impl Fn(i32)) {
    let mut asked = false;
    for signal in source {
        match answer(asked) {
            // In production this does not return: it restores the default
            // disposition and re-raises, so the process dies inside the call. The
            // terminal may be left as it was, which is precisely the trade the
            // second ask is asking for.
            Answer::Escalate => escalate(signal),
            Answer::Wake => {
                if tx.send(Wake::Signalled).is_err() {
                    break;
                }
                asked = true;
            }
        }
    }
}

/// Wake the shell for every signal that arrives, until it stops listening.
#[cfg(unix)]
pub(crate) fn forward(tx: Sender<Wake>) -> io::Result<()> {
    claim(&ARMED, || {
        // Owns a dedicated thread reading a self-pipe, which is what makes the
        // forwarding below ordinary safe code. The handler it installs writes one
        // byte, which is all a signal context may do.
        let mut signals = signal_hook::iterator::Signals::new(CAUGHT)
            .map_err(|e| io::Error::other(format!("registering {CAUGHT:?}: {e}")))?;
        std::thread::spawn(move || {
            pump(signals.forever(), &tx, |signal| {
                // The only call site of the real thing, and the reason `pump`
                // takes it rather than calling it: this ends the process.
                let _ = signal_hook::low_level::emulate_default_handler(signal);
            });
        });
        Ok(())
    })
}

/// The console control events this claims, by their documented WinAPI numbers.
#[cfg(windows)]
const CAUGHT: [u32; 3] = [
    windows_sys::Win32::System::Console::CTRL_C_EVENT,
    windows_sys::Win32::System::Console::CTRL_BREAK_EVENT,
    windows_sys::Win32::System::Console::CTRL_CLOSE_EVENT,
];

/// The shell's own end of the channel, for [`on_ctrl`] to send on.
#[cfg(windows)]
static SHELL: OnceLock<Sender<Wake>> = OnceLock::new();

/// Whether the shell has been asked to leave already.
#[cfg(windows)]
static ASKED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether answering this event means holding the grace period open.
#[cfg(windows)]
fn holds_the_grace_period(kind: u32) -> bool {
    kind == windows_sys::Win32::System::Console::CTRL_CLOSE_EVENT
}

/// The console control handler.
#[cfg(windows)]
unsafe extern "system" fn on_ctrl(kind: u32) -> windows_sys::core::BOOL {
    match reply(kind, SHELL.get(), &ASKED) {
        Reply::HandOn => 0,
        Reply::Handled => 1,
        Reply::Wait => loop {
            std::thread::park();
        },
    }
}

/// What answering a control event comes to, short of the waiting itself.
#[cfg(windows)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reply {
    /// FALSE: not answered here, so the default handler gets it.
    HandOn,
    /// TRUE: the wake is delivered and this thread is done.
    Handled,
    /// TRUE eventually, by never returning, which holds the grace period open.
    Wait,
}

/// Decide what to do with a control event.
#[cfg(windows)]
fn reply(kind: u32, shell: Option<&Sender<Wake>>, asked: &std::sync::atomic::AtomicBool) -> Reply {
    if !CAUGHT.contains(&kind) {
        // Not ours. Claiming an event this module has no answer for would
        // silently disable it.
        return Reply::HandOn;
    }
    let Some(tx) = shell else {
        // Armed but with nowhere to send, which should be impossible and is not
        // worth becoming unkillable over.
        return Reply::HandOn;
    };
    if answer(asked.swap(true, std::sync::atomic::Ordering::SeqCst)) == Answer::Escalate {
        // The second ask. FALSE reaches the default handler, which ends the
        // process: see `Answer`.
        return Reply::HandOn;
    }
    if tx.send(Wake::Signalled).is_err() {
        // `run` has already returned and dropped the receiver, so nobody is
        // listening and the default disposition is the honest answer.
        return Reply::HandOn;
    }
    if holds_the_grace_period(kind) {
        Reply::Wait
    } else {
        Reply::Handled
    }
}

/// Wake the shell for every console control event it claims.
#[cfg(windows)]
pub(crate) fn forward(tx: Sender<Wake>) -> io::Result<()> {
    claim(&ARMED, || {
        // Cannot already be set: `claim` is what makes this the only arming, and
        // it has just succeeded.
        let _ = SHELL.set(tx);

        // SAFETY: `on_ctrl` is a valid `PHANDLER_ROUTINE`, and the only state it
        // touches is `SHELL` and `ASKED`, both of which outlive it. The second
        // argument is TRUE, which adds the handler rather than removing one.
        if unsafe { windows_sys::Win32::System::Console::SetConsoleCtrlHandler(Some(on_ctrl), 1) }
            == 0
        {
            // Named, because the reader sees this on a footer with no other
            // context and `Access is denied. (os error 5)` alone says nothing
            // about which arming failed.
            return Err(io::Error::other(format!(
                "SetConsoleCtrlHandler: {}",
                io::Error::last_os_error()
            )));
        }
        Ok(())
    })
}

/// Nothing, on a target with neither mechanism.
#[cfg(not(any(unix, windows)))]
pub(crate) fn forward(_tx: Sender<Wake>) -> io::Result<()> {
    Ok(())
}

// Gated the way its items are: every test below drives `claim`, `answer` or a
// platform body, and none of those exist on a target with neither mechanism.
#[cfg(all(test, any(unix, windows)))]
mod tests {
    //! The half of this module a test can reach.

    use super::*;

    #[test]
    fn a_second_arming_is_refused() {
        // Against a local slot rather than `ARMED`, which another test in this
        // binary may already have consumed. `install_hook_in` takes its `Once`
        // for the same reason.
        let armed = OnceLock::new();
        let mut arms = 0;

        claim(&armed, || {
            arms += 1;
            Ok(())
        })
        .expect("the first arming");

        let again = claim(&armed, || {
            arms += 1;
            Ok(())
        });

        assert!(again.is_err(), "a second arming was allowed");
        assert_eq!(arms, 1, "the refused arming ran anyway");

        // The message reaches a footer with nothing else on it, so it has to say
        // why on its own. `check_drawable`'s test holds the same line.
        let message = again.expect_err("refused").to_string();
        assert!(
            message.contains("already armed"),
            "the refusal does not say why: {message}"
        );
    }

    #[test]
    fn a_failed_arming_keeps_the_claim() {
        // The doc says a failure consumes the claim rather than releasing it, and
        // nothing held that. It matters because the alternative reads more
        // forgiving and is worse: a retry would be a second attempt at whatever
        // just failed, on a platform that has already installed half of it.
        let armed = OnceLock::new();

        let first = claim(&armed, || Err(io::Error::other("the arming failed")));
        assert!(first.is_err(), "the failure was swallowed");

        let again = claim(&armed, || Ok(()));
        let message = again.expect_err("the claim was released").to_string();
        assert!(
            message.contains("already armed"),
            "the second attempt failed for a different reason: {message}"
        );
    }

    #[test]
    fn only_one_of_two_racing_arms_wins() {
        // `forward` is called once by `run`, so this is about the rule rather than
        // about a reachable path today. `OnceLock::set` is what makes it true, and
        // a future edit to something more forgiving would pass every test above.
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let armed = Arc::new(OnceLock::new());
        let arms = Arc::new(AtomicUsize::new(0));
        let wins = Arc::new(AtomicUsize::new(0));

        let racers: Vec<_> = (0..8)
            .map(|_| {
                let armed = Arc::clone(&armed);
                let arms = Arc::clone(&arms);
                let wins = Arc::clone(&wins);
                std::thread::spawn(move || {
                    let outcome = claim(&armed, || {
                        arms.fetch_add(1, Ordering::SeqCst);
                        Ok(())
                    });
                    if outcome.is_ok() {
                        wins.fetch_add(1, Ordering::SeqCst);
                    }
                })
            })
            .collect();
        for racer in racers {
            racer.join().expect("a racer panicked");
        }

        assert_eq!(wins.load(Ordering::SeqCst), 1, "not exactly one claim won");
        assert_eq!(
            arms.load(Ordering::SeqCst),
            1,
            "the arming ran more than once"
        );
    }

    #[test]
    fn the_second_ask_goes_to_the_default_disposition() {
        // The rule `pump` and `on_ctrl` both run on, asserted here because what
        // `Escalate` names ends the process and no test can call it. Without this
        // the escalation exists only in prose, and a monitor whose loop is wedged
        // is a process that cannot be stopped.
        assert_eq!(answer(false), Answer::Wake);
        assert_eq!(answer(true), Answer::Escalate);
    }

    #[cfg(unix)]
    mod forwarding {
        //! Unix only, because [`pump`] is. Windows has no drain loop, and its
        //! handler is covered by the two-process test named above.

        use std::cell::RefCell;
        use std::sync::mpsc;

        use super::*;

        /// Deliveries are signal numbers, so the fixtures name real ones: 15 is
        /// `SIGTERM` and 1 is `SIGHUP`. Escalating restores the disposition of
        /// whichever one arrived, which is why the number is carried at all.
        const TERM: i32 = 15;
        const HUP: i32 = 1;

        // Each test builds its own `seen` and closure inline rather than sharing a
        // helper: a helper returning both cannot own the cell and lend it at once,
        // and the borrow checker is right about that.

        #[test]
        fn one_delivery_is_one_wake() {
            let (tx, rx) = mpsc::channel();
            let seen = RefCell::new(Vec::new());

            pump(std::iter::once(TERM), &tx, |s| seen.borrow_mut().push(s));
            drop(tx);

            let wakes: Vec<_> = rx.into_iter().collect();
            assert_eq!(wakes.len(), 1, "one signal did not produce one wake");
            assert!(
                matches!(wakes[0], Wake::Signalled),
                "the wake was not the one the loop leaves on"
            );
            assert!(
                seen.borrow().is_empty(),
                "the first ask escalated instead of asking politely"
            );
        }

        #[test]
        fn the_second_delivery_escalates_instead_of_waking() {
            // The rule that keeps a wedged monitor killable. The first ask is a
            // wake; the second is the disposition the sender would have had if
            // nothing were armed. Swallowing it, which is what this did before the
            // audit, is how a process becomes the one you cannot get rid of.
            let (tx, rx) = mpsc::channel();
            let seen = RefCell::new(Vec::new());

            pump([TERM, HUP].into_iter(), &tx, |s| seen.borrow_mut().push(s));
            drop(tx);

            assert_eq!(
                rx.into_iter().count(),
                1,
                "the second delivery was forwarded as a wake as well"
            );
            // The signal that *arrived*, not the one before it. Escalating means
            // restoring one particular disposition, so carrying the wrong number
            // would restore the wrong one.
            assert_eq!(
                *seen.borrow(),
                vec![HUP],
                "the escalation did not name the signal that arrived"
            );
        }

        #[test]
        fn a_hung_up_shell_ends_the_pump() {
            // The thread outlives the loop it feeds: `run` returns, `rx` drops,
            // and this is what stops the forwarder from spinning against a dead
            // channel for the rest of the process's life.
            let (tx, rx) = mpsc::channel();
            drop(rx);
            let seen = RefCell::new(Vec::new());

            // Bounded rather than endless on purpose. An unbounded source would
            // make a broken `pump` hang instead of fail, and a test that hangs
            // reports the runner rather than the defect.
            let mut pulled = 0;
            pump(
                std::iter::from_fn(|| {
                    pulled += 1;
                    (pulled <= 1024).then_some(TERM)
                }),
                &tx,
                |s| seen.borrow_mut().push(s),
            );

            assert_eq!(
                pulled, 1,
                "the pump kept pulling after the shell had hung up"
            );
            // A hung-up shell is a process already leaving under its own power.
            // Escalating there would interrupt its own exit.
            assert!(
                seen.borrow().is_empty(),
                "a hung-up shell was escalated over instead of let go"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn the_caught_set_is_the_one_documented() {
        // The oracle is POSIX's own numbers rather than `signal-hook` restating
        // itself, the same way the escape sequences in `terminal` are checked
        // against DEC's. `SIGHUP` is 1, `SIGINT` is 2 and `SIGTERM` is 15 on
        // every Unix this ships to.
        assert_eq!(CAUGHT, [2, 15, 1]);

        // 9 is `SIGKILL`. Not an oversight and not fixable: it exists so that a
        // process cannot decline to die, and I8 states that rather than implying
        // otherwise.
        assert!(!CAUGHT.contains(&9));
    }

    #[cfg(windows)]
    #[test]
    fn the_caught_set_is_the_one_documented() {
        // WinAPI's own numbers, for the reason the Unix twin gives. This is also
        // what pins the handler's filter, since `on_ctrl` tests membership of
        // this array and cannot itself be called: it never returns for an event
        // it claims.
        assert_eq!(CAUGHT, [0, 1, 2]);

        // 5 is `CTRL_LOGOFF_EVENT` and 6 is `CTRL_SHUTDOWN_EVENT`, and leaving
        // them out is what hands them to the default handler.
        assert!(!CAUGHT.contains(&5));
        assert!(!CAUGHT.contains(&6));
    }

    #[cfg(windows)]
    mod answering {
        //! Every decision the console handler makes, against slots of this test's
        //! own rather than the process statics. The handler itself never returns
        //! for an event it claims, so this is the only way any of it is reachable.

        use std::sync::atomic::AtomicBool;
        use std::sync::mpsc;

        use super::*;

        /// 0, 1 and 2 are `CTRL_C`, `CTRL_BREAK` and `CTRL_CLOSE`.
        const C: u32 = 0;
        const BREAK: u32 = 1;
        const CLOSE: u32 = 2;

        #[test]
        fn the_handler_maps_hand_on_to_false() {
            // **The real handler, which is easily left with no caller at all.**
            // Moving its decisions into `reply` makes five exits testable and
            // leaves the mapping from a `Reply` back to a `BOOL` uncalled. An
            // unclaimed `CTRL_SHUTDOWN_EVENT` answered TRUE tells Windows a
            // shutdown was handled by a process that got no wake.
            assert!(
                !CAUGHT.contains(&5),
                "5 is claimed now, so handing it to the real handler would park                  forever; pick a kind outside `CAUGHT` or drop this test"
            );

            // SAFETY: `on_ctrl` performs no unsafe operation. It does read `SHELL`,
            // because `reply`'s arguments evaluate before the call, and that read is
            // a non-blocking `OnceLock::get` whose result this kind never looks at.
            // What the assert above establishes is the part that matters: for a kind
            // outside `CAUGHT`, `reply` returns before it touches `ASKED` or parks.
            assert_eq!(
                unsafe { on_ctrl(5) },
                0,
                "the handler answered TRUE to an event it does not claim"
            );
        }

        #[test]
        fn an_event_this_does_not_claim_is_handed_on() {
            let (tx, _rx) = mpsc::channel();
            let asked = AtomicBool::new(false);

            // 5 is `CTRL_LOGOFF_EVENT` and 6 is `CTRL_SHUTDOWN_EVENT`. Answering
            // either would tell Windows this process handled a shutdown it cannot.
            for kind in [5, 6] {
                assert_eq!(
                    reply(kind, Some(&tx), &asked),
                    Reply::HandOn,
                    "control event {kind} was absorbed instead of handed on"
                );
            }
            assert!(
                !asked.load(std::sync::atomic::Ordering::SeqCst),
                "an event this module does not claim consumed the one graceful ask"
            );
        }

        #[test]
        fn a_first_ask_is_answered_and_lets_its_thread_go() {
            let (tx, rx) = mpsc::channel();

            // Parking on either would leave one OS thread per delivery for the
            // life of a process meant to stay open for days. A fresh latch per
            // kind, because one ask is all each gets.
            for kind in [C, BREAK] {
                let asked = AtomicBool::new(false);
                assert_eq!(reply(kind, Some(&tx), &asked), Reply::Handled);
                assert!(
                    matches!(rx.try_recv(), Ok(Wake::Signalled)),
                    "control event {kind} was answered without delivering a wake"
                );
            }
        }

        #[test]
        fn a_closing_console_is_waited_out_instead() {
            let (tx, rx) = mpsc::channel();
            let asked = AtomicBool::new(false);

            assert_eq!(reply(CLOSE, Some(&tx), &asked), Reply::Wait);
            assert!(
                matches!(rx.try_recv(), Ok(Wake::Signalled)),
                "the wake was not delivered before the wait began"
            );
        }

        #[test]
        fn a_second_ask_is_handed_to_the_default_handler() {
            // The Windows half of the escalation, which is what keeps a wedged
            // monitor killable. Only reachable with an injected slot: the real one
            // is a process static no test can unset.
            let (tx, _rx) = mpsc::channel();
            let asked = AtomicBool::new(false);

            assert_eq!(reply(BREAK, Some(&tx), &asked), Reply::Handled);
            assert_eq!(
                reply(BREAK, Some(&tx), &asked),
                Reply::HandOn,
                "the second ask was answered again instead of escalating"
            );
        }

        #[test]
        fn an_event_with_nobody_to_tell_is_handed_on() {
            let asked = AtomicBool::new(false);
            assert_eq!(reply(BREAK, None, &asked), Reply::HandOn);

            // And a shell that has already hung up, which is `run` having returned.
            let (tx, rx) = mpsc::channel();
            drop(rx);
            let asked = AtomicBool::new(false);
            assert_eq!(
                reply(BREAK, Some(&tx), &asked),
                Reply::HandOn,
                "an event was claimed with nobody listening, which is how a process \
                 becomes unkillable"
            );
        }
    }
}
