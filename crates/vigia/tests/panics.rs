//! A picture that panics draws as undecoded, and the panic hook leaves the
//! picture worker alone and aborts everywhere else. `SPEC.md` §11.1, I8.

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

/// A view of `frame` on a 100x30 pane, after every queued picture is built.
fn settled(app: &mut App, pictures: &mut Pictures, frame: &mut Frame) -> View {
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
    assert!(
        pictures.settle(Duration::from_secs(60)),
        "the pictures were not built"
    );
    let mut view = app
        .view(frame, &mut highlighter, &history, body)
        .expect("view");
    pictures.attach(&mut view.rows, frame);
    view
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

/// Set in the child this file starts in a fresh terminal: which thread panics.
#[cfg(unix)]
const CHILD: &str = "VIGIA_PANIC_CHILD";

/// The child's half: install `run`'s hook, then panic on the thread the
/// parent asked for.
#[cfg(unix)]
#[test]
fn panic_child() {
    use std::io::Write;
    let Some(which) = std::env::var_os(CHILD) else {
        return;
    };
    vigia::install_panic_hook();
    if which == "worker" {
        let caught = std::thread::Builder::new()
            .name("vigia-pictures".to_owned())
            .spawn(|| std::panic::catch_unwind(|| panic!("a worker on purpose")).is_err())
            .expect("spawn")
            .join()
            .expect("join");
        println!("survived caught={caught}");
        std::io::stdout().flush().expect("flush");
    } else {
        panic!("the main thread on purpose");
    }
}

#[cfg(unix)]
fn child(which: &str) -> std::process::Command {
    let mut child = std::process::Command::new(std::env::current_exe().expect("this test"));
    child
        .args(["--exact", "panic_child", "--nocapture", "--test-threads=1"])
        .env(CHILD, which);
    child
}

#[cfg(unix)]
#[test]
fn worker_panic_keeps_screen() {
    let (mut running, mut master) = pty::start(child("worker"), &pty::Script::default());
    let mut seen = Vec::new();
    pty::read_until(&mut master, &mut seen, "survived", Duration::from_secs(20));
    let status = running
        .status(Duration::from_secs(10))
        .expect("the child ended");
    let text = String::from_utf8_lossy(&seen);
    assert!(
        text.contains("caught=true") && status.success(),
        "a worker's panic ended the process: {status:?} {text}"
    );
    assert!(
        !text.contains("\x1b[?1049l") && !text.contains("a worker on purpose"),
        "a worker's panic restored the terminal or printed over the pane: {text}"
    );
}

#[cfg(unix)]
#[test]
fn main_panic_restores() {
    use std::os::unix::process::ExitStatusExt;
    let (mut running, mut master) = pty::start(child("main"), &pty::Script::default());
    let mut seen = Vec::new();
    pty::read_until(
        &mut master,
        &mut seen,
        "the main thread on purpose",
        Duration::from_secs(20),
    );
    let status = running
        .status(Duration::from_secs(10))
        .expect("the child ended");
    let text = String::from_utf8_lossy(&seen);
    let restore = text.find("\x1b[?1049l");
    let message = text.find("the main thread on purpose");
    assert!(
        matches!((restore, message), (Some(r), Some(m)) if r < m),
        "the terminal was not restored before the panic message: {text:?}"
    );
    assert_eq!(
        status.signal(),
        Some(libc::SIGABRT),
        "a main-thread panic did not end in an abort after its message: {status:?}"
    );
}
