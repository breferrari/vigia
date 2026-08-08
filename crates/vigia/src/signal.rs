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
//! thread of ours and a loop, and the loop is [`pump`]. Windows hands you a
//! thread of its own making and calls the handler on it once per event, so there
//! is no loop to write and nothing of ours to spawn: the handler sends the wake
//! itself. Writing a Unix-shaped relay on Windows anyway would have cost a
//! reserved stack for the life of a process meant to stay open for days, and a
//! hop on the one exit whose budget is a grace period Windows is counting down.
//!
//! What neither can catch is also the same on both. `SIGKILL` and
//! `TerminateProcess` (`taskkill /F`) end a process without running any code it
//! owns, so no design reaches them, and I8 says so rather than implying more.

use std::io;
use std::sync::OnceLock;
use std::sync::mpsc::Sender;

use crate::Wake;

/// The process-wide right to arm, claimed once.
static ARMED: OnceLock<()> = OnceLock::new();

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
/// **Every delivery, not the first.** A second signal after the first is not
/// swallowed, because the reason a second one arrives is that the first did not
/// appear to work, and a monitor that ignored it would be the process a reader
/// cannot get rid of. Sending again is free: the loop has already broken by
/// then, and the send fails, which is what ends this.
#[cfg(unix)]
fn pump(source: impl Iterator<Item = ()>, tx: &Sender<Wake>) {
    for _ in source {
        if tx.send(Wake::Signalled).is_err() {
            break;
        }
    }
}

/// Wake the shell for every signal that arrives, until it stops listening.
///
/// The thread is detached like the watch and the input threads, and for the same
/// reason `SPEC.md` §6 gives for those: quitting means main returning, and there
/// is no portable way to interrupt a blocked signal read to join it.
///
/// Fallible because arming can fail, and the caller reports rather than refuses:
/// a monitor that will not open because it could not arm a safety net is a worse
/// outcome than one that opens without it and says so.
#[cfg(unix)]
pub(crate) fn forward(tx: Sender<Wake>) -> io::Result<()> {
    claim(&ARMED, || {
        // Owns a dedicated thread reading a self-pipe, which is what makes the
        // forwarding below ordinary safe code. The handler it installs writes one
        // byte, which is all a signal context may do.
        let mut signals = signal_hook::iterator::Signals::new(CAUGHT)?;
        std::thread::spawn(move || pump(signals.forever().map(|_| ()), &tx));
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
/// services rather than a console application, and the handler below deliberately
/// does not return, which is a thing to do to a closing console and not a thing
/// to do to a machine that is shutting down.
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

/// The console control handler.
///
/// Runs on a thread Windows creates for it, which is an ordinary thread and not
/// a POSIX signal context, so the send below is allowed to be a send.
///
/// **It never returns for an event it claims, and that is the point.** Returning
/// from a `CTRL_CLOSE_EVENT` handler tells Windows the process is finished
/// cleaning up, and the process is killed at that moment. The wake above has just
/// asked the shell to leave under its own power, and it needs those milliseconds
/// to drop the session and put the terminal back. Parking holds the grace period
/// open for exactly as long as the exit takes; the process leaving is what ends
/// this thread. Windows kills the process anyway if the exit never comes, which is
/// the same outcome as not having a handler at all.
#[cfg(windows)]
unsafe extern "system" fn on_ctrl(kind: u32) -> windows_sys::core::BOOL {
    if !CAUGHT.contains(&kind) {
        // FALSE, so the event goes to the next handler in the process's list and
        // ultimately to the default one. Claiming an event this module has no
        // answer for would silently disable it.
        return 0;
    }
    if let Some(tx) = SHELL.get() {
        let _ = tx.send(Wake::Signalled);
    }
    loop {
        std::thread::park();
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
        // touches is `SHELL`, which is set above and never unset. The second
        // argument is TRUE, which adds the handler rather than removing one.
        if unsafe { windows_sys::Win32::System::Console::SetConsoleCtrlHandler(Some(on_ctrl), 1) }
            == 0
        {
            return Err(io::Error::last_os_error());
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

#[cfg(test)]
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
    }

    #[cfg(unix)]
    mod forwarding {
        //! Unix only, because [`pump`] is. Windows has no drain loop, and its
        //! handler is covered by the two-process test named above.

        use std::sync::mpsc;

        use super::*;

        #[test]
        fn one_delivery_is_one_wake() {
            let (tx, rx) = mpsc::channel();

            pump(std::iter::once(()), &tx);
            drop(tx);

            let wakes: Vec<_> = rx.into_iter().collect();
            assert_eq!(wakes.len(), 1, "one signal did not produce one wake");
            assert!(
                matches!(wakes[0], Wake::Signalled),
                "the wake was not the one the loop leaves on"
            );
        }

        #[test]
        fn every_delivery_is_forwarded() {
            // Not one and then silence. A reader who sends a second signal is
            // saying the first did not work, and swallowing it is how a process
            // becomes the one you cannot get rid of.
            let (tx, rx) = mpsc::channel();

            pump(std::iter::repeat_n((), 3), &tx);
            drop(tx);

            assert_eq!(rx.into_iter().count(), 3, "a repeated signal was swallowed");
        }

        #[test]
        fn a_hung_up_shell_ends_the_pump() {
            // The thread outlives the loop it feeds: `run` returns, `rx` drops,
            // and this is what stops the forwarder from spinning against a dead
            // channel for the rest of the process's life.
            let (tx, rx) = mpsc::channel();
            drop(rx);

            // Bounded rather than endless on purpose. An unbounded source would
            // make a broken `pump` hang instead of fail, and a test that hangs
            // reports the runner rather than the defect.
            let mut pulled = 0;
            pump(
                std::iter::from_fn(|| {
                    pulled += 1;
                    (pulled <= 1024).then_some(())
                }),
                &tx,
            );

            assert_eq!(
                pulled, 1,
                "the pump kept pulling after the shell had hung up"
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
}
