//! The pinned file list, which `SPEC.md` §11.1 makes the middle of three
//! regions.
//!
//! > The body is three regions: a masthead, a pinned file list, a rule, and the
//! > scrolling diff.
//!
//! Five claims live here, and they fail in different ways, which is why they are
//! five tests rather than one screen inspected from five angles.
//!
//! **Its height is a function of pane height and changed-file count, and of
//! nothing else.** That is the same pair `Footer::plan` takes, and for the same
//! reason: both change only when the diff does, so neither can move content
//! under a reader who did nothing. A notice is the thing most likely to break it
//! and gets its own gate.
//!
//! **It grows with the pane, and no pane that shipped draws differently.** The
//! cap is a share of the pane rather than a flat six since
//! [#160](https://github.com/breferrari/vigia/issues/160). The two halves are one
//! gate because they are one ladder, and because the second is what says every
//! other fixture in this repo is untouched.
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

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use vigia::{
    Action, App, Body, Glyphs, LIST_SETTLED, Pointing, Position, Theme, View, Viewport,
    body_layout, regions, render,
};
use vigia_core::{Highlighter, History, Origin};

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

/// The top of every height sweep here.
///
/// **Well past any pane anyone runs, deliberately.** The cap is a share of the
/// pane since [#160](https://github.com/breferrari/vigia/issues/160), so a sweep
/// that stopped at a realistic height would be asserting the ladder's monotonicity
/// over the rungs that happen to be reachable today rather than over the rule.
const TALLEST: u16 = 120;

/// The pane `SPEC.md` §11.1 sized the list against, and the anchor of the ladder.
///
/// Every claim below about *what did not move* is measured from here, and none of
/// them restates the share. The ladder's own arithmetic is deliberately not
/// available to this file: a gate that recomputed the quarter would agree with the
/// renderer by construction and go on agreeing with it while both were wrong.
const REFERENCE: u16 = 24;

/// More changed files than any cap the sweeps below can reach.
///
/// So the changed-file clamp is never the one doing the work, and what a sweep
/// measures is the pane's own answer. Shared rather than declared per test,
/// because it is one fact: two copies would let one sweep's margin be widened
/// while the other's silently stayed where it was.
const MANY: usize = 500;

/// The tall pane [#125](https://github.com/breferrari/vigia/issues/125) named
/// when it filed this rung: *"a 50-row pane keeping six draws void where the map
/// could be"*. Taken from the issue rather than derived from the share.
const DEEP: u16 = 50;

fn chrome(app: &App) -> vigia::Chrome {
    app.chrome("fixture", None, Pointing::default(), 0, "")
}

/// The same, with the rail asked for.
///
/// **Two gates here sweep past 134 columns to reach the rail's own arm, and
/// [#295](https://github.com/breferrari/vigia/issues/295) made that a gesture.**
/// The default chrome stopped drawing a rail at any width, so both sweeps went on
/// covering the stacked shape twice under comments claiming otherwise, and one of
/// them is what `Body::split`'s docblock names as holding the notice invariant it
/// gave up when it took a `&Chrome`. Found by that change's own audit; the
/// `saw_rail` guards below are what makes it loud rather than silent next time.
fn railed(app: &App) -> vigia::Chrome {
    vigia::Chrome {
        rail: true,
        ..chrome(app)
    }
}

fn split(width: u16, height: u16, files: usize) -> Body {
    body_layout(
        Rect::new(0, 0, width, height),
        &chrome(&App::new()),
        files,
        files,
    )
}

/// [`split`], sized the way the shell sizes a region for **this** changed set.
///
/// **`split` passes the file count for both of `body_layout`'s inputs**, which
/// is right only while every drawn row is a file. Sizing a grouped region that
/// way leaves it two rows short of what the shell gives it, so the tests agree
/// with a bug rather than with the product.
fn split_for(width: u16, height: u16, frame: &vigia_core::Frame) -> Body {
    body_layout(
        Rect::new(0, 0, width, height),
        &chrome(&App::new()),
        frame.files().len(),
        vigia::list_rows_wanted(frame.files()),
    )
}

/// Each region reports its **own** bar's column, not the pane's.
///
/// **The claim the removed field could not make**
/// ([#251](https://github.com/breferrari/vigia/issues/251)). One `Option<u16>`
/// documented as *"the column both scrollbars are drawn in"* is true only while
/// both regions span the pane and their bars
/// land on the same right edge. Beside a rail
/// ([#252](https://github.com/breferrari/vigia/issues/252)) they do not.
///
/// **What this proves, and what it does not.** The expected column is restated
/// from the pane rather than read off the renderer's ladder, so a bar drawn in
/// the wrong column reddens it. It cannot tell the two regions apart **on this
/// screen**: the pane here is eighty columns, which is a stacked layout, and
/// there `Body::areas` spreads `..area` to both regions, so their rects share an
/// `x` and a `width` and therefore a bar column. A `regions` that handed
/// `Bar::region` the wrong rect would still produce these two numbers.
///
/// **Two gates catch that and neither is this one.**
/// `tests/legibility.rs::the_body_tiles_the_pane_with_no_gap_and_no_overlap`
/// compares each region's reported rect against the one the painter draws into,
/// and a mutation swapping the two reddens it along with eleven render gates.
/// `tests/rail.rs::the_two_regions_are_two_regions` is the one that looks
/// impossible and is not: since
/// [#252](https://github.com/breferrari/vigia/issues/252) a wide pane puts the
/// two regions in different columns, so the two bars land in different columns
/// and the distinction is finally drawn rather than argued.
///
/// This one owns the narrower claim, and it owns it on the screen a reader is
/// most often on: the column is a region's own right edge, and it is `None` where
/// no bar is drawn.
#[test]
fn each_region_reports_its_own_bar_column() {
    // Enough files to overflow the list and enough rows to overflow the diff, so
    // both regions have somewhere to scroll and both draw a bar.
    let scratch = Scratch::large_diff("list-own-bar", 40, 12);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    // Wide and tall enough that both regions overflow and both draw a bar, which
    // is the only screen this gate is about.
    let area = Rect::new(0, 0, 100, 20);
    let chrome = chrome(&app);
    let body = body_layout(area, &chrome, frame.files().len(), frame.files().len());
    let view = app
        .view(&mut frame, &mut highlighter, &history, body)
        .expect("view");
    let drawn = regions(area, &chrome, &view);

    let edge = area.x + area.width - 1;
    assert_eq!(
        drawn.list.bar,
        Some(edge),
        "the list's bar is not on the right edge of the region it is drawn in"
    );
    assert_eq!(
        drawn.diff.bar,
        Some(edge),
        "the diff's bar is not on the right edge of the region it is drawn in"
    );

    // Non-vacuity from the other side: a screen with nothing to scroll reports no
    // bar at all, so the two assertions above are about a drawn bar rather than
    // about a field that is always `Some`.
    let bare = View {
        total_rows: 1,
        ..View::default()
    };
    let none = regions(area, &chrome, &bare);
    assert_eq!(
        (none.list.bar, none.diff.bar),
        (None, None),
        "a screen with nothing to scroll still told the pointer about a bar"
    );
}

#[test]
fn the_list_grows_to_the_file_count_and_stops_at_the_cap() {
    // The difference between this region and a fixed one, and the whole reason
    // `assets/preview.svg` can be honest: three changed files draw three rows,
    // exactly the picture, and a formatter touching two hundred draws the cap.
    //
    // A gate over only the capped end would pass against a region that was
    // always `LIST_SETTLED` tall and padded with blanks, which is the design this
    // one was chosen over.
    for files in 1..=LIST_SETTLED {
        assert_eq!(
            split(WIDE, 24, files).list,
            files,
            "{files} changed files did not draw {files} rows"
        );
    }
    for files in [LIST_SETTLED + 1, 47, 200, 10_000] {
        assert_eq!(
            split(WIDE, 24, files).list,
            LIST_SETTLED,
            "{files} changed files drew more than the cap"
        );
    }

    // Nothing changed is B3's empty state, which has no list and no rule over
    // it: a rule above nothing is chrome announcing an absent region.
    let empty = split(WIDE, 24, 0);
    assert_eq!(empty.list, 0);
    assert!(!empty.rule, "a rule was drawn over an empty list");
}

/// The list deepens on a tall pane, and no pane that shipped draws differently.
///
/// `SPEC.md` §11.1's cap is a share of the pane since
/// [#160](https://github.com/breferrari/vigia/issues/160), floored at
/// `LIST_SETTLED`. Four claims, and they fail in different ways.
///
/// **Monotone, and stepping by at most one row per row of pane.** The first is
/// the obvious half; the second is the one the *band* rests on, one region up.
/// `Body::split` pays the band out of what the list leaves, so a cap that gained
/// two rows for one row of pane would take a band off a pane that had just grown,
/// which is the "a bigger container holds less" failure the margin ladder is
/// written as a table to avoid. It is asserted here rather than beside the band
/// because it is a property of the cap.
///
/// **Nothing at or below the reference pane moved.** Measured by reading, not by
/// restating the share: the ladder adds rungs strictly above the settled cap, and
/// a rung added to the top of a monotone ladder cannot move a boundary beneath
/// it. So every pane the rest of this suite draws is at the settled cap, and no
/// snapshot in the repo can move. A share that deepened *early* is exactly what
/// the `<= LIST_SETTLED` half catches.
///
/// **The ladder is walked rather than sampled.** A gate that exercised one height
/// would exercise one rung, and it would be the healthiest one. The witness at
/// the end fails if the sweep ever stops reaching several, so a ladder that
/// silently stopped laddering cannot pass this by going quiet.
#[test]
fn the_list_deepens_on_a_tall_pane_and_keeps_its_settled_cap_below() {
    let mut depths = std::collections::BTreeSet::new();
    let mut previous = 0usize;

    for height in 1..=TALLEST {
        let list = split(WIDE, height, MANY).list;

        assert!(
            list >= previous,
            "a pane one row taller than {}x{} drew {list} list rows where the \
             shorter one drew {previous}",
            WIDE,
            height - 1
        );
        assert!(
            list <= previous + 1,
            "at {WIDE}x{height} the list gained {} rows for one row of pane, so \
             the diff pays for the reader's taller terminal",
            list - previous
        );
        if height <= REFERENCE {
            assert!(
                list <= LIST_SETTLED,
                "at {WIDE}x{height} the list drew {list} rows, deeper than the \
                 {LIST_SETTLED} that shipped, on a pane no taller than the one \
                 §11.1 sized it against"
            );
        }

        previous = list;
        depths.insert(list);
    }

    assert_eq!(
        split(WIDE, REFERENCE, MANY).list,
        LIST_SETTLED,
        "the reference pane stopped drawing what §11.1 says it draws"
    );
    assert!(
        split(WIDE, DEEP, MANY).list > LIST_SETTLED,
        "a {DEEP}-row pane still draws {LIST_SETTLED} rows, which is the void \
         #160 was filed for"
    );
    assert!(
        depths.len() >= 4,
        "the sweep reached {} distinct depths, so it is one rung checked \
         repeatedly rather than a ladder walked",
        depths.len()
    );
}

