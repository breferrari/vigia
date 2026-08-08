//! Catching the exit nobody at this keyboard asked for.
//!
//! I8 promises the terminal back on every exit the process controls, and until
//! this module there was exactly one it did not: a signal delivered from
//! outside. `SIGINT`, `SIGTERM` and `SIGHUP` kill a process with their default
//! disposition, so neither [`Session`](crate::Session)'s `Drop` nor the panic
//! hook runs, and the reader is left in the alternate screen with no echo and a
//! mouse reporting every movement as garbage. A pane closing sends exactly that,
//! and so does a script tidying up after an agent session.
//!
//! ## Restoring inside the handler would be wrong twice over
//!
//! It is not async-signal-safe. `crossterm` allocates and writes, and a POSIX
//! handler may do neither. And a handler that restored the screen without ending
//! the loop would leave the process running invisibly, drawing frames onto the
//! reader's shell prompt.
//!
//! So the handler restores nothing. It hands the signal to the event loop, the
//! loop returns, and the ordinary `Drop` that already covers every other exit
//! covers this one too. That is the whole design, and it is why this module owns
//! no terminal state and knows nothing about `TAKEOVER`.
//!
//! **A flag would never be read.** The loop blocks on an untimed `recv`, and
//! `std` retries an interrupted wait rather than returning from it, so an
//! `AtomicBool` set by a handler is seen at the next wake and there may never be
//! one: an idle monitor is idle for hours. What wakes a blocked `recv` is a
//! send, which is why both platforms below end in a `Sender`.
//!
//! ## Two mechanisms, one guarantee
//!
//! Unix has signals; Windows has console control events. `SPEC.md` §6 records
//! why that is one decision rather than two, and it is the same reason
//! [#16](https://github.com/breferrari/vigia/issues/16) gives: a correctness
//! guarantee that means different things on different tier-1 targets is worse
//! than one stated uniformly.
//!
//! **The shapes differ because the platforms hand you different things, and only
//! there.** Unix hands you a self-pipe somebody has to drain, so there is a
//! thread of ours and a loop, and the loop is `pump`. Windows hands you a
//! thread of its own making and calls the handler on it once per event, so there
//! is no loop to write and nothing of ours to spawn: the handler sends the wake
//! itself. Writing a Unix-shaped relay on Windows anyway would have cost a
//! reserved stack for the life of a process meant to stay open for days, and a
//! hop on the one exit whose budget is a grace period Windows is counting down.
//!
//! What neither can catch is also the same on both. `SIGKILL` and
//! `TerminateProcess` (`taskkill /F`) end a process without running any code it
//! owns, so no design reaches them, and I8 says so rather than implying more.
//!
//! **`pump` is deliberately not a doc link anywhere above**, and the reason is
//! worth a line: it does not exist on Windows, so a link to it is unresolvable
//! there while resolving perfectly on Unix. `#![deny(rustdoc::broken_intra_doc_links)]`
//! turns that into an error on one platform out of three. A `#[cfg]` item is a
//! thing to name in prose, not to link to.

use std::io;
use std::sync::OnceLock;
use std::sync::mpsc::Sender;

use crate::Wake;

/// The process-wide right to arm, claimed once.
#[cfg(any(unix, windows))]
static ARMED: OnceLock<()> = OnceLock::new();

/// What a delivery is answered with, given whether one came before it.
///
/// **Arming a handler takes the sender's guarantee away, and this is what gives
/// it back.** Before this module, one `kill` ended the process, because nothing
/// caught it. After it, a `kill` is a wake for a loop, and a loop that is wedged
/// for any reason would swallow every further one: the process would be
/// unkillable by anything short of `SIGKILL`, and reaching for that is how a
/// reader ends up with the wrecked terminal this whole module exists to prevent.
/// So the *first* ask is graceful and the *second* is the disposition the sender
/// would have got if nothing were armed at all.
///
/// Data rather than a branch inside each platform, so the rule is assertable:
/// what [`Answer::Escalate`] names ends the process, and no test can call that.
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
///
/// Takes its slot rather than reaching for the static, the way
/// `terminal::install_hook_in` takes its `Once` and for the same reason: a test
/// can drive the rule against one of its own instead of racing a process-global
/// gate that another test may already have consumed.
///
/// **It exists because the two platforms disagreed about this and nothing said
/// so.** Windows refused a second arming already, as a side effect of the static
/// its handler needs. Unix would have armed twice, quite happily: two `Signals`
/// instances, two threads, two wakes per delivery. An asymmetry that small is
/// still the shape this whole change exists to reject, and here it cost one
/// function to remove.
///
/// A failed arming consumes the claim rather than releasing it. Nothing retries,
/// and a second attempt after a failure would be a second attempt at whatever
/// just failed.
#[cfg(any(unix, windows))]
fn claim(armed: &OnceLock<()>, arm: impl FnOnce() -> io::Result<()>) -> io::Result<()> {
    armed
        .set(())
        .map_err(|()| io::Error::other("signals are already armed for this process"))?;
    arm()
}

