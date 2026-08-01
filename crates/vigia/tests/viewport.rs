//! The bottom of the diff, and the blank pane that lives just above it.
//!
//! [#57](https://github.com/breferrari/vigia/issues/57) ruled that a viewport past
//! the end of the diff rests on the **last screenful** rather than the last row,
//! because the two are different places and taking the second for the first draws
//! a line of content over a blank screen while the header goes on truthfully
//! saying how many files changed.
//!
//! It fixed one of the two ways there. This file is the other, reported by
//! [#59](https://github.com/breferrari/vigia/issues/59) from a screen recording
//! and then reproduced by hand: **scrolling down normally into the tail of a
//! diff** leaves the pane half empty, because the walk that fills it simply runs
//! out of files and nothing backs it up.
//!
//! The two are worth stating side by side, because the first fix reads as though
//! it covers both and does not:
//!
//! | How you get there | `skip` when the walk ends | Backed up? |
//! |---|---|---|
//! | position past the end of the last file | `>= span` | yes, via the overshoot restart |
//! | scrolled until fewer than a screenful remain | `< span` | **no** |

#[path = "../../vigia-core/tests/support/mod.rs"]
mod support;

use ratatui::layout::Rect;
use vigia::{Action, App, Body, FileEntry, Position, View, Viewport, diff_height};
use vigia_core::{Highlighter, History};

use support::{Scratch, materialise};

/// Files in the fixture.
const FILES: usize = 12;

/// Rows one file occupies: its heading, one hunk header, and the line it replaced.
const SPAN: usize = 4;

fn body() -> usize {
    diff_height(
        Rect::new(0, 0, 80, 24),
        &App::new().chrome("fixture", None),
        FILES,
    )
}

/// Twelve files of four rows each, so the diff is 48 rows against a 22-row body.
///
/// **Deliberately more than one screenful and less than two.** The defect needs a
/// diff long enough to scroll into and short enough that the tail runs out, and a
/// fixture that is many screenfuls deep would take a hundred keystrokes to reach
/// the shape being tested.
fn fixture(name: &str) -> Scratch {
    Scratch::large_diff(name, FILES, 1)
}

#[test]
fn scrolling_to_the_bottom_never_leaves_the_pane_half_empty() {
    // The gate #59 was filed for. Scroll a row at a time, all the way past the
    // end, and assert after **every** step that the body is full.
    //
    // Every step rather than only the last, because the defect is not at the very
    // bottom: it starts the moment fewer than a screenful of rows remain below the
    // top, and it grows by one blank row per keystroke after that. A gate that
    // only checked the final position would find the pane at its emptiest and
    // report the same failure, but it would not say when it began.
    let scratch = fixture("viewport-scroll");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);

    let mut app = App::new();
    let mut highlighter = Highlighter::new();
    let history = History::new();
    let height = body();

    let total = FILES * SPAN;
    assert!(
        total > height,
        "the fixture has to be taller than the pane: {total} rows, {height} tall"
    );

    // Well past the end, so the walk has to hold the bottom rather than run off it.
    for step in 0..total + height {
        app.apply(Action::Scroll(1), &mut frame, height)
            .expect("apply");
        let view = app
            .view(
                &mut frame,
                &mut highlighter,
                &history,
                Body::diff_only(height),
            )
            .expect("view");
        assert_eq!(
            view.rows.len(),
            height,
            "step {step}: the body drew {} of {height} rows, so {} are blank, from {:?}",
            view.rows.len(),
            height - view.rows.len(),
            view.top
        );
    }
}

#[test]
fn a_page_down_past_the_end_holds_the_last_screenful() {
    // The same rule reached by the key that crosses the boundary in one jump
    // rather than a row at a time. `Page` is how a reader actually gets there, and
    // it skips every intermediate position the row-at-a-time gate walks through.
    let scratch = fixture("viewport-page");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);

    let mut app = App::new();
    let mut highlighter = Highlighter::new();
    let history = History::new();
    let height = body();

    for step in 0..6 {
        app.apply(Action::Page(1), &mut frame, height)
            .expect("apply");
        let view = app
            .view(
                &mut frame,
                &mut highlighter,
                &history,
                Body::diff_only(height),
            )
            .expect("view");
        assert_eq!(
            view.rows.len(),
            height,
            "page {step}: {} of {height} rows, from {:?}",
            view.rows.len(),
            view.top
        );
    }
}

#[test]
fn a_diff_shorter_than_the_pane_draws_what_it_has_and_no_more() {
    // The other side of the same rule, and the reason the fix cannot simply be
    // "always fill the body". A four-row diff in a twenty-two row pane genuinely
    // has eighteen rows of nothing to say, and inventing content for them would be
    // worse than the blank. What must not happen is backing *up* past the top.
    let scratch = Scratch::large_diff("viewport-short", 1, 1);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);

    let mut app = App::new();
    let mut highlighter = Highlighter::new();
    let history = History::new();
    let height = body();

    for _ in 0..10 {
        app.apply(Action::Scroll(1), &mut frame, height)
            .expect("apply");
    }
    let view = app
        .view(
            &mut frame,
            &mut highlighter,
            &history,
            Body::diff_only(height),
        )
        .expect("view");

    assert!(view.rows.len() <= height);
    assert_eq!(
        view.top,
        Position::default(),
        "a diff shorter than the pane resolves to the top"
    );
}

