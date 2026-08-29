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

/// The invariant the whole ruling rests on. B20 declines an in-app selection
/// because a cell holds what was *drawn*; B9 is worth having only because this
/// does not.
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
        "the pane drew the whole path, so the selection B20 declines would have \
         reached it and B9 is answering a question nobody has:\n{screen}"
    );

    let sent = yanked(&mut app, &mut frame).expect("`y` yanked nothing at all");
    assert_eq!(
        sent, DEEP,
        "`y` sent {sent:?} rather than the path, so it is the drawn row by another \
         route and B20 already declined that"
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
    assert_eq!(
        yanked(&mut app, &mut frame),
        None,
        "a yank on an empty tree armed a write, so the reader's clipboard is spent \
         on nothing"
    );
}
