//! A picture that panics draws as undecoded, and the panic hooks leave a panic
//! to the catch waiting for it and abort on every other. `SPEC.md` §11.1, I8.

#[cfg(unix)]
#[path = "pty/mod.rs"]
mod pty;
#[path = "../../vigia-core/tests/support/mod.rs"]
mod support;

use std::time::Duration;

use ratatui::layout::Rect;
use ratatui_image::picker::Picker;
use support::{Scratch, png};
use vigia::pictures::{self, Inks, Pictures, Stage};
use vigia::{App, Pointing, Row, View, body_layout};
use vigia_core::{Frame, Highlighter, History, Standing};

const INKS: Inks = Inks {
    canvas: (0x0d, 0x11, 0x17),
    changed: (0xf8, 0x51, 0x49),
};

/// A view of `frame` on a 100x30 pane, with its pictures attached.
fn view_of(app: &mut App, pictures: &mut Pictures, frame: &mut Frame) -> View {
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let chrome = app.chrome(
        "fixture",
        None,
        vigia::Stood {
            standing: &Standing::Current,
            now: 0,
        },
        Pointing::default(),
        Default::default(),
        "",
    );
    let body = body_layout(
        Rect::new(0, 0, 100, 30),
        &chrome,
        frame.files().len(),
        vigia::list_rows_wanted(frame.files()),
    );
    let mut view = app
        .view(frame, &mut highlighter, &history, body)
        .expect("view");
    pictures.attach(&mut view.rows, frame);
    view
}

/// A view of `frame` on a 100x30 pane, after every queued picture is built.
fn settled(app: &mut App, pictures: &mut Pictures, frame: &mut Frame) -> View {
    view_of(app, pictures, frame);
    assert!(
        pictures.settle(Duration::from_secs(60)),
        "the pictures were not built"
    );
    view_of(app, pictures, frame)
}

/// The size line's suffix and how many panels were built.
fn outcome(view: &View) -> (Option<String>, usize) {
    view.rows
        .iter()
        .find_map(|row| match row {
            Row::Sized { ready, .. } => Some((
                ready.as_ref().and_then(|ready| ready.0.suffix()),
                ready.as_ref().map_or(0, |ready| ready.0.panels.len()),
            )),
            _ => None,
        })
        .expect("a size line")
}

/// One changed PNG, its picture built by a fresh worker.
fn picture(name: &str, seed: u8) -> (Option<String>, usize) {
    let scratch = Scratch::new(name);
    scratch.write("screen.png", png(120, 260, seed));
    scratch.commit_all("baseline");
    scratch.write("screen.png", png(120, 260, seed.wrapping_add(1)));
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut app = App::new();
    app.draw_pictures(true);
    let mut pictures = Pictures::new(INKS, Picker::halfblocks(), Box::new(|| {}));
    outcome(&settled(&mut app, &mut pictures, &mut frame))
}

/// One test, because the seam is process-wide: in parallel, one test's armed
/// panic could land in another's picture.
#[test]
fn panics_draw_undecoded() {
    pictures::panic_next(Stage::Decode);
    let (said, panels) = picture("panic-decode", 1);
    assert_eq!(
        (said.as_deref(), panels),
        (Some("could not be decoded"), 0),
        "a decoder's panic was not drawn as undecoded"
    );

    pictures::panic_next(Stage::Encode);
    let (said, panels) = picture("panic-encode", 3);
    assert_eq!(
        (said.as_deref(), panels),
        (Some("could not be decoded"), 0),
        "an encoder's panic was not drawn as undecoded"
    );

    // The same worker goes on to the next picture after a caught panic.
    let scratch = Scratch::new("panic-then-next");
    scratch.write("screen.png", png(120, 260, 5));
    scratch.commit_all("baseline");
    scratch.write("screen.png", png(120, 260, 6));
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut app = App::new();
    app.draw_pictures(true);
    let mut pictures = Pictures::new(INKS, Picker::halfblocks(), Box::new(|| {}));
    pictures::panic_next(Stage::Decode);
    let (said, _) = outcome(&settled(&mut app, &mut pictures, &mut frame));
    assert_eq!(said.as_deref(), Some("could not be decoded"));
    // A new picture for the same worker. Its stamp reads the file's time,
    // which some filesystems keep to the second, so the time is set.
    scratch.write("screen.png", png(120, 260, 9));
    std::fs::File::options()
        .write(true)
        .open(scratch.path_of("screen.png"))
        .expect("open")
        .set_modified(std::time::SystemTime::now() + Duration::from_secs(10))
        .expect("set the time");
    frame.advance().expect("advance");
    let (_, panels) = outcome(&settled(&mut app, &mut pictures, &mut frame));
    assert_eq!(
        panels, 3,
        "after one picture panicked the next was not built"
    );
}

