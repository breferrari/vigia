//! The pinned file list, which `SPEC.md` §11.1 makes the middle of three
//! regions.

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
const MIN_BODY: usize = 2;

/// Eighty columns, where the footer is one line whatever the state, so nothing
/// below is entangled with I6's two-line footer.
const WIDE: u16 = 80;

/// The top of every height sweep here.
const TALLEST: u16 = 120;

/// The pane `SPEC.md` §11.1 sized the list against, and the anchor of the ladder.
const REFERENCE: u16 = 24;

/// More changed files than any cap the sweeps below can reach.
const MANY: usize = 500;

/// Taken from the issue rather than derived from the share.
const DEEP: u16 = 50;

fn chrome(app: &App) -> vigia::Chrome {
    app.chrome("fixture", None, Pointing::default(), 0, "")
}

/// The same, with the rail asked for.
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

/// [`split`], sized the way the shell sizes a region for this changed set.
fn split_for(width: u16, height: u16, frame: &vigia_core::Frame) -> Body {
    body_layout(
        Rect::new(0, 0, width, height),
        &chrome(&App::new()),
        frame.files().len(),
        vigia::list_rows_wanted(frame.files()),
    )
}

/// Each region reports its own bar's column, not the pane's.
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
#[test]
fn a_taller_pane_never_costs_the_band_its_rows() {
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
    // §11.1's no-jog rule, one region up from the footer it was written for.
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
    // `View::current` is resolved after the walk, and this is why.
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
    // The half of the ruling that is about state rather than about keys.
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

    // The caret is gone from the region, because the diff is no longer inside any file
    // the window is showing.
    assert!(
        after.top.file < after.list_top || after.top.file >= after.list_top + after.list.len(),
        "the fixture did not browse past the current file, so this proves nothing"
    );
}

#[test]
fn the_window_is_overtaken_when_the_diff_leaves_it() {
    // The other side of the rule above.
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
/// Ignored, diagnostic, not a gate, the way `vigia-core`'s
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
    // The asymmetry `View::collect` already argues against, one region up.
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
    // `Body::clamped_to` holds the layout's only subtraction and had no direct test:
    // mutating its give-back term left the suite green, because no fixture reached a
    // stale view.
    let mut checked = 0;
    let mut saw_a_clamp = false;
    let mut saw_rail = false;

    for height in 1..=40u16 {
        // Two widths past the rail's arrival, so the one subtraction this gate exists
        // for is exercised in both shapes.
        for width in [40u16, WIDE, 120, 140, 200] {
            for files in [0usize, 1, 3, LIST_SETTLED, LIST_SETTLED + 1, 200] {
                let area = Rect::new(0, 0, width, height);
                // Railed, so the two widths past 134 reach `clamped_to`'s rail
                // arm rather than sweeping the stacked shape five times. Since
                // The default chrome never draws one.
                let chrome = railed(&App::new());
                let full = body_layout(area, &chrome, files, files);
                saw_rail |= full.rail;

                for have in 0..=LIST_SETTLED + 2 {
                    let body = full.clamped_to(have);
                    if body.list != full.list {
                        saw_a_clamp = true;
                    }

                    // The footer's own height is not exposed, so it is recovered from
                    // the unclamped split rather than restated: whatever it is,
                    // clamping must not change it.
                    let footer = usize::from(height).saturating_sub(1 + full.rows());
                    assert_eq!(
                        1 + body.rows() + footer,
                        usize::from(height),
                        "at {width}x{height} over {files} files with {have} \
                         entries, {body:?} plus a header and {footer} footer rows \
                         does not tile the pane"
                    );
                    // Beside a rail there is no rule at all, which is §11.2 B11
                    // dissolved rather than reopened: the list is beside the diff and
                    // there is no boundary for a horizontal rule to be drawn on.
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
    // `View::take_list` indexes the frame through `Frame::diff`, which panics by design
    // on an index past the end of the file list.
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
                                // A sweep dimension rather than a constant, because a
                                // landing resolves inside the same walk and every
                                // degenerate shape here is one it can be asked for in.
                                landing,
                                // Not a sweep dimension, unlike `landing` above.
                                single: false,
                                // This sweep is about where the two regions land, which
                                // is decided before anything is coloured.
                                highlight: true,
                            },
                        )
                        .expect("collect");

                        assert!(
                            view.list.len() <= list_rows,
                            "asked for {list_rows} list rows and got {}",
                            view.list.len()
                        );
                        // Only while there is a window.
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
    // `Action::ScrollList(-1)` end to end.
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

    // Fewer rows than digits.
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
// The two runs, and what a drawn row addresses: `SPEC.md` §11.2 B17.
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

/// A separator opens each run, and a window scrolled into the middle of one
/// still opens with that run's own label.
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
    // that run's label, and the count is the run's total rather than what is
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

/// One run draws no separators at all, which is what keeps the default pane
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

/// A separator with no room for a file under it is not drawn.
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

/// A digit and a click address a *file*, and never the separator above it.
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

/// Pressing `a` shows the staged run on that frame, not on the next write.
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

/// Every file in the list is reachable when both runs are drawn.
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

        // And the old arithmetic really does fall short here, or this fixture cannot
        // see the defect it exists for.
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

/// The height a scroll step is measured in is the height the paint lays out.
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

        // And the two inputs are not interchangeable, which is the finding.
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

/// `last_top` is the *tightest* ceiling, not merely a top that works.
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

/// Follow keeps the caret's own file inside the window it computes.
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

/// `J` reaches the end of a grouped list.
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

/// A one-row grouped window draws the run's label, not an unlabelled file.
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

/// The plan's files run contiguously from `top`, which is what lets the painter
/// count rather than ask.
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

/// The list's scrollbar and its drag agree about how far the window can go.
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

/// The thumb keeps its length as the window scrolls.
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

/// A screenful is the complement of the ceiling, which is the definition the
/// bar, the drag and the clamp all read.
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

/// Every frame leaves a screenful a scrollbar can be asked about, including
/// the two that return before one is computed.
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

/// A drag on the list's track reaches the last file when both runs are drawn.
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

/// A file is never drawn without the label of the run it belongs to.
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

/// A list that is entirely staged still says so, and its rows still read as
/// staged.
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
    assert_eq!(
        vigia::list_rows_wanted(files),
        files.len() + 1,
        "the list asks for a row count that does not include its one heading"
    );
}

/// The `●` survives the ink that drew it, all the way to the screen.
#[test]
fn the_mark_reaches_the_drawn_pane_after_the_ink_has_drained() {
    let scratch = Scratch::large_diff("list-newest-mark", 3, 6);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();
    let mut highlighter = Highlighter::eager();

    // A write, then ten seconds of quiet rolled in the steps `Shell::draw` uses.
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

    // On the written file's own row, not anywhere on the pane.
    let name = path.rsplit('/').next().expect("a file name");
    let row = rows
        .iter()
        .find(|row| row.contains(name))
        .unwrap_or_else(|| panic!("the written file is not drawn at all:\n{drawn}"));
    assert!(
        row.contains('●'),
        "ten seconds after the only write, its own row carries no mark: {row:?}"
    );
    // Every marked row names the written file, which is the claim rather than "exactly
    // one row": `SPEC.md` §11.1 draws a file through one `Painter::file_row` in both
    // regions, so the file the diff is inside is marked in the map and on its own
    // heading.
    for marked in rows.iter().filter(|row| row.contains('●')) {
        assert!(
            marked.contains(name),
            "a row that is not the written file carries the mark: {marked:?}"
        );
    }
}
