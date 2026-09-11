//! Where the pane is standing, from the key to the header: `SPEC.md` §11.1.
//!
//! The walk itself is gated in `vigia-core/tests/standing.rs`. These are the
//! three places the reading can come apart from the run it names: the key that
//! moves it, the token the header draws, and the counts drawn beside that token.

#[path = "../../vigia-core/tests/support/mod.rs"]
mod support;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use support::Scratch;
use vigia::{
    Action, App, Asked, Chrome, Glyphs, Pointing, Position, Regions, Theme, View, Viewport,
    branch_point_of, render,
};
use vigia_core::{Frame, Highlighter, History, Reading, Standing, Worktree};

/// What joins two facts about one subject on a line of chrome.
const FACT_JOIN: &str = " · ";

/// The chevron the token carries while the list is closed, which travels with the
/// word rather than sitting outside it: the cells a click opens the list from are
/// the word and the mark together.
const OPENS: &str = " ▾";

/// The footer's follow indicator, which is the state rather than the key: the hint
/// bar names `f` whether or not the mode is acting.
const FOLLOWING: &str = "follow ▶";

/// The mark a row takes while the newest burst named its file. The painter reads its
/// own copy of the flag, so a gate over the entry alone cannot see it go missing.
const PULSE: &str = "●";

/// The whole of the footer's refusal, and the half of it a reader acts on. The
/// footer clips a notice from the right, so the second half is the one that goes.
const NOTHING_TO_READ: &str = "B picks a commit; only reads one";
const GESTURE: &str = "B picks a commit";

/// The last row a pane drew anything on, trimmed.
fn last_line(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .rfind(|row| !row.is_empty())
        .unwrap_or_default()
        .to_owned()
}

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
    branched(name, 1)
}