/// A taller pane never costs the masthead its band.
///
/// The band is last in `Body::split`'s clamp order and is paid out of what the
/// list leaves, so the two are coupled in exactly one direction: deepening the
/// list on a taller pane spends rows the band was going to be offered. The
/// arithmetic says it cannot bite — `after` is the body less the list less two,
/// and one more row of pane adds one to the body and at most one to the list — but
/// that argument rests on a step bound, and an argument is not a gate.
///
/// **The failure this refuses is silent and shaped like a feature**: a reader
/// enlarges their terminal, the map gets deeper, and the graph they were watching
/// disappears. Nothing errors, no snapshot moves, and the band is a masthead-on
/// screen so the default pane never sees it.
///
/// **Its reach is narrower than it looks, and stating that is cheaper than
/// re-deriving it.** It sees a band *removed* from a pane that had one, never one
/// *delayed*. Proved by mutation rather than by reading: a cap made to jump two
/// rows at height 24 left this green, because 24 is exactly where the band arrives
/// at this width and the jump pushed the arrival later instead of undoing it. The
/// same jump one row higher fails it at `80x25`. Delay is the ordinary shape of a
/// clamp order and needs no gate; removal is the one that reads as a bug to the
/// person watching.
#[test]
fn a_taller_pane_never_costs_the_band_its_rows() {
    // The masthead is off by default since #204, and the band only exists with it
    // on, so this gate has to ask for the screen it is about.
    let raised = vigia::Chrome {
        masthead: true,
        ..chrome(&App::new())
    };

    let mut had_a_band = false;
    let mut saw_it_arrive = false;

    for height in 1..=TALLEST {
        let body = body_layout(Rect::new(0, 0, WIDE, height), &raised, MANY, MANY);
        let band = body.graph > 0;

        if band && !had_a_band {
            saw_it_arrive = true;
        }
        assert!(
            band || !had_a_band,
            "at {WIDE}x{height} the band was undrawn on a pane taller than one \
             that drew it, so the list took a row the band was keeping"
        );
        had_a_band |= band;
    }

    assert!(
        saw_it_arrive,
        "no height in the sweep ever drew a band, so the gate proves nothing"
    );
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
    //
    // **The sweep reaches `TALLEST` rather than forty since
    // [#160](https://github.com/breferrari/vigia/issues/160)**, and the widening
    // is the point rather than thoroughness: the cap is a share of the pane now,
    // so every height above forty asks the list for rows the old flat cap never
    // let it take, and this is the gate that says the diff keeps `MIN_BODY`
    // through all of them. Widened rather than duplicated, because a second sweep
    // asserting the same thing over a taller range would be this one with a
    // different bound.
    let mut saw_a_region = false;
    let mut saw_it_give_way = false;

    for height in 1..=TALLEST {
        for files in [1usize, 3, LIST_SETTLED, 100] {
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
    // **Railed, because two of the widths below exist to reach the rail's arm**
    // and since #295 the default chrome never does. Both chromes ask, so the
    // notice is the only thing that differs between them, which is what this gate
    // is about.
    let mut app = App::new();
    let quiet = railed(&app);
    app.warn("a file vanished between being named and being read");
    let noisy = railed(&app);

    let mut compared = 0;
    let mut saw_rail = false;
    for height in 3..=40u16 {
        // The rail's widths too: a notice changes the footer's height, which
        // changes the body, and beside a rail the body divides differently.
        for width in [40u16, WIDE, 120, 140, 200] {
            for files in [1usize, 3, 100] {
                let area = Rect::new(0, 0, width, height);
                let without = body_layout(area, &quiet, files, files);
                saw_rail |= without.rail;
                let with = body_layout(area, &noisy, files, files);
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
    assert!(
        saw_rail,
        "no shape in the sweep drew a rail, so the widths past 134 are sweeping \
         the stacked shape a second time"
    );
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
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let body = split(WIDE, 24, FILES);
    assert_eq!(
        body.list, LIST_SETTLED,
        "the fixture does not fill the list"
    );

    let mut moved_window = false;
    for step in 0..FILES {
        let view = app
            .view(&mut frame, &mut highlighter, &history, body)
            .expect("view");

        assert!(
            view.list_top <= view.top.file && view.top.file < view.list_top + view.list.len(),
            "step {step}: the diff is inside file {} and the list shows {}..{}",
            view.top.file,
            view.list_top,
            view.list_top + view.list.len()
        );
        assert_eq!(
            view.list.len(),
            LIST_SETTLED,
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
    let mut highlighter = Highlighter::eager();
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
        view.top.file,
        FILES - 1,
        "the fixture resolved to the last file, so a caret taken from the \
         request would have been right by accident"
    );
    assert!(
        view.list_top <= view.top.file && view.top.file < view.list_top + view.list.len(),
        "the window does not contain the file the diff resolved into"
    );

    // And `g` is a jump rather than a scroll, which the window has to honour the
    // same way.
    app.apply(Action::Top, &mut frame, body.diff)
        .expect("apply");
    let view = app
        .view(&mut frame, &mut highlighter, &history, body)
        .expect("view");
    assert_eq!(view.top.file, 0);
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
    let mut highlighter = Highlighter::eager();
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
        LIST_SETTLED,
        "the window shrank instead of sliding"
    );

    // The caret is gone from the region, because the diff is no longer inside
    // any file the window is showing. That is honest rather than a gap: the map
    // is deliberately looking somewhere else, and inventing a caret would say
    // the diff had moved.
    assert!(
        after.top.file < after.list_top || after.top.file >= after.list_top + after.list.len(),
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
    let mut highlighter = Highlighter::eager();
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
        caught.top.file,
        FILES - 1,
        "the jump did not land on the last file"
    );
    assert!(
        caught.list_top <= caught.top.file && caught.top.file < caught.list_top + caught.list.len(),
        "the window at {} does not contain file {}",
        caught.list_top,
        caught.top.file
    );
}

/// A picture of the two regions at a scale the snapshots do not reach.
///
/// **Ignored, diagnostic, not a gate**, the way `vigia-core`'s
/// `frame_time_distribution` is. Every assertion above is structural, and
/// structure is exactly what cannot tell you whether a screen is *good*: the cap,
/// the caret and both scrollbars are each proved by a number somewhere, and none
/// of those numbers says what fifty changed files actually look like beside an
/// agent. Run it with:
///
/// ```text
/// cargo test --test list -- --ignored --nocapture the_region_at_fifty_files
/// ```
#[test]
#[ignore = "diagnostic, not a gate"]
fn the_region_at_fifty_files() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use vigia::{Theme, render};

    const FILES: usize = 50;
    let scratch = Scratch::large_diff("list-picture", FILES, 6);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);

    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    for (label, top) in [
        ("at the top", 0usize),
        ("scrolled in", 4 * 20),
        ("past the end", 4 * 200),
    ] {
        for _ in 0..top {
            app.apply(Action::Scroll(1), &mut frame, 1).expect("apply");
        }
        let area = ratatui::layout::Rect::new(0, 0, 80, 24);
        let body = body_layout(
            area,
            &app.chrome("vigia", None, Pointing::default(), 0, ""),
            FILES,
            FILES,
        );
        let view = app
            .view(&mut frame, &mut highlighter, &history, body)
            .expect("view");

        let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("terminal");
        let theme = Theme::default();
        let chrome = app.chrome("vigia", None, Pointing::default(), 0, "");
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
        println!("\n=== {FILES} files, {label} ===\n{}", terminal.backend());
    }
}

#[test]
fn the_window_survives_a_pane_too_short_to_show_it() {
    // **The asymmetry `View::collect` already argues against, one region up.**
    // For the diff, a frame with no room to draw "resolved nothing, so it has
    // nothing to say about where the reader is", and reporting a zero would drag
    // them to the top for as long as the pane stayed short. The list did the
    // opposite in exactly that situation: `take_list` zeroed `list_top`, `App`
    // stored it back, and because nothing but a diff-moving action hands the map
    // back, the window never recovered.
    //
    // Reachable by dragging a pane edge, which is a thing a reader does to a
    // monitor beside an agent, and `SPEC.md` §11.1 rules a resize "no state
    // change".
    const FILES: usize = 40;

    let scratch = Scratch::large_diff("list-short-pane", FILES, 1);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);

    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let tall = split(WIDE, 24, FILES);

    for _ in 0..12 {
        app.apply(Action::ScrollList(1), &mut frame, tall.diff)
            .expect("apply");
    }
    let browsed = app
        .view(&mut frame, &mut highlighter, &history, tall)
        .expect("view")
        .list_top;
    assert!(browsed > 0, "the fixture never browsed anywhere");

    // A pane too short for a region at all. Non-vacuity: the layout really must
    // refuse the region here, or this is a redraw at the same size.
    let short = split(WIDE, 5, FILES);
    assert_eq!(short.list, 0, "the short pane still affords a region");
    app.view(&mut frame, &mut highlighter, &history, short)
        .expect("view");

    let restored = app
        .view(&mut frame, &mut highlighter, &history, tall)
        .expect("view");
    assert_eq!(
        restored.list_top, browsed,
        "dragging the pane through a height with no region moved the map from \
         {browsed} to {}",
        restored.list_top
    );
}

#[test]
fn the_two_regions_tile_the_body_exactly() {
    // `Body::clamped_to` holds the layout's only subtraction and had no direct
    // test: mutating its give-back term left the suite green, because no fixture
    // reached a stale view. This is the property in full — the header, the list,
    // the rule, the diff and the footer account for every row of the pane, at
    // every height and for every number of entries a view might carry.
    let mut checked = 0;
    let mut saw_a_clamp = false;
    let mut saw_rail = false;

    for height in 1..=40u16 {
        // **Two widths past the rail's arrival**
        // ([#252](https://github.com/breferrari/vigia/issues/252)), so the one
        // subtraction this gate exists for is exercised in both shapes.
        // `clamped_to`'s rail arm shortens the list and gives nothing back,
        // because beside a rail there is no region below it to give to, and that
        // arm had no direct test until these two widths were added.
        for width in [40u16, WIDE, 120, 140, 200] {
            for files in [0usize, 1, 3, LIST_SETTLED, LIST_SETTLED + 1, 200] {
                let area = Rect::new(0, 0, width, height);
                // Railed, so the two widths past 134 reach `clamped_to`'s rail
                // arm rather than sweeping the stacked shape five times. Since
                // #295 the default chrome never draws one.
                let chrome = railed(&App::new());
                let full = body_layout(area, &chrome, files, files);
                saw_rail |= full.rail;

                for have in 0..=LIST_SETTLED + 2 {
                    let body = full.clamped_to(have);
                    if body.list != full.list {
                        saw_a_clamp = true;
                    }

                    // The footer's own height is not exposed, so it is recovered
                    // from the unclamped split rather than restated: whatever it
                    // is, clamping must not change it.
                    // Every region the body has, so the sum is the body rather
                    // than a subset of it. #158 added the masthead and its air,
                    // and a tiling check that missed a region would report the
                    // pane as short by exactly that region.
                    let footer = usize::from(height).saturating_sub(1 + full.rows());
                    assert_eq!(
                        1 + body.rows() + footer,
                        usize::from(height),
                        "at {width}x{height} over {files} files with {have} \
                         entries, {body:?} plus a header and {footer} footer rows \
                         does not tile the pane"
                    );
                    // **Beside a rail there is no rule at all**, which is
                    // §11.2 B11 dissolved rather than reopened: the list is beside
                    // the diff and there is no boundary for a horizontal rule to
                    // be drawn on. Written as the conjunction rather than as two
                    // gates, so the stacked claim keeps its exact form.
                    assert_eq!(
                        body.rule,
                        !body.rail && body.list > 0,
                        "a rule and a list disagree about each other: {body:?}"
                    );
                    assert_eq!(
                        body.clamped_to(have),
                        body,
                        "clamping twice is not clamping once: {body:?}"
                    );
                    checked += 1;
                }
            }
        }
    }

    assert!(checked > 1000, "the sweep checked only {checked} shapes");
    assert!(
        saw_a_clamp,
        "no shape in the sweep actually shortened the region, so the clamp is \
         never exercised"
    );
    assert!(
        saw_rail,
        "no shape in the sweep drew a rail, so `clamped_to`'s rail arm is covered \
         by a comment rather than by this gate"
    );
}

#[test]
fn collect_resolves_every_degenerate_viewport() {
    // `View::take_list` indexes the frame through `Frame::diff`, which **panics**
    // by design on an index past the end of the file list. That it cannot be
    // reached is currently proved only by a comment. This drives the public
    // signature with the positions that comment is about.
    //
    // It also covers the pair `body_layout` never produces and `View::collect`'s
    // own doc argues at length: no diff rows and a region anyway.
    const FILES: usize = 12;

    let scratch = Scratch::large_diff("list-degenerate", FILES, 1);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    for list_top in [0usize, 1, FILES - 1, FILES, FILES + 9, usize::MAX] {
        for list_rows in [0usize, 1, LIST_SETTLED, FILES, FILES + 5, 10_000] {
            for diff_rows in [0usize, 1, 22] {
                for file in [0usize, FILES - 1, FILES, FILES + 3] {
                    for (list_follows, landing) in [(true, false), (false, false), (true, true)] {
                        let view = View::collect(
                            &mut frame,
                            &mut highlighter,
                            &history,
                            Viewport {
                                position: Position { file, row: 0 },
                                anchored: false,
                                diff_rows,
                                width: 0,
                                wrap: false,
                                list_top,
                                list_rows,
                                list_follows,
                                measured: true,
                                // **A sweep dimension rather than a constant**,
                                // because a landing resolves inside the same
                                // walk and every degenerate shape here is one it
                                // can be asked for in. Pinned at `false` it left
                                // the whole grid blind to it.
                                landing,
                                // **Not a sweep dimension, unlike `landing`
                                // above.** This grid is about how the two
                                // regions divide a pane, and a pinned diff
                                // divides it identically: B16 changes which rows
                                // the body may reach, never how many rows the
                                // body has. `tests/single.rs` sweeps the pin.
                                single: false,
                                // This sweep is about where the two regions
                                // land, which is decided before anything is
                                // coloured. Highlighting on keeps it the same
                                // collect the shell runs after its first frame.
                                highlight: true,
                            },
                        )
                        .expect("collect");

                        assert!(
                            view.list.len() <= list_rows,
                            "asked for {list_rows} list rows and got {}",
                            view.list.len()
                        );
                        // **Only while there is a window.** A pane with no region
                        // hands `list_top` back untouched, which is the whole
                        // point of `the_window_survives_a_pane_too_short_to_show_it`
                        // and is what the diff's own walk does with `top.row`. So
                        // an out-of-range request survives a region-less frame and
                        // is clamped by the next one that draws. What must always
                        // hold is that a window which exists fits inside the file
                        // list, because that is what keeps `Frame::diff` — which
                        // panics on an out-of-range index by design — in range.
                        if !view.list.is_empty() {
                            assert!(
                                view.list_top + view.list.len() <= FILES,
                                "the window {}..{} runs past {FILES} files",
                                view.list_top,
                                view.list_top + view.list.len()
                            );
                        }
                        assert!(
                            view.rows.len() <= diff_rows,
                            "asked for {diff_rows} diff rows and got {}",
                            view.rows.len()
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn browsing_back_up_returns_the_window_to_the_top() {
    // `Action::ScrollList(-1)` end to end. `input.rs` gates the key mapping and
    // the follow ruling, but nothing drove the negative delta through
    // `App::apply` and `App::view`, so `saturating_add_signed`'s down-path and
    // the browse-back-up journey were untested.
    const FILES: usize = 40;

    let scratch = Scratch::large_diff("list-back-up", FILES, 1);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);

    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let body = split(WIDE, 24, FILES);

    for _ in 0..15 {
        app.apply(Action::ScrollList(1), &mut frame, body.diff)
            .expect("apply");
    }
    let out = app
        .view(&mut frame, &mut highlighter, &history, body)
        .expect("view")
        .list_top;
    assert!(out > 0, "the fixture never browsed away");

    // Further back than it went, so the saturation is exercised rather than the
    // arithmetic alone.
    for _ in 0..40 {
        app.apply(Action::ScrollList(-1), &mut frame, body.diff)
            .expect("apply");
    }
    let back = app
        .view(&mut frame, &mut highlighter, &history, body)
        .expect("view");

    assert_eq!(back.list_top, 0, "browsing back up did not reach the top");
    assert_eq!(
        back.list.len(),
        LIST_SETTLED,
        "the window lost rows on the way"
    );
}

/// Dragging the list's scrollbar to the bottom of its track shows the last file.
///
/// The sibling of `scroll.rs`'s diff-drag gate, and the reason both exist: a
/// track that maps onto the whole changed set instead of onto its travel leaves
/// its final `LIST_SETTLED` worth of track dead, because every fraction past the
/// bound clamps to the same window.
///
/// **The middle of the track is the assertion that can tell those apart**, and
/// the ends are not. Mapping onto the whole still reaches the last file, since
/// the clamp catches it; what it does is compress the live part of the track
/// into the first 85% and pin the rest. So the ends are checked because a
/// resolution ignoring the fraction entirely would pass a midpoint check alone,
/// and the midpoint is checked because the ends cannot see the defect.
#[test]
fn dragging_the_list_bar_reaches_the_first_file_and_the_last() {
    use vigia::TRACK_SCALE;

    const FILES: usize = 40;

    let scratch = Scratch::large_diff("list-drag", FILES, 1);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);

    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let body = split(WIDE, 24, FILES);

    // The app learns the region's height from a frame, the way the shell gives
    // it one, rather than being told out of band.
    app.view(&mut frame, &mut highlighter, &history, body)
        .expect("view");

    app.apply(Action::ListTo(TRACK_SCALE), &mut frame, 0)
        .expect("drag to the end");
    let view = app
        .view(&mut frame, &mut highlighter, &history, body)
        .expect("view");
    assert_eq!(
        view.list_top + view.list.len(),
        FILES,
        "the bottom of the track showed files {}..{} of {FILES}",
        view.list_top,
        view.list_top + view.list.len()
    );

    app.apply(Action::ListTo(0), &mut frame, 0)
        .expect("drag to the start");
    let view = app
        .view(&mut frame, &mut highlighter, &history, body)
        .expect("view");
    assert_eq!(view.list_top, 0, "the top of the track did not show file 0");

    // Halfway down the track is halfway down the travel, not halfway down the
    // changed set. The two differ by exactly half the region's height, which is
    // the compression the clamp hides at the ends.
    app.apply(Action::ListTo(TRACK_SCALE / 2), &mut frame, 0)
        .expect("drag to the middle");
    let view = app
        .view(&mut frame, &mut highlighter, &history, body)
        .expect("view");
    assert_eq!(
        view.list_top,
        (FILES - LIST_SETTLED) / 2,
        "halfway down the track showed files {}..{}",
        view.list_top,
        view.list_top + view.list.len()
    );
}

/// The caret walks down the window before the window moves under it.
///
/// **Reported from use, and it is the second fixed-position rule this region
/// has had.** Ending the window on the current file showed the six files before
/// the six the diff was drawing; starting the window on it fixed that and
/// pinned the caret to the first row, so scrolling a seventeen-file tree moved
/// the list on every step while the marker never went anywhere. Both are the
/// same defect: a constant row is not following a file.
///
/// So this asserts **travel**, which neither fixed rule can satisfy and which
/// `the_list_window_slides_to_keep_the_current_file_visible` cannot see, since
/// containment holds for a pinned caret too.
#[test]
fn the_caret_travels_the_window_before_the_window_moves() {
    const FILES: usize = 40;

    let scratch = Scratch::large_diff("list-travel", FILES, 1);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);

    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let body = split(WIDE, 24, FILES);
    assert_eq!(
        body.list, LIST_SETTLED,
        "the fixture does not fill the list"
    );

    // Walking the diff forward one file at a time from the start, the caret's
    // row is the file's own index until the window has to move, and the window
    // does not move until then.
    let mut seen = Vec::new();
    for file in 0..LIST_SETTLED + 4 {
        // `view` is what advances the file, not `apply`: a scroll down adds to
        // the offset and lets `View::collect` carry the overrun into the files
        // after it, so a loop that only applied would spin forever.
        let mut view = app
            .view(&mut frame, &mut highlighter, &history, body)
            .expect("view");
        let mut guard = 0;
        while view.top.file < file {
            app.apply(Action::Scroll(1), &mut frame, body.diff)
                .expect("apply");
            view = app
                .view(&mut frame, &mut highlighter, &history, body)
                .expect("view");
            guard += 1;
            assert!(guard < 10_000, "never reached file {file}");
        }
        let caret = view.top.file - view.list_top;
        seen.push((view.list_top, caret));

        if file < LIST_SETTLED {
            assert_eq!(
                view.list_top, 0,
                "the window moved at file {file}, which still fits in it"
            );
            assert_eq!(
                caret, file,
                "the caret sat at row {caret} for file {file}, so it is pinned \
                 rather than travelling"
            );
        } else {
            // Past the window, the caret rests on the last row and the window
            // advances by exactly the overshoot. More would jump the map; less
            // would leave the current file off it.
            assert_eq!(
                caret,
                LIST_SETTLED - 1,
                "at file {file} the caret left the last row, so the window moved \
                 by more than the overshoot"
            );
            assert_eq!(view.list_top, file + 1 - LIST_SETTLED);
        }
    }

    assert!(
        seen.iter().any(|(top, _)| *top > 0),
        "the window never moved at all over {} files",
        LIST_SETTLED + 4
    );
}

/// Clicking a listed file sends the diff to it.
///
/// The gesture a reader tries without being told, and the one the region most
/// obviously invites by drawing a list of names next to the thing they name.
///
/// It is **not** selection, which is what `SPEC.md` §11.2 B4 refuses: nothing is
/// remembered, no row becomes special, and the event after it means exactly what
/// it would have meant. The same argument already licensed dragging a scrollbar.
///
/// **Run at the reference pane and at a deep one, which is what makes it the
/// gate for #160's ruling on the digits as well as for the gesture.** The digit
/// range stays `1`-`LIST_SETTLED`, so on a pane whose list is deeper than that
/// the rows past it can be named by nothing on the keyboard. What keeps them
/// reachable is this: the hit-test bounds a click by the *region*, so it reaches
/// every drawn row at every height. The deepest offset in the loop is
/// `list_rows - 1`, so the tall pass clicks a row no digit can address.
#[test]
fn clicking_a_listed_file_sends_the_diff_to_it() {
    use ratatui::crossterm::event::{Event, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    use ratatui::layout::Rect;
    use vigia::{action_for, regions};

    const FILES: usize = 40;

    let mut reached_past_the_digits = false;

    for height in [REFERENCE, DEEP] {
        let scratch = Scratch::large_diff(&format!("list-click-{height}"), FILES, 1);
        let worktree = scratch.worktree();
        let mut frame = worktree.frame();
        materialise(&mut frame);

        let mut app = App::new();
        let mut highlighter = Highlighter::eager();
        let history = History::new();
        let body = split(WIDE, height, FILES);
        let view = app
            .view(&mut frame, &mut highlighter, &history, body)
            .expect("view");

        // Through the real hit-test rather than by constructing the action,
        // because the row-to-offset arithmetic is half of what can be wrong here.
        let area = Rect::new(0, 0, WIDE, height);
        let regions = regions(area, &chrome(&app), &view);
        let (list_top_row, list_rows) = (regions.list.top, regions.list.rows);
        assert!(list_rows > 1, "no region was published to click on");
        reached_past_the_digits |= usize::from(list_rows) > LIST_SETTLED;

        let click = |row: u16| {
            Event::Mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                // Not the bar's column: that is a drag, and it is checked first.
                column: 2,
                row,
                modifiers: KeyModifiers::NONE,
            })
        };

        for offset in [0u16, 2, list_rows - 1] {
            let action = action_for(&click(list_top_row + offset), regions);
            assert_eq!(action, Some(Action::ListRow(offset)));
            app.apply(action.expect("action"), &mut frame, body.diff)
                .expect("apply");
            let view = app
                .view(&mut frame, &mut highlighter, &history, body)
                .expect("view");
            assert_eq!(
                view.top,
                Position {
                    file: usize::from(offset),
                    row: 0
                },
                "at {WIDE}x{height}, a click on row {offset} did not put the diff \
                 at the top of that file"
            );
        }

        // And a click on the diff below is still inert, which is B4 standing.
        assert_eq!(action_for(&click(regions.diff.top + 1), regions), None);
    }

    assert!(
        reached_past_the_digits,
        "no pane in the sweep drew a list deeper than the digit range, so the \
         half of this gate that is about #160's ruling proves nothing"
    );
}

/// A digit key press, through the real key map rather than by construction.
fn digit(key: char) -> ratatui::crossterm::event::Event {
    use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

    Event::Key(KeyEvent::new(KeyCode::Char(key), KeyModifiers::NONE))
}

/// A digit names the row it is drawn beside, not the file that many from the top.
///
/// **The distinction the window makes, and the one an absolute index would lose.**
/// `SPEC.md` §11.1 gives the digits the *visible window* of the list: what you can
/// see is what you can name. With the list at the top the two readings agree, so
/// a gate that only ever pressed a digit from a fresh shell would pass against an
/// implementation that resolved the digit against the changed set instead. The
/// second half browses first, which is where they come apart.
///
/// It is still not selection: nothing is remembered, no row becomes special, and
/// the event after it means what it would have meant. `SPEC.md` §11.2 B4 stands
/// for the same reason it stands for a click.
#[test]
fn a_digit_jumps_to_the_file_on_that_row_of_the_window() {
    use vigia::{Regions, action_for};

    const FILES: usize = 40;

    let scratch = Scratch::large_diff("list-digit", FILES, 1);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);

    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let body = split(WIDE, 24, FILES);
    assert_eq!(
        body.list, LIST_SETTLED,
        "the fixture does not fill the list"
    );

    let opening = app
        .view(&mut frame, &mut highlighter, &history, body)
        .expect("view");
    assert_eq!(opening.list_top, 0, "the window did not start at the top");

    // `Regions::default()` deliberately: a digit is a key, so the hit-test that
    // a click needs has nothing to say about it, and passing a real region here
    // would hide a map that had started consulting one.
    let action = action_for(&digit('3'), Regions::default());
    assert_eq!(action, Some(Action::ListRow(2)));
    app.apply(action.expect("action"), &mut frame, body.diff)
        .expect("apply");
    assert_eq!(
        app.view(&mut frame, &mut highlighter, &history, body)
            .expect("view")
            .top,
        Position { file: 2, row: 0 },
        "`3` did not put the diff at the third listed file"
    );

    // Now browse, so the window and the changed set no longer agree, and press
    // the same digit again.
    for _ in 0..10 {
        app.apply(Action::ScrollList(1), &mut frame, body.diff)
            .expect("apply");
    }
    let browsed = app
        .view(&mut frame, &mut highlighter, &history, body)
        .expect("view");
    assert!(
        browsed.list_top > 0,
        "the window did not move, so this proves nothing"
    );

    app.apply(action.expect("action"), &mut frame, body.diff)
        .expect("apply");
    assert_eq!(
        app.view(&mut frame, &mut highlighter, &history, body)
            .expect("view")
            .top,
        Position {
            file: browsed.list_top + 2,
            row: 0
        },
        "`3` resolved against the changed set rather than against the window the \
         reader can see"
    );
}

/// A digit pressed after the diff shrank names no file at all.
///
/// **The case the window bound cannot catch, and the reason both bounds exist.**
/// `App::list_rows` is what the *last frame* drew, so between a paint and the
/// keystroke that follows it the changed set can get shorter: `git reset --hard`,
/// a branch switch, an agent reverting its own work. `SPEC.md` §11.1 names that
/// as an ordinary event on the pane this tool exists for rather than an edge
/// case, and it is exactly when someone looks over.
///
/// Six rows were on screen, two files are left, and `6` still passes the window
/// bound because the window is remembered from the frame that drew six. Only the
/// file-count bound refuses it. Found by mutation: deleting that bound left every
/// other gate in this suite green, because both halves of
/// `a_digit_past_the_drawn_window_is_a_no_op` are caught by the window bound
/// first and neither one isolates this.
///
/// Asserted on `App::position` rather than on the view, deliberately. A view
/// resolves a degenerate position by clamping it, so driving one would hide the
/// defect behind the very clamp that `collect_resolves_every_degenerate_viewport`
/// is about. What is wrong here is the position the keystroke *stored*.
#[test]
fn a_digit_after_the_diff_shrank_names_no_file() {
    use vigia::{Regions, action_for};

    const FILES: usize = 8;
    const LEFT: usize = 2;

    let scratch = Scratch::large_diff("list-digit-shrank", FILES, 1);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);

    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let body = split(WIDE, 24, FILES);
    assert_eq!(
        body.list, LIST_SETTLED,
        "the fixture does not fill the list"
    );

    let before = app
        .view(&mut frame, &mut highlighter, &history, body)
        .expect("view")
        .top;

    // The agent in the other pane puts most of it back.
    for index in LEFT..FILES {
        scratch.git(&["checkout", "--", &format!("src/mod_{index}.rs")]);
    }
    frame.advance().expect("advance");
    assert_eq!(frame.files().len(), LEFT, "the fixture did not shrink");

    // No view in between, which is the whole point: the reader is pressing a key
    // against the screen they can still see.
    let action = action_for(&digit('6'), Regions::default()).expect("action");
    app.apply(action, &mut frame, body.diff).expect("apply");
    assert_eq!(
        app.position(),
        before,
        "`6` moved the diff to a file that no longer exists, against a list that \
         drew {LIST_SETTLED} rows before the tree shrank to {LEFT}"
    );
}

/// A digit naming a row the list is not drawing moves nothing.
///
/// **Two shapes, because they are two different defects and only one of them is
/// interesting.** A list can fall short of the digits from either side, and the
/// arm in `App::apply` needs a bound for each.
///
/// *Fewer files than rows* is the control: the file it would name is past the end
/// of the changed set, so the file-count bound that has been there since the
/// click landed catches it. This half passes with no window bound at all.
///
/// *Fewer rows than digits* is the one that needs the window bound. A nine-row
/// pane beside an agent is ordinary, and the region gives way to the diff before
/// it reaches its cap, so `5` and `6` are on the reader's keyboard while the list
/// is drawing four rows. Without the bound the digit resolves against the file
/// list and the diff jumps to a file that is not on screen — a jump the reader
/// asked for by name, landing somewhere they cannot see they named.
#[test]
fn a_digit_past_the_drawn_window_is_a_no_op() {
    use vigia::{Regions, action_for};

    // Fewer files than rows.
    const FEW: usize = 3;
    let scratch = Scratch::large_diff("list-digit-few-files", FEW, 1);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);

    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let body = split(WIDE, 24, FEW);
    assert_eq!(body.list, FEW, "the fixture does not draw one row per file");

    let before = app
        .view(&mut frame, &mut highlighter, &history, body)
        .expect("view")
        .top;
    let action = action_for(&digit('5'), Regions::default()).expect("action");
    app.apply(action, &mut frame, body.diff).expect("apply");
    assert_eq!(
        app.view(&mut frame, &mut highlighter, &history, body)
            .expect("view")
            .top,
        before,
        "`5` moved the diff with three files changed"
    );

    // Fewer rows than digits. Eight files rather than forty: all this half needs
    // is more files than the region has rows, so the extra thirty-two are two
    // `git` fixtures' worth of setup and a `materialise` over five times the
    // diffs, on every platform in the matrix, proving the same thing. Kept clear
    // of `LIST_SETTLED` so the number does not read as related to the cap.
    const MANY: usize = 8;
    const SHORT: u16 = 9;
    let scratch = Scratch::large_diff("list-digit-short-pane", MANY, 1);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);

    let mut app = App::new();
    let body = split(WIDE, SHORT, MANY);
    assert!(
        body.list > 0 && body.list < LIST_SETTLED,
        "a {SHORT}-row pane drew {} list rows, so this proves nothing",
        body.list
    );

    let before = app
        .view(&mut frame, &mut highlighter, &history, body)
        .expect("view")
        .top;
    let past = char::from_digit(body.list as u32 + 1, 10).expect("a digit past the window");
    let action = action_for(&digit(past), Regions::default()).expect("action");
    app.apply(action, &mut frame, body.diff).expect("apply");
    assert_eq!(
        app.view(&mut frame, &mut highlighter, &history, body)
            .expect("view")
            .top,
        before,
        "`{past}` moved the diff to a file the list is not drawing, with {} rows \
         on screen",
        body.list
    );
}

// ---------------------------------------------------------------------------
// The two runs, and what a drawn row addresses: `SPEC.md` §11.2 **B17**.
// ---------------------------------------------------------------------------

/// A changed set with two runs in it, built by real `git`.
fn two_runs(name: &str) -> support::Scratch {
    let scratch = support::Scratch::new(name);
    for i in 0..6 {
        scratch.write(&format!("src/f{i}.rs"), "one\ntwo\nthree\n");
    }
    scratch.git(&["add", "-A"]);
    scratch.git(&["commit", "-m", "init"]);
    // Three staged, three left on disk.
    for i in 0..3 {
        scratch.write(&format!("src/f{i}.rs"), "one\nSTAGED\nthree\n");
    }
    scratch.git(&["add", "src/f0.rs", "src/f1.rs", "src/f2.rs"]);
    for i in 3..6 {
        scratch.write(&format!("src/f{i}.rs"), "one\nUNSTAGED\nthree\n");
    }
    scratch
}

/// **A separator opens each run, and a window scrolled into the middle of one
/// still opens with that run's own label.**
///
/// Without the second half the rows at the top of a scrolled window are
/// unattributed: a reader who scrolled would be looking at files with nothing on
/// screen saying which comparison they belong to, which is worse than the label
/// costing a row.
#[test]
fn each_run_opens_with_its_own_separator_wherever_the_window_starts() {
    let scratch = two_runs("list-runs");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.show_staged(true);
    frame.advance().expect("advance");
    let files = frame.files();
    assert_eq!(files.len(), 6, "the fixture holds three files in each run");

    // From the top: the unstaged run's label, its files, then the staged run's.
    let plan = vigia::list_plan(files, 0, 8);
    assert_eq!(
        plan[0],
        vigia::Slot::Group {
            origin: Origin::Unstaged,
            count: 3
        },
        "the window does not open with the run it is showing"
    );
    assert_eq!(
        plan[4],
        vigia::Slot::Group {
            origin: Origin::Staged,
            count: 3
        },
        "the second run gains no separator of its own: {plan:?}"
    );

    // Scrolled so the window starts inside the staged run: it still opens with
    // that run's label, and the count is the run's **total** rather than what is
    // visible, so the number answers *how much is there*.
    let plan = vigia::list_plan(files, 4, 3);
    assert_eq!(
        plan[0],
        vigia::Slot::Group {
            origin: Origin::Staged,
            count: 3
        },
        "a window opened mid-run draws unattributed rows: {plan:?}"
    );
}

/// **One run draws no separators at all**, which is what keeps the default pane
/// exactly what it has always been.
#[test]
fn a_single_run_spends_no_row_on_a_label_that_says_nothing() {
    let scratch = two_runs("list-one-run");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    // Staged hidden: one run.
    frame.advance().expect("advance");
    let plan = vigia::list_plan(frame.files(), 0, 8);
    assert!(
        plan.iter().all(|slot| matches!(slot, vigia::Slot::File(_))),
        "a one-run list drew a separator, so the default pane pays for a \
         distinction it is not making: {plan:?}"
    );
}

/// **A separator with no room for a file under it is not drawn.**
///
/// It would be a label naming a run the window cannot show: a row of the map spent
/// saying nothing about the map.
#[test]
fn a_separator_is_not_drawn_with_no_room_for_a_file_beneath_it() {
    let scratch = two_runs("list-tight");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.show_staged(true);
    frame.advance().expect("advance");

    // Four rows from the top: label, three files — and the staged label would be
    // the fifth, so it does not appear.
    let plan = vigia::list_plan(frame.files(), 0, 4);
    assert_eq!(plan.len(), 4);
    assert!(
        matches!(plan[3], vigia::Slot::File(_)),
        "the window ends on a label naming a run it cannot show: {plan:?}"
    );
}

/// **A digit and a click address a *file*, and never the separator above it.**
///
/// The defect this closes is silent: added blind to the window's first file, an
/// offset past a separator names the file *before* the one under the pointer, the
/// jump lands, and nothing on screen says the reader went somewhere they did not
/// point at.
#[test]
fn a_drawn_row_addresses_the_file_under_it_and_a_separator_addresses_nothing() {
    let scratch = two_runs("list-address");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.show_staged(true);
    frame.advance().expect("advance");
    let files = frame.files();

    // Rows: 0 label, 1..3 unstaged, 4 label, 5..7 staged.
    let at = |row| vigia::file_at(files, 0, 8, row);
    assert_eq!(at(0), None, "the unstaged run's label addresses a file");
    assert_eq!(at(1), Some(0));
    assert_eq!(at(3), Some(2));
    assert_eq!(at(4), None, "the staged run's label addresses a file");
    assert_eq!(
        at(5),
        Some(3),
        "the first staged row addresses the file before it, which is the \
         off-by-one a separator introduces and nothing on screen would show"
    );
    assert_eq!(at(7), Some(5));
    assert_eq!(at(8), None, "a row past the window addresses a file");

    // And the naive arithmetic really does disagree, or this asserts nothing.
    assert_ne!(
        at(5),
        Some(5),
        "list_top + offset happens to be right here, so this fixture cannot see \
         the defect it exists for"
    );
}

/// The region asks for the rows it will draw, separators included.
///
/// Measured on the first grouped snapshot taken: a view sized from its files alone
/// spent two of its rows on separators and drew the staged run's heading with
/// **none of its files** under it — the run the reader pressed `a` for, announced
/// and then empty.
#[test]
fn the_list_asks_for_the_rows_its_separators_will_take() {
    let scratch = two_runs("list-rows-wanted");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();

    frame.advance().expect("advance");
    assert_eq!(
        vigia::list_rows_wanted(frame.files()),
        frame.files().len(),
        "a one-run list asks for more rows than it has files"
    );

    frame.show_staged(true);
    frame.advance().expect("advance");
    assert_eq!(
        vigia::list_rows_wanted(frame.files()),
        frame.files().len() + 2,
        "a grouped list asks for its files alone, so the region is two rows short \
         and the last run loses its tail"
    );
}

/// **Pressing `a` shows the staged run on that frame, not on the next write.**
///
/// The defect this closes reached a live pane: `Action::ToggleStaged` told the
/// frame what to walk and nothing re-walked, because `Frame::advance` runs on a
/// **tick** and a keypress is not one. So the reader pressed the key, the header
/// said `0 staged` over a worktree with two staged files, and the pane went on
/// showing exactly what it showed before — until something happened to be written,
/// which on a tree an agent has finished with may be never.
///
/// That is the failure `SPEC.md` §11.2 B17 is named for, one layer down: a key
/// that does nothing a reader can see. The three toggles beside it need no advance
/// because they rearrange rows the frame already holds; this one changes what the
/// frame *contains*.
#[test]
fn asking_for_the_staged_run_fills_it_on_the_same_frame() {
    let scratch = two_runs("list-toggle-advances");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    assert_eq!(
        frame.files().len(),
        3,
        "the fixture opens with the unstaged run alone"
    );

    let mut app = App::new();
    app.apply(Action::ToggleStaged, &mut frame, 20)
        .expect("apply");

    assert_eq!(
        frame.files().len(),
        6,
        "the staged run is empty on the frame the reader asked for it, so `a` \
         draws nothing until something else happens to be written"
    );
    assert!(
        frame
            .files()
            .iter()
            .any(|change| change.origin == Origin::Staged),
        "the frame grew rows that are not the staged run"
    );

    // And pressing it again puts the pane back on the same frame, rather than
    // leaving the run on screen until the next write.
    app.apply(Action::ToggleStaged, &mut frame, 20)
        .expect("apply");
    assert_eq!(
        frame.files().len(),
        3,
        "the staged run is still drawn after the reader asked for it to go"
    );
}

/// **Every file in the list is reachable when both runs are drawn.**
///
/// The window's top was clamped to `files - rows`, which compares a count of
/// *files* against a count of *drawn rows*. A grouped window spends one or two of
/// those rows on separators, so the clamp stops the window one or two files short
/// and the tail of the staged run cannot be scrolled to at all — silently, since
/// nothing on screen says the map has an end it will not show.
#[test]
fn the_last_file_is_reachable_when_the_list_is_grouped() {
    let scratch = two_runs("list-tail-reachable");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.show_staged(true);
    frame.advance().expect("advance");
    let files = frame.files().len();
    assert_eq!(files, 6);

    // A window shorter than the changed set, so the clamp is what decides where
    // it can stop.
    for rows in 3..=files {
        let ceiling = vigia::last_top(frame.files(), rows);
        let reached: Vec<usize> = vigia::list_plan(frame.files(), ceiling, rows)
            .iter()
            .filter_map(|slot| match slot {
                vigia::Slot::File(at) => Some(*at),
                vigia::Slot::Group { .. } => None,
            })
            .collect();
        assert!(
            reached.contains(&(files - 1)),
            "a window of {rows} rows clamped to its furthest top ({ceiling}) \
             cannot reach the last file: it draws {reached:?}"
        );

        // **And the old arithmetic really does fall short here**, or this fixture
        // cannot see the defect it exists for. The naive clamp compares a count of
        // files against a count of drawn rows, and the separators are the
        // difference.
        let naive = files.saturating_sub(rows);
        let naive_reach: Vec<usize> = vigia::list_plan(frame.files(), naive, rows)
            .iter()
            .filter_map(|slot| match slot {
                vigia::Slot::File(at) => Some(*at),
                vigia::Slot::Group { .. } => None,
            })
            .collect();
        if !naive_reach.contains(&(files - 1)) {
            assert!(
                ceiling > naive,
                "the naive clamp misses the last file at {rows} rows and the \
                 rule agrees with it anyway"
            );
        }
    }

    // Non-vacuity over the sweep: at least one width has to be a width the naive
    // clamp gets wrong, or the loop above compared two identical answers.
    let short = 3;
    assert!(
        vigia::last_top(frame.files(), short) > files.saturating_sub(short),
        "no window in this fixture separates the two clamps, so the gate proves \
         nothing"
    );
}

/// **The height a scroll step is measured in is the height the paint lays out.**
///
/// `diff_height` and the paint path both split the body, and they have to agree or
/// a page-down steps by more rows than the diff has. They took the same inputs
/// until the list's row budget stopped being its file count: `diff_height` went on
/// passing `files` for both, so with the staged run drawn it computed a diff up to
/// two rows taller than the one the reader is looking at.
#[test]
fn the_scroll_step_is_measured_in_the_height_the_paint_uses() {
    let scratch = two_runs("list-diff-height");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.show_staged(true);
    frame.advance().expect("advance");

    let chrome = vigia::Chrome {
        staged: Some(3),
        ..vigia::App::new().chrome("fixture", None, vigia::Pointing::default(), 0, "")
    };
    // Both derived from the changed set the way `Shell::diff_rows_for` and
    // `Shell::paint` derive them, which is what the two seams now do by
    // construction: each is handed the slice rather than a pair of numbers.
    let files = frame.files();
    let (count, wanted) = (files.len(), vigia::list_rows_wanted(files));

    let mut separated = 0usize;
    for height in 10..=30u16 {
        let at = Rect::new(0, 0, 80, height);
        let stepped = vigia::diff_height(at, &chrome, count, wanted);
        let laid = body_layout(at, &chrome, count, wanted).diff;
        assert_eq!(
            stepped, laid,
            "at 80x{height} a scroll step is measured in {stepped} rows where the \
             paint lays out {laid}"
        );

        // **And the two inputs are not interchangeable, which is the finding.**
        // `diff_height` took one number and passed it for both until B17 gave the
        // list a row budget its file count no longer equals. Counting the heights
        // where the old form disagrees is what stops this gate from being the
        // identity it would otherwise be: `diff_height` *is* `body_layout(..).diff`,
        // so comparing them with the same arguments asserts nothing at all.
        if vigia::diff_height(at, &chrome, count, count) != laid {
            separated += 1;
        }
    }

    assert!(
        separated > 0,
        "no height in the sweep tells the two inputs apart, so this fixture \
         cannot see a caller that passes the file count for both"
    );
}

/// **`last_top` is the *tightest* ceiling, not merely a top that works.**
///
/// The first version returned the **largest** top from which the last file is
/// drawn, which for any window is the last file's own index: trivially true, and a
/// ceiling that never binds. The window could then scroll past the end and leave
/// blank rows under the last file, which is the thing the clamp exists to prevent
/// and which its own docblock claims it does.
///
/// **The gate that missed it asserted only that the ceiling shows the last file**,
/// satisfied vacuously by the wrong answer. Naming the row *below* is what makes
/// it a bound rather than an example.
#[test]
fn the_lists_ceiling_is_the_tightest_top_that_still_shows_the_last_file() {
    let scratch = two_runs("list-tight-ceiling");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.show_staged(true);
    frame.advance().expect("advance");
    let files = frame.files();
    let last = files.len() - 1;

    let draws = |top: usize, rows: usize| {
        vigia::list_plan(files, top, rows)
            .iter()
            .any(|slot| matches!(slot, vigia::Slot::File(at) if *at == last))
    };

    for rows in 2..=files.len() + 2 {
        let ceiling = vigia::last_top(files, rows);
        assert!(
            draws(ceiling, rows),
            "the ceiling for {rows} rows does not show the last file"
        );
        assert!(
            ceiling == 0 || !draws(ceiling - 1, rows),
            "the ceiling for {rows} rows is {ceiling}, and {} shows the last file \
             too, so it is an example rather than a bound and the window can \
             scroll past the end",
            ceiling - 1
        );
    }
}

/// **Follow keeps the caret's own file inside the window it computes.**
///
/// The window's forward push was `current + 1 - rows`, which subtracts a count of
/// **drawn rows** from a **file index**. Once a window can hold rows that are not
/// files the two are different units, and the arithmetic lands short: the list
/// scrolls, the file the diff is inside is not among the rows it drew, and the
/// caret marking that file simply is not there.
#[test]
fn follow_marks_the_current_file_at_every_window_it_chooses() {
    let scratch = two_runs("list-follow-caret");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.show_staged(true);
    frame.advance().expect("advance");

    let mut highlighter = Highlighter::eager();
    let history = History::new();
    for height in 12..=24u16 {
        for current in 0..frame.files().len() {
            let mut app = App::new();
            let body = split_for(80, height, &frame);
            // Put the diff inside `current`, the way a digit does, then collect
            // and ask whether that file is on the map the list drew.
            app.apply(Action::File(current as isize), &mut frame, 40)
                .expect("apply");
            for _ in 0..current {
                app.apply(Action::File(1), &mut frame, 40).expect("apply");
            }
            let view = app
                .view(&mut frame, &mut highlighter, &history, body)
                .expect("collect");
            if view.list.is_empty() {
                continue;
            }
            let drawn: Vec<usize> = vigia::list_plan(frame.files(), view.list_top, view.list.len())
                .iter()
                .filter_map(|slot| match slot {
                    vigia::Slot::File(at) => Some(*at),
                    vigia::Slot::Group { .. } => None,
                })
                .collect();
            assert!(
                drawn.contains(&view.top.file),
                "at 80x{height} with the diff inside file {} the list drew \
                 {drawn:?} from top {}, so the caret's own file is not on the map",
                view.top.file,
                view.list_top
            );
        }
    }
}

/// **`J` reaches the end of a grouped list.**
///
/// `browse` clamps with `files - list_rows`, which is the naive bound the ceiling
/// replaced — and `take_list` only takes the smaller of the two, so it cannot
/// raise one that is already too low. A reader scrolling the map with `J` stopped
/// one or two files short of the end with nothing saying so.
#[test]
fn browsing_reaches_the_bottom_of_a_grouped_list() {
    let scratch = two_runs("list-browse-tail");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.show_staged(true);
    frame.advance().expect("advance");
    let last = frame.files().len() - 1;

    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let body = split_for(80, 16, &frame);
    // Prime `list_rows`, then scroll the map to its end and past it.
    app.view(&mut frame, &mut highlighter, &history, body)
        .expect("collect");
    for _ in 0..20 {
        app.apply(Action::ScrollList(1), &mut frame, 40)
            .expect("apply");
    }
    let view = app
        .view(&mut frame, &mut highlighter, &history, body)
        .expect("collect");

    let drawn: Vec<usize> = vigia::list_plan(frame.files(), view.list_top, view.list.len())
        .iter()
        .filter_map(|slot| match slot {
            vigia::Slot::File(at) => Some(*at),
            vigia::Slot::Group { .. } => None,
        })
        .collect();
    assert!(
        drawn.contains(&last),
        "`J` held to the end drew {drawn:?} from top {}, so the last file of the \
         staged run cannot be reached at all",
        view.list_top
    );
}

/// **A one-row grouped window draws the run's label, not an unlabelled file.**
///
/// The opposite rule, that furniture gives way before content, was overruled
/// after a real worktree drew a *staged* file on the last row beneath a heading
/// that said `unstaged`: the saved row is not worth the list stating the wrong
/// run for a
/// change. See `no_file_is_ever_planned_without_its_own_runs_label`, which is
/// the general form; this one pins the narrowest case, where the whole window
/// is the label.
///
/// The original defect this test was written for is still closed, and that is
/// the part to preserve: a one-row grouped list must not come out **empty**.
/// The label is pushed and kept, never emitted and retracted, so the region
/// always draws the row `Body::split` gave it.
#[test]
fn a_one_row_list_draws_its_runs_label() {
    let scratch = two_runs("list-one-row");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.show_staged(true);
    frame.advance().expect("advance");
    let files = frame.files();

    for top in 0..files.len() {
        let plan = vigia::list_plan(files, top, 1);
        assert_eq!(
            plan.len(),
            1,
            "a one-row list from top {top} drew {} rows",
            plan.len()
        );
        assert!(
            matches!(plan[0], vigia::Slot::Group { .. }),
            "a one-row list from top {top} drew a file with no run label above \
             it, which files the change under whatever heading happens to be \
             on screen: {plan:?}"
        );
    }

    // And two rows is where a label first earns its keep: one for it, one for the
    // file under it.
    let plan = vigia::list_plan(files, 0, 2);
    assert!(
        matches!(plan[0], vigia::Slot::Group { .. }) && matches!(plan[1], vigia::Slot::File(0)),
        "two rows do not buy a label and the file beneath it: {plan:?}"
    );
}

/// **The plan's files run contiguously from `top`, which is what lets the painter
/// count rather than ask.**
///
/// `Painter::list` walks `view.list` and increments a file index per drawn file,
/// starting at `view.list_top`. That is only correct while the plan's files are
/// `top`, `top + 1`, … with nothing skipped — and it is a coupling that prose
/// alone holds, between a function in `view.rs` and a loop in `render.rs` that
/// never calls it. If the plan ever grew a gap, every row below it would draw the
/// caret and the hover mark against the wrong file, silently.
///
/// Swept over every `(top, rows)` the fixture can produce, on a grouped list,
/// because separators are the only thing that has ever made the two counts differ.
#[test]
fn the_plans_files_run_contiguously_from_the_top_it_was_given() {
    let scratch = two_runs("list-contiguous");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.show_staged(true);
    frame.advance().expect("advance");
    let files = frame.files();

    let mut saw_a_separator = false;
    for top in 0..files.len() {
        for rows in 1..=files.len() + 3 {
            let plan = vigia::list_plan(files, top, rows);
            saw_a_separator |= plan
                .iter()
                .any(|slot| matches!(slot, vigia::Slot::Group { .. }));
            let drawn: Vec<usize> = plan
                .iter()
                .filter_map(|slot| match slot {
                    vigia::Slot::File(at) => Some(*at),
                    vigia::Slot::Group { .. } => None,
                })
                .collect();
            let want: Vec<usize> = (top..top + drawn.len()).collect();
            assert_eq!(
                drawn, want,
                "the plan from top {top} with {rows} rows drew {drawn:?}, which is \
                 not a run starting at {top}: the painter counts files as it draws \
                 them and would mark the wrong one"
            );
            assert!(
                plan.len() <= rows,
                "the plan from top {top} drew {} rows where the region has {rows}",
                plan.len()
            );
        }
    }
    assert!(
        saw_a_separator,
        "the sweep drew no separator at all, so it never exercised the only thing \
         that can make the two counts differ"
    );
}

/// **The list's scrollbar and its drag agree about how far the window can go.**
///
/// All three terms of that bar are files: the position is a file index, the total
/// is the changed set, and the span has to be one too. Handed the *row* count it
/// over-reported how much of the list was on screen, so the thumb drew longer than
/// the travel `Action::ListTo` maps the track onto — the pointer reached the
/// bottom of a thumb that said it was already there.
#[test]
fn the_lists_bar_is_measured_in_files_at_both_ends() {
    let scratch = two_runs("list-bar-units");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.show_staged(true);
    frame.advance().expect("advance");

    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let body = split_for(80, 16, &frame);
    let view = app
        .view(&mut frame, &mut highlighter, &history, body)
        .expect("collect");

    assert!(
        view.list
            .iter()
            .any(|row| matches!(row, vigia::ListRow::Group { .. })),
        "the fixture drew no separator, so the row count and the file count are \
         the same number and this asserts nothing"
    );
    assert_eq!(
        view.listed_files(),
        view.list
            .iter()
            .filter(|row| matches!(row, vigia::ListRow::File(_)))
            .count(),
        "the span counts something other than the files on screen"
    );
    assert!(
        view.listed_files() < view.list.len(),
        "a grouped window spends no row on a separator, so the two units cannot \
         be told apart here"
    );

    // And the span never claims more of the list than there is travel for: the
    // window can start at `last_top` at the furthest, so what it shows plus what
    // it can still scroll past has to reach the whole changed set.
    let ceiling = vigia::last_top(frame.files(), view.listed_files().max(1));
    assert!(
        ceiling + view.listed_files() >= view.files,
        "the window shows {} files and can start no later than {ceiling}, which \
         together do not reach the {} the tree has: the bar would report travel \
         the list does not have",
        view.listed_files(),
        view.files
    );
}

/// **The thumb keeps its length as the window scrolls.**
///
/// That is the reader-visible half of the units fix, and it is the only form of it
/// a gate can hold without restating the renderer's arithmetic. A span taken from
/// *what this window shows* varies with where the window starts — a window opening
/// on a run boundary spends one separator and one inside a run spends two — so the
/// thumb grew and shrank under the pointer as the list moved. A screenful is a
/// property of the list, not of the position.
///
/// **Read off the painted buffer at every reachable top**, because computing the
/// length from the same numbers `render` uses would be the renderer agreeing with
/// itself. The non-vacuity check is what makes it bite: at least one top must be a
/// top where the two candidate spans genuinely differ, or the sweep compares a
/// number with itself.
#[test]
fn the_lists_thumb_keeps_its_length_as_the_window_moves() {
    let scratch = support::Scratch::large_diff("list-thumb-constant", 24, 6);
    let worktree = scratch.worktree();
    let staged: Vec<String> = (0..12).map(|i| format!("src/mod_{i}.rs")).collect();
    let mut args: Vec<&str> = vec!["add"];
    args.extend(staged.iter().map(String::as_str));
    scratch.git(&args);

    let mut frame = worktree.frame();
    frame.show_staged(true);
    frame.advance().expect("advance");

    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let at = Rect::new(0, 0, 80, 30);
    let chrome = chrome(&app);
    let body = body_layout(
        at,
        &chrome,
        frame.files().len(),
        vigia::list_rows_wanted(frame.files()),
    );
    app.view(&mut frame, &mut highlighter, &history, body)
        .expect("prime the row count");

    let mut lengths: Vec<(usize, usize)> = Vec::new();
    let mut differed = 0usize;
    for step in 0..12 {
        for _ in 0..2 {
            app.apply(Action::ScrollList(1), &mut frame, 40)
                .expect("apply");
        }
        let view = app
            .view(&mut frame, &mut highlighter, &history, body)
            .expect("collect");
        let told = regions(at, &chrome, &view);
        let Some(bar) = told.list.bar else { continue };

        let mut buf = ratatui::buffer::Buffer::empty(at);
        vigia::render(
            &mut buf,
            at,
            &view,
            &vigia::Theme::default(),
            Glyphs::default(),
            &chrome,
        );
        let thumb = (told.list.top..told.list.top + told.list.rows)
            .filter(|y| buf[(bar, *y)].symbol() == "█")
            .count();
        if thumb > 0 {
            lengths.push((view.list_top, thumb));
        }
        // The two candidate spans really are different numbers at this position,
        // or the sweep is comparing one quantity with itself.
        if view.listed_files() != view.list_span {
            differed += 1;
        }
        let _ = step;
    }

    assert!(
        lengths.len() > 2,
        "the sweep measured {} thumbs, which is too few to say the length is \
         constant",
        lengths.len()
    );
    assert!(
        differed > 0,
        "no window in the sweep told a screenful from what that window shows, so \
         this fixture cannot see the two being confused"
    );
    let first = lengths[0].1;
    for (top, thumb) in &lengths {
        assert_eq!(
            *thumb, first,
            "the thumb is {thumb} rows with the window at file {top} and {first} \
             rows elsewhere, so it changes length as the reader scrolls: \
             {lengths:?}"
        );
    }
}

/// **A screenful is the complement of the ceiling**, which is the definition the
/// bar, the drag and the clamp all read.
///
/// Pinned because three call sites depend on it and none of them can see the
/// others: a mutation putting the *row* count back was invisible to every
/// behavioural gate in the suite, because the two differ by the separator count
/// and a short track rounds that away.
#[test]
fn a_screenful_of_list_is_what_the_window_can_never_scroll_past() {
    let scratch = two_runs("list-span-definition");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.show_staged(true);
    frame.advance().expect("advance");

    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let mut separated = 0usize;
    for height in 12..=26u16 {
        let body = split_for(80, height, &frame);
        let view = app
            .view(&mut frame, &mut highlighter, &history, body)
            .expect("collect");
        if view.list.is_empty() {
            continue;
        }
        let ceiling = vigia::last_top(frame.files(), body.list);
        assert_eq!(
            view.list_span,
            view.files.saturating_sub(ceiling).max(1),
            "at 80x{height} the bar reports a screenful of {} where the window \
             can never start past {ceiling} of {}",
            view.list_span,
            view.files
        );
        if view.list_span != body.list {
            separated += 1;
        }
    }
    assert!(
        separated > 0,
        "no height told a screenful from the region's row count, so this gate \
         cannot see the two being confused"
    );
}

/// **Every frame leaves a screenful a scrollbar can be asked about**, including
/// the two that return before one is computed.
///
/// `take_list` returns early on a pane with no list region and `View::collect`
/// returns early on an empty worktree, so a span left at its initial value is what
/// those frames hand the bar. Zero reads as *this window shows none of the list*,
/// which is `scrollable` answering yes; nothing draws a bar on either frame today,
/// and only because `bar_for`'s own track-height guard catches it first. A field
/// that is safe because of a guard somewhere else is the shape this module keeps
/// finding, so the value is right at the source instead.
#[test]
fn a_frame_with_no_list_still_reports_a_screenful_that_scrolls_nothing() {
    let scratch = two_runs("list-span-early-returns");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.show_staged(true);
    frame.advance().expect("advance");

    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    // A pane too short for a list region at all.
    let body = split_for(80, 7, &frame);
    let view = app
        .view(&mut frame, &mut highlighter, &history, body)
        .expect("collect");
    assert!(
        view.list.is_empty(),
        "the fixture drew a list, so this is the wrong pane"
    );
    assert!(
        view.list_span >= view.files,
        "a pane with no list region reports a screenful of {} against {} files, \
         which is a bar claiming there is somewhere to scroll",
        view.list_span,
        view.files
    );

    // And an empty worktree, which returns even earlier.
    let clean = support::Scratch::new("list-span-clean");
    clean.write("a.txt", "one\n");
    clean.git(&["add", "-A"]);
    clean.git(&["commit", "-m", "init"]);
    let worktree = clean.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    assert!(frame.files().is_empty(), "the fixture is not clean");
    let body = split_for(80, 20, &frame);
    let view = App::new()
        .view(&mut frame, &mut highlighter, &history, body)
        .expect("collect");
    assert!(
        view.list_span >= 1,
        "an empty worktree reports a screenful of zero"
    );
}

/// **A drag on the list's track reaches the last file when both runs are drawn.**
///
/// `Action::ListTo` clamps through the same ceiling `browse` does, and until this
/// existed nothing said so on a grouped list: the drag gate in this file uses an
/// ungrouped fixture, so reverting the travel to `files - list_rows` stayed green.
/// `browse` cannot rescue it either, because it only ever takes the *smaller* of
/// the two bounds.
#[test]
fn dragging_a_grouped_list_to_the_bottom_reaches_the_last_file() {
    let scratch = two_runs("list-drag-grouped");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.show_staged(true);
    frame.advance().expect("advance");
    let last = frame.files().len() - 1;

    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let body = split_for(80, 16, &frame);
    app.view(&mut frame, &mut highlighter, &history, body)
        .expect("prime the row count");

    // The bottom of the track, which is what a reader dragging the thumb all the
    // way down produces.
    app.apply(Action::ListTo(vigia::TRACK_SCALE), &mut frame, 40)
        .expect("apply");
    let view = app
        .view(&mut frame, &mut highlighter, &history, body)
        .expect("collect");

    let drawn: Vec<usize> = vigia::list_plan(frame.files(), view.list_top, view.list.len())
        .iter()
        .filter_map(|slot| match slot {
            vigia::Slot::File(at) => Some(*at),
            vigia::Slot::Group { .. } => None,
        })
        .collect();
    assert!(
        drawn.contains(&last),
        "a drag to the bottom of the track drew {drawn:?} from top {}, so the \
         last file of the staged run cannot be reached with the pointer",
        view.list_top
    );
}

/// **A file is never drawn without the label of the run it belongs to.**
///
/// Reported from a real worktree: the window's last row held a *staged* file
/// while the only label above it said `unstaged`, so the list
/// stated something false about where that change lived. The cause was the
/// old rule that furniture gives way before content does, which dropped the
/// run label whenever the window had room for one more thing and spent it on
/// the file instead.
///
/// The reader overruled it, and the reason is that the two outcomes are not
/// comparable. A list one row shorter is merely smaller, and the rail already
/// says there is more; a file sitting under the wrong run's heading is the
/// list asserting the wrong thing about the reader's index. So the label now
/// comes first and the file follows only if a row is left.
///
/// Swept over every `(top, rows)` the fixture reaches, because the defect only
/// appears where a run boundary meets the bottom edge of the window.
#[test]
fn no_file_is_ever_planned_without_its_own_runs_label() {
    let scratch = two_runs("list-labelled");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.show_staged(true);
    frame.advance().expect("advance");
    let files = frame.files();

    let mut saw_a_boundary = false;
    for top in 0..files.len() {
        for rows in 1..=files.len() + 3 {
            let plan = vigia::list_plan(files, top, rows);
            let mut labelled: Option<vigia_core::Origin> = None;
            for slot in &plan {
                match slot {
                    vigia::Slot::Group { origin, .. } => labelled = Some(*origin),
                    vigia::Slot::File(at) => {
                        let origin = files[*at].origin;
                        saw_a_boundary |= labelled.is_some();
                        assert_eq!(
                            labelled,
                            Some(origin),
                            "at top {top} with {rows} rows, file {at} is {origin:?} \
                             but the label above it says {labelled:?}, so the list \
                             files the change under the wrong run:\n{plan:?}"
                        );
                    }
                }
            }
        }
    }
    assert!(saw_a_boundary, "the sweep never drew a label at all");
}

/// **A list that is entirely staged still says so, and its rows still read as
/// staged.**
///
/// Reported from a real worktree: with both changed files staged and nothing
/// unstaged, the list drew no `staged` heading at all and the kind
/// letters took the ordinary ink rather than the staged one. Unstaging one file
/// made both headings and both inks appear. So the view that most needs to
/// announce itself, the one where *everything* is staged, was the one that said
/// nothing, and it was indistinguishable from the default unstaged view.
///
/// One decision caused both halves. `Runs::grouped` asked whether **both** runs
/// held files, and `Heading::origin` is `grouped.then_some(entry.origin)`, so a
/// single-run view dropped its run's identity entirely: no label to draw, and
/// `None` for the ink to match on, which falls to the unstaged case.
///
/// Dropping it is right for a list that is entirely unstaged, because unstaged
/// is what a reader is looking at unless told otherwise. It is wrong for a list
/// that is entirely staged, which is the one case where the absence of a label
/// states the opposite of the truth. So the question is now whether a staged run
/// exists at all.
#[test]
fn an_entirely_staged_list_announces_itself() {
    let scratch = support::Scratch::new("list-all-staged");
    for i in 0..3 {
        scratch.write(&format!("src/f{i}.rs"), "one\ntwo\nthree\n");
    }
    scratch.git(&["add", "-A"]);
    scratch.git(&["commit", "-m", "init"]);
    for i in 0..3 {
        scratch.write(&format!("src/f{i}.rs"), "one\nSTAGED\nthree\n");
    }
    scratch.git(&["add", "-A"]);

    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.show_staged(true);
    frame.advance().expect("advance");
    let files = frame.files();
    assert_eq!(files.len(), 3, "the fixture is not three staged files");

    let plan = vigia::list_plan(files, 0, 8);
    assert!(
        plan.iter().any(|slot| matches!(
            slot,
            vigia::Slot::Group { origin: staged_origin, .. } if *staged_origin == vigia_core::Origin::Staged
        )),
        "an entirely staged list drew no staged heading, so it reads as the \
         default unstaged view:\n{plan:?}"
    );
    // And the region must be sized for the heading it now draws, or the label
    // is announced and the run's tail falls off the bottom (#313).
    assert_eq!(
        vigia::list_rows_wanted(files),
        files.len() + 1,
        "the list asks for a row count that does not include its one heading"
    );
}

/// The `●` survives the ink that drew it, all the way to the screen.
///
/// **The wiring gate for [#345](https://github.com/breferrari/vigia/issues/345)**,
/// and the third one that row needed. `vigia-core` proves `History::newest`
/// outlives `History::recency`, and `tests/render.rs` proves the painter reads
/// the right field, and **neither of them proves the two are joined**: a build
/// that filled `FileEntry::newest` from the recency again would satisfy both and
/// fail a reader. Mutation found exactly that, which is the shape this project's
/// record calls a gate modelling the function rather than the program.
///
/// So this one drives the real path: a real worktree, a real `History` rolled
/// forward the way the shell rolls it, `App::view` building the entry, and the
/// drawn pane read back.
#[test]
fn the_mark_reaches_the_drawn_pane_after_the_ink_has_drained() {
    let scratch = Scratch::large_diff("list-newest-mark", 3, 6);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();
    let mut highlighter = Highlighter::eager();

    // A write, then ten seconds of quiet rolled in the steps `Shell::draw` uses.
    // The number is the ruling's rather than `PULSE_SAMPLES`', for the reason the
    // core's own gate gives: a gate written in terms of the constant it pins moves
    // with it.
    let path = frame.files()[0].path.clone();
    let start = std::time::Instant::now();
    let mut history = History::starting_at(start);
    let wrote = start + std::time::Duration::from_millis(1);
    history.record([path.as_str()], wrote);
    let mut now = wrote;
    for _ in 0..2_000u32 {
        now += std::time::Duration::from_millis(5);
        history.record_sized([], now);
    }

    let area = Rect::new(0, 0, 100, 20);
    let chrome = chrome(&app);
    let body = body_layout(area, &chrome, frame.files().len(), frame.files().len());
    let view = app
        .view(&mut frame, &mut highlighter, &history, body)
        .expect("view");
    let mut buf = Buffer::empty(area);
    render(
        &mut buf,
        area,
        &view,
        &Theme::default(),
        Glyphs::default(),
        &chrome,
    );
    let rows: Vec<String> = (0..area.height)
        .map(|y| {
            (0..area.width)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
        })
        .collect();
    let drawn = rows.join("\n");

    // **On the written file's own row, not anywhere on the pane.** The first
    // spelling asked whether a `●` reached the screen at all, which a mark on the
    // wrong row satisfies just as well: the fixture has three files and only one
    // of them was written.
    let name = path.rsplit('/').next().expect("a file name");
    let row = rows
        .iter()
        .find(|row| row.contains(name))
        .unwrap_or_else(|| panic!("the written file is not drawn at all:\n{drawn}"));
    assert!(
        row.contains('●'),
        "ten seconds after the only write, its own row carries no mark: {row:?}"
    );
    // **Every marked row names the written file**, which is the claim rather than
    // "exactly one row": `SPEC.md` §11.1 draws a file through one `Painter::file_row`
    // in both regions, so the file the diff is inside is marked in the map and on
    // its own heading. Counting rows would have made that design a failure.
    for marked in rows.iter().filter(|row| row.contains('●')) {
        assert!(
            marked.contains(name),
            "a row that is not the written file carries the mark: {marked:?}"
        );
    }
}
