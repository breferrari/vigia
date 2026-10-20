//! The terminal writer, against the real binary in a terminal that reads slowly
//! or not at all. `SPEC.md` §6, the terminal writer, and I8.
#![cfg(unix)]

#[path = "pty/mod.rs"]
mod pty;
#[path = "../../vigia-core/tests/support/mod.rs"]
mod support;

use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use support::{Scratch, TempDir};

/// Set in the child this file starts as itself.
const CHILD: &str = "VIGIA_WRITER_CHILD";

/// The end of a frame: ratatui hides the cursor after every draw.
const FRAME_END: &str = "\x1b[?25l";

/// What the restore ends the alternate screen with.
const LEAVE: &str = "\x1b[?1049l";

/// A repository whose diff is far taller than the pane. Each line is one
/// letter twelve times, the next letter on the next line, so a scroll of any
/// number of rows not a multiple of 26 rewrites every cell of every row.
fn tall(name: &str) -> Scratch {
    wide(name, 12)
}

/// [`tall`], with lines `width` letters long.
fn wide(name: &str, width: usize) -> Scratch {
    let scratch = Scratch::new(name);
    let lines = |n: usize| {
        (0..n)
            .map(|i| {
                format!(
                    "{}\n",
                    char::from(b'a' + (i % 26) as u8).to_string().repeat(width)
                )
            })
            .collect::<String>()
    };
    scratch.write("a.txt", "");
    scratch.commit_all("baseline");
    scratch.write("a.txt", lines(3000));
    scratch
}

/// The real binary on `scratch`, with nothing of the reader's around it.
fn pane(scratch: &Scratch, home: &TempDir) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_vigia"));
    command
        .arg(scratch.root())
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join("config"))
        .env("XDG_STATE_HOME", home.path().join("state"))
        .env("TERM", "xterm-256color")
        .env("VIGIA_UPDATE", "off")
        .env("VIGIA_PICTURES", "halfblocks")
        .env_remove("TMUX");
    command
}

fn script() -> pty::Script<'static> {
    pty::Script {
        quiet: true,
        size: Some((100, 40, 1000, 800)),
        ..pty::Script::default()
    }
}

/// What the terminal showed, read by a thread of its own.
type Seen = Arc<Mutex<Vec<(Instant, Vec<u8>)>>>;

/// Everything the terminal shows, read on a thread at `rate` bytes a second,
/// or as fast as it comes with `None`.
fn reader(master: File, rate: Option<u64>) -> Seen {
    paused_reader(master, rate, Arc::default())
}

/// [`reader`], reading nothing while `paused` is set.
fn paused_reader(mut master: File, rate: Option<u64>, paused: Arc<AtomicBool>) -> Seen {
    let seen: Seen = Arc::default();
    let into = Arc::clone(&seen);
    std::thread::spawn(move || {
        let mut buffer = vec![0u8; if rate.is_some() { 64 } else { 65536 }];
        let mut due = Instant::now();
        loop {
            if paused.load(Ordering::Acquire) {
                std::thread::sleep(Duration::from_millis(2));
                continue;
            }
            let Ok(read) = master.read(&mut buffer) else {
                return;
            };
            if read == 0 {
                return;
            }
            into.lock()
                .expect("seen")
                .push((Instant::now(), buffer[..read].to_vec()));
            if let Some(rate) = rate {
                due = due.max(Instant::now()) + Duration::from_secs_f64(read as f64 / rate as f64);
                if let Some(wait) = due.checked_duration_since(Instant::now()) {
                    std::thread::sleep(wait);
                }
            }
        }
    });
    seen
}

