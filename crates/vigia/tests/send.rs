//! `SPEC.md` §11.1: what the button coming up puts on the clipboard, and what the
//! footer says about it.
//!
//! An OSC 52 write draws nothing, so the whole rendering suite stays green
//! whichever way this behaves. Everything here is what no drawn cell can show.

#[path = "../../vigia-core/tests/support/mod.rs"]
mod support;

use ratatui::layout::Rect;
use vigia::{App, Pointing, View, body_layout};
use vigia_core::{Frame, Highlighter, History};

use support::{Scratch, materialise};

/// Deep enough that a heading has to be elided at the width I6 names, so the path
/// the release sends is one no cell holds.
const DEEP: &str = "crates/vigia-core/src/very/deeply/nested/module/frame.rs";

/// The width I6 is named for.
const NARROW: u16 = 40;

/// A worktree holding one deeply nested file. The `Scratch` is returned rather
/// than the `Frame`, because a frame borrows the worktree it walks.
fn deep_scratch(name: &str) -> Scratch {
    let scratch = Scratch::new(name);
    scratch.write(DEEP, "one\ntwo\nthree\n");
    scratch
}

/// Paint one frame, which is what the release then resolves its payload against.
fn painted(app: &mut App, frame: &mut Frame) -> View {
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let chrome = app.chrome("fixture", None, Pointing::default(), 0, "");
    let body = body_layout(Rect::new(0, 0, NARROW, 24), &chrome, 1, 1);
    app.view(frame, &mut highlighter, &history, body)
        .expect("view")
}

/// The bytes the button coming up hands the loop to send, resolved the way
/// `Shell::send_wash` resolves them: against the frame last painted.
fn released(app: &mut App, view: &View, span: Option<(usize, usize)>) -> Option<String> {
    if let Some(lines) = span.and_then(|span| view.lines_in(span)) {
        app.send(&lines);
    }
    app.take_sending().map(|sending| sending.text)
}

/// Taken, so a repeated batch cannot spend the reader's clipboard twice.
#[test]
fn a_send_is_taken_once() {
    let scratch = deep_scratch("send-once");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();

    let view = painted(&mut app, &mut frame);
    assert_eq!(
        released(&mut app, &view, Some((0, 0))).as_deref(),
        Some(DEEP)
    );
    assert_eq!(
        app.take_sending(),
        None,
        "the payload was still pending after being taken, so the loop would send it \
         again on the next frame"
    );
}

/// Nothing washed is nothing to send. An OSC 52 write carrying nothing clears the
/// reader's clipboard, so a release that resolved to no lines must not reach it.
#[test]
fn a_release_with_nothing_washed_sends_nothing() {
    let scratch = deep_scratch("send-empty");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();

    // Drawn first, so the frame really did collect and this is a release with no
    // wash rather than a release before there was a screen.
    let view = painted(&mut app, &mut frame);
    assert_eq!(
        released(&mut app, &view, None),
        None,
        "a release with nothing washed armed a write, so the reader's clipboard is \
         spent on nothing"
    );
}

/// The payload is the frame's and not the gesture's. A tick rebuilds the file list
/// from a fresh status walk, and `Frame::advance` between the paint and the button
/// coming up is the ordinary case in a batch, not a corner: the agent in the other
/// pane writes while the reader drags.
#[test]
fn a_write_between_the_paint_and_the_release_does_not_move_what_is_sent() {
    let scratch = deep_scratch("send-across-a-tick");
    // Sorts before DEEP's `crates/`, so creating it later shifts DEEP's index.
    let earlier = "aaa/first.rs";
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();

    // The frame the release resolves against is this one, drawn against a list
    // holding one file.
    let view = painted(&mut app, &mut frame);
    assert_eq!(frame.files().len(), 1, "the fixture is not one file");

    // Then the tree changes under it, exactly as a batch carrying a tick and a
    // release does, and DEEP is no longer index zero.
    scratch.write(earlier, "one\n");
    frame.advance().expect("advance");
    assert_eq!(
        frame.files().first().map(|at| at.path.as_str()),
        Some(earlier),
        "the fixture did not reorder, so this proves nothing about a stale index"
    );

    assert_eq!(
        released(&mut app, &view, Some((0, 0))).as_deref(),
        Some(DEEP),
        "the send followed the index rather than the frame that was painted, so the \
         reader copied a file they were not looking at"
    );
}

/// The confirmation is the only feedback there is, because OSC 52 has no reply,
/// and the event that would erase it is the one this tool exists to watch.
#[test]
fn a_write_does_not_erase_what_a_send_just_said() {
    let mut app = App::new();
    app.flash("sent 3 lines to the clipboard");

    // What `Wake::Tick` does on every write, and it must reach the lasting slot
    // rather than the reader's own confirmation.
    app.clear_notice();
    assert_eq!(
        app.notice(),
        Some("sent 3 lines to the clipboard"),
        "a file write wiped the send's confirmation, so the one signal the reader \
         gets is destroyed by the thing they are watching for"
    );
}

/// A warning with no expiry outlives a confirmation with one, rather than being
/// buried by it and then cleared on its clock.
#[test]
fn a_lasting_warning_survives_underneath_a_send() {
    let mut app = App::new();
    app.warn("not watching: the watch stopped");
    app.flash("sent 3 lines to the clipboard");
    assert_eq!(app.notice(), Some("sent 3 lines to the clipboard"));

    app.clear_flash();
    assert_eq!(
        app.notice(),
        Some("not watching: the watch stopped"),
        "the send's clock took the watch-loss warning with it, and nothing will \
         raise that warning again: the tick that would have is what stopped"
    );
}

/// The footer draws from `Chrome`, so a message the accessor reports and the
/// chrome does not is a message nobody ever sees.
#[test]
fn what_the_footer_is_handed_is_what_the_pane_is_showing() {
    let mut app = App::new();
    app.flash("sent 3 lines to the clipboard");
    assert_eq!(
        app.chrome("fixture", None, Pointing::default(), 0, "")
            .notice
            .as_deref(),
        app.notice(),
        "the chrome carries a different notice than the accessor reports, so the \
         reader is told one thing and shown another"
    );
}
