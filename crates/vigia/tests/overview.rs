//! `SPEC.md` §11.1: the body as the file list alone, on `o`.

#[path = "../../vigia-core/tests/support/mod.rs"]
mod support;

use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use vigia::{
    Action, App, Body, Chrome, Glyphs, LIST_SETTLED, Pointing, Regions, Theme, View, action_for,
    body_layout, regions, render,
};
use vigia_core::{Frame, FrameStats, Highlighter, History};

use support::{Scratch, delta, materialise};

/// A key event, spelled once.
fn press(key: char) -> Event {
    Event::Key(KeyEvent::new(KeyCode::Char(key), KeyModifiers::NONE))
}

/// Files in the fixture. More than [`LIST_SETTLED`], so a list that ran to the
/// body and a list that stopped at the shipped cap are different screens.
const FILES: usize = 30;

/// Lines each file rewrites. Asymmetric on purpose: with equal counts a fold
/// taking the removed side off the added one draws the right number anyway.
const ADDED: usize = 3;
const REMOVED: usize = 1;

/// The pane every gate here measures unless it says otherwise.
const WIDE: u16 = 80;

/// A pane deep enough that the shipped quarter-pane cap and the body are far
/// apart: twelve rows of list against forty-eight of body.
const DEEP: u16 = 50;

/// The narrow rung `SPEC.md`'s I6 pins.
const NARROW: u16 = 40;

/// A pane wide enough that `r` would draw a rail if `o` let it.
const RAIL_WIDTH: u16 = 160;

fn fixture(name: &str) -> Scratch {
    let scratch = Scratch::new(name);
    for index in 0..FILES {
        scratch.write(&format!("src/f{index}.rs"), "one\ntwo\nthree\n");
    }
    scratch.git(&["add", "-A"]);
    scratch.git(&["commit", "-m", "init"]);
    for index in 0..FILES {
        let mut lines: Vec<String> = Vec::new();
        for line in 0..ADDED {
            lines.push(format!("added {line}"));
        }
        for line in REMOVED..3 {
            lines.push(format!("kept {line}"));
        }
        scratch.write(
            &format!("src/f{index}.rs"),
            format!("{}\n", lines.join("\n")),
        );
    }
    scratch
}

/// A chrome that has asked for the state under test, and optionally the rail.
fn chrome_of(app: &App, rail: bool) -> Chrome {
    let mut chrome = app.chrome("fixture", None, Pointing::default(), 0, "");
    chrome.rail = rail;
    chrome
}

/// A shell with `o` pressed, which is where most gates here start.
fn watching(frame: &mut Frame) -> App {
    let mut app = App::past_first_paint();
    app.apply(Action::ToggleOverview, frame, 0).expect("apply");
    app
}

/// The layout a pane of this size gives the shell in front of it.
fn laid(app: &App, at: Rect, files: usize, rail: bool) -> Body {
    body_layout(at, &chrome_of(app, rail), files, files)
}

/// Collect one screen, and report the layout it was collected against.
fn screen(app: &mut App, frame: &mut Frame, at: Rect, rail: bool) -> (Body, View) {
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let files = frame.files().len();
    let body = laid(app, at, files, rail);
    let view = app
        .view(frame, &mut highlighter, &history, body)
        .expect("view");
    (body, view)
}

/// Every cell of a drawn pane, row by row.
fn drawn(app: &mut App, frame: &mut Frame, at: Rect, rail: bool) -> (Buffer, View, Chrome) {
    let (body, view) = screen(app, frame, at, rail);
    let chrome = chrome_of(app, rail);
    let mut buf = Buffer::empty(at);
    render(
        &mut buf,
        at,
        &view,
        &Theme::default(),
        Glyphs::default(),
        &chrome,
    );
    // The painter and the collect have to have been handed the same shape, or the
    // rows read back below belong to a layout nothing asked for.
    assert_eq!(
        body,
        body_layout(at, &chrome, view.files, view.list.len()).clamped_to(view.list.len()),
        "the pane was drawn against a different body than it was collected for"
    );
    (buf, view, chrome)
}