/// The same tree with `commits` commits on the branch rather than one.
///
/// A second commit is what separates the three readings: one commit alone holds
/// less than everything since the branch point, so a gate about the reading
/// cannot be satisfied by a run that would answer either question.
fn branched(name: &str, commits: usize) -> Scratch {
    let scratch = Scratch::new(name);
    scratch.write("src/base.rs", "one\n");
    scratch.git(&["add", "-A"]);
    scratch.git(&["commit", "-m", "init"]);
    scratch.git(&["branch", "-M", "main"]);
    scratch.git(&["checkout", "-b", "work"]);
    for nth in 0..commits {
        scratch.write(
            &format!("src/committed_{nth}.rs"),
            "two\nand a second line\n",
        );
        scratch.git(&["add", "-A"]);
        scratch.git(&["commit", "-m", &format!("on the branch, {nth}")]);
    }
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
fn body_line(view: &View, standing: &Standing, app: &App) -> String {
    drawn_row(view, standing, app, 1, 120)
}

/// The header a `View` and a position draw together, as one line of text.
fn header(view: &View, standing: &Standing, app: &App) -> String {
    drawn_row(view, standing, app, 0, 120)
}

/// The same header on a pane `width` columns across.
fn header_at(view: &View, standing: &Standing, app: &App, width: u16) -> String {
    drawn_row(view, standing, app, 0, width)
}

/// Row `y` of a `width`-column pane drawing `view` from where `standing` says.
///
/// The position goes in whole rather than as the word it draws. The word and the
/// reading are two answers about one place, and a helper that set the first by hand
/// could hand the painter a header reading `only` beside an empty state naming a
/// range, which is the disagreement these gates exist to catch.
fn drawn_row(view: &View, standing: &Standing, app: &App, y: u16, width: u16) -> String {
    let chrome = Chrome {
        menu: None,
        ..app.chrome(
            "fixture",
            Some("work"),
            vigia::Stood { standing, now: 0 },
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
    let drawn = header(&view, &Standing::Current, &app);
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
    let view = drawn(&mut frame, standing.clone());
    let drawn = header(&view, &standing, &app);
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
    let (live_files, live_header) = (live.files, header(&live, &Standing::Current, &app));

    let standing = since(&worktree);
    let label = standing.label();
    let parked = drawn(&mut frame, standing.clone());
    let (parked_files, parked_header) = (parked.files, header(&parked, &standing, &app));

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
            drawn.contains(&format!("{position}{OPENS}{FACT_JOIN}{files} changed")),
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
    assert_eq!(
        *app.asked(),
        Asked::Current,
        "a pane opens standing somewhere other than the working tree"
    );
    for wanted in [Asked::BranchPoint, Asked::Current, Asked::BranchPoint] {
        app.apply(Action::ToggleStanding, &mut frame, 20)
            .expect("the toggle asked the shell to quit");
        assert_eq!(
            *app.asked(),
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
    app.stands(Asked::Current);
    assert_eq!(
        *app.asked(),
        Asked::Current,
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

    let view = drawn(&mut frame, standing.clone());
    assert_eq!(
        view.files, 0,
        "the fixture has {} changed files, so the pane below is not the empty \
         state this gate is about",
        view.files
    );

    let line = body_line(&view, &standing, &app);
    assert!(
        line.contains(&format!("no changes {label}")),
        "the empty pane says {line:?} while standing {label:?}, so it names a \
         comparison it is not making"
    );

    // Non-vacuity: the same empty pane, read the other way, keeps today's words.
    let live = drawn(&mut frame, Standing::Current);
    assert_eq!(live.files, 0, "the live pane is not empty either");
    let line = body_line(&live, &Standing::Current, &app);
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
    let view = drawn(&mut frame, standing.clone());

    let mut carried = 0usize;
    for width in 40u16..=120 {
        let drawn = header_at(&view, &standing, &app, width);
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

/// A worktree with `count` commits behind the branch point, so the list has
/// history to page through as well as its two named rows.
fn deep(name: &str, count: usize) -> Scratch {
    let scratch = Scratch::new(name);
    scratch.write("src/base.rs", "one\n");
    scratch.git(&["add", "-A"]);
    scratch.git(&["commit", "-m", "init"]);
    scratch.git(&["checkout", "-b", "work"]);
    for nth in 0..count {
        scratch.write("src/base.rs", format!("line {nth}\n"));
        scratch.git(&["add", "-A"]);
        scratch.git(&["commit", "-m", &format!("step {nth}")]);
    }
    scratch.write("src/written.rs", "four\n");
    scratch
}

/// A pane `width` by `height` drawing `view` with `app`'s chrome, as the buffer.
fn screen(view: &View, app: &App, width: u16, height: u16) -> ratatui::buffer::Buffer {
    let standing = Standing::Current;
    let chrome = app.chrome(
        "fixture",
        Some("work"),
        vigia::Stood {
            standing: &standing,
            now: 0,
        },
        Pointing::default(),
        Default::default(),
        "",
    );
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
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
    terminal.backend().buffer().clone()
}

/// The rows of places the list drew, frame and the air inside it taken off.
///
/// Indexed the way the caret is, so a gate naming a row names the row the caret would
/// land on. Counting the frame by hand in each gate is how one off-by-one lands in
/// several of them at once.
fn list_rows(buffer: &ratatui::buffer::Buffer) -> Vec<String> {
    let rows = box_rows(buffer);
    let air = vigia::OVERLAY_FRAME / 2;
    if rows.len() <= vigia::OVERLAY_FRAME {
        return Vec::new();
    }
    rows[air..rows.len() - air].to_vec()
}

/// The chrome a pane draws with, so a gate asking `regions` asks about the screen it
/// just read rather than about one nobody drew.
fn chrome_of(app: &App) -> Chrome {
    app.chrome(
        "fixture",
        Some("work"),
        vigia::Stood {
            standing: &Standing::Current,
            now: 0,
        },
        Pointing::default(),
        Default::default(),
        "",
    )
}

/// The same pane with the pointer resting on `on`.
fn marked(view: &View, app: &App, on: vigia::Hovered) -> ratatui::buffer::Buffer {
    let chrome = Chrome {
        hovered: Some(on),
        ..chrome_of(app)
    };
    let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");
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
    terminal.backend().buffer().clone()
}

/// Every row of a drawn pane, trailing blanks trimmed.
fn rows_of(buffer: &ratatui::buffer::Buffer) -> Vec<String> {
    (buffer.area.top()..buffer.area.bottom())
        .map(|y| {
            (buffer.area.left()..buffer.area.right())
                .map(|x| buffer[(x, y)].symbol().to_owned())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect()
}

/// The rows the list's box drew, frame included, or an empty vector on a pane that
/// drew none.
///
/// Found from its own top edge downwards rather than by matching any row that carries
/// a pipe: the box is drawn over rows that are already there, so a diff row it half
/// covers carries one too.
fn box_rows(buffer: &ratatui::buffer::Buffer) -> Vec<String> {
    let rows = rows_of(buffer);
    let Some(top) = rows
        .iter()
        .position(|row| row.contains('┌') || row.contains('╭'))
    else {
        return Vec::new();
    };
    let bottom = rows
        .iter()
        .rposition(|row| row.contains('└') || row.contains('╰'))
        .unwrap_or(rows.len() - 1);
    rows[top..=bottom].to_vec()
}

/// An app with the list open over `worktree`'s history, as the shell fills it.
fn opened(worktree: &Worktree, frame: &mut Frame, app: &mut App, page: usize) {
    app.apply(Action::TogglePositions, frame, 20)
        .expect("the key asked the shell to quit");
    let walk = worktree.commits_from(None, page).expect("a page");
    let (at, named) = worktree.branch_point().expect("a branch point");
    let _ = at;
    app.set_places(vigia::Places {
        commits: walk.commits,
        more: walk.more,
        current: Some(vigia::Facts {
            files: 1,
            added: 1,
            removed: 0,
        }),
        point: Some((
            named,
            Some(vigia::Facts {
                files: 2,
                added: 3,
                removed: 0,
            }),
        )),
    });
}

/// The cells the header's token occupies, read off the drawn buffer rather than off
/// the layout the pointer is told about.
///
/// The two have to agree, and a gate that asked `Regions` would be asking one of them
/// about itself: a published span over cells the painter never inked passes that way
/// and fails under a finger.
fn token_cells(buffer: &ratatui::buffer::Buffer, word: &str) -> (u16, u16, u16) {
    let row = rows_of(buffer)[0].clone();
    let at = row.find(word).expect("the header draws no position token");
    let before: u16 = row[..at].chars().count().try_into().expect("a narrow pane");
    let wide: u16 = word.chars().count().try_into().expect("a narrow pane");
    (before, 0, wide)
}

/// A left press at one cell.
fn press(column: u16, row: u16) -> Event {
    Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::NONE,
    })
}

/// A bare key.
fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

/// `B` and a press on the drawn token both ask for the list.
///
/// The click is resolved against the cells the painter actually inked, so the one
/// derivation behind the token's ink and its hit span is what this holds: publish a
/// span the header never drew and the press below lands on nothing.
#[test]
fn the_key_and_the_token_both_open_the_list() {
    let scratch = deep("standing-open", 3);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    let app = App::new();
    let view = drawn(&mut frame, Standing::Current);

    assert_eq!(
        vigia::action_for(&key(KeyCode::Char('B')), Regions::default()),
        Some(Action::TogglePositions),
        "`B` does not open the list, so a pane tmux sends no mouse events to cannot \
         reach it at all"
    );

    let buffer = screen(&view, &app, 80, 20);
    let token = format!("{}{OPENS}", Standing::Current.label());
    let (left, row, wide) = token_cells(&buffer, &token);
    let regions = vigia::regions(
        ratatui::layout::Rect::new(0, 0, 80, 20),
        &chrome_of(&app),
        &view,
    );
    let published = regions.position.expect("the header publishes no token");
    assert_eq!(
        (published.left, published.row, published.width),
        (left, row, wide),
        "the cells a click is told about are not the cells the header drew, so the \
         token is a target somewhere the reader cannot see"
    );
    for column in [left, left + wide - 1] {
        assert_eq!(
            vigia::action_for(&press(column, row), regions),
            Some(Action::TogglePositions),
            "a press on column {column} of the drawn token does not open the list"
        );
    }
    assert_eq!(
        vigia::action_for(&press(left + wide, row), regions),
        None,
        "the cell past the token opens the list, so the target is wider than the word"
    );
}

/// The chevron says a list lives there, and says which way it will go.
#[test]
fn the_token_carries_the_chevron_and_it_flips() {
    let scratch = deep("standing-chevron", 2);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    let mut app = App::new();
    let view = drawn(&mut frame, Standing::Current);

    let closed = rows_of(&screen(&view, &app, 80, 20))[0].clone();
    assert!(
        closed.contains("current ▾"),
        "a closed list draws no chevron after the token, so nothing at rest says the \
         word can be pressed: {closed:?}"
    );

    opened(&worktree, &mut frame, &mut app, 8);
    let open = rows_of(&screen(&view, &app, 80, 20))[0].clone();
    assert!(
        open.contains("current ▴"),
        "the chevron does not turn over while the list is open, so the token does not \
         say how to put it away: {open:?}"
    );
}

/// The pointer marks the token, and marks nothing beside it.
#[test]
fn the_token_carries_the_hover_mark() {
    let scratch = deep("standing-hover", 2);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    let app = App::new();
    let view = drawn(&mut frame, Standing::Current);

    let buffer = screen(&view, &app, 80, 20);
    let token = format!("{}{OPENS}", Standing::Current.label());
    let (left, row, wide) = token_cells(&buffer, &token);
    let regions = vigia::regions(
        ratatui::layout::Rect::new(0, 0, 80, 20),
        &chrome_of(&app),
        &view,
    );
    assert_eq!(
        regions.hover_at(left, row),
        Some(vigia::Hovered::Position),
        "the pointer resting on the token is not told it is on anything, so nothing \
         invites the press"
    );
    assert_eq!(
        regions.hover_at(left + wide, row),
        None,
        "the cell past the token marks itself, so the mark is wider than the target"
    );

    let lit = rows_of(&marked(&view, &app, vigia::Hovered::Position))[0].clone();
    assert_eq!(
        lit,
        rows_of(&buffer)[0],
        "the mark changed the header's text rather than its weight"
    );
    let plain = screen(&view, &app, 80, 20);
    let hovered = marked(&view, &app, vigia::Hovered::Position);
    assert_ne!(
        (
            plain[(left, row)].style(),
            plain[(left + wide - 1, row)].style()
        ),
        (
            hovered[(left, row)].style(),
            hovered[(left + wide - 1, row)].style()
        ),
        "the token's cells take the same ink under a pointer as at rest, so the mark \
         is invisible"
    );
}

/// Its title is the reading, so the list says what a chosen row will do.
#[test]
fn the_title_is_the_reading() {
    let scratch = deep("standing-title", 3);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    let mut app = App::new();
    let view = drawn(&mut frame, Standing::Current);
    opened(&worktree, &mut frame, &mut app, 8);

    let drawn_box = box_rows(&screen(&view, &app, 80, 20));
    let top = drawn_box.first().expect("the pane draws no box at all");
    assert!(
        top.contains(Standing::SINCE),
        "the box's title is not the reading, so nothing says what choosing a row \
         does: {top:?}"
    );
}

/// `current` and the branch point lead the list, and the point is named by the
/// branch rather than by a hash.
#[test]
fn current_and_the_branch_point_are_the_top_two_rows() {
    let scratch = deep("standing-named", 3);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    let mut app = App::new();
    let view = drawn(&mut frame, Standing::Current);
    opened(&worktree, &mut frame, &mut app, 8);

    let rows = list_rows(&screen(&view, &app, 80, 20));
    let named: Vec<&String> = rows.iter().take(2).collect();
    assert!(
        named[0].contains(Standing::CURRENT),
        "the first row is not the live pane: {:?}",
        named[0]
    );
    let (_, point) = worktree.branch_point().expect("a branch point");
    assert!(
        named[1].contains(&point),
        "the second row does not name the branch point by its branch: {:?}",
        named[1]
    );
    assert!(
        point.chars().any(|c| !c.is_ascii_hexdigit()),
        "the branch point is named {point:?}, which is a hash rather than something a \
         reader would type"
    );
}

/// Each named row carries the facts of the run it names, and a commit row carries
/// its own three cells instead.
#[test]
fn a_named_row_carries_the_facts_of_the_run_it_names() {
    let scratch = deep("standing-facts-rows", 3);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    let mut app = App::new();
    let view = drawn(&mut frame, Standing::Current);
    opened(&worktree, &mut frame, &mut app, 8);

    let rows = list_rows(&screen(&view, &app, 80, 20));
    assert!(
        rows[0].contains("1 changed") && rows[0].contains("+1") && rows[0].contains("-0"),
        "the live row does not carry the run's count and total: {:?}",
        rows[0]
    );
    assert!(
        rows[1].contains("2 changed") && rows[1].contains("+3"),
        "the branch point row does not carry its own run's facts: {:?}",
        rows[1]
    );
    let walked = worktree.commits_from(None, 1).expect("a page");
    let newest = walked.commits.first().expect("a commit");
    assert!(
        rows[2].contains(&newest.named) && rows[2].contains(&newest.subject),
        "a commit row does not carry its abbreviation and its subject: {:?}",
        rows[2]
    );
    assert!(
        !rows[2].contains("changed"),
        "a commit row counts a run, and a commit is a point rather than a run: {:?}",
        rows[2]
    );
}

/// The caret opens where the pane is standing, so the list says where you are.
#[test]
fn the_caret_opens_on_the_row_the_pane_stands_on() {
    let scratch = deep("standing-caret", 3);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    let mut app = App::new();
    let view = drawn(&mut frame, Standing::Current);

    opened(&worktree, &mut frame, &mut app, 8);
    assert_eq!(
        app.positions_caret().expect("no caret").at,
        0,
        "a pane standing at `current` opens the list somewhere other than its own row"
    );

    // And standing elsewhere moves it, by the commit rather than by a remembered index.
    let walked = worktree.commits_from(None, 3).expect("a page");
    let third = walked.commits[1].clone();
    app.apply(Action::TogglePositions, &mut frame, 20)
        .expect("close");
    app.stands(Asked::At(Standing::Since {
        at: third.id,
        named: third.named.clone(),
    }));
    opened(&worktree, &mut frame, &mut app, 8);
    let at = app.positions_caret().expect("no caret").at;
    let rows = list_rows(&screen(&view, &app, 80, 20));
    assert!(
        rows[at].contains(&third.named),
        "the caret opened on row {at}, which is not the commit the pane is standing \
         on: {:?}",
        rows[at]
    );
}

/// Choosing a row moves where the pane stands.
#[test]
fn choosing_a_row_moves_the_position() {
    let scratch = deep("standing-pick", 4);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    let mut app = App::new();
    let _ = drawn(&mut frame, Standing::Current);
    opened(&worktree, &mut frame, &mut app, 8);

    // Down twice from `current` lands on the newest commit, past the branch point.
    for _ in 0..2 {
        app.apply(Action::PositionsMove(1), &mut frame, 20)
            .expect("move");
    }
    app.apply(Action::PositionsPick, &mut frame, 20)
        .expect("pick");

    let newest = worktree.commits_from(None, 1).expect("a page").commits[0].clone();
    assert_eq!(
        *app.asked(),
        Asked::At(Standing::Since {
            at: newest.id,
            named: newest.named.clone(),
        }),
        "choosing a commit row did not ask the pane to stand there"
    );
    assert!(
        !app.positions_open(),
        "the list stayed up over the body it just sent the reader to look at"
    );
}

/// The header's facts follow the row that was chosen.
#[test]
fn the_headers_facts_follow_the_row_that_was_chosen() {
    let scratch = deep("standing-follows", 4);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    let app = App::new();

    let live = drawn(&mut frame, Standing::Current);
    let live_files = live.files;

    // Three commits back rather than the newest, so the range holds what those
    // commits changed as well as the unstaged write and the two counts differ.
    let walked = worktree.commits_from(None, 3).expect("a page");
    let newest = walked.commits.last().expect("three commits").clone();
    let chosen = Standing::Since {
        at: newest.id,
        named: newest.named.clone(),
    };
    let label = chosen.label();
    let parked = drawn(&mut frame, chosen.clone());
    let header = header(&parked, &chosen, &app);

    assert!(
        header.contains(&format!(
            "{label}{OPENS}{FACT_JOIN}{} changed",
            parked.files
        )),
        "the header does not count the run the chosen row named: {header:?}"
    );
    assert!(
        label.contains(&newest.named),
        "the token does not name the commit that was chosen: {label:?}"
    );
    assert_ne!(
        parked.files, live_files,
        "the two runs hold the same count, so this gate cannot tell them apart"
    );
}

/// Every way out of the list is the way out of the gestures sheet.
#[test]
fn the_list_closes_the_way_the_sheet_closes() {
    let scratch = deep("standing-close", 3);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    let mut app = App::new();
    let view = drawn(&mut frame, Standing::Current);
    opened(&worktree, &mut frame, &mut app, 8);

    let regions = vigia::regions(
        ratatui::layout::Rect::new(0, 0, 80, 20),
        &chrome_of(&app),
        &view,
    );
    let over = regions.positions.expect("the pane drew no box");
    assert_eq!(
        vigia::positions_route(&key(KeyCode::Esc), Some(over)),
        vigia::PositionsRoute::Close,
        "`Esc` does not close the list"
    );
    assert_eq!(
        vigia::positions_route(&press(over.close.0, over.close.1), Some(over)),
        vigia::PositionsRoute::Close,
        "the close control does not close the list"
    );
    assert_eq!(
        vigia::positions_route(&press(0, 0), Some(over)),
        vigia::PositionsRoute::Close,
        "a press outside the box does not close it, which is the sheet's own rule"
    );
    assert_eq!(
        vigia::positions_route(&key(KeyCode::Char('r')), Some(over)),
        vigia::PositionsRoute::Through,
        "`r` stopped reaching the map underneath, so the mode is wider than its edge \
         says it is"
    );
}

/// One overlay at a time, whichever one the reader asks for.
#[test]
fn only_one_overlay_is_ever_up() {
    let scratch = deep("standing-one-overlay", 2);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    let mut app = App::new();
    let _ = drawn(&mut frame, Standing::Current);

    app.apply(Action::ToggleMenu, &mut frame, 20).expect("menu");
    app.apply(Action::TogglePositions, &mut frame, 20)
        .expect("list");
    assert!(
        app.positions_open() && !app.menu_open(),
        "opening the list left the config menu up beside it, which B22 refuses"
    );

    app.apply(Action::ToggleMenu, &mut frame, 20).expect("menu");
    assert!(
        app.menu_open() && !app.positions_open(),
        "opening the menu left the list up beside it"
    );

    app.apply(Action::TogglePositions, &mut frame, 20)
        .expect("list");
    app.apply(Action::ToggleSheet, &mut frame, 20)
        .expect("sheet");
    assert!(
        !app.positions_open(),
        "opening the gestures sheet left the list up beside it"
    );
}

/// The walk extends one page before the caret runs out of rows.
///
/// The extension itself needs a repository, so the decision is what this holds: a
/// caret inside the last page asks for more, and one that has everything does not.
#[test]
fn the_list_extends_before_the_caret_reaches_its_end() {
    let scratch = deep("standing-extend", 12);
    let worktree = scratch.worktree();
    let page = 6;
    let walk = worktree.commits_from(None, page).expect("a page");
    assert!(
        walk.more,
        "the fixture's history is not longer than one page, so this gate cannot see \
         the walk extend"
    );
    let places = vigia::Places {
        commits: walk.commits,
        more: walk.more,
        current: None,
        point: None,
    };

    let top = vigia::positions::Caret { at: 0, top: 0 };
    assert!(
        vigia::resume_from(top, &places, Reading::Since).is_none(),
        "a caret at the top asked for another page, so opening the list walks the \
         whole history"
    );
    let last = vigia::positions::Caret {
        at: places.rows(Reading::Since) - 1,
        top: 0,
    };
    // The commit as well as the decision: a caret near the end that resumed from the
    // wrong place would draw a second page that is not behind the first, and nothing
    // on screen would say so.
    assert_eq!(
        vigia::resume_from(last, &places, Reading::Since).map(|at| at.id),
        places.commits.last().map(|at| at.id),
        "the walk resumes from something other than the last commit it holds"
    );

    let exhausted = vigia::Places {
        more: false,
        ..places.clone()
    };
    assert!(
        vigia::resume_from(last, &exhausted, Reading::Since).is_none(),
        "a walk with nothing left behind it still asks for another page"
    );

    // And the append, which is the other half: the next page goes on the end, `more`
    // comes from the page that knows it, and a tip handed back twice is dropped rather
    // than drawn twice.
    let mut grown = places.clone();
    let from = vigia::resume_from(last, &places, Reading::Since)
        .expect("a resume point")
        .id;
    let next = worktree
        .commits_from(Some(from), page)
        .expect("a second page");
    let (was, coming) = (grown.rows(Reading::Since), next.commits.len());
    assert!(
        coming > 0,
        "the second page is empty, so the append is unasserted"
    );
    grown.extend(next);
    assert_eq!(
        grown.rows(Reading::Since),
        was + coming,
        "the second page did not go onto the end of the first"
    );
    let ids: Vec<String> = grown.commits.iter().map(|at| at.named.clone()).collect();
    let mut unique = ids.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(
        ids.len(),
        unique.len(),
        "a commit is in the list twice, so a resumed walk repeated its own tip"
    );

    // A page of commits already held changes nothing but the tail.
    let repeat = worktree
        .commits_from(None, page)
        .expect("the first page again");
    let held = grown.rows(Reading::Since);
    grown.extend(repeat);
    assert_eq!(
        grown.rows(Reading::Since),
        held,
        "a page of commits already held was appended a second time"
    );
}

/// Nothing goes inert while the list is open. The second reading is what changes
/// that, and the first must not.
#[test]
fn nothing_is_inert_while_the_list_is_open() {
    let scratch = deep("standing-live", 3);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    let mut app = App::new();
    let _ = drawn(&mut frame, Standing::Current);
    let before = app.settings();
    opened(&worktree, &mut frame, &mut app, 8);

    // The letters the list does not own still reach the map, and still do their work.
    for action in [
        Action::ToggleFollow,
        Action::ToggleNotes,
        Action::ToggleStaged,
    ] {
        app.apply(action, &mut frame, 20).expect("the toggle quit");
    }
    let after = app.settings();
    assert!(
        after.follow != before.follow
            && after.notes != before.notes
            && after.staged != before.staged,
        "a toggle went inert while the list was open, which is #509's rule and not \
         this one's"
    );
    assert!(
        app.positions_open(),
        "a toggle underneath the list put the list away"
    );
}

/// `current` keeps counting while the list is open, under either standing.
///
/// Under `since` the live row is a run this frame is not drawing, so it is the one
/// the shell walks; the gate drives the same seam by handing the app what a walk of
/// that run holds and asserting the row moved.
#[test]
fn current_keeps_counting_while_the_list_is_open() {
    let scratch = deep("standing-counting", 3);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    let mut app = App::new();
    let view = drawn(&mut frame, Standing::Current);
    opened(&worktree, &mut frame, &mut app, 8);

    let before = list_rows(&screen(&view, &app, 80, 20))[0].clone();
    assert!(
        before.contains("1 changed"),
        "the live row does not count the tree the fixture wrote: {before:?}"
    );

    // A second write to the tree, and the row the shell refreshes moves with it.
    scratch.write("src/second.rs", "five\n");
    let mut places = app.places().clone();
    let walked = worktree
        .count_of(vigia_core::Origin::Unstaged, None)
        .expect("a count");
    places.current = Some(vigia::Facts {
        files: walked.shown,
        added: 2,
        removed: 0,
    });
    app.set_places(places);

    let after = list_rows(&screen(&view, &app, 80, 20))[0].clone();
    assert!(
        after.contains("2 changed"),
        "the live row did not count the write that arrived while the list was open: \
         {after:?}"
    );
}

/// Every pane the box could be asked to draw in, including the ones below its floor.
///
/// The bounds are the drawer's rather than the product's, which is the config menu's
/// sweep and its reason: I6 names forty columns, `positions_plan` is asked for a box at
/// every size a terminal can be, and the region below forty is exactly where an
/// underflow or a rect past the buffer hides from every other gate here. Zero is in
/// both ranges because a pane can be reported at zero between a resize and the frame
/// after it.
const SWEEP_WIDTHS: std::ops::RangeInclusive<u16> = 0..=160;
const SWEEP_HEIGHTS: std::ops::RangeInclusive<u16> = 0..=48;

/// The box never leaves the pane, and never covers the row the header owns.
#[test]
fn the_list_never_leaves_the_pane_at_any_size_a_terminal_can_be() {
    let scratch = deep("standing-sweep", 9);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    let mut app = App::new();
    let view = drawn(&mut frame, Standing::Current);
    opened(&worktree, &mut frame, &mut app, 12);

    let chrome = chrome_of(&app);
    let (mut drew, mut declined) = (0usize, 0usize);
    for w in SWEEP_WIDTHS {
        for h in SWEEP_HEIGHTS {
            let at = ratatui::layout::Rect::new(0, 0, w, h);
            let laid = vigia::regions(at, &chrome, &view);
            let Some(box_at) = laid.positions else {
                declined += 1;
                continue;
            };
            drew += 1;
            assert!(
                box_at.left + box_at.width <= w && box_at.top + box_at.height <= h,
                "at {w}x{h} the box runs from ({}, {}) for {}x{}, past the pane",
                box_at.left,
                box_at.top,
                box_at.width,
                box_at.height
            );
            // The header owns row zero, so a box over it covers the one fact a reader
            // cannot recover from the body.
            assert!(box_at.top >= 1, "at {w}x{h} the box covers the header row");

            // And what was published is what was painted: the box has to be on the
            // screen rather than only in a rect, or the pointer is told about cells
            // nothing drew.
            let buffer = screen(&view, &app, w, h);
            let edges: Vec<u16> = (box_at.top..box_at.top + box_at.height)
                .filter(|y| {
                    (box_at.left..box_at.left + box_at.width)
                        .any(|x| buffer[(x, *y)].symbol() != " ")
                })
                .collect();
            assert_eq!(
                edges.len(),
                usize::from(box_at.height),
                "at {w}x{h} the published box has {} rows and the painted one has {}",
                box_at.height,
                edges.len()
            );
        }
    }
    // Non-vacuity both ways: a sweep that drew nothing passes every assertion above,
    // and one that drew everywhere never reaches the floor this exists to cross.
    assert!(
        drew > 0 && declined > 0,
        "the sweep drew {drew} boxes and declined {declined}, so it never crossed the \
         floor it exists to cross"
    );
}

/// `b` then `B` opens the list on the branch point's row, not on the first one.
///
/// The rows are walked on the frame after the key, so at the moment `B` arrives the
/// branch point's row does not exist to land on. The caret is owed its row until the
/// walk hands one over, and this is the path where that matters: it is how a reader
/// who pressed `b` and then wanted somewhere else gets there.
#[test]
fn the_caret_lands_on_the_branch_point_once_its_row_is_walked() {
    let scratch = deep("standing-owed", 4);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    let mut app = App::new();
    let _ = drawn(&mut frame, Standing::Current);

    // `b`, then `B`, on a pane that has never drawn a list: no rows are walked yet.
    app.apply(Action::ToggleStanding, &mut frame, 20)
        .expect("b");
    assert_eq!(*app.asked(), Asked::BranchPoint);
    app.apply(Action::TogglePositions, &mut frame, 20)
        .expect("B");
    assert!(
        app.places().point.is_none(),
        "the fixture walked a branch point before the shell did, so this gate is not \
         standing where the defect was"
    );

    // The walk arrives, and the caret lands on the row it was owed.
    let (_, named) = worktree.branch_point().expect("a branch point");
    let walk = worktree.commits_from(None, 8).expect("a page");
    app.set_places(vigia::Places {
        commits: walk.commits,
        more: walk.more,
        current: Some(vigia::Facts::default()),
        point: Some((named, Some(vigia::Facts::default()))),
    });
    assert_eq!(
        app.positions_caret().expect("no caret").at,
        1,
        "the list opened on the live pane's row while the pane was standing at the \
         branch point, so it says the reader is somewhere they are not"
    );

    // And a reader who moves the caret keeps it. Moved while the caret is still owed,
    // which is the order that makes the clearing load bearing: rows arriving after would
    // otherwise land it back on the row the pane stands on.
    app.apply(Action::ToggleStanding, &mut frame, 20)
        .expect("b");
    assert_eq!(*app.asked(), Asked::Current);
    app.apply(Action::PositionsMove(2), &mut frame, 20)
        .expect("move");
    let moved = app.positions_caret().expect("no caret").at;
    assert_ne!(
        moved, 0,
        "the move went nowhere, so the assertion below is vacuous"
    );
    app.set_places(app.places().clone());
    assert_eq!(
        app.positions_caret().expect("no caret").at,
        moved,
        "rows arriving moved the caret the reader had put somewhere, so scrolling the          list fights whatever the pane happens to be standing on"
    );
}

/// A commit on the branch the list is on grows it; a checkout replaces it.
///
/// The first is the ordinary case and the one worth protecting: the pane watches a
/// tree an agent is committing into, so clearing the rows whenever HEAD moves would
/// take the reader's scroll depth away several times a minute. The second shares no
/// history, and two branches interleaved by date are a list that is neither.
#[test]
fn a_commit_grows_the_list_and_a_checkout_replaces_it() {
    let scratch = deep("standing-anchor", 6);
    let worktree = scratch.worktree();

    // A reader who has scrolled: two pages walked, not one.
    let first = worktree.commits_from(None, 3).expect("a page");
    let mut places = vigia::Places {
        commits: first.commits.clone(),
        more: first.more,
        current: None,
        point: None,
    };
    let from = first.commits.last().expect("three commits").id;
    places.extend(worktree.commits_from(Some(from), 3).expect("a second page"));
    let scrolled = places.rows(Reading::Since);
    let deepest = places.commits.last().expect("commits").id;
    assert!(
        scrolled > 3,
        "the fixture did not scroll, so nothing here is at risk"
    );

    // The same history again changes nothing at all.
    let again = worktree.commits_from(None, 3).expect("a page");
    assert!(
        !places.re_anchor(again),
        "a page of the history already held was read as another branch"
    );
    assert_eq!(
        places.rows(Reading::Since),
        scrolled,
        "an unchanged history moved the rows"
    );

    // One commit lands on the same branch: it goes on the front and the rest stays.
    scratch.write("src/base.rs", "one more\n");
    scratch.git(&["add", "-A"]);
    scratch.git(&["commit", "-m", "the agent committed"]);
    let grown = worktree.commits_from(None, 3).expect("a page");
    assert!(
        !places.re_anchor(grown),
        "a commit on the branch the list is already on replaced every row"
    );
    assert_eq!(
        places.rows(Reading::Since),
        scrolled + 1,
        "the new commit did not go onto the front of what was already walked"
    );
    assert_eq!(
        places.commits.first().map(|at| at.subject.clone()),
        Some("the agent committed".to_owned()),
        "the list does not lead with the commit that just landed"
    );
    assert_eq!(
        places.commits.last().map(|at| at.id),
        Some(deepest),
        "the reader's scroll depth was discarded by a commit on their own branch"
    );

    // A checkout onto a divergent branch shares no head, so the rows are replaced.
    scratch.git(&["checkout", "-b", "other", "HEAD~4"]);
    scratch.write("src/base.rs", "elsewhere\n");
    scratch.git(&["add", "-A"]);
    scratch.git(&["commit", "-m", "on the other branch"]);
    let moved = worktree.commits_from(None, 3).expect("a page");
    assert!(
        places.re_anchor(moved),
        "a checkout onto a branch the list shares no head with grew the list instead \
         of replacing it, so it now draws two histories interleaved by date"
    );
    assert_eq!(
        places.commits.first().map(|at| at.subject.clone()),
        Some("on the other branch".to_owned()),
        "the replaced list does not lead with the branch it is now on"
    );
    assert_eq!(
        places.commits.len(),
        3,
        "the replaced list is not the page it was replaced by"
    );
}

/// A row the pane stands on that the list does not hold opens on the first row.
///
/// Reachable: a commit picked from the list and then garbage-collected, or a walk
/// re-anchored by a checkout while the pane stands on the old branch's commit. The
/// fallback has to be the live pane's row, which is the one row that always exists.
#[test]
fn a_standing_commit_the_list_does_not_hold_opens_on_the_first_row() {
    let scratch = deep("standing-absent", 3);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    let mut app = App::new();
    let _ = drawn(&mut frame, Standing::Current);

    // A short page on purpose: the branch point has to sit behind what the list holds,
    // or the fallback below is never reached.
    let walk = worktree.commits_from(None, 2).expect("a page");
    let held = walk.commits[1].clone();
    app.set_places(vigia::Places {
        commits: walk.commits.clone(),
        more: walk.more,
        current: None,
        point: None,
    });
    app.stands(Asked::At(Standing::Since {
        at: held.id,
        named: held.named.clone(),
    }));
    app.apply(Action::TogglePositions, &mut frame, 20)
        .expect("B");
    assert_eq!(
        app.positions_caret().expect("no caret").at,
        2,
        "the caret did not find the commit the pane is standing on, so the fallback \
         below is not being told apart from a match"
    );
    app.apply(Action::TogglePositions, &mut frame, 20)
        .expect("close");

    // And one it does not hold: the branch point's own commit, which is behind every
    // row the page walked.
    let (at, named) = worktree.branch_point().expect("a branch point");
    assert!(
        !walk.commits.iter().any(|commit| commit.id == at),
        "the fixture's page holds the branch point, so this gate is not standing on a \
         commit the list is missing"
    );
    app.stands(Asked::At(Standing::Since { at, named }));
    app.apply(Action::TogglePositions, &mut frame, 20)
        .expect("B");
    assert_eq!(
        app.positions_caret().expect("no caret").at,
        0,
        "a pane standing on a commit the list does not hold opened somewhere other \
         than the live pane's row, which is the one row that is always there"
    );
}

/// A named row's totals hold the header's own field widths, not just its words.
///
/// `-N` ends on the box's right inset and `+N` sits a count cell to its left, so the
/// two are read against each other by their sigils. Asserting the words alone leaves
/// the arithmetic free: drop a column from the field and every number still appears.
#[test]
fn a_named_rows_totals_stand_in_the_columns_the_header_counts_in() {
    let scratch = deep("standing-columns", 3);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    let mut app = App::new();
    let view = drawn(&mut frame, Standing::Current);
    opened(&worktree, &mut frame, &mut app, 8);

    let buffer = screen(&view, &app, 80, 20);
    let row = list_rows(&buffer)[0].clone();
    let plus = row.find("+1").expect("the live row draws no added total");
    let minus = row.find("-0").expect("the live row draws no removed total");
    assert_eq!(
        minus - plus,
        "+1".len() + vigia::COUNT_CELL - "-0".len() + 1,
        "the gap between the two totals is not the count cell the header lays its own \
         out against, so the sigils no longer line up down the column: {row:?}"
    );

    // The count sits a gap left of the added total, so the three cells are one field
    // rather than two that happen to be near each other.
    let changed = row.find("1 changed").expect("the live row draws no count");
    assert_eq!(
        plus - (changed + "1 changed".len()),
        vigia::positions_gap(),
        "the count is not a gap from the totals, so the row reads as two fields: {row:?}"
    );

    // And the right edge: `-0` ends on the box's inset, whatever the numbers spell.
    let edge = row.rfind('│').expect("the row draws no frame");
    assert_eq!(
        edge - (minus + "-0".len()),
        vigia::OVERLAY_FRAME / 2 + 1,
        "the removed total does not end on the box's own inset: {row:?}"
    );
}

/// A branch with three commits on it, one staged file and one written, so the
/// three readings hold three different runs and none can satisfy another's
/// assertion by accident.
fn history(name: &str) -> Scratch {
    branched(name, 2)
}

/// The newest commit on the branch, as a row of the position list names it.
fn tip(worktree: &Worktree) -> vigia_core::Landmark {
    worktree
        .commits_from(None, 1)
        .expect("a page of history")
        .commits
        .into_iter()
        .next()
        .expect("a commit")
}

/// Stand the app at `commit` under `reading`, the way choosing a row of the list
/// does, and walk the frame there.
fn stand_at(app: &mut App, frame: &mut Frame, commit: &vigia_core::Landmark, reading: Reading) {
    let (at, named) = (commit.id, commit.named.clone());
    app.stands(Asked::At(match reading {
        Reading::Since => Standing::Since { at, named },
        Reading::Only => Standing::Only { at, named },
    }));
    frame.stand(match app.asked() {
        Asked::At(standing) => standing.clone(),
        other => panic!("the request is {other:?} rather than the commit just asked for"),
    });
    frame.advance().expect("advance");
}

/// Everything one pane draws, through `App::view` and the painter rather than
/// through a hand-built `View`: the gates below are about cells, and the
/// assignment that fills a field from the engine is invisible to a literal.
struct Painted {
    text: String,
    view: View,
}

impl Painted {
    /// Whether any drawn cell carries `mark`.
    fn draws(&self, mark: &str) -> bool {
        self.text.contains(mark)
    }

    /// The file rows of the pinned list, without the run separators between them.
    fn entries(&self) -> impl Iterator<Item = &vigia::FileEntry> {
        self.view.list.iter().filter_map(vigia::ListRow::entry)
    }
}

/// One pane, painted. `history` is the watch's store, which the sparkline, the
/// pulse and the recency ramp are read from.
fn painted(app: &mut App, frame: &mut Frame, watch: &History) -> Painted {
    painted_at(app, frame, watch, 120)
}

/// The same, on a pane `width` columns across.
fn painted_at(app: &mut App, frame: &mut Frame, watch: &History, width: u16) -> Painted {
    let pane = ratatui::layout::Rect {
        x: 0,
        y: 0,
        width,
        height: 30,
    };
    let mut highlighter = Highlighter::eager();
    let standing = frame.standing().clone();
    let chrome = app.chrome(
        "fixture",
        Some("work"),
        vigia::Stood {
            standing: &standing,
            now: 0,
        },
        Pointing::default(),
        Default::default(),
        "",
    );
    let body = vigia::body_layout(pane, &chrome, frame.files().len(), frame.files().len());
    let view = app
        .view(frame, &mut highlighter, watch, body)
        .expect("collect a view");
    // Rebuilt after the collect the way the shell rebuilds it, so a count this
    // frame placed reaches this frame's footer.
    let chrome = app.chrome(
        "fixture",
        Some("work"),
        vigia::Stood {
            standing: &standing,
            now: 0,
        },
        Pointing::default(),
        Default::default(),
        "",
    );
    let mut terminal = Terminal::new(TestBackend::new(pane.width, pane.height)).expect("terminal");
    let theme = Theme::default();
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
    let backend = terminal.backend().clone();
    Painted {
        text: rows_of(backend.buffer()).join(
            "
",
        ),
        view,
    }
}

/// A watch store that has just seen every path in `frame` written.
fn watched(frame: &Frame) -> History {
    let mut watch = History::new();
    let paths: Vec<String> = frame
        .files()
        .iter()
        .map(|change| change.path.clone())
        .collect();
    watch.record(paths.iter().map(String::as_str), std::time::Instant::now());
    watch
}

/// A note on the first line of the first file the pane draws.
fn note_on(frame: &mut Frame) -> Vec<vigia_core::Note> {
    let (_, diff) = frame.diff(0).expect("a diff");
    let (line, text) = diff.rows_on(vigia_core::Side::New)[0];
    vec![vigia_core::Note {
        id: "n0".to_owned(),
        path: diff.path.clone(),
        side: vigia_core::Side::New,
        line,
        text: text.to_owned(),
        body: "a question for the agent".to_owned(),
        status: vigia_core::Status::Open,
        reply: None,
        written: std::time::SystemTime::now(),
    }]
}

/// The token spells the second reading and the commit it names.
#[test]
fn the_token_reads_only_and_the_id() {
    let scratch = history("only-token");
    let worktree = scratch.worktree();
    let commit = tip(&worktree);
    let mut app = App::new();
    let mut frame = worktree.frame();
    stand_at(&mut app, &mut frame, &commit, Reading::Only);

    let standing = frame.standing().clone();
    let mut highlighter = Highlighter::eager();
    let watch = History::new();
    let view = View::collect(&mut frame, &mut highlighter, &watch, viewport()).expect("collect");

    assert_eq!(standing.label(), format!("only {}", commit.named));
    let drawn = header(&view, &standing, &app);
    assert!(
        drawn.contains(&format!(
            "fixture{FACT_JOIN}work{FACT_JOIN}only {}",
            commit.named
        )),
        "the header does not name the commit the pane is reading alone: {drawn:?}"
    );
}

/// One key flips the reading, and the token and the body follow it together.
#[test]
fn the_key_flips_the_reading_with_the_list_closed() {
    let scratch = history("only-flip");
    let worktree = scratch.worktree();
    let commit = tip(&worktree);
    let mut app = App::new();
    let mut frame = worktree.frame();
    stand_at(&mut app, &mut frame, &commit, Reading::Since);
    let since_files = frame.files().len();

    app.apply(Action::ToggleReading, &mut frame, 0)
        .expect("the key");
    assert_eq!(app.reading(), Reading::Only, "the key flipped nothing");
    let Asked::At(standing) = app.asked().clone() else {
        panic!("the flip moved the pane off the commit it was standing at");
    };
    assert_eq!(standing.at(), Some(commit.id), "the flip moved the commit");
    frame.stand(standing);
    frame.advance().expect("advance");

    // Non-vacuity, and the point of the reading: one commit is not everything
    // since it, and the fixture is built so the two cannot be the same number.
    assert!(
        frame.files().len() < since_files,
        "the commit alone holds {} files against the range's {since_files}, so \
         the two readings are drawing one run and the flip is unasserted",
        frame.files().len()
    );

    app.apply(Action::ToggleReading, &mut frame, 0)
        .expect("the key again");
    assert_eq!(
        app.reading(),
        Reading::Since,
        "the key is a mode rather than a toggle: it does not read back"
    );
}

/// The same key with the box open, which is where a reader meets the two words.
#[test]
fn the_key_flips_the_reading_with_the_list_open() {
    let scratch = history("only-flip-open");
    let worktree = scratch.worktree();
    let commit = tip(&worktree);
    let mut app = App::new();
    let mut frame = worktree.frame();
    stand_at(&mut app, &mut frame, &commit, Reading::Since);
    app.apply(Action::TogglePositions, &mut frame, 0)
        .expect("open the list");
    assert!(app.positions_open(), "the list did not open");

    app.apply(Action::ToggleReading, &mut frame, 0)
        .expect("the key");

    assert_eq!(app.reading(), Reading::Only, "the key flipped nothing");
    assert!(
        app.positions_open(),
        "flipping the reading closed the list, so the reader has to reopen it to \
         see what the word now lists"
    );
}

/// Where no commit is named there is no reading to flip, and the footer says so.
///
/// `current` has no commit at all, and the branch point's is one the *other*
/// branch made: reading it alone would draw work this branch did not do.
#[test]
fn the_key_refuses_where_no_commit_is_named() {
    let scratch = history("only-refusal");
    let worktree = scratch.worktree();
    let mut app = App::new();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");

    for asked in [Asked::Current, Asked::BranchPoint] {
        app.stands(asked.clone());
        app.clear_notice();
        app.apply(Action::ToggleReading, &mut frame, 0)
            .expect("the key");
        assert_eq!(
            *app.asked(),
            asked,
            "the key moved the pane off {asked:?}, which is not a reading it can \
             be read the other way"
        );
        assert_eq!(app.reading(), Reading::Since);
        let said = app.notice().unwrap_or_default().to_owned();
        assert!(
            said.contains("only") && said.contains('B'),
            "the footer says {said:?} from {asked:?}, which does not say what \
             could not be done or which gesture leads to a commit"
        );
    }

    // And the gesture reaches the footer at every width the pane has, which one
    // width cannot say: a notice is one token the footer clips from the right, and
    // between forty-five and fifty-seven it shares its row with a note count and a
    // position, which leaves it around twenty columns. At forty it has the row to
    // itself, so a gate that asked only there would have passed on a line cut in
    // half everywhere above it.
    app.set_notes(note_on(&mut frame));
    app.clear_notice();
    app.apply(Action::ToggleReading, &mut frame, 0)
        .expect("the key");
    let mut whole = 0usize;
    for width in 40u16..=140 {
        let drawn = painted_at(&mut app, &mut frame, &History::new(), width);
        let footer = last_line(&drawn.text);
        assert!(
            footer.contains(GESTURE),
            "the refusal at {width} columns is {footer:?}, which has lost the \
             gesture that answers it"
        );
        whole += usize::from(footer.contains(NOTHING_TO_READ));
    }
    // Non-vacuity: a line nothing ever draws whole would satisfy the sweep.
    assert!(
        whole > 40,
        "the whole refusal was drawn at only {whole} of the widths swept, so the \
         sweep is about a line the pane never finds room for"
    );
}

/// Under `only` the list is commit rows alone, and its title is the reading.
#[test]
fn the_list_under_only_draws_commits_alone() {
    let scratch = history("only-list");
    let worktree = scratch.worktree();
    let commit = tip(&worktree);
    let page = worktree.commits_from(None, 9).expect("a page");
    let places = vigia::Places {
        commits: page.commits.clone(),
        more: page.more,
        current: Some(vigia::Facts::default()),
        point: Some(("main".to_owned(), None)),
    };

    let live = places.rows(Reading::Since);
    let parked = places.rows(Reading::Only);
    assert_eq!(
        parked,
        page.commits.len(),
        "the list under `only` draws something other than the commits"
    );
    assert_eq!(
        live - parked,
        2,
        "the list lost {} rows rather than the two named places, which is the \
         explanation the box gives instead of a line of text",
        live - parked
    );
    assert!(
        matches!(
            places.row_at(0, Reading::Only),
            Some(vigia::positions::Row::Commit(0))
        ),
        "the first row under `only` is not the newest commit"
    );
    assert_eq!(
        vigia::positions::title(Reading::Only),
        "only",
        "the box is titled with something other than the reading it will apply"
    );

    // And the caret opens on the row the pane stands on, under the reading it
    // stands in: a list of destinations says where you are in no ink at all.
    let mut app = App::new();
    let mut frame = worktree.frame();
    stand_at(&mut app, &mut frame, &commit, Reading::Only);
    app.set_places(places);
    app.apply(Action::TogglePositions, &mut frame, 0)
        .expect("open the list");
    assert_eq!(
        app.positions_caret().map(|caret| caret.at),
        Some(0),
        "the caret did not open on the commit the pane is standing at"
    );
}

/// Choosing a row keeps the reading the box is titled with.
#[test]
fn a_chosen_row_keeps_the_reading() {
    let scratch = history("only-pick");
    let worktree = scratch.worktree();
    let page = worktree.commits_from(None, 9).expect("a page");
    let commit = page.commits[0].clone();
    let second = page.commits[1].clone();

    let mut app = App::new();
    let mut frame = worktree.frame();
    stand_at(&mut app, &mut frame, &commit, Reading::Only);
    app.set_places(vigia::Places {
        commits: page.commits,
        more: page.more,
        current: None,
        point: None,
    });
    app.apply(Action::TogglePositions, &mut frame, 0)
        .expect("open the list");
    app.apply(Action::PositionsMove(1), &mut frame, 0)
        .expect("move the caret");
    app.apply(Action::PositionsPick, &mut frame, 0)
        .expect("stand there");

    let Asked::At(standing) = app.asked().clone() else {
        panic!("choosing a commit row did not stand the pane at a commit");
    };
    assert_eq!(
        standing.at(),
        Some(second.id),
        "the caret moved one row and the pane stood somewhere else"
    );
    assert_eq!(
        standing.reading(),
        Reading::Only,
        "choosing a second commit put the reader back into the other reading, \
         which the box's own title said it would not"
    );
}

/// A commit that changed nothing names itself rather than the reading.
///
/// `no changes only a1b2c3` is not a sentence, and the id is on the header
/// directly above, so the line points rather than repeats.
#[test]
fn the_empty_state_names_the_commit_rather_than_the_reading() {
    let scratch = Scratch::new("only-empty");
    scratch.write("src/base.rs", "one\n");
    scratch.git(&["add", "-A"]);
    scratch.git(&["commit", "-q", "-m", "init"]);
    scratch.git(&[
        "commit",
        "-q",
        "--allow-empty",
        "-m",
        "a commit that did nothing",
    ]);
    let worktree = scratch.worktree();
    let commit = tip(&worktree);

    let mut app = App::new();
    let mut frame = worktree.frame();
    stand_at(&mut app, &mut frame, &commit, Reading::Only);
    let standing = frame.standing().clone();
    let mut highlighter = Highlighter::eager();
    let watch = History::new();
    let view = View::collect(&mut frame, &mut highlighter, &watch, viewport()).expect("collect");

    assert_eq!(view.files, 0, "the empty commit drew rows");
    let line = body_line(&view, &standing, &app);
    assert!(
        line.contains("nothing in this commit"),
        "the empty pane says {line:?} while reading one commit alone"
    );
    assert!(
        !line.contains("only "),
        "the line repeats the reading, so it reads as a phrase rather than a \
         sentence: {line:?}"
    );
}

/// `b` brings a parked pane home, as it does from anywhere else.
#[test]
fn b_comes_home_from_only() {
    let scratch = history("only-home");
    let worktree = scratch.worktree();
    let commit = tip(&worktree);
    let mut app = App::new();
    let mut frame = worktree.frame();
    stand_at(&mut app, &mut frame, &commit, Reading::Only);

    app.apply(Action::ToggleStanding, &mut frame, 0)
        .expect("the key");

    assert_eq!(
        *app.asked(),
        Asked::Current,
        "`b` from a commit read alone does not come home, so the reader has no \
         one key back to the pane they started on"
    );
}

/// Nothing goes inert under `since`, which is the half no `only` gate can see.
///
/// A lever that fired in both readings would satisfy every assertion below about
/// the parked pane and silence the live one too, and nothing there would say so.
#[test]
fn nothing_goes_inert_under_since() {
    let scratch = history("since-live");
    let worktree = scratch.worktree();
    let commit = tip(&worktree);
    let mut app = App::new();
    let mut frame = worktree.frame();
    stand_at(&mut app, &mut frame, &commit, Reading::Since);
    let watch = watched(&frame);
    app.set_notes(note_on(&mut frame));

    let before = app.settings();
    for action in [
        Action::ToggleFollow,
        Action::ToggleStaged,
        Action::ToggleNotes,
    ] {
        app.apply(action, &mut frame, 0).expect("the key");
    }
    let after = app.settings();
    assert_ne!(
        after.follow, before.follow,
        "`f` reached nothing under `since`"
    );
    assert_ne!(
        after.staged, before.staged,
        "`a` reached nothing under `since`"
    );
    assert_ne!(
        after.notes, before.notes,
        "`c` reached nothing under `since`"
    );

    // Put the three back, so the pane below is the one a reader opens on.
    for action in [
        Action::ToggleFollow,
        Action::ToggleStaged,
        Action::ToggleNotes,
    ] {
        app.apply(action, &mut frame, 0).expect("the key");
    }
    let drawn = painted(&mut app, &mut frame, &watch);
    assert!(
        drawn.view.notes.writable,
        "a note cannot be written on a live pane"
    );
    assert!(
        drawn.draws("✎"),
        "the reader's note has no mark:\n{}",
        drawn.text
    );
    assert!(
        drawn.draws("1 note"),
        "the footer does not count the reader's note on a live pane, so the \
         parked gate's count assertion would be satisfied by a footer that \
         never counts:\n{}",
        drawn.text
    );
    assert!(
        drawn.entries().any(|entry| entry.newest),
        "no row carries the pulse, so the watch's own marks are unasserted here"
    );
    assert!(
        drawn.draws(PULSE),
        "no row draws the pulse, so the parked gate's absence of one is satisfied \
         by a painter that never draws it at all:\n{}",
        drawn.text
    );
    assert!(
        drawn
            .entries()
            .any(|entry| entry.spark.iter().any(|bucket| *bucket > 0)),
        "no sparkline has a sample in it on a live pane"
    );
}

/// Under `only` the notes are not drawn, and `c` reaches nothing.
#[test]
fn the_notes_go_inert_under_only() {
    let scratch = history("only-notes");
    let worktree = scratch.worktree();
    let commit = tip(&worktree);
    let mut app = App::new();
    let mut frame = worktree.frame();
    stand_at(&mut app, &mut frame, &commit, Reading::Only);
    let watch = History::new();
    app.set_notes(note_on(&mut frame));

    let drawn = painted(&mut app, &mut frame, &watch);
    assert!(
        !drawn.draws("✎"),
        "a note is marked on a line the agent beside the pane cannot act \
         on:\n{}",
        drawn.text
    );
    assert!(
        !drawn.view.notes.writable,
        "the screen still offers to open a box on a historical line"
    );
    assert!(
        drawn.view.anchor_at(1).is_none(),
        "a press on a content row would still anchor a note into history"
    );
    assert!(
        !drawn.draws("1 note"),
        "the footer counts a conversation about a tree the body is not \
         drawing:\n{}",
        drawn.text
    );

    let before = app.settings();
    app.apply(Action::ToggleNotes, &mut frame, 0)
        .expect("the key");
    assert_eq!(
        app.settings().notes,
        before.notes,
        "`c` flipped the note rows on a pane that draws none"
    );
}

/// Under `only` the staged run is neither walked nor counted, and `a` is inert.
#[test]
fn the_staged_run_goes_inert_under_only() {
    let scratch = history("only-staged");
    let worktree = scratch.worktree();
    let commit = tip(&worktree);
    let mut app = App::new();
    let mut frame = worktree.frame();
    stand_at(&mut app, &mut frame, &commit, Reading::Only);
    let watch = History::new();

    let before = app.settings();
    app.apply(Action::ToggleStaged, &mut frame, 0)
        .expect("the key");
    assert_eq!(
        app.settings().staged,
        before.staged,
        "`a` asked for a run that is only definable against the index"
    );

    // And with the toggle already on, which is a reader who pressed `a` before
    // they moved: the run must not follow them into history.
    let mut app = App::new();
    let mut frame = worktree.frame();
    app.apply(Action::ToggleStaged, &mut frame, 0)
        .expect("the key on a live pane");
    assert!(app.staged(), "`a` reached nothing on the live pane");
    stand_at(&mut app, &mut frame, &commit, Reading::Only);

    assert!(
        !frame
            .files()
            .iter()
            .any(|change| change.path == "src/staged.rs"),
        "the staged run is walked beside a commit's own diff"
    );
    let drawn = painted(&mut app, &mut frame, &watch);
    assert!(
        !drawn.draws("staged"),
        "the header counts a staged run beside a historical one:\n{}",
        drawn.text
    );
}

/// Under `only` follow reaches nothing, and the flag returns as it was.
#[test]
fn follow_goes_inert_under_only_and_returns_as_it_was() {
    let scratch = history("only-follow");
    let worktree = scratch.worktree();
    let commit = tip(&worktree);
    let mut app = App::new();
    let mut frame = worktree.frame();
    // The pane opens following, which is I5, so this is the state a reader is in
    // when they stand somewhere else rather than one the gate had to arrange.
    assert!(
        app.settings().follow,
        "the pane no longer opens following, so the gate below arranges its own          starting state and says nothing about the one a reader has"
    );

    stand_at(&mut app, &mut frame, &commit, Reading::Only);
    let watch = History::new();
    app.apply(Action::ToggleFollow, &mut frame, 0)
        .expect("the key");
    assert!(
        app.settings().follow,
        "`f` disengaged follow while the pane was parked, so what the reader \
         left running is not what they come back to"
    );

    // The indicator rather than the word: the hint bar names the key either way,
    // and a gate that could not tell the two apart would pass on a pane drawing
    // no hints at all.
    let drawn = painted(&mut app, &mut frame, &watch);
    assert!(
        !drawn.draws(FOLLOWING),
        "the footer says the pane is following while `f` reaches nothing:\n{}",
        drawn.text
    );

    // And it is still on when the reading goes back, which is what keeping the
    // flag is for.
    app.apply(Action::ToggleReading, &mut frame, 0)
        .expect("the key");
    assert!(
        app.settings().follow,
        "follow did not return as it was when the reading went back"
    );
}

/// Under `only` the watch's time series describes another moment, so it is not
/// drawn. The heat strip is positional within a file and survives.
#[test]
fn the_sparkline_and_the_pulse_go_inert_under_only_and_the_heat_strip_survives() {
    let scratch = history("only-watch");
    let worktree = scratch.worktree();
    let commit = tip(&worktree);
    let mut app = App::new();
    let mut frame = worktree.frame();
    stand_at(&mut app, &mut frame, &commit, Reading::Only);
    // The store holds every path the commit touched, so a pane reading it would
    // draw a sparkline and a pulse on every row. That is what must not happen.
    let watch = watched(&frame);

    let drawn = painted(&mut app, &mut frame, &watch);
    assert!(
        drawn.entries().next().is_some(),
        "the parked pane lists no files, so nothing below is asserted"
    );
    for entry in drawn.entries() {
        assert!(
            entry.spark.iter().all(|bucket| *bucket == 0),
            "{} draws a sparkline of writes that happened after the commit it \
             is a row of",
            entry.path
        );
        assert!(
            !entry.newest,
            "{} carries the pulse, which says the watch just saw it written",
            entry.path
        );
        assert_eq!(
            entry.recency,
            vigia_core::Recency::Cold,
            "{} is drawn on a recency rung the watch cannot know about here",
            entry.path
        );
    }
    assert!(
        !drawn.draws(PULSE),
        "the pane draws the pulse over a commit the watch was never describing,          which the field above cannot see: the painter reads its own copy:
{}",
        drawn.text
    );
    assert!(
        drawn
            .entries()
            .any(|entry| entry.heat.iter().any(|bucket| bucket.total() > 0)),
        "no heat strip has a bucket in it, so the half that survives is \
         unasserted and this gate would pass on a pane that drew nothing"
    );
}

/// A write to the tree under `only` says so once and moves nothing.
#[test]
fn a_write_under_only_says_so_and_the_count_is_the_bursts() {
    assert_eq!(vigia::arrival_line(0), None, "an empty burst is not news");
    assert_eq!(
        vigia::arrival_line(1).as_deref(),
        Some("1 file written in the working tree"),
        "the line does not read as a sentence at one file"
    );
    assert_eq!(
        vigia::arrival_line(3).as_deref(),
        Some("3 files written in the working tree"),
        "the line does not say how much moved, or does not name the tree"
    );
    // It has to name the tree rather than the pane: a reader standing at a
    // commit reads `3 files changed` as the commit's own count.
    for said in [vigia::arrival_line(1), vigia::arrival_line(9)] {
        let said = said.expect("a burst says something");
        assert!(
            said.contains("working tree"),
            "{said:?} does not say which of the two the count is about"
        );
    }
}

/// The tick reaches the frame on a live pane and no frame on a parked one.
///
/// The predicate rather than the arm, and then the arm read for the call: a walk
/// under `only` re-diffs two commits that cannot have changed, and the arriving
/// marks it feeds would pulse rows the watch was never describing.
#[test]
fn a_parked_tick_reaches_no_frame() {
    assert!(
        Standing::Current.reading().is_live(),
        "the live pane's tick would skip its own walk"
    );
    let at = tip(&history("only-tick").worktree()).id;
    let named = "a1b2c3".to_owned();
    assert!(
        Standing::Since {
            at,
            named: named.clone()
        }
        .reading()
        .is_live(),
        "a range ending at the working tree stopped walking on a write"
    );
    assert!(
        !Standing::Only { at, named }.reading().is_live(),
        "a commit read alone still walks on every write to the tree"
    );

    let source = include_str!("../src/lib.rs");
    let arm = source
        .split("Wake::Tick(paths) => {")
        .nth(1)
        .expect("the tick arm is gone");
    let arm = &arm[..arm
        .find("Wake::WatchLost")
        .expect("the tick arm never ends")];
    // The guard whole, not the call inside it. A search for `shell.app.live()`
    // alone is answered by any expression that mentions it, `if false && ...`
    // included, and that mutant leaves a parked pane walking on every write with
    // every assertion here green.
    let (parked, walked) = (
        arm.find("if !frame.is_live() {")
            .expect("the tick no longer branches on where the pane stands"),
        arm.find("frame.advance()")
            .expect("the tick no longer walks"),
    );
    assert!(
        parked < walked,
        "the tick walks before it asks whether anything it walks for is drawn, \
         so a parked pane pays a status walk on every write"
    );
    // And the branch has to leave, or the lines below it run anyway.
    let branch = &arm[parked..walked];
    assert!(
        branch.contains("continue;"),
        "the parked branch falls through into the live path, so a parked pane \
         draws arrival marks on rows the watch is not describing:\n{branch}"
    );
    assert!(
        arm.contains("arrival_line"),
        "the tick no longer says anything on a write it does not act on, so a \
         parked pane cannot be told apart from a tree that stopped changing"
    );
}

/// A checkout takes the parked commit off the branch the list walks, and the caret
/// does not go and mark a different one as where the pane is.
///
/// The caret is the only ink saying *you are here*, so a caret sent to row 0 when
/// the standing is nowhere in the list claims the newest commit of a branch the
/// reader never asked for. Under `only` that row is a commit and looks exactly like
/// a correct answer.
#[test]
fn a_caret_never_marks_a_row_the_pane_is_not_standing_on() {
    let scratch = history("only-checkout");
    let worktree = scratch.worktree();
    let page = worktree.commits_from(None, 9).expect("a page");
    let commit = page.commits[1].clone();

    let mut app = App::new();
    let mut frame = worktree.frame();
    stand_at(&mut app, &mut frame, &commit, Reading::Only);
    app.set_places(vigia::Places {
        commits: page.commits.clone(),
        more: page.more,
        current: None,
        point: None,
    });
    app.apply(Action::TogglePositions, &mut frame, 0)
        .expect("open the list");
    assert_eq!(
        app.positions_caret().map(|caret| caret.at),
        Some(1),
        "the caret did not open on the commit the pane is standing at, so the \
         move below is unasserted"
    );

    // The rows the shell would hand over after a checkout to a branch this commit
    // is not on: a page that reaches nothing held, which is what `re_anchor`
    // replaces wholesale.
    let elsewhere = history("only-checkout-elsewhere");
    let other = elsewhere
        .worktree()
        .commits_from(None, 9)
        .expect("another branch's page");
    let mut places = app.places().clone();
    assert!(
        places.re_anchor(other.clone()),
        "the second page reaches a commit the first held, so this is not the \
         checkout case the gate is named for"
    );
    app.owe_caret();
    app.set_places(places);

    assert_eq!(
        app.positions_caret().map(|caret| caret.at),
        Some(1),
        "the caret moved to a row of a branch the pane is not standing on, so \
         the box marks a commit the reader never chose as the one they are at"
    );
}

/// The three inert keys say nothing, and that is a decision rather than an
/// oversight.
///
/// `O` refuses out loud because nothing on screen changes when it cannot act. These
/// three are the opposite: the token reads `only`, the note marks are gone, the
/// staged count is gone and the follow indicator is gone, so the pane is already
/// answering, and a line per press would be noise on a surface whose whole thesis is
/// that it can be glanced at.
#[test]
fn the_inert_keys_say_nothing_because_the_pane_already_does() {
    let scratch = history("only-silence");
    let worktree = scratch.worktree();
    let commit = tip(&worktree);
    let mut app = App::new();
    let mut frame = worktree.frame();
    stand_at(&mut app, &mut frame, &commit, Reading::Only);

    for action in [
        Action::ToggleNotes,
        Action::ToggleStaged,
        Action::ToggleFollow,
    ] {
        app.clear_notice();
        app.apply(action, &mut frame, 0).expect("the key");
        assert_eq!(
            app.notice(),
            None,
            "{action:?} put a line on the footer, and the pane below it is \
             already saying the same thing in every surface the reading silenced"
        );
    }

    // Non-vacuity: the footer is reachable from this pane, so the silence above is
    // the keys' and not the fixture's.
    app.apply(Action::ToggleStanding, &mut frame, 0)
        .expect("come home");
    app.apply(Action::ToggleReading, &mut frame, 0)
        .expect("the key with no commit to flip");
    assert!(
        app.notice().is_some(),
        "nothing reaches the footer from this pane at all, so the assertions \
         above hold whatever the inert keys do"
    );
}

/// Only a position that will not resolve moves the reader, and only where they
/// are standing somewhere to be moved from.
///
/// The rule is about the pair, and the pair is what a source-read cannot assert.
/// Under `only` it is the difference between a corrupt read of one object emptying
/// a pane that is fine and the next wake trying again: `Worktree::only` reaches two
/// of these variants and the shell must answer them differently.
#[test]
fn only_a_position_that_will_not_resolve_brings_the_pane_home() {
    let scratch = history("comes-home");
    let at = tip(&scratch.worktree()).id;
    let gone = || std::io::Error::other("the object database has lost it");
    let everywhere = [
        Standing::Current,
        Standing::Since {
            at,
            named: "main".to_owned(),
        },
        Standing::Only {
            at,
            named: "a1b2c3".to_owned(),
        },
    ];

    for standing in &everywhere {
        let home = matches!(standing, Standing::Current);
        assert_eq!(
            vigia::comes_home(&vigia_core::Error::Standing(Box::new(gone())), standing),
            !home,
            "a position that will not resolve does the wrong thing at {standing:?}"
        );
        // Every other failure, including both the `only` walk produces past its own
        // commit, leaves the reader where they are.
        for other in [
            vigia_core::Error::Comparison(Box::new(gone())),
            vigia_core::Error::Status(Box::new(gone())),
            vigia_core::Error::History(Box::new(gone())),
            vigia_core::Error::NoBranchPoint,
        ] {
            assert!(
                !vigia::comes_home(&other, standing),
                "{other:?} took the reader off {standing:?}, so one object the \
                 object database could not read empties a pane that is fine"
            );
        }
    }
}