fn text(seen: &Seen) -> String {
    let bytes: Vec<u8> = seen
        .lock()
        .expect("seen")
        .iter()
        .flat_map(|(_, b)| b.clone())
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Whether the restore at the end of `text` began by ending any escape string
/// a frame left open.
fn restored_after_terminator(text: &str) -> bool {
    let Some(leave) = text.rfind(LEAVE) else {
        return false;
    };
    text[..leave]
        .rfind("\x1b\\")
        .is_some_and(|end| leave - end < 200)
}

#[test]
fn quit_bounded() {
    // The terminal reads nothing: the pane's writer waits on it, the quit key
    // is read anyway, and the frames still queued are dropped rather than
    // written. The kernel holds the process's last step until its terminal has
    // read what it already took, so the end is measured from when reading
    // resumes: it must come at once, with little left to read.
    let (scratch, home) = (tall("writer-quit"), TempDir::new("writer-quit-home"));
    let (mut child, mut master) = pty::start(pane(&scratch, &home), &script());
    std::thread::sleep(Duration::from_secs(2));
    for _ in 0..30 {
        master.write_all(b"j").expect("press j");
    }
    master.write_all(b"q").expect("press q");
    std::thread::sleep(Duration::from_secs(1));
    let resumed = Instant::now();
    let seen = reader(master.try_clone().expect("the terminal's other end"), None);
    let status = child.exited_unread(Duration::from_secs(3));
    assert!(
        status.is_some(),
        "the pane did not end once its terminal read again"
    );
    assert!(
        resumed.elapsed() < Duration::from_millis(500),
        "the pane took {:?} to end after its terminal read again, so it was still writing its queue",
        resumed.elapsed()
    );
    std::thread::sleep(Duration::from_millis(100));
    let left = text(&seen).len();
    assert!(
        left < 16 * 1024,
        "{left} bytes reached the terminal after the quit, so the queue was written rather than dropped"
    );
    drop(master);
}

#[test]
fn signal_restores() {
    for (name, signal) in [("SIGTERM", libc::SIGTERM), ("SIGHUP", libc::SIGHUP)] {
        let (scratch, home) = (tall("writer-signal"), TempDir::new("writer-signal-home"));
        let (mut child, mut master) = pty::start(pane(&scratch, &home), &script());
        let reading = master.try_clone().expect("the terminal's other end");
        // Unread, with keys behind it, so the writer is part way through a frame
        // when the signal lands.
        std::thread::sleep(Duration::from_secs(2));
        for _ in 0..30 {
            master.write_all(b"j").expect("press j");
        }
        std::thread::sleep(Duration::from_millis(300));
        // SAFETY: a pid this test started, and a signal number from libc.
        unsafe { libc::kill(child.pid() as i32, signal) };
        let began = Instant::now();
        let seen = reader(reading, None);
        let status = child.exited_unread(Duration::from_secs(3));
        assert!(status.is_some(), "{name} did not end the pane");
        // The pane's own part is at most 500 ms: the writer's 200 ms stop and the
        // restore's 300 ms deadline. The rest is the runner's.
        assert!(
            began.elapsed() < Duration::from_secs(1),
            "{name} took {:?} to end a pane whose terminal was behind, past the 500 ms \
             the writer's stop and the restore's deadline allow, plus 500 ms for the runner",
            began.elapsed()
        );
        std::thread::sleep(Duration::from_millis(300));
        let shown = text(&seen);
        assert!(
            restored_after_terminator(&shown),
            "after {name} the terminal was not restored, or the restore did not end an open escape string first"
        );
        drop(master);
    }
}

#[test]
fn writer_child() {
    let Some(which) = std::env::var_os(CHILD) else {
        return;
    };
    match which.to_string_lossy().as_ref() {
        "panic" => vigia::panic_mid_frame(),
        "flags" => {
            let (before, during) = vigia::output_flags();
            println!("flags before={before} during={during}");
        }
        other => panic!("no child {other}"),
    }
}

fn child(which: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().expect("this test"));
    command
        .args(["--exact", "writer_child", "--nocapture", "--test-threads=1"])
        .env(CHILD, which);
    command
}