/// Set in the child this file starts in a fresh terminal: what it does.
#[cfg(unix)]
const CHILD: &str = "VIGIA_PANIC_CHILD";

/// The child's half: install the hooks `main` and `run` install, then panic
/// where the parent asked.
#[cfg(unix)]
#[test]
fn panic_child() {
    use std::io::Write;
    let Some(which) = std::env::var_os(CHILD) else {
        return;
    };
    let which = which.to_string_lossy().into_owned();
    if which == "double" {
        // A panic while a caught one unwinds: no catch can take it.
        struct Again;
        impl Drop for Again {
            fn drop(&mut self) {
                panic!("a second panic on purpose");
            }
        }
        vigia::install_panic_hook();
        vigia_core::caught(|| {
            let _again = Again;
            panic!("a caught panic on purpose");
        });
        println!("survived");
        return;
    }
    if which == "main" {
        // The pane's hook alone: it must end the process without `main`'s.
        vigia::install_panic_hook();
        panic!("the main thread on purpose");
    }
    vigia::install_abort_hook();
    if which == "caught-base" {
        // The base hook alone, as in every mode before the pane's goes on.
        let caught = vigia_core::caught(|| panic!("a caught panic on purpose")).is_none();
        println!("survived caught={caught}");
        return;
    }
    if which == "thread" {
        // As in `vigia mcp`: the base hook alone, on a thread of its own.
        let _ = std::thread::spawn(|| panic!("a server thread on purpose")).join();
        println!("survived");
        return;
    }
    vigia::install_panic_hook();
    match which.as_str() {
        "caught" => {
            let caught = std::thread::spawn(|| {
                vigia_core::caught(|| panic!("a caught panic on purpose")).is_none()
            })
            .join()
            .expect("join");
            println!("survived caught={caught}");
        }
        "header" => {
            // Past the first header read, so a retry with more bytes would
            // find the header that panicked the first time.
            let scratch = Scratch::new("panic-header-child");
            scratch.write("screen.png", support::screen_png(1));
            scratch.commit_all("baseline");
            scratch.write("screen.png", support::screen_png(2));
            let worktree = scratch.worktree();
            let mut frame = worktree.frame();
            pictures::panic_next(Stage::Header);
            frame.advance().expect("advance");
            let mut app = App::new();
            let mut pictures = Pictures::new(INKS, Picker::halfblocks(), Box::new(|| {}));
            let view = view_of(&mut app, &mut pictures, &mut frame);
            let binary = view
                .rows
                .iter()
                .any(|row| matches!(row, Row::Reason(line) if line == "binary"));
            println!("survived binary={binary}");
        }
        "outside" => {
            let scratch = Scratch::new("panic-outside-child");
            scratch.write("screen.png", png(120, 260, 1));
            scratch.commit_all("baseline");
            scratch.write("screen.png", png(120, 260, 2));
            let worktree = scratch.worktree();
            let mut frame = worktree.frame();
            frame.advance().expect("advance");
            let mut app = App::new();
            app.draw_pictures(true);
            let mut pictures = Pictures::new(INKS, Picker::halfblocks(), Box::new(|| {}));
            pictures::panic_next(Stage::Worker);
            let _ = view_of(&mut app, &mut pictures, &mut frame);
            pictures.settle(Duration::from_secs(20));
            println!("survived");
        }
        _ => panic!("the main thread on purpose"),
    }
    std::io::stdout().flush().expect("flush");
}

#[cfg(unix)]
fn child(which: &str) -> std::process::Command {
    let mut child = std::process::Command::new(std::env::current_exe().expect("this test"));
    child
        .args(["--exact", "panic_child", "--nocapture", "--test-threads=1"])
        .env(CHILD, which);
    child
}

