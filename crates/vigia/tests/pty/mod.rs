//! A test binary run again as a child inside a fresh terminal, for the startup
//! queries. Unix only: the pty is the device a terminal pane gives a process.
#![allow(dead_code)]

use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// What the terminal does while the child runs.
pub struct Script<'a> {
    /// What the query ends with, which says it has arrived.
    pub marker: &'a str,
    /// Sent once the query arrives, after `delay`, or nothing.
    pub answer: Option<&'a [u8]>,
    /// How long after the query the answer goes.
    pub delay: Duration,
    /// The child's stdin is not the terminal.
    pub piped: bool,
    /// The child has the terminal on stdio but not as its controlling
    /// terminal, so `/dev/tty` does not open.
    pub detached: bool,
}

/// Run `command` in a fresh terminal under `script`, wait for its `asked`
/// line, then type a byte. Everything the terminal showed.
pub fn run(mut command: Command, script: Script<'_>) -> String {
    let (terminal, slave) = open();
    command
        .stdout(Stdio::from(slave.try_clone().expect("dup")))
        .stderr(Stdio::null());
    if script.piped {
        command.stdin(Stdio::null());
    } else {
        command.stdin(Stdio::from(slave.try_clone().expect("dup")));
    }
    let detached = script.detached;
    // SAFETY: only async-signal-safe calls between fork and exec. The terminal
    // is taken through stdout, which is it in every case here.
    unsafe {
        command.pre_exec(move || {
            if libc::setsid() < 0 || (!detached && libc::ioctl(1, libc::TIOCSCTTY as _, 0) < 0) {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let _child = Reaped(command.spawn().expect("spawn the child"));
    drop(slave);
    // Declared after the child, so it is closed first on every way out: a
    // child exiting on a terminal waits for its output to drain.
    let mut master = terminal;

    let mut seen = Vec::new();
    if let Some(answer) = script.answer {
        read_until(
            &mut master,
            &mut seen,
            script.marker,
            Duration::from_secs(20),
        );
        std::thread::sleep(script.delay);
        master.write_all(answer).expect("answer the query");
    }
    read_until(&mut master, &mut seen, "asked ", Duration::from_secs(20));
    read_until(&mut master, &mut seen, "\n", Duration::from_secs(5));
    // The child reads its terminal in line mode, so the byte needs its line.
    master.write_all(b"z\n").expect("type a byte");
    read_until(&mut master, &mut seen, "got z", Duration::from_secs(10));
    String::from_utf8_lossy(&seen).into_owned()
}

/// The child's `asked` line.
pub fn asked_line(text: &str) -> &str {
    text.lines()
        .find(|line| line.contains("asked "))
        .unwrap_or_else(|| panic!("the child never finished asking: {text}"))
}

/// The milliseconds the child's `asked` line says the query took.
pub fn waited_ms(text: &str) -> f64 {
    asked_line(text)
        .split("asked ")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|w| w.parse().ok())
        .unwrap_or(f64::MAX)
}

/// A new pty: its master, and the slave the child gets.
fn open() -> (File, OwnedFd) {
    let (mut master, mut slave) = (0, 0);
    // SAFETY: both out-pointers are valid; the rest may be null.
    let opened = unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            // `*const` on Linux and `*mut` on macOS.
            std::ptr::null_mut::<libc::termios>() as _,
            std::ptr::null_mut::<libc::winsize>() as _,
        )
    };
    assert_eq!(opened, 0, "openpty failed");
    // SAFETY: openpty handed back two open descriptors this test owns.
    unsafe {
        (
            File::from(OwnedFd::from_raw_fd(master)),
            OwnedFd::from_raw_fd(slave),
        )
    }
}

/// A child that is killed and reaped when the test is done with it, failed or
/// not: one blocked on its terminal never exits by itself.
struct Reaped(Child);

impl Drop for Reaped {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Read the terminal into `seen` until it holds `needle`, failing after `limit`.
fn read_until(master: &mut File, seen: &mut Vec<u8>, needle: &str, limit: Duration) {
    let deadline = Instant::now() + limit;
    while !String::from_utf8_lossy(seen).contains(needle) {
        let left = deadline.saturating_duration_since(Instant::now());
        assert!(
            !left.is_zero(),
            "no {needle:?} from the child: {}",
            String::from_utf8_lossy(seen)
        );
        let mut fds = libc::pollfd {
            fd: master.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: one valid pollfd.
        let ready = unsafe { libc::poll(&mut fds, 1, left.as_millis() as libc::c_int) };
        if ready <= 0 {
            continue;
        }
        let mut buffer = [0u8; 4096];
        match master.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(read) => seen.extend_from_slice(&buffer[..read]),
        }
    }
}
