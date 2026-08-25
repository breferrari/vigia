//! The scroll position, against a real frame.
//!
//! Kept apart from `reads.rs`, which asks what a screen *costs*. This file asks
//! whether it lands in the right place, and the two need different fixtures: a
//! cost measurement wants one file taller than the screen, and scrolling wants
//! many files short enough to move between.
//!
//! The case worth knowing about before reading any of it:
//! [`vigia_core::Frame::diff`] **panics** on an index past the end of the file
//! list, deliberately, and the file list is rebuilt from scratch on every
//! [`vigia_core::Frame::advance`]. A scroll position is exactly the index that
//! outlives that rebuild. So the agent in the other pane committing its work is
//! enough to crash a shell that trusts its own position, with no input on this
//! side at all, and clamping is not defensive programming here but the contract.

#[path = "../../vigia-core/tests/support/mod.rs"]
mod support;

use ratatui::layout::Rect;
use vigia::{Action, App, Body, Position, Row, View, diff_height};
use vigia_core::{Frame, Highlighter, History};

use support::{Scratch, generated, materialise};

/// Files in the scrolling fixture.
const FILES: usize = 40;

/// Rows one file of the fixture's **content** occupies: its heading, one hunk
/// header, and the one line it replaced.
const SPAN: usize = 4;

/// Rows one file's whole block occupies: [`SPAN`] plus the blank that closes it.
///
/// **The unit a row index maps onto a position through**, since
/// [#165](https://github.com/breferrari/vigia/issues/165) gave every file but
/// the last a trailing [`Row::Gap`]. Before it, block and span were the same
/// number and the arithmetic below could use either; they are not, and the one
/// that is wrong now is the one that reads `SPAN`.
const BLOCK: usize = SPAN + 1;

/// Rows the fixture's whole diff occupies.
///
/// Not `FILES * BLOCK`: the last file closes the stream, so it has no blank
/// after it, which is the exception `view::gap_rows` carries and `SPEC.md`
/// §11.1's "the bottom of the diff is content" is the reason for.
const TOTAL: usize = FILES * BLOCK - 1;

fn body() -> usize {
    // Eighty columns, where the footer is one line whatever the state, so the
    // scroll arithmetic below is not entangled with I6's two-line footer.
    diff_height(
        Rect::new(0, 0, 80, 24),
        &App::new().chrome("fixture", None, None, None, None, None),
        FILES,
    )
}

/// The layout these gates ask for: the diff region alone.
///
/// List-free deliberately. Every gate here is about how `View::collect` crosses
/// files, and a pinned region would couple their row arithmetic to a cap they
/// are not about. `Body::diff_only` documents that this is a real short-pane
/// state rather than a test convenience.
fn split() -> Body {
    Body::diff_only(body())
}

/// Many files, each a single rewritten line, so scrolling crosses them quickly.
fn fixture(name: &str) -> Scratch {
    Scratch::large_diff(name, FILES, 1)
}

/// Drive one action and report where the next frame would start from.
fn after(
    app: &mut App,
    frame: &mut Frame,
    highlighter: &mut Highlighter,
    history: &History,
    action: Action,
) -> Position {
    app.apply(action, frame, body()).expect("apply");
    app.view(frame, highlighter, history, split())
        .expect("view")
        .top
}

#[test]
fn the_fixture_is_the_shape_the_rest_of_this_file_assumes() {
    // Every assertion below is arithmetic over BLOCK, so it is worth one test
    // rather than a comment. Get this wrong and the others pass vacuously.
    //
    let scratch = fixture("shell-scroll-shape");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);

    assert_eq!(frame.files().len(), FILES);
    // **Two shapes since [#165](https://github.com/breferrari/vigia/issues/165)**,
    // and the last file is the whole of the difference: every file but it
    // carries the blank that closes its block, so a guard asserting one number
    // would be silently right about half the files and silently wrong about the
    // other.
    for index in [0, FILES / 2] {
        assert_eq!(
            vigia::rows_in(&mut frame, index).expect("rows"),
            BLOCK,
            "file {index} is not {BLOCK} rows, so its closing blank is missing"
        );
    }
    assert_eq!(
        vigia::rows_in(&mut frame, FILES - 1).expect("rows"),
        SPAN,
        "the last file is not {SPAN} rows, so it gained a closing blank and \
         the bottom of the diff is no longer content"
    );
}

#[test]
fn scrolling_down_and_back_up_returns_to_where_it_started() {
    // The two directions are different code: down hands the overrun to
    // `View::collect` to carry across files, up has to walk back and ask each
    // file how tall it is. A round trip is the cheapest way to catch either one
    // being off by a row.
    let scratch = fixture("shell-scroll-round");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    for rows in [1, 3, SPAN as isize, 17, (SPAN * 12) as isize] {
        let start = app
            .view(&mut frame, &mut highlighter, &history, split())
            .expect("view")
            .top;
        let moved = after(
            &mut app,
            &mut frame,
            &mut highlighter,
            &history,
            Action::Scroll(rows),
        );
        assert_ne!(
            moved, start,
            "scrolling {rows} rows from {start:?} moved nowhere"
        );
        let back = after(
            &mut app,
            &mut frame,
            &mut highlighter,
            &history,
            Action::Scroll(-rows),
        );
        assert_eq!(
            back, start,
            "scrolling {rows} rows down from {start:?} and back up landed on \
             {back:?}"
        );
    }
}

