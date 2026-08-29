//! `SPEC.md` §11.2 B9: what `y` puts on the clipboard.
//!
//! An OSC 52 write draws nothing, so the whole rendering suite stays green
//! whichever way this behaves. Everything here is what no drawn cell can show.

#[path = "../../vigia-core/tests/support/mod.rs"]
mod support;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use vigia::{Action, App, Glyphs, Pointing, Theme, body_layout, render};
use vigia_core::{Frame, Highlighter, History};

use support::{Scratch, materialise};

/// Deep enough that `render::elide_head` has to cut it at the width I6 names,
/// and distinctive at both ends so a truncation at either is visible.
const DEEP: &str = "crates/vigia-core/src/very/deeply/nested/module/frame.rs";

/// The mark the renderer leaves where it dropped the head of a path.
const ELIDED: char = '…';

/// The width I6 is named for, and the width B9 says a selection cannot reach a
/// long path at.
const NARROW: u16 = 40;

/// A worktree holding one deeply nested file. The `Scratch` is returned rather
/// than the `Frame`, because a frame borrows the worktree it walks.
fn deep_scratch(name: &str) -> Scratch {
    let scratch = Scratch::new(name);
    scratch.write(DEEP, "one\ntwo\nthree\n");
    scratch
}

/// What `y` hands the loop to send.
fn yanked(app: &mut App, frame: &mut Frame) -> Option<String> {
    app.apply(Action::Yank, frame, 24).expect("apply");
    app.take_yank()
}