#[test]
fn panic_restores() {
    // Read slowly, so the writer is inside the image when the panic lands.
    let (mut running, master) = pty::start(child("panic"), &pty::Script::default());
    let seen = reader(
        master.try_clone().expect("the terminal's other end"),
        Some(64_000),
    );
    assert!(
        running.exited_unread(Duration::from_secs(10)).is_some(),
        "the child did not end"
    );
    std::thread::sleep(Duration::from_millis(500));
    let shown = text(&seen);
    let opened = shown.find("\x1b_Ga=t").expect("the frame never started");
    let sent = shown.matches('A').count();
    assert!(
        sent < 300_000,
        "the writer kept writing the frame after the panic: {sent} of 300000 bytes"
    );
    let after = &shown[opened..];
    assert!(
        restored_after_terminator(after),
        "the restore after a panic mid-image did not end the image's escape string first"
    );
    drop(master);
}

#[test]
fn stdout_flags() {
    let (mut running, mut master) = pty::start(child("flags"), &pty::Script::default());
    let mut seen = Vec::new();
    let _ = running.finish(&mut master, &mut seen, Duration::from_secs(10));
    let shown = String::from_utf8_lossy(&seen);
    assert!(
        shown.contains("flags before=false during=false"),
        "the writer's descriptor changed standard output's flags: {shown}"
    );
}

/// How long [`skipped_draw`]'s lines are.
const RUN: usize = 200;

#[test]
fn skipped_draw() {
    // Keys arrive while the terminal reads nothing. They are applied as they
    // come, the frames for them are held, and on release a few frames show the
    // newest state rather than one frame per key. The frames are wide so the
    // kernel holds only a couple: a Linux pty holds about 20 KB.
    let (scratch, home) = (wide("writer-skip", RUN), TempDir::new("writer-skip-home"));
    let script = pty::Script {
        size: Some((250, 60, 2500, 1200)),
        ..script()
    };
    let (mut child, mut master) = pty::start(pane(&scratch, &home), &script);
    let paused = Arc::new(AtomicBool::new(false));
    let seen = paused_reader(
        master.try_clone().expect("the terminal's other end"),
        None,
        Arc::clone(&paused),
    );
    std::thread::sleep(Duration::from_secs(2));
    paused.store(true, Ordering::Release);
    std::thread::sleep(Duration::from_millis(50));
    for _ in 0..40 {
        master.write_all(b"j").expect("press j");
        std::thread::sleep(Duration::from_millis(10));
    }
    std::thread::sleep(Duration::from_millis(200));
    let released = Instant::now();
    paused.store(false, Ordering::Release);
    std::thread::sleep(Duration::from_millis(400));
    master.write_all(b"q").expect("press q");
    assert!(
        child.exited_unread(Duration::from_secs(3)).is_some(),
        "the pane did not end"
    );
    let after: Vec<u8> = seen
        .lock()
        .expect("seen")
        .iter()
        .filter(|(at, _)| *at >= released)
        .flat_map(|(_, bytes)| bytes.clone())
        .collect();
    let after = String::from_utf8_lossy(&after);
    let frames = after.matches(FRAME_END).count();
    assert!(
        frames <= 10,
        "forty keys held behind a terminal that was not reading drew {frames} frames on \
         release, where the newest state and the scroll mark's own frames are all there should be"
    );
    let rows = (b'a'..=b'z')
        .map(|letter| {
            after
                .matches(&char::from(letter).to_string().repeat(RUN))
                .count()
        })
        .sum::<usize>();
    assert!(
        rows >= 20,
        "{rows} rows were redrawn on release, so the scroll the held keys made was not applied"
    );
}

