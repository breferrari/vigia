//! The masthead's default and its one gesture.
//!
//! `SPEC.md` §11.1 rules the worktree churn band **hidden until a reader asks
//! for it**, which is [#204](https://github.com/breferrari/vigia/issues/204)
//! reversing the default the toggle shipped under.
//!
//! > `m` shows the masthead and hides it again, and it starts hidden.
//!
//! A separate binary from `render.rs` for the reason that file's own header
//! gives about `input.rs`: the question is different. `render.rs` builds a
//! [`vigia::Chrome`] by hand and asks what the drawer does with one, which is
//! exactly what cannot answer this, because a hand-built chrome says whatever
//! the test wrote in it. What is gated here is the **shipped** answer: the
//! chrome an untouched [`App`] produces, and what `m` does to it.
//!
//! That pair is worth its own file now rather than a line in another because of
//! which way the default points. The toggle's app side had no gate at all when
//! it landed, and while the band was drawn by default a broken `m` cost a reader
//! four rows they could not get back. It now costs them the element entirely.

#[path = "../../vigia-core/tests/support/mod.rs"]
mod support;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use vigia::{Action, App, Chrome, Theme, body_layout, render};
use vigia_core::{Highlighter, History};

use support::{Scratch, materialise};

/// A pane with room to spare, so nothing here is answered by a floor.
///
/// `GRAPH_KEEP` and `GRAPH_FLOOR` both decline the band on a pane that cannot
/// afford it, and a gate written at either edge would pass against a build that
/// had no default at all. Every assertion below is made where the band **is**
/// affordable, and `the_shipped_shell_starts_with_the_band_hidden` says so in an
/// assertion rather than in this comment.
const WIDE: u16 = 80;
const TALL: u16 = 24;

/// Changed files in every fixture here, which is what `assets/preview.svg` draws.
const FILES: usize = 3;

fn area() -> Rect {
    Rect::new(0, 0, WIDE, TALL)
}

fn chrome(app: &App) -> Chrome {
    app.chrome("fixture", None, None, None, None, None)
}

#[test]
fn the_shipped_shell_starts_with_the_band_hidden() {
    // **The default, read off the chrome the shell actually publishes** rather
    // than off the field, because the field is private and the chrome is what
    // every drawer sees.
    let shipped = chrome(&App::new());
    assert!(
        !shipped.masthead,
        "a shell nobody has pressed a key on published a masthead"
    );

    // And the default reaches the **layout**, which is the half a boolean cannot
    // prove: the rows are the cost, and a default that never got as far as
    // `Body::split` would leave them reserved and blank.
    let hidden = body_layout(area(), &shipped, FILES);
    assert_eq!(
        (hidden.graph, hidden.air),
        (0, 0),
        "the shipped default still reserved masthead rows"
    );

    // **Not vacuous**, and this is the assertion that makes it so. The same pane
    // asked for a band draws one, so what the two above measured is the default
    // rather than a pane too small to carry the element at all.
    let asked = body_layout(
        area(),
        &Chrome {
            masthead: true,
            ..shipped
        },
        FILES,
    );
    assert!(
        asked.graph > 0,
        "the fixture pane cannot draw a band at any setting, so nothing above is a gate"
    );
}

#[test]
fn m_shows_the_band_and_hides_it_again() {
    // **The only way to the element now.** With the default hidden there is no
    // config file, no flag and nothing that persists between runs, so a `m` that
    // stopped flipping the state would put the band out of reach with every
    // other gate in the suite staying green: they all build their own chrome.
    let scratch = Scratch::large_diff("masthead-toggle", FILES, 20);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);

    let mut app = App::new();
    // The diff's height, which is what `App::apply` takes and what a scroll
    // would be clamped against. The masthead's own arm reads neither, and this
    // gate deliberately hands it the real number anyway rather than a zero:
    // an arm that grew a dependency on either should be caught by the gate that
    // presses the key, not by the next reader.
    let height = body_layout(area(), &chrome(&app), FILES).diff;

    assert!(!chrome(&app).masthead, "the shell did not start hidden");

    for press in 1..=4 {
        let running = app
            .apply(Action::ToggleMasthead, &mut frame, height)
            .expect("m");
        assert!(running, "press {press}: m asked the shell to quit");

        let shown = press % 2 == 1;
        let now = chrome(&app);
        assert_eq!(
            now.masthead,
            shown,
            "press {press}: the band should be {}",
            if shown { "drawn" } else { "gone" }
        );

        // Through the layout as well as through the flag, and on **every** press
        // rather than at the end: a toggle that flipped a boolean the split had
        // stopped reading would satisfy a flag-only gate on all four.
        let body = body_layout(area(), &now, FILES);
        assert_eq!(
            body.graph > 0,
            shown,
            "press {press}: the layout disagrees with the state"
        );
    }
}

#[test]
fn the_branch_stays_on_a_pane_with_no_masthead() {
    // **What keeps the every-frame `.git/HEAD` read honest.** `Shell::paint`
    // reads the branch on every frame under the rule *never touch a file the
    // frame does not draw*, and what satisfies that rule is the header's ladder
    // rather than the masthead: #158 moved the branch to the header, and #204
    // makes the difference load bearing, since the masthead is now absent unless
    // a reader asks for it. A branch that had stayed up there would make most
    // frames read a file they draw nothing from.
    let scratch = Scratch::large_diff("masthead-branch", FILES, 20);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);

    const BRANCH: &str = "some-branch";

    let mut app = App::new();
    let mut highlighter = Highlighter::new();
    let history = History::new();
    let drawn = app.chrome("fixture", Some(BRANCH), None, None, None, None);
    assert!(
        !drawn.masthead,
        "the fixture asked for a masthead, so this proves nothing about a pane without one"
    );

    let body = body_layout(area(), &drawn, FILES);
    let view = app
        .view(&mut frame, &mut highlighter, &history, body)
        .expect("view");
    let mut buf = Buffer::empty(area());
    render(&mut buf, area(), &view, &Theme::default(), &drawn);

    let header: String = (0..WIDE).map(|x| buf[(x, 0)].symbol()).collect();
    assert!(
        header.contains(BRANCH),
        "a pane with the masthead hidden drew no branch: {header:?}"
    );
}
