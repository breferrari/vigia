//! The pinned file list, which `SPEC.md` §11.1 makes the upper of two regions.
//!
//! > The body is two regions: a pinned file list, a rule, and the scrolling
//! > diff.
//!
//! Four claims live here, and they fail in different ways, which is why they are
//! four tests rather than one screen inspected from four angles.
//!
//! **Its height is a function of pane height and changed-file count, and of
//! nothing else.** That is the same pair `Footer::plan` takes, and for the same
//! reason: both change only when the diff does, so neither can move content
//! under a reader who did nothing. A notice is the thing most likely to break it
//! and gets its own gate.
//!
//! **It gives way before the diff does.** A monitor whose diff has been squeezed
//! out by the map of the diff has stopped being one.
//!
//! **It tracks the diff.** The caret marks the file the diff is inside, and the
//! window slides to keep that file on screen, so the region is correct with no
//! interaction the way I5 requires of everything else.
//!
//! What is deliberately *not* here is what the region costs: that is an I4 claim
//! and `tests/reads.rs` is where every I4 claim in this crate lives, beside the
//! ones bounding the diff walk rather than in a file of its own.

#[path = "../../vigia-core/tests/support/mod.rs"]
mod support;

use ratatui::layout::Rect;
use vigia::{Action, App, Body, LIST_ROWS, Position, body_layout};
use vigia_core::{Highlighter, History};

use support::{Scratch, materialise};

/// The smallest diff the layout will leave behind, restated rather than
/// imported.
///
/// `render.rs` keeps this as a private constant. A test that shared it would
/// agree with the renderer by construction, which is the same reason the
/// sparkline's ramp and the heat strip's block are restated in
/// `tests/legibility.rs`.
const MIN_BODY: usize = 2;

/// Eighty columns, where the footer is one line whatever the state, so nothing
/// below is entangled with I6's two-line footer.
const WIDE: u16 = 80;

fn chrome(app: &App) -> vigia::Chrome {
    app.chrome("fixture", None)
}

fn split(width: u16, height: u16, files: usize) -> Body {
    body_layout(Rect::new(0, 0, width, height), &chrome(&App::new()), files)
}

#[test]
fn the_list_grows_to_the_file_count_and_stops_at_the_cap() {
    // The difference between this region and a fixed one, and the whole reason
    // `assets/preview.svg` can be honest: three changed files draw three rows,
    // exactly the picture, and a formatter touching two hundred draws the cap.
    //
    // A gate over only the capped end would pass against a region that was
    // always `LIST_ROWS` tall and padded with blanks, which is the design this
    // one was chosen over.
    for files in 1..=LIST_ROWS {
        assert_eq!(
            split(WIDE, 24, files).list,
            files,
            "{files} changed files did not draw {files} rows"
        );
    }
    for files in [LIST_ROWS + 1, 47, 200, 10_000] {
        assert_eq!(
            split(WIDE, 24, files).list,
            LIST_ROWS,
            "{files} changed files drew more than the cap"
        );
    }

    // Nothing changed is B3's empty state, which has no list and no rule over
    // it: a rule above nothing is chrome announcing an absent region.
    let empty = split(WIDE, 24, 0);
    assert_eq!(empty.list, 0);
    assert!(!empty.rule, "a rule was drawn over an empty list");
}

#[test]
fn the_list_region_gives_way_before_the_diff_falls_below_min_body() {
    // The ordering rule. The list is what shrinks, and it shrinks to nothing
    // rather than taking the diff below two rows, because a pane showing a map
    // of a diff it can no longer show has stopped being a monitor.
    //
    // Swept over every height a pane can plausibly be rather than checked at the
    // boundary, because the boundary moves with the footer's own height and a
    // gate that hardcoded it would be restating the arithmetic it is checking.
    let mut saw_a_region = false;
    let mut saw_it_give_way = false;

    for height in 1..=40u16 {
        for files in [1usize, 3, LIST_ROWS, 100] {
            let body = split(WIDE, height, files);
            if body.list > 0 {
                saw_a_region = true;
                assert!(
                    body.diff >= MIN_BODY,
                    "at {WIDE}x{height} over {files} files the list took {} rows \
                     and left the diff {}, below MIN_BODY",
                    body.list,
                    body.diff
                );
                assert!(body.rule, "a list was drawn with no rule under it");
            } else {
                saw_it_give_way = true;
                assert!(!body.rule, "a rule was drawn with no list above it");
            }
        }
    }

    assert!(saw_a_region, "no height in the sweep drew a region at all");
    assert!(
        saw_it_give_way,
        "no height in the sweep was short enough to drop the region, so the \
         giving-way half proves nothing"
    );
}