#[test]
fn resize_while_held() {
    // The window narrows while frames are held back: on release the screen is
    // drawn for the new width, not the one the held frames were laid out for.
    let (scratch, home) = (
        wide("writer-resize", RUN),
        TempDir::new("writer-resize-home"),
    );
    let script = pty::Script {
        size: Some((250, 60, 2500, 1200)),
        ..script()
    };
    let (mut child, mut master) = pty::start(pane(&scratch, &home), &script);
    let paused = Arc::new(AtomicBool::new(false));
    let seen = paused_reader(
        master.try_clone().expect("the terminal's other end"),
        None,
        Arc::clone(&paused),
    );
    std::thread::sleep(Duration::from_secs(2));
    paused.store(true, Ordering::Release);
    std::thread::sleep(Duration::from_millis(50));
    for _ in 0..20 {
        master.write_all(b"j").expect("press j");
        std::thread::sleep(Duration::from_millis(10));
    }
    let narrow = libc::winsize {
        ws_row: 40,
        ws_col: 120,
        ws_xpixel: 1200,
        ws_ypixel: 800,
    };
    // SAFETY: a descriptor this test owns, and a valid winsize.
    unsafe { libc::ioctl(master.as_raw_fd(), libc::TIOCSWINSZ, &narrow) };
    for _ in 0..20 {
        master.write_all(b"j").expect("press j");
        std::thread::sleep(Duration::from_millis(10));
    }
    std::thread::sleep(Duration::from_millis(200));
    paused.store(false, Ordering::Release);
    std::thread::sleep(Duration::from_millis(600));
    master.write_all(b"q").expect("press q");
    let status = child.exited_unread(Duration::from_secs(3));
    assert!(
        status.is_some_and(|status| status.success()),
        "the pane did not end cleanly after a resize while held: {status:?}"
    );
    let shown = text(&seen);
    let cleared = shown
        .rfind("\x1b[2J")
        .expect("the resize never cleared the screen");
    let last = &shown[cleared..];
    let full = (b'a'..=b'z')
        .map(|letter| {
            last.matches(&char::from(letter).to_string().repeat(RUN))
                .count()
        })
        .sum::<usize>();
    let narrowed = (b'a'..=b'z')
        .map(|letter| {
            last.matches(&char::from(letter).to_string().repeat(80))
                .count()
        })
        .sum::<usize>();
    assert_eq!(
        full, 0,
        "after the resize a row was drawn {RUN} wide on a screen 120 wide"
    );
    assert!(
        narrowed >= 20,
        "{narrowed} rows were drawn after the resize, so the narrow screen was never drawn"
    );
}

#[test]
fn locked_terminal() {
    // The terminal's device cannot be opened again by name: the pane writes
    // through the standard output it was given instead of refusing to start.
    // SAFETY: no arguments, no failure.
    if unsafe { libc::geteuid() } == 0 {
        eprintln!("locked_terminal: root opens a locked device, so the fallback cannot be reached");
        return;
    }
    let (scratch, home) = (tall("writer-locked"), TempDir::new("writer-locked-home"));
    let script = pty::Script {
        locked: true,
        ..script()
    };
    let (mut child, mut master) = pty::start(pane(&scratch, &home), &script);
    let seen = reader(master.try_clone().expect("the terminal's other end"), None);
    let until = Instant::now() + Duration::from_secs(10);
    while !text(&seen).contains(FRAME_END) && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(20));
    }
    // Past the startup queries, which would read the key as their answer.
    std::thread::sleep(Duration::from_secs(1));
    master.write_all(b"q").expect("press q");
    let status = child.exited_unread(Duration::from_secs(3));
    assert!(
        status.is_some_and(|status| status.success()),
        "the pane on a terminal it could not open again did not run and quit: {status:?}"
    );
    let shown = text(&seen);
    assert!(
        shown.contains(FRAME_END) && shown.contains(&"a".repeat(12)),
        "the pane on a terminal it could not open again drew nothing"
    );
}

#[test]
fn output_lost() {
    // Input on one terminal and output on another, so closing the output's end
    // can end the pane only through the writer.
    let (scratch, home) = (tall("writer-lost"), TempDir::new("writer-lost-home"));
    let (mut child, mut input, mut output, mut stderr) = pty::split(pane(&scratch, &home));
    // SAFETY: a descriptor this test owns.
    unsafe { libc::fcntl(output.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK) };
    let mut seen = Vec::new();
    let mut buffer = [0u8; 65536];
    // Past the startup queries too, which would read the keys as their answer.
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(2) {
        match output.read(&mut buffer) {
            Ok(n) if n > 0 => seen.extend_from_slice(&buffer[..n]),
            _ => std::thread::sleep(Duration::from_millis(5)),
        }
    }
    assert!(text_of(&seen).contains(FRAME_END), "the pane never drew");
    drop(output);
    for _ in 0..5 {
        let _ = input.write_all(b"j");
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        child.exited_unread(Duration::from_secs(3)).is_some(),
        "the pane went on with no terminal to draw on"
    );
    let mut said = String::new();
    let _ = stderr.read_to_string(&mut said);
    assert!(
        said.contains("stopped taking output"),
        "the pane did not say its output was lost: {said:?}"
    );
}

