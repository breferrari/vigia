//! The background query against terminals that answer, do not, or stop
//! halfway. `SPEC.md` §11.1.
#![cfg(unix)]

#[path = "pty/mod.rs"]
mod pty;

use std::io::{Read, Write};
use std::process::Command;
use std::time::{Duration, Instant};

/// Set in the child this test starts inside a fresh terminal.
const CHILD: &str = "VIGIA_ASK_CHILD";
/// Set in the child when its stdin is `/dev/tty` itself.
const DEVTTY: &str = "VIGIA_ASK_DEVTTY";
/// Set in the child when it asks for the palette too.
const PALETTE: &str = "VIGIA_ASK_PALETTE";
/// Room for a loaded machine past a bound.
const SLACK: Duration = Duration::from_millis(200);

/// The child's half: ask, say how long it took and what came back, then read
/// one byte from the terminal, which it gets only if nothing else is reading.
#[test]
fn ask_child() {
    if std::env::var_os(CHILD).is_none() {
        return;
    }
    if std::env::var_os(DEVTTY).is_some() {
        use std::os::fd::AsRawFd;
        let tty = std::fs::File::open("/dev/tty").expect("the child has a terminal");
        // SAFETY: both descriptors are open; stdin is replaced by a copy of the tty.
        unsafe { libc::dup2(tty.as_raw_fd(), 0) };
    }
    let began = Instant::now();
    let reply = vigia::ask(vigia::BACKGROUND_WAIT, std::env::var_os(PALETTE).is_some());
    let waited = began.elapsed();
    // SAFETY: fcntl on stdin, which is open.
    let flags = unsafe { libc::fcntl(0, libc::F_GETFL) };
    println!(
        "asked {:.3} {} blocking={}",
        waited.as_secs_f64() * 1000.0,
        String::from_utf8_lossy(&reply).escape_debug(),
        flags & libc::O_NONBLOCK == 0
    );
    std::io::stdout().flush().expect("flush");
    let mut byte = [0u8; 1];
    // Without a controlling terminal, the terminal is stdin.
    match std::fs::File::open("/dev/tty") {
        Ok(mut tty) => tty.read_exact(&mut byte),
        Err(_) => std::io::stdin().read_exact(&mut byte),
    }
    .expect("read the byte");
    println!("got {}", byte[0] as char);
}

#[test]
fn silent_ask_bounded() {
    let text = asked(Ask::default());
    assert_typed(&text);
    assert_within(&text, vigia::BACKGROUND_WAIT);
}

#[test]
fn answered_ask() {
    let text = asked(Ask {
        answer: Some(b"\x1b]11;rgb:0000/0000/0000\x1b\\"),
        ..Ask::default()
    });
    assert_typed(&text);
    assert!(
        pty::asked_line(&text).contains("]11;rgb:"),
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
    assert_within(&text, vigia::BACKGROUND_WAIT);
}

#[test]
fn piped_answered_ask() {
    // On macOS the query falls back to `/dev/tty`, which `poll` cannot see.
    let text = asked(Ask {
        answer: Some(b"\x1b]11;rgb:0000/0000/0000\x1b\\"),
        piped: true,
        ..Ask::default()
    });
    assert_typed(&text);
    assert!(
        pty::asked_line(&text).contains("]11;rgb:"),
        "an answer through /dev/tty was not read: {text}"
    );
    assert_within(&text, vigia::BACKGROUND_WAIT);
}

#[test]
fn detached_answered_ask() {
    // As under `setsid`: the terminal on stdio, and no `/dev/tty` to open.
    let text = asked(Ask {
        answer: Some(b"\x1b]11;rgb:0000/0000/0000\x1b\\"),
        detached: true,
        ..Ask::default()
    });
    assert_typed(&text);
    assert!(
        pty::asked_line(&text).contains("]11;rgb:"),
        "an answer on stdin was not read without a controlling terminal: {text}"
    );
}

#[test]
fn devtty_stdin_blocking() {
    // macOS cannot poll `/dev/tty`, and stdin shares this one's flags.
    let text = asked(Ask {
        answer: Some(b"\x1b]11;rgb:0000/0000/0000\x1b\\"),
        devtty: true,
        ..Ask::default()
    });
    assert_typed(&text);
    let line = pty::asked_line(&text);
    assert!(
        line.contains("]11;rgb:") && line.contains("blocking=true"),
        "stdin was left non-blocking, or the answer was lost: {line}"
    );
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
    assert_within(&text, vigia::ANSWERING);
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
    /// The child has no controlling terminal.
    detached: bool,
    /// The child's stdin is `/dev/tty` itself.
    devtty: bool,
}

fn assert_typed(text: &str) {
    assert!(
        text.contains("got z"),
        "the key after the query never reached the reader: {text}"
    );
}

fn assert_within(text: &str, bound: Duration) {
    let waited = pty::waited_ms(text);
    assert!(
        waited < (bound + SLACK).as_secs_f64() * 1000.0,
        "the query waited {waited}ms, past its {bound:?}: {}",
        pty::asked_line(text)
    );
}

/// Run the child in a fresh terminal behaving as `ask` says.
fn asked(ask: Ask) -> String {
    let mut child = Command::new(std::env::current_exe().expect("this test"));
    child
        .args(["--exact", "ask_child", "--nocapture", "--test-threads=1"])
        .env(CHILD, "1");
    if ask.palette {
        child.env(PALETTE, "1");
    }
    if ask.devtty {
        child.env(DEVTTY, "1");
    }
    pty::run(
        child,
        pty::Script {
            marker: "]11;?",
            answer: ask.answer,
            delay: Duration::ZERO,
            piped: ask.piped,
            detached: ask.detached,
            quiet: false,
        },
    )
}