#[test]
fn scrolling_off_the_end_of_a_file_continues_into_the_next_one() {
    // The reason the position is a file plus an offset rather than one row
    // number. Each of these steps crosses a file boundary, and the row it lands
    // on has to be the next file's, not the same file's clamped.
    let scratch = fixture("shell-scroll-cross");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    let mut seen = Vec::new();
    for _ in 0..(BLOCK * 3) {
        seen.push(after(
            &mut app,
            &mut frame,
            &mut highlighter,
            &history,
            Action::Scroll(1),
        ));
    }

    let expected: Vec<Position> = (1..=BLOCK * 3)
        .map(|n| Position {
            file: n / BLOCK,
            row: n % BLOCK,
        })
        .collect();
    assert_eq!(
        seen, expected,
        "one row at a time did not walk file boundaries cleanly"
    );
}

#[test]
fn scrolling_up_walks_file_boundaries_the_same_way_down_does() {
    // The mirror of the test above, and not a duplicate of it. Down hands the
    // overrun to `View::collect` to carry; up walks back and asks each file how
    // tall it is, so the two boundary crossings are different code.
    //
    // A round trip cannot tell them apart, which is the point of doing this
    // separately. Every round trip in this file ends at file zero, and file zero
    // clamps to row zero, so a step that lands one row short at every boundary
    // still arrives at exactly (0, 0). Found by mutation: subtracting one from
    // the previous file's height left the whole suite green.
    let scratch = fixture("shell-scroll-back");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    let start = BLOCK * 3;
    let landed = after(
        &mut app,
        &mut frame,
        &mut highlighter,
        &history,
        Action::Scroll(start as isize),
    );
    assert_eq!(
        landed,
        Position { file: 3, row: 0 },
        "the walk did not start"
    );

    let mut seen = Vec::new();
    for _ in 0..start {
        seen.push(after(
            &mut app,
            &mut frame,
            &mut highlighter,
            &history,
            Action::Scroll(-1),
        ));
    }

    // Absolute row `start` counting down, resolved back into a file and an offset.
    let expected: Vec<Position> = (0..start)
        .map(|step| {
            let absolute = start - step - 1;
            Position {
                file: absolute / BLOCK,
                row: absolute % BLOCK,
            }
        })
        .collect();
    assert_eq!(
        seen, expected,
        "stepping up one row at a time did not walk file boundaries cleanly"
    );
}

#[test]
fn the_bottom_of_the_diff_is_content_rather_than_blank() {
    // Scrolling past the end must rest on the last **screenful**, not on the
    // last row. Past it draws an empty pane, which in a monitor is
    // indistinguishable from a broken one.
    //
    // > [!warning] This gate used to assert the defect it was named against
    // >
    // > Until [#57](https://github.com/breferrari/vigia/issues/57) the body of
    // > this test read `assert_eq!(view.rows.len(), 1)`, under this name and
    // > under the comment above it. Both describe the right rule; the assertion
    // > pinned the wrong behaviour in place, and it is the strongest kind of
    // > wrong a test can be, because a defect with a green gate over it is one
    // > nobody goes looking for. It survived being read every time this file was
    // > touched.
    // >
    // > The tell, and it generalises: **an exact small count where the rule is
    // > about a bound.** "Rests on content rather than blank" is a claim about
    // > the screen being full; `== 1` is a claim about it being nearly empty.
    // > `SPEC.md` §7 carries it now.
    let scratch = fixture("shell-scroll-bottom");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();

    app.apply(
        Action::Scroll((SPAN * FILES * 4) as isize),
        &mut frame,
        body(),
    )
    .expect("scroll");
    let view = drawn(&mut app, &mut frame);
    let (rows, top) = (view.rows.len(), view.top);

    assert_eq!(
        rows,
        body(),
        "scrolling far past the end drew {rows} rows of a {} row body, leaving \
         {} blank under a diff with {} rows to spare",
        body(),
        body() - rows,
        TOTAL
    );
    // The bottom of the diff and nowhere else. Asserted through the position as
    // well as the count, because a screen full of the *wrong* rows satisfies the
    // count on its own.
    assert_eq!(
        top.file * BLOCK + top.row + body(),
        TOTAL,
        "the screen is full but ends somewhere other than the end of the diff: {top:?}"
    );
    assert!(
        matches!(
            view.rows.last().expect("a drawn row"),
            Row::Line { .. } | Row::Note { .. }
        ),
        "the bottom row is {:?} rather than content",
        view.rows.last()
    );
}