fn text_of(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn query_skipped() {
    // A terminal that answers the background query and then reads nothing
    // while the first frame waits: the picture query is not sent at all, since
    // it would land inside that frame.
    let (scratch, home) = (
        wide("writer-unasked", 280),
        TempDir::new("writer-unasked-home"),
    );
    let mut command = pane(&scratch, &home);
    command.env_remove("VIGIA_PICTURES");
    let script = pty::Script {
        size: Some((300, 100, 3000, 2000)),
        ..script()
    };
    let (mut child, mut master) = pty::start(command, &script);
    let mut seen = Vec::new();
    let mut buffer = [0u8; 65536];
    let until = Instant::now() + Duration::from_secs(10);
    while !String::from_utf8_lossy(&seen).contains("\x1b]11;?") && Instant::now() < until {
        if let Ok(read) = master.read(&mut buffer) {
            seen.extend_from_slice(&buffer[..read]);
        }
    }
    master
        .write_all(b"\x1b]11;rgb:1e1e/1e1e/2e2e\x1b\\")
        .expect("answer");
    // Past the writer's wait for the first frame, unread.
    std::thread::sleep(Duration::from_millis(1500));
    let rest = reader(master.try_clone().expect("the terminal's other end"), None);
    std::thread::sleep(Duration::from_secs(2));
    master.write_all(b"q").expect("press q");
    let _ = child.exited_unread(Duration::from_secs(3));
    let mut shown = String::from_utf8_lossy(&seen).into_owned();
    shown.push_str(&text(&rest));
    assert!(
        shown.contains(FRAME_END),
        "the pane never drew, so this proves nothing"
    );
    assert!(
        !shown.contains("\x1b_Gi=31"),
        "the picture query went out though the first frame had not reached the terminal"
    );
}

#[test]
fn query_after_drain() {
    // A terminal that answers the background query, so the picture query goes
    // out after the first paint: its bytes must start after a frame has ended.
    let (scratch, home) = (tall("writer-query"), TempDir::new("writer-query-home"));
    let mut command = pane(&scratch, &home);
    command.env_remove("VIGIA_PICTURES");
    let (mut child, mut master) = pty::start(command, &script());
    let mut seen = Vec::new();
    let mut buffer = [0u8; 65536];
    let until = Instant::now() + Duration::from_secs(10);
    let mut answered = (false, false);
    while Instant::now() < until {
        let mut fds = libc::pollfd {
            fd: master.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: one valid pollfd.
        if unsafe { libc::poll(&mut fds, 1, 50) } <= 0 {
            if answered.1 {
                break;
            }
            continue;
        }
        let Ok(read) = master.read(&mut buffer) else {
            break;
        };
        seen.extend_from_slice(&buffer[..read]);
        let shown = String::from_utf8_lossy(&seen);
        if !answered.0 && shown.contains("\x1b]11;?") {
            answered.0 = true;
            master
                .write_all(b"\x1b]11;rgb:1e1e/1e1e/2e2e\x1b\\")
                .expect("answer");
        }
        if !answered.1 && shown.contains("\x1b_Gi=31") {
            answered.1 = true;
            master.write_all(b"\x1b[0n").expect("answer the status");
        }
    }
    master.write_all(b"q").expect("press q");
    let _ = child.exited_unread(Duration::from_secs(3));
    let shown = String::from_utf8_lossy(&seen).into_owned();
    let query = shown
        .find("\x1b_Gi=31")
        .expect("the picture query was never sent");
    assert!(
        shown[..query].ends_with(FRAME_END),
        "the picture query started inside a frame: {:?}",
        &shown[query.saturating_sub(80)..query]
    );
}
