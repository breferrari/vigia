//! Where the pane is standing, from the key to the header: `SPEC.md` §11.1.
//!
//! The walk itself is gated in `vigia-core/tests/standing.rs`. These are the
//! three places the reading can come apart from the run it names: the key that
//! moves it, the token the header draws, and the counts drawn beside that token.

#[path = "../../vigia-core/tests/support/mod.rs"]
mod support;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use support::Scratch;
use vigia::{
    Action, App, Chrome, Glyphs, Pointing, Position, Theme, View, Viewport, branch_point_of, render,
};
use vigia_core::{Frame, Highlighter, History, Standing, Worktree};

/// What joins two facts about one subject on a line of chrome.
const FACT_JOIN: &str = " · ";

fn viewport() -> Viewport {
    Viewport {
        position: Position { file: 0, row: 0 },
        anchored: false,
        diff_rows: 20,
        width: 120,
        wrap: false,
        list_top: 0,
        list_rows: 8,
        list_follows: true,
        measured: true,
        landing: false,
        single: false,
        highlight: false,
    }
}

/// The tree every gate here reads: one file committed on a branch, one staged,
/// one written and not staged, so the two readings cannot agree by accident.
fn scratch(name: &str) -> Scratch {
    let scratch = Scratch::new(name);
    scratch.write("src/base.rs", "one\n");
    scratch.git(&["add", "-A"]);
    scratch.git(&["commit", "-m", "init"]);
    scratch.git(&["checkout", "-b", "work"]);
    scratch.write("src/committed.rs", "two\n");
    scratch.git(&["add", "-A"]);
    scratch.git(&["commit", "-m", "on the branch"]);
    scratch.write("src/staged.rs", "three\n");
    scratch.git(&["add", "src/staged.rs"]);
    scratch.write("src/written.rs", "four\n");
    scratch
}

/// The branch point this worktree's `b` resolves to, as the shell resolves it.
fn since(worktree: &Worktree) -> Standing {
    let (at, named) = worktree.branch_point().expect("a branch point");
    Standing::Since { at, named }
}

/// The body's first row, which on an empty pane is B3's one line.
fn body_line(view: &View, position: &str, app: &App) -> String {
    drawn_row(view, position, app, 1, 120)
}

/// The header a `View` and a position draw together, as one line of text.
fn header(view: &View, position: &str, app: &App) -> String {
    drawn_row(view, position, app, 0, 120)
}

/// The same header on a pane `width` columns across.
fn header_at(view: &View, position: &str, app: &App, width: u16) -> String {
    drawn_row(view, position, app, 0, width)
}

/// Row `y` of a `width`-column pane drawing `view` while standing where
/// `position` says.
fn drawn_row(view: &View, position: &str, app: &App, y: u16, width: u16) -> String {
    let chrome = Chrome {
        position: position.to_owned(),
        ..app.chrome(
            "fixture",
            Some("work"),
            position,
            Pointing::default(),
            Default::default(),
            "",
        )
    };
    let mut terminal = Terminal::new(TestBackend::new(width, 8)).expect("terminal");
    let theme = Theme::default();
    terminal
        .draw(|f| {
            let area = f.area();
            render(
                f.buffer_mut(),
                area,
                view,
                &theme,
                Glyphs::default(),
                &chrome,
            );
        })
        .expect("draw");
    let backend = terminal.backend().clone();
    let buffer = backend.buffer();
    (buffer.area.left()..buffer.area.right())
        .map(|x| buffer[(x, y)].symbol().to_owned())
        .collect::<String>()
        .trim()
        .to_owned()
}

/// Walk `frame` where `standing` names and collect what a pane would draw.
fn drawn(frame: &mut Frame, standing: Standing) -> View {
    frame.stand(standing);
    frame.advance().expect("advance");
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    View::collect(frame, &mut highlighter, &history, viewport()).expect("collect")
}

/// A pane nobody has moved says so, rather than saying nothing.
///
/// The token is drawn always rather than only where it is interesting, which is
/// what the reader ruled: a header that says nothing while standing `current`
/// and says `since main` otherwise teaches the reading by its absence.
#[test]
fn the_token_reads_current_on_a_live_pane() {
    let scratch = scratch("standing-current");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    let app = App::new();
    let view = drawn(&mut frame, Standing::Current);

    assert_eq!(
        Standing::Current.label(),
        "current",
        "the default position spells itself something else, so the token below \
         is not the one a pane opens on"
    );
    let drawn = header(&view, &Standing::Current.label(), &app);
    assert!(
        drawn.contains(&format!("fixture{FACT_JOIN}work{FACT_JOIN}current")),
        "a pane nobody has moved does not say where it is standing: {drawn:?}"
    );
}