#[test]
fn home_and_end_go_to_the_first_and_last_file() {
    // `End` is the last *file*, from its top, not the last row of the whole
    // diff. Finding that row means adding up every file's height, and every
    // height means a diff, which is the read I4 exists to forbid. Asserted rather
    // than left to a comment because it is a deliberate limit, not an oversight.
    let scratch = fixture("shell-scroll-ends");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    assert_eq!(
        after(
            &mut app,
            &mut frame,
            &mut highlighter,
            &history,
            Action::Bottom
        ),
        Position {
            file: FILES - 1,
            row: 0
        }
    );
    assert_eq!(
        after(
            &mut app,
            &mut frame,
            &mut highlighter,
            &history,
            Action::Top
        ),
        Position { file: 0, row: 0 }
    );
}

#[test]
fn n_and_p_step_one_file_and_land_on_its_heading() {
    // The granularity between a row and the whole diff. Row zero is the heading,
    // which is the resolution a list click and a follow jump already use, and it
    // is what makes the step cost no diff: nothing asks how tall anything is.
    let scratch = fixture("shell-scroll-files");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    // Three steps rather than one, so "moved once" and "steps every time" are
    // different assertions. Each lands on a heading, never one row past it.
    for file in 1..=3 {
        assert_eq!(
            after(
                &mut app,
                &mut frame,
                &mut highlighter,
                &history,
                Action::File(1)
            ),
            Position { file, row: 0 },
            "`n` did not land on file {file}'s heading"
        );
    }
    for file in [2, 1] {
        assert_eq!(
            after(
                &mut app,
                &mut frame,
                &mut highlighter,
                &history,
                Action::File(-1)
            ),
            Position { file, row: 0 },
            "`p` did not land on file {file}'s heading"
        );
    }

    // **`p` from inside a file goes to the previous file, not to this one's
    // heading**, and that is the ruling rather than the easy reading. The pager
    // reflex of "this section first" would make one key mean two things depending
    // on where the viewport happened to be, which `SPEC.md` §11.1 refuses across
    // this whole map; `g` is the key that reaches a top.
    assert_eq!(
        after(
            &mut app,
            &mut frame,
            &mut highlighter,
            &history,
            Action::Scroll(2)
        ),
        Position { file: 1, row: 2 },
        "the fixture does not put the viewport inside file 1"
    );
    assert_eq!(
        after(
            &mut app,
            &mut frame,
            &mut highlighter,
            &history,
            Action::File(-1)
        ),
        Position { file: 0, row: 0 },
        "`p` from inside a file stopped at that file's heading, so the key means \
         two things depending on where the reader was"
    );
}

#[test]
fn the_file_step_stops_at_both_ends() {
    // **Neither key ever moves the view in the direction opposite to itself.**
    // Clamping the file index and always landing on row zero would give `n` at
    // the last file a backwards jump to that file's heading, and `p` at the first
    // a forwards one to the top: both would be a key undoing what its own arrow
    // says. There is no such file, so nothing moves.
    let scratch = fixture("shell-scroll-file-ends");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    assert_eq!(
        after(
            &mut app,
            &mut frame,
            &mut highlighter,
            &history,
            Action::File(-1)
        ),
        Position { file: 0, row: 0 },
        "`p` at the first file moved"
    );

    // And from *inside* the first file, which is the case a saturating index
    // would quietly turn into `g`.
    after(
        &mut app,
        &mut frame,
        &mut highlighter,
        &history,
        Action::Scroll(2),
    );
    assert_eq!(
        after(
            &mut app,
            &mut frame,
            &mut highlighter,
            &history,
            Action::File(-1)
        ),
        Position { file: 0, row: 2 },
        "`p` inside the first file jumped to its heading, which is `g`'s job"
    );

    after(
        &mut app,
        &mut frame,
        &mut highlighter,
        &history,
        Action::Bottom,
    );
    assert_eq!(
        after(
            &mut app,
            &mut frame,
            &mut highlighter,
            &history,
            Action::File(1)
        ),
        Position {
            file: FILES - 1,
            row: 0
        },
        "`n` at the last file moved"
    );
}

#[test]
fn a_page_keeps_one_row_of_overlap() {
    // A page that moved a whole screen would leave nothing shared between the
    // two, and a reader loses their place at the seam. One row of overlap is what
    // every pager does and it is worth one assertion.
    let scratch = fixture("shell-scroll-page");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    let rows = body();
    let landed = after(
        &mut app,
        &mut frame,
        &mut highlighter,
        &history,
        Action::Page(1),
    );
    let absolute = landed.file * BLOCK + landed.row;
    assert_eq!(
        absolute,
        rows - 1,
        "a page of {rows} rows moved {absolute}, so the screens do not overlap"
    );

    assert_eq!(
        after(
            &mut app,
            &mut frame,
            &mut highlighter,
            &history,
            Action::Page(-1)
        ),
        Position { file: 0, row: 0 },
        "paging back did not return to the top"
    );
}