#[test]
fn a_notice_does_not_change_the_list_height() {
    // §11.1's no-jog rule, one region up from the footer it was written for. A
    // notice is transient: a file that vanished between being named and being
    // read, a repository mid-`git gc`. If one could resize the region, the
    // reader's diff would jump a row and back every time one flickered, which is
    // the same thing a resize is forbidden from doing.
    //
    // Follow state is swept too. It changes the *footer's* height, so it changes
    // the body, and this is what says the list still divides that body the same
    // way rather than absorbing the difference itself.
    let mut app = App::new();
    let quiet = chrome(&app);
    app.warn("a file vanished between being named and being read");
    let noisy = chrome(&app);

    let mut compared = 0;
    for height in 3..=40u16 {
        for width in [40u16, WIDE, 120] {
            for files in [1usize, 3, 100] {
                let area = Rect::new(0, 0, width, height);
                let without = body_layout(area, &quiet, files);
                let with = body_layout(area, &noisy, files);
                assert_eq!(
                    without, with,
                    "at {width}x{height} over {files} files a notice changed the \
                     layout from {without:?} to {with:?}"
                );
                compared += 1;
            }
        }
    }
    assert!(compared > 0, "the sweep compared nothing");
}

#[test]
fn the_list_window_slides_to_keep_the_current_file_visible() {
    // The tracking half, and what makes the caret worth drawing. Scroll the diff
    // a file at a time and the window has to follow, or the map stops being a
    // map of where the reader is.
    //
    // Asserted at **every** step rather than at the end, because the failure is
    // not at the far end of the walk: a window that never moved would satisfy a
    // final check on a fixture short enough for one screenful and fail silently
    // on a longer one.
    const FILES: usize = 40;
    const SPAN: usize = 4;

    let scratch = Scratch::large_diff("list-track", FILES, 1);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);

    let mut app = App::new();
    let mut highlighter = Highlighter::new();
    let history = History::new();
    let body = split(WIDE, 24, FILES);
    assert_eq!(body.list, LIST_ROWS, "the fixture does not fill the list");

    let mut moved_window = false;
    for step in 0..FILES {
        let view = app
            .view(&mut frame, &mut highlighter, &history, body)
            .expect("view");

        assert_eq!(
            view.current, view.top.file,
            "step {step}: the caret names a different file from the diff's top"
        );
        assert!(
            view.list_top <= view.current && view.current < view.list_top + view.list.len(),
            "step {step}: the diff is inside file {} and the list shows {}..{}",
            view.current,
            view.list_top,
            view.list_top + view.list.len()
        );
        assert_eq!(
            view.list.len(),
            LIST_ROWS,
            "step {step}: the window shrank instead of sliding"
        );
        if view.list_top > 0 {
            moved_window = true;
        }

        // A whole file per step, so the current file changes every time and the
        // window is really asked to move rather than being carried by one long
        // file's worth of rows.
        for _ in 0..SPAN {
            app.apply(Action::Scroll(1), &mut frame, body.diff)
                .expect("apply");
        }
    }

    assert!(
        moved_window,
        "the window never left the top, so sliding was never exercised"
    );
}