/// Everything the pane actually draws at [`NARROW`], as text.
fn drawn(app: &mut App, frame: &mut Frame) -> String {
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let chrome = app.chrome("fixture", None, Pointing::default(), 0, "");
    let body = body_layout(Rect::new(0, 0, NARROW, 24), &chrome, 1, 1);
    let view = app
        .view(frame, &mut highlighter, &history, body)
        .expect("view");
    let theme = Theme::default();
    let mut terminal = Terminal::new(TestBackend::new(NARROW, 24)).expect("terminal");
    terminal
        .draw(|f| {
            let area = f.area();
            render(
                f.buffer_mut(),
                area,
                &view,
                &theme,
                Glyphs::default(),
                &chrome,
            );
        })
        .expect("draw");
    let buffer = terminal.backend().buffer().clone();
    (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol().to_owned())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The invariant the whole ruling rests on: a cell holds what was *drawn*, so
/// a copy taken from cells is the elided label. B9 is a semantic copy and is
/// worth having because it is not that.
#[test]
fn the_yanked_path_is_the_whole_path_and_not_the_drawn_one() {
    let scratch = deep_scratch("yank-whole-path");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();

    let screen = drawn(&mut app, &mut frame);
    assert!(
        screen.contains(ELIDED),
        "nothing on a {NARROW}-column pane elided {DEEP:?}, so this proves nothing \
         about the case B9 exists for:\n{screen}"
    );
    assert!(
        !screen.contains(DEEP),
        "the pane drew the whole path, so a copy taken from the cells would have \
         reached it and B9 is answering a question nobody has:\n{screen}"
    );

    let sent = yanked(&mut app, &mut frame).expect("`y` yanked nothing at all");
    assert_eq!(
        sent, DEEP,
        "`y` sent {sent:?} rather than the path, so it is the drawn row by another \
         route, which is the one thing this key must not be"
    );
    assert!(
        !sent.contains(ELIDED),
        "the sent string carries the renderer's elision mark, so it is a reading of \
         the screen rather than a path"
    );
}

/// Taken, so a repeated batch cannot spend the reader's clipboard twice.
#[test]
fn a_yank_is_sent_once() {
    let scratch = deep_scratch("yank-once");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();
    // The caret is resolved by drawing, so a yank before the first frame has
    // nothing to point at.
    let _ = drawn(&mut app, &mut frame);

    assert_eq!(yanked(&mut app, &mut frame).as_deref(), Some(DEEP));
    assert_eq!(
        app.take_yank(),
        None,
        "the path was still pending after being taken, so the loop would send it \
         again on the next frame"
    );
}

/// An empty worktree has no caret file, and a yank there writes nothing rather
/// than clearing what the reader had.
#[test]
fn a_yank_with_nothing_to_copy_sends_nothing() {
    let scratch = Scratch::new("yank-empty");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    assert!(frame.files().is_empty(), "the fixture is not an empty tree");

    let mut app = App::new();
    // Drawn first, or the caret would be unset because no frame has resolved one
    // and this would pass on an empty tree and a full one alike.
    let _ = drawn(&mut app, &mut frame);
    assert_eq!(
        yanked(&mut app, &mut frame),
        None,
        "a yank on an empty tree armed a write, so the reader's clipboard is spent \
         on nothing"
    );
}

/// A tick rebuilds the file list from a fresh status walk, so an index is only
/// good for the frame it was resolved against. `Frame::advance` between the draw
/// and the keypress is the ordinary case in a batch, not a corner: the agent in
/// the other pane writes while the reader watches.
#[test]
fn a_write_between_the_draw_and_the_key_does_not_move_the_yank() {
    let scratch = deep_scratch("yank-across-a-tick");
    // Sorts before DEEP's `crates/`, so creating it later shifts DEEP's index.
    let earlier = "aaa/first.rs";
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();

    // The caret is resolved on this draw, against a list holding one file.
    let _ = drawn(&mut app, &mut frame);
    assert_eq!(frame.files().len(), 1, "the fixture is not one file");

    // Then the tree changes under it, exactly as a batch carrying a tick and a
    // key does, and DEEP is no longer index zero.
    scratch.write(earlier, "one\n");
    frame.advance().expect("advance");
    assert_eq!(
        frame.files().first().map(|at| at.path.as_str()),
        Some(earlier),
        "the fixture did not reorder, so this proves nothing about a stale index"
    );

    assert_eq!(
        yanked(&mut app, &mut frame).as_deref(),
        Some(DEEP),
        "the yank followed the index rather than the path, so the reader copied a \
         file they were not looking at"
    );
}

/// The confirmation is the only feedback there is, because OSC 52 has no reply,
/// and the event that would erase it is the one this tool exists to watch.
#[test]
fn a_write_does_not_erase_what_a_yank_just_said() {
    let mut app = App::new();
    app.flash("sent src/lib.rs to the clipboard");

    // What `Wake::Tick` does on every write, and it must reach the lasting slot
    // rather than the reader's own confirmation.
    app.clear_notice();
    assert_eq!(
        app.notice(),
        Some("sent src/lib.rs to the clipboard"),
        "a file write wiped the yank's confirmation, so the one signal the reader \
         gets is destroyed by the thing they are watching for"
    );
}

/// A warning with no expiry outlives a confirmation with one, rather than being
/// buried by it and then cleared on its clock.
#[test]
fn a_lasting_warning_survives_underneath_a_yank() {
    let mut app = App::new();
    app.warn("not watching: the watch stopped");
    app.flash("sent src/lib.rs to the clipboard");
    assert_eq!(app.notice(), Some("sent src/lib.rs to the clipboard"));

    app.clear_flash();
    assert_eq!(
        app.notice(),
        Some("not watching: the watch stopped"),
        "the yank's clock took the watch-loss warning with it, and nothing will \
         raise that warning again: the tick that would have is what stopped"
    );
}

/// The footer draws from `Chrome`, so a message the accessor reports and the
/// chrome does not is a message nobody ever sees.
#[test]
fn what_the_footer_is_handed_is_what_the_pane_is_showing() {
    let mut app = App::new();
    app.flash("sent src/lib.rs to the clipboard");
    assert_eq!(
        app.chrome("fixture", None, Pointing::default(), 0, "")
            .notice
            .as_deref(),
        app.notice(),
        "the chrome carries a different notice than the accessor reports, so the \
         reader is told one thing and shown another"
    );
}