#[test]
fn a_half_page_keeps_no_overlap_because_it_already_is_one() {
    // The deliberate asymmetry with the gate above, asserted rather than left as
    // a comment. A page takes a row off its step so the two screens share
    // something; a half page already leaves half the screen standing, so taking a
    // row as well would pay twice for one anchor and put `d` and `u` out of step
    // with each other.
    let scratch = fixture("shell-scroll-half");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    let rows = body();
    let landed = after(
        &mut app,
        &mut frame,
        &mut highlighter,
        &history,
        Action::HalfPage(1),
    );
    let absolute = landed.file * BLOCK + landed.row;
    assert_eq!(
        absolute,
        rows / 2,
        "a half page of {rows} rows moved {absolute} rather than half of them"
    );

    // **The round trip is the half that odd bodies can break.** Both directions
    // floor, so they agree; a step that rounded one way would leave `u` a row
    // short of where `d` started, once per press, and a reader would drift.
    assert_eq!(
        after(
            &mut app,
            &mut frame,
            &mut highlighter,
            &history,
            Action::HalfPage(-1)
        ),
        Position { file: 0, row: 0 },
        "half a page back did not return to the top of a {rows} row body"
    );

    // Non-vacuity: the two steps really are different sizes, so this gate cannot
    // be satisfied by a `HalfPage` that quietly forwards to `Page`.
    assert_ne!(
        rows / 2,
        rows - 1,
        "the fixture's body is too short to tell a half page from a whole one"
    );
}

#[test]
fn a_position_survives_the_file_it_pointed_into_disappearing() {
    // The panic this whole clamp exists for. The reader scrolls to the last file,
    // the agent in the other pane commits, and the file list the position was
    // resolved against no longer exists. `Frame::diff` panics on that index by
    // design, so nothing downstream would survive it.
    let scratch = fixture("shell-scroll-shrink");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    let far = after(
        &mut app,
        &mut frame,
        &mut highlighter,
        &history,
        Action::Bottom,
    );
    assert_eq!(
        far.file,
        FILES - 1,
        "the fixture never reached its last file"
    );

    // Half the changes go away. The position now names a file past the end.
    for index in (FILES / 2)..FILES {
        scratch.git(&["checkout", "--", &format!("src/mod_{index}.rs")]);
    }
    frame.advance().expect("advance");
    assert_eq!(frame.files().len(), FILES / 2, "the fixture did not shrink");

    let view = app
        .view(&mut frame, &mut highlighter, &history, split())
        .expect("view");
    assert_eq!(
        view.top.file,
        FILES / 2 - 1,
        "the position was not pulled back to the last file that still exists"
    );
    assert!(
        !view.rows.is_empty(),
        "the clamp landed somewhere that draws nothing"
    );

    // And all the way to nothing, which is the other end of the same case.
    scratch.commit_all("everything");
    frame.advance().expect("advance");
    assert_eq!(frame.files().len(), 0, "the worktree is not clean");

    let view = app
        .view(&mut frame, &mut highlighter, &history, split())
        .expect("view");
    assert_eq!(view.files, 0);
    assert_eq!(view.top, Position::default());
    assert!(view.rows.is_empty());
}

#[test]
fn a_screen_with_no_room_for_a_body_still_resolves() {
    // A pane dragged down to two rows leaves the header and the footer and
    // nothing between them. `diff_height` returns zero there, and asking for zero
    // rows has to be an answer rather than a panic or an unclamped position.
    let scratch = fixture("shell-scroll-flat");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    // Scroll somewhere first, or the assertion below cannot tell a preserved
    // position from a reset one: they are both (0, 0) at the top of the diff.
    let before = after(
        &mut app,
        &mut frame,
        &mut highlighter,
        &history,
        Action::Scroll((BLOCK * 7 + 2) as isize),
    );
    assert_eq!(
        before,
        Position { file: 7, row: 2 },
        "the scroll did not land"
    );

    for height in [0, 1] {
        let view = app
            .view(
                &mut frame,
                &mut highlighter,
                &history,
                Body::diff_only(height),
            )
            .expect("view");
        assert_eq!(view.rows.len(), height);
        assert_eq!(view.files, FILES);
        // A frame with no room to draw must not decide where the reader is. It
        // resolved nothing, so it has nothing to say about the position, and
        // reporting one would drag the reader back to the top of the file for as
        // long as the pane stayed short.
        assert_eq!(
            view.top, before,
            "a {height}-row screen moved the reader from {before:?} to {:?}",
            view.top
        );
    }

    // And the position survives being dragged short and back, which is the whole
    // sequence a reader actually performs.
    let view = app
        .view(&mut frame, &mut highlighter, &history, split())
        .expect("view");
    assert_eq!(view.rows.len(), body());
    assert_eq!(
        view.top, before,
        "dragging the pane short and back lost the reader's place"
    );
}