#[test]
fn the_caret_follows_a_jump_rather_than_the_position_it_was_asked_for() {
    // `View::current` is resolved **after** the walk, and this is why. A position
    // can overshoot its file, point past a list the agent in the other pane has
    // shortened, or be backed up to rest the diff's last row on the bottom. Mark
    // the caret from the request and it names a file the diff is not in, on
    // exactly the frames that moved.
    const FILES: usize = 12;

    let scratch = Scratch::large_diff("list-jump", FILES, 1);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);

    let mut app = App::new();
    let mut highlighter = Highlighter::new();
    let history = History::new();
    let body = split(WIDE, 24, FILES);

    // Far past the end, which resolves back to the last screenful rather than to
    // the file that was asked for.
    for _ in 0..FILES * 8 {
        app.apply(Action::Scroll(1), &mut frame, body.diff)
            .expect("apply");
    }
    let view = app
        .view(&mut frame, &mut highlighter, &history, body)
        .expect("view");

    assert_ne!(
        view.current,
        FILES - 1,
        "the fixture resolved to the last file, so a caret taken from the \
         request would have been right by accident"
    );
    assert_eq!(
        view.current, view.top.file,
        "the caret did not follow the resolved position"
    );
    assert!(
        view.list_top <= view.current && view.current < view.list_top + view.list.len(),
        "the window does not contain the file the diff resolved into"
    );

    // And `g` is a jump rather than a scroll, which the window has to honour the
    // same way.
    app.apply(Action::Top, &mut frame, body.diff)
        .expect("apply");
    let view = app
        .view(&mut frame, &mut highlighter, &history, body)
        .expect("view");
    assert_eq!(view.current, 0);
    assert_eq!(view.top, Position::default());
    assert_eq!(view.list_top, 0, "the window did not follow the jump home");
}

#[test]
fn scrolling_the_list_leaves_the_diff_where_it_was() {
    // The half of the ruling that is about state rather than about keys. `J`
    // moves a window over the map; the diff does not move, and follow stays
    // engaged, so an agent writing in the other pane goes on dragging the diff
    // to what it just wrote while a reader browses the changed set.
    const FILES: usize = 40;

    let scratch = Scratch::large_diff("list-browse", FILES, 1);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);

    let mut app = App::new();
    let mut highlighter = Highlighter::new();
    let history = History::new();
    let body = split(WIDE, 24, FILES);

    let before = app
        .view(&mut frame, &mut highlighter, &history, body)
        .expect("view");
    assert!(app.following(), "the shell did not start following");

    for _ in 0..10 {
        app.apply(Action::ScrollList(1), &mut frame, body.diff)
            .expect("apply");
    }
    let after = app
        .view(&mut frame, &mut highlighter, &history, body)
        .expect("view");

    assert_eq!(
        after.top, before.top,
        "scrolling the list moved the diff from {:?} to {:?}",
        before.top, after.top
    );
    assert!(app.following(), "scrolling the list disengaged follow mode");
    assert!(
        after.list_top > before.list_top,
        "the window did not move: {} to {}",
        before.list_top,
        after.list_top
    );
    assert_eq!(
        after.list.len(),
        LIST_ROWS,
        "the window shrank instead of sliding"
    );

    // The caret is gone from the region, because the diff is no longer inside
    // any file the window is showing. That is honest rather than a gap: the map
    // is deliberately looking somewhere else, and inventing a caret would say
    // the diff had moved.
    assert!(
        after.current < after.list_top || after.current >= after.list_top + after.list.len(),
        "the fixture did not browse past the current file, so this proves nothing"
    );
}

#[test]
fn the_window_is_overtaken_when_the_diff_leaves_it() {
    // The other side of the rule above. Browsing sticks, right up until the
    // thing the map is a map *of* moves somewhere the map cannot show, at which
    // point the map has to follow. Without this, a reader who scrolled the list
    // once would have a region that never agreed with the diff again.
    const FILES: usize = 40;

    let scratch = Scratch::large_diff("list-overtaken", FILES, 1);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);

    let mut app = App::new();
    let mut highlighter = Highlighter::new();
    let history = History::new();
    let body = split(WIDE, 24, FILES);

    for _ in 0..20 {
        app.apply(Action::ScrollList(1), &mut frame, body.diff)
            .expect("apply");
    }
    let browsed = app
        .view(&mut frame, &mut highlighter, &history, body)
        .expect("view");
    assert!(browsed.list_top > 0, "the window never moved");

    // Now take the diff somewhere the window cannot show.
    app.apply(Action::Bottom, &mut frame, body.diff)
        .expect("apply");
    let caught = app
        .view(&mut frame, &mut highlighter, &history, body)
        .expect("view");

    assert_eq!(
        caught.current,
        FILES - 1,
        "the jump did not land on the last file"
    );
    assert!(
        caught.list_top <= caught.current && caught.current < caught.list_top + caught.list.len(),
        "the window at {} does not contain file {}",
        caught.list_top,
        caught.current
    );
}
