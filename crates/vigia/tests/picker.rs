//! The picture query against a terminal that never answers. `SPEC.md` §11.1.
#![cfg(unix)]

use std::io::{Read, Write};
use std::os::fd::{FromRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Set in the child this test starts inside a fresh terminal.
const CHILD: &str = "VIGIA_PICKER_CHILD";

/// The child's half: ask, then read one byte from the terminal itself. It can
/// only get the byte if nothing else is still reading.
#[test]
fn silent_child() {
    if std::env::var_os(CHILD).is_none() {
        return;
    }
    // Inside raw mode and left after, as the session holds it in `run`.
    ratatui::crossterm::terminal::enable_raw_mode().expect("raw mode");
    let began = Instant::now();
    let (picker, answer) = vigia::picker(true);
    let waited = began.elapsed();
    ratatui::crossterm::terminal::disable_raw_mode().expect("leave raw mode");
    let mut tty = std::fs::File::open("/dev/tty").expect("the child has a terminal");
    println!(
        "asked {:.3} {:?} {answer:?}",
        waited.as_secs_f64() * 1000.0,
        picker.protocol_type()
    );
    std::io::stdout().flush().expect("flush");
    let mut byte = [0u8; 1];
    tty.read_exact(&mut byte).expect("read the byte");
    println!("got {}", byte[0] as char);
}

#[test]
fn silent_terminal_no_reader() {
    let text = asked_in_pty(None);
    assert!(text.contains("got z"), "the typed byte was eaten: {text}");
    let line = asked_line(&text);
    assert!(
        line.contains("Halfblocks"),
        "a silent terminal chose a protocol: {line}"
    );
    let waited: f64 = line
        .split("asked ")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|w| w.parse().ok())
        .unwrap_or(f64::MAX);
    assert!(
        waited < vigia::PICTURES_WAIT.as_secs_f64() * 1000.0 + 200.0,
        "the query waited {waited}ms on a silent terminal: {line}"
    );
}

#[test]
fn kitty_answer_read() {
    // What kitty sends back: the graphics reply, the cell size, the status.
    let text = asked_in_pty(Some(b"\x1b_Gi=31;OK\x1b\\\x1b[6;20;10t\x1b[0n"));
    assert!(text.contains("got z"), "the typed byte was eaten: {text}");
    let line = asked_line(&text);
    assert!(
        line.contains("Kitty") && line.contains("kitty: true") && line.contains("cell: true"),
        "an answering kitty was not read: {line}"
    );
}

#[test]
fn status_only_halfblocks() {
    let text = asked_in_pty(Some(b"\x1b[0n"));
    assert!(text.contains("got z"), "the typed byte was eaten: {text}");
    let line = asked_line(&text);
    assert!(
        line.contains("Halfblocks") && line.contains("cell: false"),
        "a terminal that answered only its status chose a protocol: {line}"
    );
}

fn asked_line(text: &str) -> &str {
    text.lines()
        .find(|line| line.contains("asked "))
        .expect("the asked line")
}

/// Run the child in a fresh terminal that sends `answer` once the query arrives,
/// or nothing, then type a byte; everything the terminal showed.
fn asked_in_pty(answer: Option<&[u8]>) -> String {
    asked_after(answer, Duration::ZERO)
}

#[test]
fn late_answer_read() {
    // Past the background query's own wait. The query ends on the status reply,
    // which this answer carries last.
    let text = asked_after(
        Some(b"\x1b_Gi=31;OK\x1b\\\x1b[6;20;10t\x1b[0n"),
        Duration::from_millis(250),
    );
    assert!(
        text.contains("got z"),
        "a late answer reached the reader as keys: {text}"
    );
    let line = asked_line(&text);
    assert!(line.contains("Kitty"), "a late answer was not read: {line}");
}

#[test]
fn key_during_silent_query() {
    // A key and no answer, partway into the wait of a terminal silent to the
    // query. The key is the query's to drop; the one after it is the reader's.
    let text = asked_after(Some(b"k"), vigia::PICTURES_WAIT / 3);
    assert!(
        text.contains("got z"),
        "the key after the query was lost: {text}"
    );
    let line = asked_line(&text);
    let waited: f64 = line
        .split("asked ")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|w| w.parse().ok())
        .unwrap_or(f64::MAX);
    assert!(
        line.contains("Halfblocks") && waited < vigia::PICTURES_WAIT.as_secs_f64() * 1000.0 + 200.0,
        "a key typed into a silent query stalled it or chose a protocol: {line}"
    );
}

/// [`asked_in_pty`] with the answer sent `late` after the query arrives.
fn asked_after(answer: Option<&[u8]>, late: Duration) -> String {
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
        .args(["--exact", "silent_child", "--nocapture", "--test-threads=1"])
        .env(CHILD, "1")
        .env("TERM", "xterm-256color")
        .env_remove("TERM_PROGRAM")
        .env_remove(vigia::pictures::PICTURES_VAR)
        .stdin(Stdio::from(slave.try_clone().expect("dup")))
        .stdout(Stdio::from(slave.try_clone().expect("dup")))
        .stderr(Stdio::null());
    // SAFETY: only async-signal-safe calls between fork and exec.
    unsafe {
        child.pre_exec(|| {
            if libc::setsid() < 0 || libc::ioctl(0, libc::TIOCSCTTY as _, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut running = child.spawn().expect("spawn the child");
    drop(slave);

    let mut seen = Vec::new();
    if let Some(answer) = answer {
        read_until(&mut master, &mut seen, "[5n", Duration::from_secs(20));
        std::thread::sleep(late);
        master.write_all(answer).expect("answer the query");
    }
    read_until(&mut master, &mut seen, "asked ", Duration::from_secs(20));
    read_until(&mut master, &mut seen, "\n", Duration::from_secs(5));
    // The child's terminal is back in line mode, so the byte needs its line.
    master.write_all(b"z\n").expect("type a byte");
    read_until(&mut master, &mut seen, "got z", Duration::from_secs(10));
    // Closed first: a child exiting on a terminal waits for its output to drain.
    drop(master);
    let _ = running.wait();
    String::from_utf8_lossy(&seen).into_owned()
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