#[test]
fn only_the_action_that_reads_the_height_is_given_one() {
    // `Action::needs_height` exists so the shell can skip an uncached
    // terminal-size syscall and a `Chrome` allocation on every action that does
    // not read the answer, which matters because a drained trackpad flick is up
    // to sixty-four actions between two paints.
    //
    // The exhaustive match in `needs_height` catches an action nobody
    // classified. It cannot catch one classified **wrongly**, and that failure
    // is silent: the action is handed a zero height and quietly moves the
    // viewport by the wrong amount. So the claim is checked against `App::apply`
    // itself, by driving each action twice with heights that could not both be
    // right and asserting the answer did not depend on which.
    //
    // **Two things were blind here until [#297](https://github.com/breferrari/vigia/issues/297),
    // and between them they let a wrong classification ship.**
    //
    // The first is that every `App` was unpinned. `Action::Bottom` reads the
    // height only under `SPEC.md` §11.2 B16's pin, where it rests the file's last
    // row on the bottom; unpinned it is a jump to a heading and no height can
    // move it. So the classification was checked in the one state that cannot see
    // it, `Bottom` stayed in the `false` arm, `crate::run` handed it a zero, and
    // the resting row saturated back to the whole span. Every action is driven
    // pinned **and** unpinned now, and `needs_height` is a claim about whether
    // *any* reachable state reads it.
    //
    // The second is that it compared `View::top`, which is the position after
    // `View::collect` has resolved it. That walk clamps, so two different
    // requests land on the same drawn row and the difference the gate exists to
    // see is exactly what the clamp hides: `G` writing the whole span and `G`
    // writing the resting row draw the identical screen. What separates them is
    // the position the shell **keeps**, which is what the next action in the same
    // drained batch moves from. `App::position` is read before any view, and the
    // drawn top is kept as a second signal rather than the only one.
    let scratch = fixture("shell-scroll-height");
    let worktree = scratch.worktree();

    // Every action, so a new variant reaches this list by failing to be in it.
    let actions = [
        Action::Scroll(SPAN as isize * 3),
        Action::Scroll(-(SPAN as isize)),
        Action::Top,
        Action::Bottom,
        Action::Redraw,
        Action::ToggleFollow,
        Action::Page(1),
        Action::Page(-1),
        Action::HalfPage(1),
        Action::HalfPage(-1),
        // Measured in files, so no height can change where it arrives. Both
        // directions, because the backwards one lands by *not* moving and a
        // no-op is exactly the shape that passes a height check vacuously.
        Action::File(1),
        Action::File(-1),
        // The pin itself, which moves no viewport and so must not be told a
        // height. It is in this list because B16 added it, and the list is what
        // a new variant has to reach.
        Action::ToggleSingle,
    ];

    // **Built once, because none of it varies with the action or the height.**
    // The frame is a forty-file `gix` diff and `Highlighter::new` loads the
    // default syntax set, and this loop runs them twice per action: rebuilding
    // them per iteration cost 0.61s against 0.23s hoisted, on a suite that runs
    // on three platforms. Sharing the frame is safe because nothing here calls
    // `advance` and the fixture worktree never changes, so `apply` and `view`
    // only touch the diff cache. What is **not** hoisted is the `App`: its
    // position is the thing being measured, so each run starts from a new one.
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let full = body();

    for action in actions {
        // Whether the answer moved with the height, in each configuration.
        let mut moved_anywhere = false;
        for pinned in [false, true] {
            // Two heights far enough apart that any action reading one would land
            // somewhere different. Started from the same place each time.
            let landed: Vec<(Position, Position)> = [0usize, full]
                .into_iter()
                .map(|height| {
                    let mut app = App::new();
                    app.apply(Action::Scroll(SPAN as isize * 8), &mut frame, full)
                        .expect("seed");
                    if pinned {
                        app.apply(Action::ToggleSingle, &mut frame, full)
                            .expect("pin");
                    }
                    app.apply(action, &mut frame, height).expect("apply");
                    // The retained request first, then what the walk resolved it
                    // to. The clamp can make two requests draw one screen, so the
                    // first is the sensitive one and the second is corroboration.
                    let kept = app.position();
                    let drawn = app
                        .view(&mut frame, &mut highlighter, &history, split())
                        .expect("view")
                        .top;
                    (kept, drawn)
                })
                .collect();

            let moved = landed[0] != landed[1];
            moved_anywhere |= moved;
            assert!(
                action.needs_height() || !moved,
                "{action:?} says it does not need the height and moved when it \
                 changed{}, so the shell is about to hand it a zero: {:?} against \
                 {:?}",
                if pinned { " under a pin" } else { "" },
                landed[0],
                landed[1]
            );
        }

        assert!(
            !action.needs_height() || moved_anywhere,
            "{action:?} says it needs the height and lands in the same place \
             without one, pinned or not, so either the claim or the action is wrong"
        );
    }
}

