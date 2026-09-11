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
use vigia_core::{Frame, Highlighter, History, Standing, Worktree};

/// What joins two facts about one subject on a line of chrome.
const FACT_JOIN: &str = " · ";

/// The chevron the token carries while the list is closed, which travels with the
/// word rather than sitting outside it: the cells a click opens the list from are
/// the word and the mark together.
const OPENS: &str = " ▾";

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
        menu: None,
        position: position.to_owned(),
        ..app.chrome(
            "fixture",
            Some("work"),
            vigia::Stood { position, now: 0 },
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
    let position = Standing::Current.label();
    let chrome = app.chrome(
        "fixture",
        Some("work"),
        vigia::Stood {
            position: &position,
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
    let air = vigia::positions::POSITIONS_FRAME / 2;
    if rows.len() <= vigia::positions::POSITIONS_FRAME {
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
            position: &Standing::Current.label(),
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
    let parked = drawn(&mut frame, chosen);
    let header = header(&parked, &label, &app);

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
        !vigia::wants_more(top, &places),
        "a caret at the top asked for another page, so opening the list walks the \
         whole history"
    );
    let last = vigia::positions::Caret {
        at: places.len() - 1,
        top: 0,
    };
    assert!(
        vigia::wants_more(last, &places),
        "a caret at the last row walked does not extend, so the box stalls where the \
         history does not"
    );

    let exhausted = vigia::Places {
        more: false,
        ..places
    };
    assert!(
        !vigia::wants_more(last, &exhausted),
        "a walk with nothing left behind it still asks for another page"
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
