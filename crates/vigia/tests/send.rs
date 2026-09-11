//! `SPEC.md` §11.1: what the button coming up puts on the clipboard, and what the
//! footer says about it.
//!
//! An OSC 52 write draws nothing, so the whole rendering suite stays green
//! whichever way this behaves. Everything here is what no drawn cell can show.

#[path = "../../vigia-core/tests/support/mod.rs"]
mod support;

use std::ffi::OsStr;
use std::time::{Duration, Instant};

use ratatui::layout::Rect;
use vigia::{
    App, Carrier, NOTICE_LINGER, Pointing, Route, View, Voice, body_layout, plan, put, remote,
    settled, system_tools, tmux_command,
};
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
    let chrome = app.chrome(
        "fixture",
        None,
        vigia::Stood {
            position: "current",
            now: 0,
        },
        Pointing::default(),
        Default::default(),
        "",
    );
    let body = body_layout(Rect::new(0, 0, NARROW, 24), &chrome, 1, 1);
    app.view(frame, &mut highlighter, &history, body)
        .expect("view")
}

/// The bytes the button coming up hands the loop to send, resolved the way
/// `Shell::send_wash` resolves them: against the frame last painted.
fn released(app: &mut App, view: &View, span: (usize, usize)) -> Option<String> {
    if let Some(lines) = view.lines_in(span) {
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
    assert_eq!(released(&mut app, &view, (0, 0)).as_deref(), Some(DEEP));
    assert_eq!(
        app.take_sending(),
        None,
        "the payload was still pending after being taken, so the loop would send it \
         again on the next frame"
    );
}

/// Nothing to send is nothing sent. An OSC 52 write carrying nothing clears the
/// reader's clipboard, so the guard is driven here rather than around: a helper that
/// declines to call `App::send` proves only that `Option` short-circuits.
#[test]
fn a_payload_with_no_text_in_it_never_reaches_the_clipboard() {
    for lines in [
        vec![],
        vec![String::new()],
        vec![String::new(), String::new()],
    ] {
        let mut app = App::new();
        app.send(&lines);
        assert_eq!(
            app.take_sending(),
            None,
            "{} blank line(s) armed a write, so the reader's clipboard is spent on \
             nothing",
            lines.len()
        );
    }

    // And the guard is not simply refusing everything.
    let mut app = App::new();
    app.send(&[String::new(), "one".to_owned()]);
    assert!(
        app.take_sending().is_some(),
        "a payload with a line in it was refused, so the guard above proves nothing"
    );
}

/// One batch can carry two releases, and only one send leaves it. The last gesture
/// decides, including when it decides on nothing: a drag over blank rows has to
/// retire the one before it rather than let it through on the batch's single send.
#[test]
fn a_second_release_over_blank_rows_retires_the_first() {
    let mut app = App::new();

    app.send(&["one".to_owned(), "two".to_owned()]);
    app.send(&[String::new()]);
    assert_eq!(
        app.take_sending(),
        None,
        "the blank release left the first gesture's lines queued, so the batch's one \
         send spends the clipboard on a selection the reader had moved off"
    );

    // And a second gesture with text in it does replace the first, which is the half
    // that already held and must go on holding.
    app.send(&["one".to_owned()]);
    app.send(&["two".to_owned()]);
    assert_eq!(
        app.take_sending().map(|sending| sending.text),
        Some("two".to_owned()),
        "the second gesture did not replace the first"
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
        released(&mut app, &view, (0, 0)).as_deref(),
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
    app.flash(
        "sent 3 lines to the clipboard",
        Instant::now() + NOTICE_LINGER,
        Voice::Said,
    );

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
    let now = Instant::now();
    app.warn("not watching: the watch stopped");
    app.flash(
        "sent 3 lines to the clipboard",
        now + NOTICE_LINGER,
        Voice::Said,
    );
    assert_eq!(app.notice(), Some("sent 3 lines to the clipboard"));

    // The deadline is the shell's to act on now, because a spent message is
    // still drawn while it leaves. What `App` owes is the deadline itself.
    let until = app.flash_until().expect("a flash carries its own deadline");
    assert_eq!(until, now + NOTICE_LINGER);
    assert!(
        !settled(Some(until), until - Duration::from_millis(1)),
        "the confirmation was spent a millisecond early"
    );
    assert!(settled(Some(until), until));

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
    app.flash(
        "sent 3 lines to the clipboard",
        Instant::now() + NOTICE_LINGER,
        Voice::Said,
    );
    assert_eq!(
        app.chrome(
            "fixture",
            None,
            vigia::Stood {
                position: "current",
                now: 0
            },
            Pointing::default(),
            Default::default(),
            ""
        )
        .notice
        .as_deref(),
        app.notice(),
        "the chrome carries a different notice than the accessor reports, so the \
         reader is told one thing and shown another"
    );
}

// ---------------------------------------------------------------------------
// Which way a copy leaves the pane. The escape alone reaches nothing inside
// tmux, and the machine's own clipboard reaches the wrong machine over ssh, so
// what matters is the order the routes are tried in.
// ---------------------------------------------------------------------------

/// A carrier that records what it was asked and answers as it was told to.
#[derive(Default)]
struct Fake {
    /// What each route was handed, in the order the routes were tried.
    tried: Vec<(&'static str, String)>,
    /// Which routes refuse.
    refuse: Vec<&'static str>,
}

impl Fake {
    fn take(&mut self, route: &'static str, text: &str) -> std::io::Result<()> {
        self.tried.push((route, text.to_owned()));
        if self.refuse.contains(&route) {
            return Err(std::io::Error::other(format!("{route} refused it")));
        }
        Ok(())
    }

    /// The routes it was asked, in order.
    fn order(&self) -> Vec<&'static str> {
        self.tried.iter().map(|(route, _)| *route).collect()
    }
}

impl Carrier for Fake {
    fn to_system(&mut self, text: &str) -> std::io::Result<()> {
        self.take("system", text)
    }

    fn to_tmux(&mut self, text: &str) -> std::io::Result<()> {
        self.take("tmux", text)
    }

    fn to_terminal(&mut self, sequence: &str) -> std::io::Result<()> {
        self.take("escape", sequence)
    }
}

/// A reader sitting at the machine gets its own clipboard first, which needs
/// nothing of the terminal and nothing of tmux.
#[test]
fn the_plan_at_the_machine_tries_its_own_clipboard_first() {
    assert_eq!(plan(None, false), [Route::System, Route::Escape]);
}

/// **The ordering this is all for.** Over `ssh` the machine's own clipboard is
/// on the far end, where nobody can see it, and it would succeed at being set:
/// the chain would end there and the escape that crosses back would never go.
#[test]
fn the_plan_over_ssh_never_tries_the_machines_clipboard() {
    assert_eq!(plan(None, true), [Route::Escape]);
    assert_eq!(
        plan(Some(OsStr::new("/tmp/tmux-1000/default,3412,0")), true),
        [Route::Tmux, Route::Escape],
        "a remote pane inside tmux still has tmux to pass it on"
    );
}

/// Inside tmux the escape is discarded, so tmux goes between the two.
#[test]
fn the_plan_inside_tmux_puts_tmux_before_the_escape() {
    assert_eq!(
        plan(Some(OsStr::new("/tmp/tmux-1000/default,3412,0")), false),
        [Route::System, Route::Tmux, Route::Escape]
    );
    // The variable survives a shell clearing it, and an empty one is not a pane.
    assert_eq!(
        plan(Some(OsStr::new("")), false),
        [Route::System, Route::Escape]
    );
}

/// The escape is last on every plan, because it is the one route every pane has.
#[test]
fn the_escape_ends_every_plan() {
    for tmux in [None, Some(OsStr::new("x"))] {
        for remote in [true, false] {
            let made = plan(tmux, remote);
            assert_eq!(
                made.last(),
                Some(&Route::Escape),
                "a plan ended somewhere else: {made:?}"
            );
        }
    }
}

/// Which of the three the daemon sets depends on how the session was opened, so
/// any of them is enough to say the reader is elsewhere.
#[test]
fn an_ssh_session_is_any_of_the_three_variables() {
    for one in ["SSH_CONNECTION", "SSH_CLIENT", "SSH_TTY"] {
        assert!(
            remote(|name| name == one),
            "{one} did not say so on its own"
        );
    }
    assert!(
        !remote(|_| false),
        "a session with none of them read as remote"
    );
}

/// The command is the one that makes tmux write the clipboard outward, rather
/// than only filling a buffer of its own.
#[test]
fn the_tmux_command_hands_the_text_over_and_asks_for_the_clipboard() {
    let command = tmux_command();
    assert_eq!(command.get_program(), "tmux");
    let args: Vec<&str> = command
        .get_args()
        .map(|arg| arg.to_str().expect("ascii"))
        .collect();
    assert_eq!(
        args,
        ["load-buffer", "-w", "-"],
        "`-w` is what reaches the clipboard and `-` is what reads standard input"
    );
}

/// Every tool is fed on standard input, so nothing in a copy can be read as an
/// argument, and a headless Unix offers none rather than a broken one.
#[test]
fn the_machines_tools_are_fed_on_standard_input() {
    for (wayland, x11) in [(true, false), (false, true), (true, true)] {
        let tools = system_tools(wayland, x11);
        assert!(
            !tools.is_empty(),
            "a display was set and no tool was offered for it"
        );
        for (program, args) in tools {
            assert!(
                !args.contains(&"-"),
                "{program} was given the text as an argument rather than on its input"
            );
        }
    }
    if cfg!(not(any(target_os = "macos", windows))) {
        assert!(
            system_tools(false, false).is_empty(),
            "a Unix with no display offered a clipboard tool for a clipboard it has not got"
        );
    }
}

/// The first route that carries it is the last one asked.
#[test]
fn the_first_route_that_carries_it_ends_the_chain() {
    let mut fake = Fake::default();
    put(&mut fake, "one\ntwo", &plan(Some(OsStr::new("x")), false)).expect("carried");
    assert_eq!(
        fake.order(),
        ["system"],
        "the chain went on past a route that took it"
    );
    assert_eq!(
        fake.tried[0].1, "one\ntwo",
        "the text was changed on the way"
    );
}

/// A route that refuses falls through to the next, and the escape is what the
/// last of them writes.
#[test]
fn a_route_that_refuses_falls_through_to_the_next() {
    let mut fake = Fake {
        refuse: vec!["system", "tmux"],
        ..Fake::default()
    };
    put(&mut fake, "one", &plan(Some(OsStr::new("x")), false)).expect("the escape carried it");
    assert_eq!(fake.order(), ["system", "tmux", "escape"]);
    let (_, wrote) = fake.tried.last().expect("nothing was written");
    assert!(
        wrote.starts_with("\u{1b}]52;c;") && wrote.ends_with("\u{1b}\\"),
        "what the escape route wrote was not the escape: {wrote:?}"
    );
}

/// Every route refusing is the one case the reader is told about, and until
/// these routes existed no test could reach that arm at all.
#[test]
fn a_copy_no_route_could_carry_is_an_error() {
    let mut fake = Fake {
        refuse: vec!["system", "tmux", "escape"],
        ..Fake::default()
    };
    let refused = put(&mut fake, "one", &plan(Some(OsStr::new("x")), false))
        .expect_err("something carried it");
    assert!(
        refused.to_string().contains("escape"),
        "the error is not the last route's, which is the one that speaks for the rest: {refused}"
    );
}