/// The signals this catches, by their POSIX numbers.
///
/// `SIGQUIT` is deliberately absent: its default dumps core, and that is
/// something whoever sent it asked for. `SIGKILL` is absent because it cannot be
/// caught at all, which is a statement about the platform rather than a choice.
#[cfg(unix)]
const CAUGHT: [i32; 3] = [
    signal_hook::consts::signal::SIGINT,
    signal_hook::consts::signal::SIGTERM,
    signal_hook::consts::signal::SIGHUP,
];

/// Send one [`Wake::Signalled`] per delivery, until the shell has hung up.
///
/// A free function over an iterator rather than a loop inside [`forward`], for
/// the reason [`drain`](crate::drain) and [`branch_for`](crate::branch_for) are
/// functions: neither `run` nor a signal handler can be driven from a test, so a
/// rule left inside either is a rule nothing can gate. What is left in
/// [`forward`] is arming, which is the part a test genuinely cannot reach.
///
/// Unix only, because it is the drain that Unix needs and Windows does not. The
/// module header says which platform hands you which; a copy of this on the
/// Windows path would have been a loop with nothing to iterate.
///
/// **The second delivery is not a repeat of the first, it is a harder ask**, and
/// [`Answer`] is where that rule lives. The first wakes the shell. The second
/// goes to the default disposition, which for all three of [`CAUGHT`] ends the
/// process, so a reader whose first `kill` appeared to do nothing is never left
/// arguing with a process that cannot be stopped.
///
/// It takes the signal *number* for exactly that reason: escalating means
/// restoring the disposition of the signal that actually arrived, so this is the
/// one place the number is load bearing rather than decorative.
///
/// A send that fails is the other ending, and it is not an escalation. It means
/// `run` has already returned and dropped the receiver, so the process is on its
/// way out under its own power and killing it here would interrupt its own exit.
///
/// **`escalate` is a parameter for the same reason [`Console`](crate::terminal)
/// is a trait**: what it does in production is end the process, so a test driving
/// this function with the real one would kill the test runner rather than report.
/// Passing it in is what makes the escalation rule observable instead of merely
/// reasoned about, and it is how the signal number is checked to be the one that
/// actually arrived rather than the one before it.
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
///
/// The thread is detached like the watch and the input threads, and for the same
/// reason the crate header's *"Why four threads"* gives for those: quitting means
/// main returning, and there is no portable way to interrupt a blocked signal
/// read to join it.
///
/// Fallible because arming can fail, and the caller reports rather than refuses:
/// a monitor that will not open because it could not arm a safety net is a worse
/// outcome than one that opens without it and says so. The error names the call
/// that failed, because the reader sees it on a footer with no other context.
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
///
/// **Windows has no per-event registration**, so unlike the Unix list above this
/// one is a filter applied inside the handler rather than a subscription: the
/// handler is called for every control event the process ever receives, and this
/// array is what decides which of them it answers for.
///
/// `CTRL_LOGOFF_EVENT` and `CTRL_SHUTDOWN_EVENT` are excluded. They reach
/// services rather than a console application, and answering TRUE to one would
/// tell Windows this process has handled a shutdown it cannot actually handle.
/// Leaving them out is what hands them to the default handler intact.
#[cfg(windows)]
const CAUGHT: [u32; 3] = [
    windows_sys::Win32::System::Console::CTRL_C_EVENT,
    windows_sys::Win32::System::Console::CTRL_BREAK_EVENT,
    windows_sys::Win32::System::Console::CTRL_CLOSE_EVENT,
];

/// The shell's own end of the channel, for [`on_ctrl`] to send on.
///
/// A static because a console control handler is a bare function pointer with
/// nowhere to hang state. `mpsc::Sender<T>` is `Sync`, so this holds the shell's
/// real sender rather than a relay, and the handler wakes the loop directly.
#[cfg(windows)]
static SHELL: OnceLock<Sender<Wake>> = OnceLock::new();