/// Drive the view once and hand back the whole screenful.
///
/// The four gates below are all assertions that a **full** screen came back, so
/// what they compare against is [`body`] itself. `view.rows.len() == body()`
/// reads as arithmetic where what it means is "no blank rows under content that
/// exists".
///
/// Returns the [`View`] rather than the two numbers most callers want, because
/// one of them wants a third thing and a second helper to fetch it would drive
/// the view twice: `App::view` writes its resolved position back, so a second
/// call is a second resolution of a position the first one already moved.
fn drawn(app: &mut App, frame: &mut Frame) -> View {
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    app.view(frame, &mut highlighter, &history, split())
        .expect("view")
}

#[test]
fn a_diff_that_shrank_under_the_viewport_still_fills_the_screen() {
    // **The half that was reported**, and the one no fixture in this suite could
    // have produced: every other gate here holds the file list still and moves
    // the window. This moves the file list.
    //
    // The sequence is the one that found it, run literally. A worktree of forty
    // changed files, scrolled deep into, then `git reset --hard` with two
    // untracked files left behind — an agent in the other pane reverting its own
    // work, which is an ordinary event on the pane this tool exists for and
    // exactly when someone looks over. The retained position was reasonable when
    // it was taken and names a row the new diff does not have.
    //
    // What made it monitor-class rather than cosmetic: the header went on
    // truthfully saying two files had changed over a blank body, so one thing on
    // screen contradicted the other two. `SPEC.md` §11.1 ruled the empty state
    // into existence to stop "nothing changed" and "this has stopped working"
    // looking alike, and this reintroduced that ambiguity by another route.
    // **Tall files, not the shared forty-by-four fixture, and this is the part
    // that took a mutation run to get right.** What survives the shrink is the
    // row *offset within a file*, because the file index is clamped to the new
    // list and the offset is not. Over four-row files the deepest offset
    // reachable is three, which is smaller than anything the surviving file
    // holds, so the walk never reaches the overshoot branch at all and the gate
    // passes against the unfixed code. Sixty-two row files give the offset
    // somewhere to be large.
    //
    // Recorded rather than quietly corrected: this gate asserted the right
    // outcome and reproduced the wrong situation, which is the same shape as the
    // gate #57 was found under and would have been just as green.
    let scratch = Scratch::large_diff("shell-scroll-shrunk", FILES, 30);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();

    // To the bottom of the old diff, and disengaged from follow the way a
    // reader's own scroll disengages it. A following viewport is dragged back by
    // I5 on the next tick and would never see this.
    app.apply(Action::Scroll(10_000), &mut frame, body())
        .expect("scroll");
    let before = drawn(&mut app, &mut frame).top;
    assert!(
        before.file > 1 && before.row > 25,
        "the fixture did not leave a deep enough offset to be shrunk out from \
         under: {before:?}. The offset is what survives, so a small one makes \
         this gate pass against the defect it is written for"
    );

    // Twenty-five lines, so the surviving file is taller than the screen on its
    // own. That is the common shape and the one that needs no walk backwards,
    // which is the shape that was reported.
    scratch.git(&["reset", "--hard"]);
    scratch.write("kept_one.md", generated(25, "kept-one"));
    scratch.write("kept_two.md", generated(25, "kept-two"));
    materialise(&mut frame);
    assert_eq!(frame.files().len(), 2, "the fixture is not two files");

    let view = drawn(&mut app, &mut frame);
    let (rows, top) = (view.rows.len(), view.top);
    assert_eq!(
        rows,
        body(),
        "the diff shrank under the viewport and the screen came back with {rows} \
         of {} rows drawn, so {} were blank while the header said two files had \
         changed. Top: {top:?}",
        body(),
        body() - rows
    );
}

#[test]
fn a_last_file_shorter_than_the_screen_is_filled_from_the_ones_above_it() {
    // The case a subtraction inside the last file cannot reach, and the reason
    // `last_screenful` walks. When the final file has fewer rows than the body,
    // backing off within it still leaves the screen short: the top has to land
    // in an earlier file, which is a question the forward walk has no way to
    // ask, because its borrow of the current file is live until that file is
    // drawn.
    //
    // Five-row blocks against a twenty-two row body means five of them fill it, so
    // this exercises a real walk rather than the degenerate one.
    let scratch = fixture("shell-scroll-short-tail");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();

    app.apply(Action::Scroll(10_000), &mut frame, body())
        .expect("scroll");
    let view = drawn(&mut app, &mut frame);
    let (rows, top) = (view.rows.len(), view.top);

    assert!(
        top.file < FILES - 1,
        "the top stayed in the last file, which is {SPAN} rows and cannot fill a \
         {} row body on its own: {top:?}",
        body()
    );
    assert_eq!(rows, body(), "the walk back left the screen short");
}

