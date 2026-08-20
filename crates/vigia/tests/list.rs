//! The pinned file list, which `SPEC.md` §11.1 makes the middle of three
//! regions.
//!
//! > The body is three regions since 2026-08-17: a masthead, a pinned file list,
//! > a rule, and the scrolling diff.
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

use ratatui::layout::Rect;
use vigia::{Action, App, Body, Glyphs, LIST_SETTLED, Position, View, Viewport, body_layout};
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

/// The tall pane [#125](https://github.com/breferrari/vigia/issues/125) named
/// when it filed this rung: *"a 50-row pane keeping six draws void where the map
/// could be"*. Taken from the issue rather than derived from the share.
const DEEP: u16 = 50;

fn chrome(app: &App) -> vigia::Chrome {
    app.chrome("fixture", None, None, None, None, None)
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
    // More files than any cap this sweep can reach, so the changed-file clamp is
    // never the one doing the work and what is measured is the pane's own answer.
    const MANY: usize = 500;

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
    const MANY: usize = 500;

    // The masthead is off by default since #204, and the band only exists with it
    // on, so this gate has to ask for the screen it is about.
    let raised = vigia::Chrome {
        masthead: true,
        ..chrome(&App::new())
    };

    let mut had_a_band = false;
    let mut saw_it_arrive = false;

    for height in 1..=TALLEST {
        let body = body_layout(Rect::new(0, 0, WIDE, height), &raised, MANY);
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
    let mut highlighter = Highlighter::new();
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
            &app.chrome("vigia", None, None, None, None, None),
            FILES,
        );
        let view = app
            .view(&mut frame, &mut highlighter, &history, body)
            .expect("view");

        let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("terminal");
        let theme = Theme::default();
        let chrome = app.chrome("vigia", None, None, None, None, None);
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
    let mut highlighter = Highlighter::new();
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

    for height in 1..=40u16 {
        for width in [40u16, WIDE, 120] {
            for files in [0usize, 1, 3, LIST_SETTLED, LIST_SETTLED + 1, 200] {
                let area = Rect::new(0, 0, width, height);
                let chrome = chrome(&App::new());
                let full = body_layout(area, &chrome, files);

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
                    assert_eq!(
                        body.rule,
                        body.list > 0,
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
    let mut highlighter = Highlighter::new();
    let history = History::new();

    for list_top in [0usize, 1, FILES - 1, FILES, FILES + 9, usize::MAX] {
        for list_rows in [0usize, 1, LIST_SETTLED, FILES, FILES + 5, 10_000] {
            for diff_rows in [0usize, 1, 22] {
                for file in [0usize, FILES - 1, FILES, FILES + 3] {
                    for list_follows in [true, false] {
                        let view = View::collect(
                            &mut frame,
                            &mut highlighter,
                            &history,
                            Viewport {
                                position: Position { file, row: 0 },
                                anchored: false,
                                diff_rows,
                                list_top,
                                list_rows,
                                list_follows,
                                measured: true,
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
    let mut highlighter = Highlighter::new();
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
    let mut highlighter = Highlighter::new();
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
    let mut highlighter = Highlighter::new();
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
        let mut highlighter = Highlighter::new();
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
    let mut highlighter = Highlighter::new();
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
    let mut highlighter = Highlighter::new();
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
    let mut highlighter = Highlighter::new();
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