/// Standing at the branch point names the branch rather than a hash.
#[test]
fn the_token_names_the_branch_point_under_since() {
    let scratch = scratch("standing-since");
    let worktree = scratch.worktree();
    let standing = since(&worktree);
    let label = standing.label();
    assert!(
        label.starts_with("since "),
        "the token under `since` reads {label:?}"
    );
    let named = label.trim_start_matches("since ").to_owned();
    assert!(
        !named.is_empty() && named.chars().any(|c| !c.is_ascii_hexdigit()),
        "the token names {named:?}, which is a commit id rather than something a \
         reader would type"
    );

    let mut frame = worktree.frame();
    let app = App::new();
    let view = drawn(&mut frame, standing);
    let drawn = header(&view, &label, &app);
    assert!(
        drawn.contains(&format!("fixture{FACT_JOIN}work{FACT_JOIN}{label}")),
        "the header does not carry where the pane is standing: {drawn:?}"
    );
}

/// The count beside the token is the run the token names, in both readings.
///
/// The two are computed a walk apart, and nothing else can see them disagree: a
/// header drawn from the position with counts from the previous frame reads as a
/// correct sentence about the wrong comparison.
#[test]
fn the_headers_facts_describe_what_the_token_names() {
    let scratch = scratch("standing-facts");
    let worktree = scratch.worktree();
    let app = App::new();
    let mut frame = worktree.frame();

    let live = drawn(&mut frame, Standing::Current);
    let (live_files, live_header) = (live.files, header(&live, "current", &app));

    let standing = since(&worktree);
    let label = standing.label();
    let parked = drawn(&mut frame, standing);
    let (parked_files, parked_header) = (parked.files, header(&parked, &label, &app));

    // Non-vacuity: the two runs have to differ, or one number satisfies both
    // assertions and the gate proves nothing.
    assert!(
        parked_files > live_files,
        "the branch point run holds {parked_files} files against the live pane's \
         {live_files}, so the fixture does not separate the two readings"
    );

    for (files, position, drawn) in [
        (live_files, "current", &live_header),
        (parked_files, label.as_str(), &parked_header),
    ] {
        assert!(
            drawn.contains(&format!("{position}{FACT_JOIN}{files} changed")),
            "standing {position:?} the header does not count the run it names: \
             {drawn:?}"
        );
    }
}

/// One key, both directions, and the second press is the way back.
#[test]
fn one_key_flips_the_reading_and_is_a_toggle() {
    let scratch = scratch("standing-toggle");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");

    let mut app = App::new();
    assert!(
        !app.standing(),
        "a pane opens standing somewhere other than the working tree"
    );
    for wanted in [true, false, true] {
        app.apply(Action::ToggleStanding, &mut frame, 20)
            .expect("the toggle asked the shell to quit");
        assert_eq!(
            app.standing(),
            wanted,
            "the key does not flip the reading, so it is a switch rather than a \
             toggle"
        );
    }
}

/// The branch point is resolved once and kept, and a refusal keeps nothing.
///
/// The resolution itself is gated in `vigia-core`. This is the step between the
/// key and the walk: it runs on every press, so re-resolving would put a
/// revision walk on the frame path, and a failure left behind would be inherited
/// by the next press.
#[test]
fn the_branch_point_is_taken_once_and_a_refusal_keeps_nothing() {
    let scratch = scratch("standing-resolve");
    let worktree = scratch.worktree();

    let mut held = None;
    let first = branch_point_of(&mut held, &worktree).expect("a branch point");
    assert_eq!(
        held.as_ref(),
        Some(&first),
        "the resolution was handed back and not kept, so the next press resolves \
         it again"
    );

    // Kept rather than re-derived: a standing already held is returned as it is,
    // whatever the repository would resolve to now.
    let mut held = Some(Standing::Since {
        at: first.at().expect("a commit to measure from"),
        named: "held".to_owned(),
    });
    let again = branch_point_of(&mut held, &worktree).expect("the held standing");
    assert_eq!(
        again.label(),
        "since held",
        "a shell that had already stood somewhere resolved the branch point a \
         second time"
    );

    // And a worktree with nothing to measure from leaves the slot empty.
    let bare = Scratch::new("standing-refused");
    bare.write("src/a.rs", "one\n");
    bare.git(&["add", "-A"]);
    bare.git(&["commit", "-m", "init"]);
    bare.write("src/a.rs", "two\n");
    let bare = bare.worktree();

    let mut held = None;
    assert!(
        branch_point_of(&mut held, &bare).is_err(),
        "the fixture has a branch point after all, so the refusal below never \
         happens"
    );
    assert!(
        held.is_none(),
        "a refused resolution left a position behind, so the next press stands \
         somewhere the shell never resolved"
    );

    // The shell's own answer to that refusal, which is to put the reading back.
    let mut app = App::new();
    let mut frame = bare.frame();
    frame.advance().expect("advance");
    app.apply(Action::ToggleStanding, &mut frame, 20)
        .expect("the toggle asked the shell to quit");
    app.stands(false);
    assert!(
        !app.standing(),
        "the reading stayed at the branch point the shell could not resolve"
    );
}