/// One region's rows, as text, over the region's own columns.
///
/// Not `support::rows_of`, which trims each row's trailing blanks: the gate at
/// forty columns is about a row occupying exactly its region's width, and a
/// trimmed row cannot fail it.
fn text_of(buf: &Buffer, region: vigia::Region) -> String {
    (region.top..region.top + region.rows)
        .map(|row| {
            (region.left..region.left + region.width)
                .map(|col| buf[(col, row)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn the_fixture_is_the_shape_the_rest_of_this_file_assumes() {
    // Two claims, and the second is why the counts are asymmetric: a gate over a
    // `+1 -1` fixture cannot tell the added side from the removed one.
    let scratch = fixture("shell-overview-shape");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);

    assert_eq!(frame.files().len(), FILES);
    let shipped = laid(
        &App::past_first_paint(),
        Rect::new(0, 0, WIDE, DEEP),
        FILES,
        false,
    );
    assert!(
        FILES > shipped.list,
        "the fixture has {FILES} files against a shipped list of {} on a {DEEP}-row \
         pane, so a list that ran to the body and one that stopped at the cap \
         would draw the same rows",
        shipped.list
    );
    let churn = frame.churn().expect("churn").expect("measured");
    assert_ne!(
        churn.added, churn.removed,
        "the fixture changes as many lines as it removes, so a header total folded \
         off the wrong side of every diff draws the right number anyway"
    );
}

#[test]
fn o_is_what_asks_for_the_list_alone_and_o_is_what_gives_the_diff_back() {
    // The binding itself, through the real key resolution. Without this every gate
    // in this file reaches the state by naming the action, and `o` could be bound to
    // anything at all.
    assert_eq!(
        action_for(&press('o'), Regions::default()),
        Some(Action::ToggleOverview),
        "`o` resolves to no action, so nothing on a keyboard reaches this state"
    );

    let scratch = fixture("shell-overview-toggle");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let at = Rect::new(0, 0, WIDE, DEEP);

    let mut app = App::past_first_paint();
    let (before, _) = screen(&mut app, &mut frame, at, false);
    assert!(
        before.diff > 0 && !before.overview,
        "a shell that has pressed nothing is already in the state `o` asks for, \
         so nothing below can fail"
    );

    app.apply(Action::ToggleOverview, &mut frame, 0)
        .expect("apply");
    let (asked, view) = screen(&mut app, &mut frame, at, false);
    assert!(asked.overview, "`o` did not reach the layout");
    assert_eq!(asked.diff, 0, "`o` left the pane a diff region");
    assert!(!asked.rule, "`o` drew the rule the diff used to sit under");
    assert!(
        view.rows.is_empty(),
        "`o` collected {} diff row(s), so the walk ran anyway",
        view.rows.len()
    );

    app.apply(Action::ToggleOverview, &mut frame, 0)
        .expect("apply");
    let (after, _) = screen(&mut app, &mut frame, at, false);
    assert_eq!(
        after, before,
        "`o` twice is not the pane it started from, so the gesture is a ratchet"
    );
}

#[test]
fn the_overview_draws_the_list_and_no_diff_row() {
    let scratch = fixture("shell-overview-drawn");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let at = Rect::new(0, 0, WIDE, DEEP);
    let mut app = watching(&mut frame);

    let (buf, view, chrome) = drawn(&mut app, &mut frame, at, false);
    let where_it_is = regions(at, &chrome, &view);
    assert_eq!(
        where_it_is.diff.rows, 0,
        "the pane published a diff region a pointer could reach"
    );
    assert!(
        where_it_is.list.rows > 0,
        "the pane published no list region, so there is nothing to read"
    );

    let listed = text_of(&buf, where_it_is.list);
    for index in 0..view.list.len() {
        let path = &frame.files()[index].path;
        assert!(
            listed.contains(path.as_str()),
            "{path:?} is not among the list's own rows:\n{listed}"
        );
    }

    // Content is what a diff draws and a list does not, and the fixture's own
    // added lines are the string only a diff row can carry.
    let whole: String = (0..at.height)
        .map(|row| {
            (0..at.width)
                .map(|col| buf[(col, row)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !whole.contains("added 0"),
        "a diff content row reached the pane:\n{whole}"
    );
}

#[test]
fn the_overview_builds_one_diff_per_listed_row_and_no_diff_body() {
    // `SPEC.md` §11.1's own bound over this region, and deliberately not the
    // stronger claim the issue asked for: a list row's counters and heat strip are
    // read off its `FileDiff`, so a state that built none would draw a list with
    // no numbers in it. What this asserts is that nothing beyond the drawn rows is
    // built, and that the diff-body walk does not run at all.
    let scratch = fixture("shell-overview-reads");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let at = Rect::new(0, 0, WIDE, DEEP);
    let mut app = watching(&mut frame);

    let before = frame.stats();
    let (body, view) = screen(&mut app, &mut frame, at, false);
    let cost: FrameStats = delta(before, frame.stats());

    assert!(
        view.list.len() > LIST_SETTLED,
        "the list drew {} row(s), which is inside the shipped cap, so this gate \
         is measuring the pane every other state already draws",
        view.list.len()
    );
    assert_eq!(
        cost.computed + cost.reused,
        view.list.len() as u64,
        "the frame built {} diff(s) for {} drawn list row(s)",
        cost.computed + cost.reused,
        view.list.len()
    );
    assert!(
        view.rows.is_empty() && body.diff == 0,
        "the diff-body walk ran: {} row(s) over a diff region of {}",
        view.rows.len(),
        body.diff
    );
}

#[test]
fn the_header_still_counts_every_changed_line_in_the_overview() {
    // The overview collect runs FIRST, on a frame that has never been walked. The
    // other order proves only that a cache returns what was put into it: an
    // ordinary collect fills every file's span on its way past, and a total folded
    // out of that cache afterwards would be right however this state behaved.
    let scratch = fixture("shell-overview-total");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let at = Rect::new(0, 0, WIDE, DEEP);

    let mut app = watching(&mut frame);
    let (_, watched) = screen(&mut app, &mut frame, at, false);
    let total = watched
        .churn
        .expect("the overview left the header with no total to draw");

    let mut plain = App::past_first_paint();
    let (_, ordinary) = screen(&mut plain, &mut frame, at, false);
    assert_eq!(
        Some(total),
        ordinary.churn,
        "the overview's header total is not the run's, so the height walk it is \
         folded out of does not run in this state"
    );
    assert!(
        total.added > 0 && total.removed > 0,
        "the fixture changed nothing, so the comparison above cannot fail"
    );
}

#[test]
fn the_list_takes_the_rows_the_diff_gave_up() {
    // The quarter-pane cap is a share of the pane *because the map may not grow at
    // the diff's expense*. There is no diff here to charge it to, so the ceiling is
    // the body.
    let scratch = fixture("shell-overview-depth");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let at = Rect::new(0, 0, WIDE, DEEP);

    let mut plain = App::past_first_paint();
    let (capped, _) = screen(&mut plain, &mut frame, at, false);
    let mut app = watching(&mut frame);
    let (deep, view) = screen(&mut app, &mut frame, at, false);

    assert!(
        capped.list < FILES,
        "the shipped pane drew all {FILES} file(s) without being capped, so the \
         comparison below is against nothing"
    );
    assert!(
        deep.list > capped.list,
        "the overview drew {} list row(s) against the capped {}, so the cap is \
         still being applied where its own reason does not reach",
        deep.list,
        capped.list
    );
    // Against the capped body rather than against itself: `deep.lead + deep.list`
    // *is* `deep.rows()` for this shape by construction, so the two sides have to
    // come from different panes' arithmetic or the assertion cannot fail.
    assert_eq!(
        deep.rows(),
        capped.rows(),
        "the two shapes report different bodies on one pane, so rows went \
         somewhere the layout does not name"
    );
    // Every changed file, not `deep.list`: the region is the whole body here, and
    // what the ruling is about is how many files a reader can see at once.
    assert_eq!(
        view.list.len(),
        FILES,
        "the collect drew {} of {FILES} file(s) into a region with room for {}",
        view.list.len(),
        deep.list
    );
}

#[test]
fn a_list_shorter_than_the_body_leaves_the_rest_blank() {
    // The other side of the rung above: fewer files than rows means blank rows,
    // not a diff quietly reappearing under them.
    let scratch = Scratch::large_diff("shell-overview-short", 2, 4);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let at = Rect::new(0, 0, WIDE, DEEP);
    let mut app = watching(&mut frame);

    let (body, view) = screen(&mut app, &mut frame, at, false);
    assert_eq!(view.files, 2, "the fixture is not two files");
    assert_eq!(view.list.len(), 2, "the list is not the changed-file count");
    // The region stays the whole body and the two entries fill the top of it, which
    // is what makes the rest blank rather than absent.
    assert!(
        body.list > view.list.len(),
        "the region shrank to its entries, so the rows below them left the body"
    );
    assert_eq!(body.diff, 0, "a diff region reappeared under a short list");
    assert!(
        !body.rule,
        "a rule was drawn under a list with nothing below it"
    );
    assert!(body.overview, "the short list stopped being the overview");
}

#[test]
fn an_empty_worktree_still_says_so_in_the_overview() {
    // B3's sentence is drawn inside the diff's region, so a state that took that
    // region away unconditionally would answer an empty worktree with an empty
    // pane, which is the one screen a monitor may not draw.
    let scratch = Scratch::new("shell-overview-empty");
    scratch.write("src/a.rs", "one\n");
    scratch.git(&["add", "-A"]);
    scratch.git(&["commit", "-m", "init"]);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    assert_eq!(frame.files().len(), 0, "the fixture is not clean");

    let at = Rect::new(0, 0, WIDE, DEEP);
    let mut app = watching(&mut frame);
    let (body, _) = screen(&mut app, &mut frame, at, false);
    assert!(
        body.diff > 0,
        "a clean worktree in the overview kept no region for B3's sentence"
    );

    // Read out of the diff's own region, not off the whole pane: the header and the
    // footer draw whatever the worktree is doing, so a sweep of the pane passes
    // whether or not the sentence itself was ever drawn.
    let (buf, view, chrome) = drawn(&mut app, &mut frame, at, false);
    let where_it_is = regions(at, &chrome, &view);
    assert!(
        where_it_is.diff.rows > 0,
        "the pane published no diff region, so there is nowhere for the sentence"
    );
    let said = text_of(&buf, where_it_is.diff);
    assert!(
        said.split_whitespace().count() > 2,
        "the diff's region says nothing about a clean worktree:\n{said}"
    );
}

#[test]
fn the_overview_and_the_rail_do_not_fight() {
    // A list beside a diff needs a diff. `o` takes the region away, so it is
    // decided first, and B14's rule is what says the request survives it: the
    // reader who asked for both gets the rail back the moment `o` goes off.
    let scratch = fixture("shell-overview-rail");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let at = Rect::new(0, 0, RAIL_WIDTH, DEEP);

    let mut plain = App::past_first_paint();
    let (railed, _) = screen(&mut plain, &mut frame, at, true);
    assert!(
        railed.rail,
        "this pane is too narrow for a rail, so nothing below is about the rail"
    );

    let mut app = watching(&mut frame);
    let (both, view) = screen(&mut app, &mut frame, at, true);
    assert!(
        both.overview,
        "the rail won, and it has no diff to sit beside"
    );
    assert!(
        !both.rail,
        "the pane drew a rail beside a diff it does not have"
    );
    assert_eq!(both.diff, 0, "the rail left a diff region behind");
    assert!(
        view.rows.is_empty(),
        "the railed overview walked the diff anyway"
    );

    // And the request is still standing, so `o` gives the rail back rather than
    // the reader having to ask for it twice.
    app.apply(Action::ToggleOverview, &mut frame, 0)
        .expect("apply");
    let (given_back, _) = screen(&mut app, &mut frame, at, true);
    assert_eq!(
        given_back, railed,
        "turning the overview off did not give back the rail the file asked for"
    );
}

#[test]
fn a_pane_with_no_room_draws_the_overview_without_panicking() {
    let scratch = fixture("shell-overview-tiny");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = watching(&mut frame);

    for height in 0..6u16 {
        for width in [1u16, NARROW, WIDE] {
            let at = Rect::new(0, 0, width, height);
            let (body, _) = screen(&mut app, &mut frame, at, false);
            assert!(
                body.rows() <= usize::from(height),
                "a {width}x{height} pane laid out {} body row(s)",
                body.rows()
            );
            if height > 0 {
                let (_, view) = screen(&mut app, &mut frame, at, false);
                let chrome = chrome_of(&app, false);
                let mut buf = Buffer::empty(at);
                render(
                    &mut buf,
                    at,
                    &view,
                    &Theme::default(),
                    Glyphs::default(),
                    &chrome,
                );
            }
        }
    }
}

#[test]
fn the_overview_is_legible_at_forty_columns() {
    // I6, where the list is the only thing on screen and so has nowhere to hide a
    // row that over-occupies.
    let scratch = fixture("shell-overview-narrow");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let at = Rect::new(0, 0, NARROW, DEEP);
    let mut app = watching(&mut frame);

    let (buf, view, chrome) = drawn(&mut app, &mut frame, at, false);
    let where_it_is = regions(at, &chrome, &view);
    assert!(
        where_it_is.list.rows > 0,
        "the narrow pane published no list region"
    );
    // The region is the whole pane, so a row that fills it exactly is a row that
    // neither over-occupies the pane nor stops short of it.
    assert_eq!(
        where_it_is.list.width, NARROW,
        "the list is not the pane's full width, so the row widths below say \
         nothing about the pane"
    );
    let listed = text_of(&buf, where_it_is.list);
    for (row, line) in listed.lines().enumerate() {
        assert_eq!(
            line.chars().count(),
            usize::from(NARROW),
            "list row {row} is {} cells on a {NARROW}-column pane",
            line.chars().count()
        );
    }
    // The path is the row's own subject, so a rung that dropped it would leave a
    // row of numbers naming nothing.
    assert!(
        listed.contains("f0.rs"),
        "no listed path survived forty columns:\n{listed}"
    );
}