/// Run the child `which` to its end: what the terminal showed, and how it ended.
#[cfg(unix)]
fn ran(which: &str, script: pty::Script<'_>) -> (String, std::process::ExitStatus) {
    let (mut running, mut master) = pty::start(child(which), &script);
    let mut seen = Vec::new();
    let status = running
        .finish(&mut master, &mut seen, Duration::from_secs(30))
        .expect("the child ended");
    (String::from_utf8_lossy(&seen).into_owned(), status)
}

#[cfg(unix)]
fn aborted(status: std::process::ExitStatus) -> bool {
    use std::os::unix::process::ExitStatusExt;
    status.signal() == Some(libc::SIGABRT)
}

#[cfg(unix)]
#[test]
fn caught_panic_keeps_screen() {
    let (text, status) = ran("caught", pty::Script::default());
    assert!(
        text.contains("survived caught=true") && status.success(),
        "a caught panic ended the process: {status:?} {text}"
    );
    assert!(
        !text.contains("\x1b[?1049l") && !text.contains("a caught panic on purpose"),
        "a caught panic restored the terminal or printed over the pane: {text}"
    );
}

#[cfg(unix)]
#[test]
fn caught_panic_base_hook() {
    let (text, status) = ran("caught-base", pty::Script::default());
    assert!(
        text.contains("survived caught=true") && status.success(),
        "the base hook ended the process on a panic a catch was waiting for: {status:?} {text}"
    );
}

#[cfg(unix)]
#[test]
fn frame_header_panic() {
    let (text, status) = ran("header", pty::Script::default());
    assert!(
        text.contains("survived binary=true") && status.success(),
        "a header's panic on the frame thread ended the pane, or the file did not \
         draw as binary: {status:?} {text}"
    );
    assert!(
        !text.contains("\x1b[?1049l"),
        "a header's panic restored the terminal: {text}"
    );
}

#[cfg(unix)]
#[test]
fn uncaught_worker_aborts() {
    let (text, status) = ran("outside", pty::Script::default());
    assert!(
        aborted(status) && !text.contains("survived"),
        "a panic on the worker outside its catch did not end the process: {status:?} {text}"
    );
    assert!(
        text.contains("a test asked Worker to panic"),
        "the worker's panic was not reported: {text}"
    );
}

#[cfg(unix)]
#[test]
fn main_panic_restores() {
    let (text, status) = ran("main", pty::Script::default());
    let restore = text.find("\x1b[?1049l");
    let message = text.find("the main thread on purpose");
    assert!(
        matches!((restore, message), (Some(r), Some(m)) if r < m),
        "the terminal was not restored before the panic message: {text:?}"
    );
    assert!(
        aborted(status),
        "a main-thread panic did not end in an abort after its message: {status:?}"
    );
}

#[cfg(unix)]
#[test]
fn server_thread_aborts() {
    // As in `vigia mcp`: stdout alone, which it speaks JSON on.
    let (stdout, status) = ran(
        "thread",
        pty::Script {
            quiet: true,
            ..pty::Script::default()
        },
    );
    assert!(
        aborted(status) && !stdout.contains("survived"),
        "a background thread's panic left the process running: {status:?} {stdout}"
    );
    assert!(
        !stdout.contains('\x1b') && !stdout.contains("a server thread on purpose"),
        "the panic wrote to stdout: {stdout:?}"
    );
    // And with stderr: the panic is still reported before the abort.
    let (both, _) = ran("thread", pty::Script::default());
    assert!(
        both.contains("a server thread on purpose"),
        "the base hook did not report the panic: {both}"
    );
}

#[cfg(unix)]
#[test]
fn double_panic_restores() {
    let (text, status) = ran("double", pty::Script::default());
    assert!(
        text.contains("\x1b[?1049l") && !text.contains("survived"),
        "a second panic inside a catch did not restore the terminal: {text:?}"
    );
    assert!(
        aborted(status),
        "a second panic did not end in an abort: {status:?}"
    );
}

/// `main` installs the base hook before it does anything else.
#[test]
fn main_installs_abort_hook() {
    let main = include_str!("../src/main.rs");
    let hook = main.find("vigia::install_abort_hook();");
    let dispatch = main.find("match vigia::request_for(");
    assert!(
        matches!((hook, dispatch), (Some(h), Some(d)) if h < d),
        "main does not install the base panic hook before any mode runs"
    );
}