/// An empty pane says which comparison it is empty *for*, and off the live pane
/// that comparison is the token's.
///
/// B3 spends the body's one row on the fact that is the body's own. `no unstaged
/// changes` under `since main` spends it on a walk this frame did not make, which
/// is worse than spending it on nothing.
#[test]
fn an_empty_since_run_says_what_it_found_nothing_since() {
    let scratch = Scratch::new("standing-empty");
    scratch.write("src/base.rs", "one\n");
    scratch.git(&["add", "-A"]);
    scratch.git(&["commit", "-m", "baseline"]);
    scratch.git(&["branch", "-M", "main"]);
    scratch.git(&["checkout", "-q", "-b", "work"]);
    // Added on the branch and taken away again, so the branch has commits to
    // measure from, nothing to show for them, and a clean worktree: both readings
    // are empty and the words are the only thing that tells them apart.
    scratch.write("src/gone.rs", "temporary\n");
    scratch.git(&["add", "-A"]);
    scratch.git(&["commit", "-m", "add it"]);
    std::fs::remove_file(scratch.root().join("src/gone.rs")).expect("remove");
    scratch.git(&["add", "-A"]);
    scratch.git(&["commit", "-m", "take it away"]);

    let worktree = scratch.worktree();
    let standing = since(&worktree);
    let label = standing.label();
    let app = App::new();
    let mut frame = worktree.frame();

    let view = drawn(&mut frame, standing);
    assert_eq!(
        view.files, 0,
        "the fixture has {} changed files, so the pane below is not the empty \
         state this gate is about",
        view.files
    );

    let line = body_line(&view, &label, &app);
    assert!(
        line.contains(&format!("no changes {label}")),
        "the empty pane says {line:?} while standing {label:?}, so it names a \
         comparison it is not making"
    );

    // Non-vacuity: the same empty pane, read the other way, keeps today's words.
    let live = drawn(&mut frame, Standing::Current);
    assert_eq!(live.files, 0, "the live pane is not empty either");
    let line = body_line(&live, "current", &app);
    assert!(
        line.contains("no unstaged changes"),
        "the live pane stopped saying which comparison it is empty for: {line:?}"
    );
}

/// Narrowing a pane that is standing somewhere never makes it read like the live
/// one.
///
/// The token outlives every other fact on the header's left, the branch included.
/// A rung that has given it up says exactly what a live pane says at the same
/// width, and the header is the only place the reading is written down.
#[test]
fn narrowing_a_since_pane_never_reads_like_the_current_one() {
    let scratch = scratch("standing-narrow");
    let worktree = scratch.worktree();
    let standing = since(&worktree);
    let label = standing.label();
    let app = App::new();
    let mut frame = worktree.frame();
    let view = drawn(&mut frame, standing);

    let mut carried = 0usize;
    for width in 40u16..=120 {
        let drawn = header_at(&view, &label, &app, width);
        let left = drawn
            .split("  ")
            .next()
            .unwrap_or_default()
            .trim()
            .to_owned();
        // The floor of every ladder on this side carries no fact at all, so there
        // is nothing there to mistake for a comparison. Asserted rather than
        // skipped: a rung that has given the token up while still carrying
        // something else is the defect, and skipping would hide it.
        if !left.contains(&label) {
            assert_eq!(
                left, "fixture",
                "at {width} columns the header gave up where it stands and kept \
                 {left:?}, which is what a live pane draws at the same width"
            );
            continue;
        }
        carried += 1;
    }
    assert!(
        carried > 0,
        "no width drew more than the worktree name, so the sweep asserts nothing"
    );
}

/// A walk that moved the pane puts it back at the top, and a press alone does not.
///
/// `ToggleStaged`'s reason one step further out: the file set changes wholesale,
/// so a row index into the old one names an unrelated file in the new one. The
/// press cannot do it, because the walk is the shell's and a request that is
/// refused or fails to walk has moved the reader nowhere.
#[test]
fn a_walk_that_moved_the_pane_puts_it_back_at_the_top() {
    let scratch = scratch("standing-scroll");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");

    let mut app = App::new();
    app.apply(Action::Scroll(3), &mut frame, 20)
        .expect("scroll");
    let scrolled = app.position();
    assert_ne!(
        scrolled,
        Position::default(),
        "the fixture did not move the pane, so the reset below proves nothing"
    );

    app.apply(Action::ToggleStanding, &mut frame, 20)
        .expect("the toggle asked the shell to quit");
    assert_eq!(
        app.position(),
        scrolled,
        "the press moved the pane on its own, so a refused branch point throws \
         the reader to the top of a run they never left"
    );

    app.stood();
    assert_eq!(
        app.position(),
        Position::default(),
        "the pane kept a row index from the run it was reading before"
    );
}

/// A refusal says what could not be measured from, in words a reader can act on.
#[test]
fn a_branch_with_nothing_to_measure_from_says_so_in_its_own_words() {
    let bare = Scratch::new("standing-words");
    bare.write("src/a.rs", "one\n");
    bare.git(&["add", "-A"]);
    bare.git(&["commit", "-m", "init"]);
    let bare = bare.worktree();

    let mut held = None;
    let refused = branch_point_of(&mut held, &bare).expect_err("a branch point");
    let said = refused.to_string();
    assert!(
        said.contains("no other branch to measure from"),
        "the refusal reads {said:?}, which does not tell a reader what is missing"
    );
}