#[test]
fn the_resolved_position_is_stable_once_it_reaches_the_bottom() {
    // A viewport that has been backed up must **stay** backed up. Storing a
    // resolved position and then resolving it again has to be a fixed point, or
    // the pane creeps upward by a row per frame while nothing is pressed, which is
    // the worst version of this: motion with no input at all, against I5.
    let scratch = fixture("viewport-stable");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);

    let mut app = App::new();
    let mut highlighter = Highlighter::new();
    let history = History::new();
    let height = body();

    for _ in 0..FILES * SPAN + height {
        app.apply(Action::Scroll(1), &mut frame, height)
            .expect("apply");
    }
    let settled = app
        .view(
            &mut frame,
            &mut highlighter,
            &history,
            Body::diff_only(height),
        )
        .expect("view")
        .top;

    for round in 0..3 {
        let again = app
            .view(
                &mut frame,
                &mut highlighter,
                &history,
                Body::diff_only(height),
            )
            .expect("view")
            .top;
        assert_eq!(again, settled, "round {round} moved a settled viewport");
    }
}

#[test]
fn a_backed_up_body_holds_the_rows_its_own_position_names() {
    // **A full body can still be the wrong body**, and a count cannot see it. The
    // restart throws away the partial screen it built before backing up; leave
    // those rows in place and the walk simply tops them up to `height`, so every
    // gate above stays green while the pane shows a stale prefix followed by rows
    // from somewhere else entirely. A mutation proved that, by deleting the clear
    // and surviving.
    //
    // The oracle is the position the view reports. Collecting from it again, with
    // no back-up allowed, has to produce the same rows: the resolved position is
    // what the next frame starts from, so if it does not describe what is on
    // screen then the screen and the scroll state have already disagreed.
    let scratch = fixture("viewport-content");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);

    let mut app = App::new();
    let mut highlighter = Highlighter::new();
    let history = History::new();
    let height = body();

    // **Resolved after every step, not once at the end**, and that is the whole
    // difference. Scrolling seventy times without drawing leaves the position far
    // past the end, which is the *overshoot* path, and that one throws nothing
    // away because `take_file` has not run yet. The short path is only reachable
    // by letting each frame store its resolved position back, which is what the
    // shell does and what this therefore has to do. The first version of this gate
    // scrolled in a bare loop and passed against the mutation it was written for.
    for step in 0..FILES * SPAN + height {
        app.apply(Action::Scroll(1), &mut frame, height)
            .expect("apply");
        let drawn = app
            .view(
                &mut frame,
                &mut highlighter,
                &history,
                Body::diff_only(height),
            )
            .expect("view");

        let from_position = View::collect(
            &mut frame,
            &mut highlighter,
            &history,
            Viewport {
                position: drawn.top,
                anchored: false,
                diff_rows: height,
                ..Viewport::default()
            },
        )
        .expect("collect");

        assert_eq!(
            drawn.rows, from_position.rows,
            "step {step}: the body does not match the position it reports, {:?}",
            drawn.top
        );
    }
}

#[test]
fn the_last_row_of_the_diff_is_always_on_screen_at_the_bottom() {
    // What "rests at the bottom" means, asserted as content rather than as a row
    // count. A body that is full of the *wrong* rows satisfies every gate above.
    let scratch = fixture("viewport-last");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);

    let mut app = App::new();
    let mut highlighter = Highlighter::new();
    let history = History::new();
    let height = body();

    for _ in 0..FILES * SPAN + height {
        app.apply(Action::Scroll(1), &mut frame, height)
            .expect("apply");
    }
    let view: View = app
        .view(
            &mut frame,
            &mut highlighter,
            &history,
            Body::diff_only(height),
        )
        .expect("view");

    // The final file's heading has to be somewhere on screen, and the rows after
    // it have to be the ones that end the diff.
    let last = view
        .rows
        .iter()
        .filter_map(|row| match row {
            vigia::Row::File(FileEntry { path, .. }) => Some(path.clone()),
            _ => None,
        })
        .next_back()
        .expect("a file heading on screen");
    // Asked of the frame rather than guessed from the fixture's naming. Twelve
    // generated files sort `mod_9` *after* `mod_11`, because git orders paths as
    // bytes and not as numbers, so the obvious `FILES - 1` is the wrong name and
    // was the first thing this gate got wrong.
    let want = &frame.files()[FILES - 1].path;
    assert_eq!(
        &last, want,
        "the last file on screen is {last}, not the diff's last file"
    );
}