#[test]
fn a_diff_shorter_than_the_screen_starts_at_the_top() {
    // The other end of the same rule, and the one that must **not** be "fill the
    // screen": a diff with fewer rows than the body has nothing to fill it with,
    // so the honest picture is every row it has, from the first.
    //
    // Without a floor, `last_screenful` walking back until it has a screenful is
    // a loop that runs off the front of the file list, and the obvious
    // off-by-one puts the top in a file that does not exist —
    // `vigia_core::Frame::diff` **panics** on an index past the end, which this
    // file's own header warns about.
    let scratch = Scratch::large_diff("shell-scroll-short-diff", 2, 1);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();

    app.apply(Action::Scroll(10_000), &mut frame, body())
        .expect("scroll");
    let view = drawn(&mut app, &mut frame);
    let (rows, top) = (view.rows.len(), view.top);

    assert_eq!(
        top,
        Position::default(),
        "a diff shorter than the screen did not start at the top"
    );
    // `BLOCK + SPAN` and not `2 * SPAN`: the first file carries the blank that
    // closes its block and the second is the last, so it does not.
    assert_eq!(
        rows,
        BLOCK + SPAN,
        "a two file diff drew {rows} rows rather than the {} it has",
        BLOCK + SPAN
    );
}

/// Dragging the diff's scrollbar lands where the thumb says it will.
///
/// **The gesture and the readout have to be the same arithmetic**, and for one
/// day they were not. The bar was made row-exact when I4 was narrowed on
/// 2026-08-01, and `Action::DiffTo` went on resolving its fraction against the
/// *file count*, which is what it had counted before. The two agree only when
/// every file is one row tall.
///
/// The fixture is deliberately few-files-many-rows, which is the shape that
/// exposes it and the shape a reader actually has: three long files gave the
/// old code three landing spots for a track dozens of rows tall, so the pointer
/// moved and the diff either jumped to a heading or did not move at all.
///
/// Two claims, because they fail separately. Dragging *within* one file has to
/// move the diff, which file granularity cannot do at all. And the bottom of
/// the track has to reach the last screenful, which is the half that stays
/// broken if the fraction is mapped onto the whole diff instead of onto its
/// travel.
#[test]
fn dragging_the_diff_bar_resolves_to_a_row_and_reaches_the_end() {
    use vigia::TRACK_SCALE;

    const FILES: usize = 3;
    const HEIGHT: usize = 12;

    let scratch = Scratch::large_diff("diff-drag", FILES, 60);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);

    let total = frame.height(vigia::rows_of).expect("total rows");
    assert!(
        total > FILES * HEIGHT,
        "the fixture is too short to tell a row from a file: {total} rows over {FILES} files"
    );

    let mut app = App::new();
    let mut seen = Vec::new();
    for step in 0..=8u32 {
        let at = (step * TRACK_SCALE) / 8;
        app.apply(Action::DiffTo(at), &mut frame, HEIGHT)
            .expect("drag");
        seen.push(app.position());
    }

    // Monotonic and strictly moving: eight drags down a track this long may not
    // produce eight identical positions, which is exactly what the file-granular
    // resolution produced for the six steps that fell inside one file.
    for pair in seen.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        assert!(
            (b.file, b.row) > (a.file, a.row),
            "a drag further down the track did not move the diff further down: \
             {a:?} then {b:?}"
        );
    }

    // The whole track is live. The bottom of it is the last screenful, not the
    // top of the last file and not one screenful short of the end.
    let end = seen.last().copied().expect("a last position");
    let mut rows_above = 0;
    for file in 0..end.file {
        rows_above += frame.rows_of(file, vigia::rows_of).expect("rows");
    }
    assert_eq!(
        rows_above + end.row,
        total - HEIGHT,
        "the bottom of the track landed at {end:?}, which is row {} of {total}",
        rows_above + end.row
    );
}

#[test]
fn the_counting_twins_agree_with_the_rows_drawn() {
    // **The gate `view::rows_of`'s own doc asks for and did not have.** It and
    // `span_of` "have to agree exactly: one is what the screen draws and the
    // other is what the scrollbar is scaled against", and until
    // [#165](https://github.com/breferrari/vigia/issues/165) nothing checked it:
    // every other gate here reads a *position*, and a total that drifted from
    // the walk moves no position at all. It moves the thumb, silently, on a bar
    // this project already had to make row-exact once.
    //
    // A threshold-shaped claim, so no snapshot can see it: the two numbers are
    // computed on different code paths from the same frame and never appear on
    // screen together.
    let scratch = fixture("shell-scroll-twins");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    // Non-vacuity first, stated as the claim rather than through a number that
    // only implies it on a fixture of this exact shape: a one-file diff has no
    // boundary for the two counts to disagree about, and one file with two hunks
    // would satisfy a `total > SPAN` proxy while meeting none.
    assert!(
        frame.files().len() > 1,
        "the fixture is one file, so there is no inter-file blank for the two          counts to disagree about"
    );

    // The total the bar is scaled against, blanks included.
    let total = vigia::diff_rows(&mut frame).expect("total");

    // The rows the walk actually produces, given more room than the diff needs
    // so nothing is clipped and the count is the whole stream.
    let view = View::collect(
        &mut frame,
        &mut highlighter,
        &history,
        vigia::Viewport {
            position: Position::default(),
            anchored: false,
            diff_rows: TOTAL * 2,
            ..vigia::Viewport::default()
        },
    )
    .expect("view");

    assert!(
        view.rows.len() < TOTAL * 2,
        "the walk filled the window it was given, so its row count is the \
         window's rather than the diff's and this compares two different things"
    );
    assert_eq!(
        total,
        view.rows.len(),
        "the scrollbar is scaled against {total} rows and the walk drew {}, so \
         the bar cannot reach its own bottom",
        view.rows.len()
    );
}

