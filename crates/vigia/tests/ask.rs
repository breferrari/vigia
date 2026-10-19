//! The background query against terminals that answer, do not, or stop
//! halfway. `SPEC.md` §11.1.
#![cfg(unix)]

use std::io::{Read, Write};
use std::os::fd::{FromRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Set in the child this test starts inside a fresh terminal.
const CHILD: &str = "VIGIA_ASK_CHILD";
/// Set in the child when it asks for the palette too.
const PALETTE: &str = "VIGIA_ASK_PALETTE";
/// The background query's wait, as `run` passes it.
const WAIT: Duration = Duration::from_millis(150);
/// How long a palette answer that has begun is given to finish.
const ANSWERING: Duration = Duration::from_secs(1);
/// Room for a loaded machine past a bound.
const SLACK: Duration = Duration::from_millis(200);

/// The child's half: ask, say how long it took and what came back, then read
/// one byte from the terminal, which it gets only if nothing else is reading.
#[test]
fn ask_child() {
    if std::env::var_os(CHILD).is_none() {
        return;
    }
    let began = Instant::now();
    let reply = vigia::ask(WAIT, std::env::var_os(PALETTE).is_some());
    let waited = began.elapsed();
    println!(
        "asked {:.3} {}",
        waited.as_secs_f64() * 1000.0,
        String::from_utf8_lossy(&reply).escape_debug()
    );
    std::io::stdout().flush().expect("flush");
    let mut tty = std::fs::File::open("/dev/tty").expect("the child has a terminal");
    let mut byte = [0u8; 1];
    tty.read_exact(&mut byte).expect("read the byte");
    println!("got {}", byte[0] as char);
}

#[test]
fn silent_ask_bounded() {
    let text = asked(Ask::default());
    assert_typed(&text);
    assert_within(&text, WAIT);
}

#[test]
fn answered_ask() {
    let text = asked(Ask {
        answer: Some(b"\x1b]11;rgb:0000/0000/0000\x1b\\"),
        ..Ask::default()
    });
    assert_typed(&text);
    assert!(
        asked_line(&text).contains("]11;rgb:"),
        "the answer was not read: {text}"
    );
}

#[test]
fn piped_stdin_ask() {
    let text = asked(Ask {
        piped: true,
        ..Ask::default()
    });
    assert_typed(&text);
    assert_within(&text, WAIT);
}

#[test]
fn partial_palette_ask() {
    // The background and no more: the palette has begun and never ends.
    let text = asked(Ask {
        answer: Some(b"\x1b]11;rgb:0000/0000/0000\x1b\\"),
        palette: true,
        ..Ask::default()
    });
    assert_typed(&text);
    assert_within(&text, ANSWERING);
}

/// How the terminal behaves toward one child.
#[derive(Default)]
struct Ask {
    /// Sent once the query arrives, or nothing.
    answer: Option<&'static [u8]>,
    /// The child asks for the palette too.
    palette: bool,
    /// The child's stdin is not the terminal.
    piped: bool,
}

fn assert_typed(text: &str) {
    assert!(
        text.contains("got z"),
        "the key after the query never reached the reader: {text}"
    );
}

fn assert_within(text: &str, bound: Duration) {
    let line = asked_line(text);
    let waited: f64 = line
        .split("asked ")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|w| w.parse().ok())
        .unwrap_or(f64::MAX);
    assert!(
        waited < (bound + SLACK).as_secs_f64() * 1000.0,
        "the query waited {waited}ms, past its {bound:?}: {line}"
    );
}

fn asked_line(text: &str) -> &str {
    text.lines()
        .find(|line| line.contains("asked "))
        .unwrap_or_else(|| panic!("the child never finished asking: {text}"))
}

/// Run the child in a fresh terminal behaving as `ask` says, then type a byte;
/// everything the terminal showed.
fn asked(ask: Ask) -> String {
    let (mut master, slave) = {
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
                std::fs::File::from(OwnedFd::from_raw_fd(master)),
                OwnedFd::from_raw_fd(slave),
            )
        }
    };
    let mut child = Command::new(std::env::current_exe().expect("this test"));
    child
        .args(["--exact", "ask_child", "--nocapture", "--test-threads=1"])
        .env(CHILD, "1")
        .stdout(Stdio::from(slave.try_clone().expect("dup")))
        .stderr(Stdio::null());
    if ask.palette {
        child.env(PALETTE, "1");
    }
    if ask.piped {
        child.stdin(Stdio::null());
    } else {
        child.stdin(Stdio::from(slave.try_clone().expect("dup")));
    }
    // SAFETY: only async-signal-safe calls between fork and exec. The terminal
    // is taken through stdout, which is it in every case here.
    unsafe {
        child.pre_exec(|| {
            if libc::setsid() < 0 || libc::ioctl(1, libc::TIOCSCTTY as _, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let _running = Reaped(child.spawn().expect("spawn the child"));
    drop(slave);

    let mut seen = Vec::new();
    if let Some(answer) = ask.answer {
        read_until(&mut master, &mut seen, "]11;?", Duration::from_secs(20));
        master.write_all(answer).expect("answer the query");
    }
    read_until(&mut master, &mut seen, "asked ", Duration::from_secs(20));
    read_until(&mut master, &mut seen, "\n", Duration::from_secs(5));
    // The child reads its terminal in line mode, so the byte needs its line.
    master.write_all(b"z\n").expect("type a byte");
    read_until(&mut master, &mut seen, "got z", Duration::from_secs(10));
    String::from_utf8_lossy(&seen).into_owned()
}

/// A child that is killed and reaped when the test is done with it, failed or
/// not: one blocked on its terminal never exits by itself.
struct Reaped(std::process::Child);

impl Drop for Reaped {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Read the terminal into `seen` until it holds `needle`, failing after `limit`.
fn read_until(master: &mut std::fs::File, seen: &mut Vec<u8>, needle: &str, limit: Duration) {
    use std::os::fd::AsRawFd;
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