/// Whether the shell has been asked to leave already.
///
/// A static for the reason [`SHELL`] is one, and swapped rather than loaded so
/// that two control events arriving at once cannot both count as the first.
#[cfg(windows)]
static ASKED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether answering this event means holding the grace period open.
///
/// Only `CTRL_CLOSE_EVENT` has one. Returning from its handler tells Windows the
/// process has finished cleaning up and it is killed at that moment, so the
/// handler has to stay inside the call for as long as the exit takes. The other
/// two are ordinary: the wake has been delivered, the loop will act on it, and
/// answering TRUE lets this thread go rather than leaving one parked for the life
/// of the process.
#[cfg(windows)]
fn holds_the_grace_period(kind: u32) -> bool {
    kind == windows_sys::Win32::System::Console::CTRL_CLOSE_EVENT
}

/// The console control handler.
///
/// Runs on a thread Windows creates for it, which is an ordinary thread and not
/// a POSIX signal context, so the send below is allowed to be a send.
///
/// **Every path that does not deliver a wake answers FALSE**, which hands the
/// event to the next handler and ultimately to the default one. That is the whole
/// of the care needed here: claiming an event and then not acting on it would
/// make this process the one a reader cannot close, which is a worse failure than
/// the one the module exists to fix.
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
///
/// Split out of [`on_ctrl`] and taking its state as parameters, the way [`claim`]
/// takes its slot and `terminal::install_hook_in` takes its `Once`. The handler
/// itself cannot be driven from a test at all: for a claimed event it deliberately
/// never returns, and it reads two process-global statics that a test cannot
/// unset. Every decision it makes is here instead, where all five are asserted.
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
///
/// See the Unix twin above for why arming is fallible. There is no thread and no
/// loop here, which is not an omission: Windows calls [`on_ctrl`] on a thread of
/// its own once per event, so the only thing left to do is give the handler
/// somewhere to send.
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
///
/// The same posture [`memory`](crate::memory) takes for a readout it cannot get:
/// report nothing rather than guess. No tier-1 target reaches this, and a target
/// that does keeps exactly the behaviour that preceded this module.
#[cfg(not(any(unix, windows)))]
pub(crate) fn forward(_tx: Sender<Wake>) -> io::Result<()> {
    Ok(())
}

// Gated the way its items are: every test below drives `claim`, `answer` or a
// platform body, and none of those exist on a target with neither mechanism.
#[cfg(all(test, any(unix, windows)))]
mod tests {
    //! The half of this module a test can reach.
    //!
    //! Arming itself is not here: installing a real handler is process-global and
    //! delivering to it needs a second process, which is
    //! `terminal::tests::an_external_signal_ends_the_loop_and_the_terminal_goes_back`.
    //! What is here is the claim, the forwarding rule, and the set of events
    //! answered for, which are the three places a silent change would go
    //! unnoticed.

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
            // **The real handler, which round 2 left with no caller at all.** Moving
            // its decisions into `reply` made five exits testable and left the
            // mapping from a `Reply` back to a `BOOL` with nothing calling it. An
            // unclaimed `CTRL_SHUTDOWN_EVENT` answered TRUE tells Windows a
            // shutdown was handled by a process that got no wake.
            //
            // **What this adds is measured rather than assumed, and it is less than
            // it first looked.** Turning `Reply::HandOn` into 1 fails two tests, not
            // none: this one, and the wedged-child gate, which reaches `HandOn` on
            // its second delivery. The audit that asked for this test claimed the
            // whole suite stayed green, and that was wrong. What this genuinely buys
            // is the same catch in a millisecond instead of the wedged gate's thirty
            // seconds, and on a runner where that gate skips for want of a console
            // it is the only thing left holding the mapping.
            //
            // Kind 5 is `CTRL_LOGOFF_EVENT`, which is safe to hand the real handler
            // because it returns on the first line: it never reads `SHELL`, never
            // touches `ASKED`, and never parks.
            //
            // Checked before it is handed over, and not decoratively: if `CAUGHT`
            // ever gained 5, the handler would deliver a wake nobody waits for and
            // then park, and this test would **hang** rather than fail. A test that
            // hangs reports the runner instead of the defect, which is the rule
            // `a_hung_up_shell_ends_the_pump` bounds its own source for.
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