#[test]
fn every_jump_lands_on_a_heading_and_never_on_a_gap() {
    // **The property a trailing blank could quietly take away.** Every jump on
    // this map resolves through `App::jump_to` to `Position { file, row: 0 }`,
    // and row 0 of a file is its heading only while the blank that closes a
    // block **trails** rather than leads
    // ([#165](https://github.com/breferrari/vigia/issues/165)). A leading blank
    // would put one above every heading a reader jumped to, and nothing else in
    // this suite reads the row a jump landed on: the position gates all compare
    // `Position`s, which are identical either way.
    //
    // So this asserts the drawn row rather than the position, which is the only
    // form of the claim that can tell the two designs apart.
    let scratch = fixture("shell-scroll-jumps");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    // Scrolled off a boundary first, so a jump has somewhere to come back from
    // and "landed on a heading" is not just "never moved".
    app.apply(Action::Scroll((BLOCK * 5 + 2) as isize), &mut frame, body())
        .expect("scroll");

    for (name, action) in [
        ("n", Action::File(1)),
        ("p", Action::File(-1)),
        ("a digit", Action::ListRow(2)),
        ("g", Action::Top),
        ("G", Action::Bottom),
    ] {
        app.apply(action, &mut frame, body()).expect("apply");
        let view = app
            .view(&mut frame, &mut highlighter, &history, split())
            .expect("view");
        assert!(
            matches!(view.rows.first(), Some(Row::File(_))),
            "{name} landed on {:?} rather than on a file's heading",
            view.rows.first()
        );
        assert_eq!(
            view.top.row, 0,
            "{name} landed inside a file rather than at its top: {:?}",
            view.top
        );
    }
}

#[test]
fn a_walk_back_survives_the_file_it_pointed_into_disappearing() {
    // **`a_position_survives_the_file_it_pointed_at_disappearing` above covers the
    // *draw*; this covers the *gesture*, and until #297's second audit round
    // nothing did.** That one lets `View::collect` clamp a stale position on the
    // way to the screen, which is the path a redraw takes. `App::up` reaches the
    // frame ahead of any collect: it walks back a file at a time asking each how
    // tall it is, through `rows_in` into `Frame::diff`, which indexes the file
    // list directly and panics past its end.
    //
    // **The batch is what makes it reachable.** The shell drains every pending
    // action before it paints, so a `Wake::Tick` carrying an agent's commit and a
    // wheel-up arriving together are applied with no frame between them, and the
    // position resolved against the old list is handed straight to the new one.
    // No test drove a scroll across an advance without a paint, so the crash sat
    // behind a suite that exercised both halves separately.
    let scratch = fixture("shell-scroll-back-shrink");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    // Deep into the changed set, resolved, so the position names a real file.
    app.apply(Action::Bottom, &mut frame, body())
        .expect("apply");
    let far = app
        .view(&mut frame, &mut highlighter, &history, split())
        .expect("view")
        .top;
    assert_eq!(
        far.file,
        FILES - 1,
        "the fixture never reached its last file"
    );

    // The other pane commits, and the wheel-up arrives in the same drain.
    scratch.commit_all("everything the agent was working on");
    frame.advance().expect("advance");
    assert_eq!(frame.files().len(), 0, "the worktree is not clean");

    app.apply(Action::Scroll(-1), &mut frame, body())
        .expect("a scroll back over a changed set that is gone");
    let view = app
        .view(&mut frame, &mut highlighter, &history, split())
        .expect("view");
    assert!(view.rows.is_empty(), "a clean worktree drew diff rows");

    // And the half-shrunk case, which is the commoner one: files still exist and
    // the position names one past their end.
    let scratch = fixture("shell-scroll-back-half");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();

    app.apply(Action::Bottom, &mut frame, body())
        .expect("apply");
    let _ = app
        .view(&mut frame, &mut highlighter, &history, split())
        .expect("view");
    for index in (FILES / 2)..FILES {
        scratch.git(&["checkout", "--", &format!("src/mod_{index}.rs")]);
    }
    frame.advance().expect("advance");
    assert_eq!(frame.files().len(), FILES / 2, "the fixture did not shrink");

    app.apply(Action::Scroll(-(SPAN as isize)), &mut frame, body())
        .expect("a scroll back over a shortened changed set");
    let view = app
        .view(&mut frame, &mut highlighter, &history, split())
        .expect("view");
    assert!(
        view.top.file < FILES / 2,
        "the walk back left the position on file {}, past the {} that exist",
        view.top.file,
        FILES / 2
    );
}
