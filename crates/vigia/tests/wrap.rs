//! `SPEC.md` §11.2 **B19**: a long line continues on the row below, on `w`.
//!
//! Every gate here is one line of [#272](https://github.com/breferrari/vigia/issues/272)'s
//! own exit criteria, and the list is worth restating because it is what the
//! ruling promised rather than what the code happens to do: `w` toggles wrapping
//! and nothing else, off on launch; a line longer than the text width is readable
//! to its end; no row over-occupies its region at any width; every jump still
//! lands where it landed; and the diff's own rows are exactly the rows they were,
//! so the thumb does not move when the key is pressed.
//!
//! **The fixture is one file with lines of known width**, which is the whole of
//! what separates this from `legibility.rs`. There the sweep is over widths with
//! a hand-built view; here the width is fixed and the *content* is chosen so that
//! "fits", "wraps once" and "wraps past the cap" are three distinguishable
//! things. A fixture whose lines were all far past the cap would satisfy every
//! gate below while never once drawing a tail a reader could finish.
//!
//! **And it is driven through `App::view` rather than by building a `View`**,
//! because the width a row is laid out against reaches the walk through
//! `Body::diff_width`, and a hand-built `Viewport` would be the gate choosing the
//! number the code is supposed to derive.

#[path = "../../vigia-core/tests/support/mod.rs"]
mod support;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use vigia::{
    Action, App, Body, Glyphs, Pointing, Row, Theme, View, body_layout, diff_height, regions,
    render,
};
use vigia_core::{Frame, Highlighter, History};

use support::{Scratch, materialise};

/// The pane every gate here draws on.
///
/// Eighty by twenty-four: the pane a reader is likeliest to be on, and the one
/// §11.1 measures its own clipping figures against.
const PANE: Rect = Rect::new(0, 0, 80, 24);

/// A token nothing else in the fixture spells, at the **end** of the long line.
///
/// The oracle for *readable to its end*. Asserting the line's own prefix proves
/// nothing: the prefix is what a clipped row already draws.
const TAIL: &str = "ZOMBIE";

/// A token at the end of a line that is short enough to fit.
const SHORT_TAIL: &str = "FITS";

/// Columns of leading blank on the deeply indented line, past twice what half of
/// an eighty-column pane's content leaves.
const DEEP: usize = 60;

/// Build the fixture: one committed file, then a rewrite whose lines are chosen
/// widths.
///
/// `long` is the line the gates wrap. It is a little over one text width and well
/// under two, so its tail fits the continuation exactly and `›` on the lower row
/// would be a defect rather than the cap.
fn fixture(name: &str) -> Scratch {
    let scratch = Scratch::new(name);
    scratch.write("src/lines.rs", "one\ntwo\nthree\nfour\nfive\n");
    scratch.commit_all("base");

    let long = format!("let value = \"{}\"; // {TAIL}", "x".repeat(70));
    let short = format!("let small = \"aa\"; // {SHORT_TAIL}");
    // Far past the cap: two rows cannot hold it, so the lower row clips and says
    // so. This is what makes the cap a *bound* here rather than a description.
    let huge = format!("let huge = \"{}\"; // {TAIL}", "y".repeat(400));
    let deep = format!(
        "{}let deep = \"{}\"; // {TAIL}",
        " ".repeat(DEEP),
        "z".repeat(70)
    );
    scratch.write(
        "src/lines.rs",
        format!("{long}\n{short}\n{huge}\n{deep}\nfive\n"),
    );
    scratch
}

fn chrome_of(app: &App) -> vigia::Chrome {
    app.chrome("fixture", None, Pointing::default(), 0, "")
}

fn split(app: &App) -> Body {
    body_layout(PANE, &chrome_of(app), 1, 1)
}

/// Every row of the diff region, as strings, with trailing blanks trimmed.
fn drawn(
    app: &mut App,
    frame: &mut Frame,
    highlighter: &mut Highlighter,
    history: &History,
) -> (View, Vec<String>) {
    let chrome = chrome_of(app);
    let laid = body_layout(PANE, &chrome, 1, 1);
    let view = app
        .view(frame, highlighter, history, laid)
        .expect("a view of the fixture");
    let mut buf = Buffer::empty(PANE);
    render(
        &mut buf,
        PANE,
        &view,
        &Theme::default(),
        Glyphs::default(),
        &chrome,
    );
    let laid_regions = regions(PANE, &chrome, &view);
    let rows = (laid_regions.diff.top..laid_regions.diff.top + laid_regions.diff.rows)
        .map(|row| {
            let mut text = String::new();
            for col in 0..PANE.width {
                text.push_str(buf[(col, row)].symbol());
            }
            text.trim_end().to_owned()
        })
        .collect();
    (view, rows)
}

/// A shell with wrapping on, which is one press of `w`.
fn wrapped(frame: &mut Frame) -> App {
    let mut app = App::new();
    let body = diff_height(PANE, &chrome_of(&app), 1, 1);
    app.apply(Action::ToggleWrap, frame, body).expect("apply");
    app
}

fn open(name: &str) -> (Scratch, Highlighter, History) {
    (fixture(name), Highlighter::eager(), History::new())
}

#[test]
fn the_fixture_is_the_shape_the_rest_of_this_file_assumes() {
    // **Three widths of line, and the gates below cannot tell them apart without
    // it.** A fixture whose lines were all past the cap would let *every* gate
    // here pass against a build that drew a mark and no tail, because there would
    // be no line whose tail a reader could reach. The short line is the other
    // half: it is what says the continuation is a property of the line rather
    // than of the mode.
    let (scratch, mut highlighter, history) = open("shell-wrap-shape");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = wrapped(&mut frame);
    let (view, _) = drawn(&mut app, &mut frame, &mut highlighter, &history);

    let wraps = view
        .rows
        .iter()
        .filter(|row| matches!(row, Row::Wrap { .. }))
        .count();
    let lines = view
        .rows
        .iter()
        .filter(|row| matches!(row, Row::Line { .. }))
        .count();
    assert!(
        wraps > 0 && wraps < lines,
        "the fixture wraps {wraps} of {lines} content rows, so it cannot tell a \
         line that continues from one that does not"
    );
    assert!(
        view.gutter.is_some(),
        "the walk took no gutter decision, so nothing below is measuring the \
         width the rows were wrapped against"
    );
}

#[test]
fn a_long_line_is_readable_to_its_end_only_when_wrapping_is_on() {
    // **The whole of the issue, as one comparison.** Off, the tail of the long
    // line is nowhere on the pane and there is no route to it: no wrap, no pan,
    // and the takeover holds the mouse. On, it is drawn.
    //
    // Both halves are asserted because either alone is satisfied by a build that
    // ignores the mode: a gate that only checked "on" would pass against a shell
    // that wrapped unconditionally, which is the default #204's reasoning refuses.
    let (scratch, mut highlighter, history) = open("shell-wrap-readable");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);

    let mut plain = App::new();
    let (_, clipped) = drawn(&mut plain, &mut frame, &mut highlighter, &history);
    assert!(
        clipped.iter().any(|row| row.contains("let value")),
        "the long line is not drawn at all, so its missing tail says nothing:\n{}",
        clipped.join("\n")
    );
    assert!(
        !clipped.iter().any(|row| row.contains(TAIL)),
        "the pane reached the tail of a long line with wrapping off:\n{}",
        clipped.join("\n")
    );

    let mut app = wrapped(&mut frame);
    let (_, rows) = drawn(&mut app, &mut frame, &mut highlighter, &history);
    assert!(
        rows.iter().any(|row| row.contains(TAIL)),
        "a long line is still not readable to its end with wrapping on:\n{}",
        rows.join("\n")
    );
}

#[test]
fn a_head_that_continues_is_not_marked_and_the_mark_moves_to_the_tail() {
    // **`›` says *rightward*, and there is nothing to the right of a row whose
    // content is on the row below.** So the head loses the mark and the lower row
    // keeps it, but only where the cap actually cuts something: the huge line is
    // past two rows and the long one is not.
    //
    // The two lines are checked together rather than separately, because a build
    // that never marked anything satisfies the first assertion and a build that
    // marked every head satisfies the second.
    let (scratch, mut highlighter, history) = open("shell-wrap-marks");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = wrapped(&mut frame);
    let (_, rows) = drawn(&mut app, &mut frame, &mut highlighter, &history);

    let head = rows
        .iter()
        .position(|row| row.contains("let value"))
        .expect("the long line's head row");
    assert!(
        !rows[head].ends_with('›'),
        "a head row whose tail is on the row below is marked as continuing \
         rightward:\n{}",
        rows[head]
    );
    assert!(
        rows[head + 1].contains(TAIL) && !rows[head + 1].ends_with('›'),
        "the long line's tail fits the continuation and was marked anyway:\n{}",
        rows[head + 1]
    );

    let huge = rows
        .iter()
        .position(|row| row.contains("let huge"))
        .expect("the huge line's head row");
    assert!(
        rows[huge + 1].ends_with('›'),
        "a line past the cap was cut on its lower row without saying so:\n{}",
        rows[huge + 1]
    );
}

#[test]
fn a_continuation_blanks_its_gutter_and_marks_its_sigil() {
    // **The two cells that say *this is not a new line*.** A real line always has
    // a number, so the blank is the cheap half; the `↳` is what survives the
    // gutter being dropped entirely, which is what happens at the narrow widths
    // this gesture is for.
    //
    // Read against the row above it rather than against a column literal, because
    // a literal would restate the layout arithmetic instead of checking that the
    // two rows share it.
    let (scratch, mut highlighter, history) = open("shell-wrap-gutter");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = wrapped(&mut frame);
    let (view, rows) = drawn(&mut app, &mut frame, &mut highlighter, &history);

    let gutter = view.gutter.expect("a gutter decision");
    assert!(
        gutter > 0,
        "this pane draws no line numbers, so the blank below says nothing"
    );

    let head = rows
        .iter()
        .position(|row| row.contains("let value"))
        .expect("the long line's head row");
    let above: Vec<char> = rows[head].chars().collect();
    let below: Vec<char> = rows[head + 1].chars().collect();

    // The head's number, then the continuation's blank where it was.
    let numbered: String = above.iter().take(gutter + 3).collect();
    assert!(
        numbered.chars().any(|c| c.is_ascii_digit()),
        "the head row carries no line number, so this gate is not reading the \
         gutter: {numbered:?}"
    );
    let blanked: String = below.iter().take(gutter + 1).collect();
    assert!(
        blanked.chars().all(|c| c == ' '),
        "a continuation drew something in the gutter: {blanked:?}"
    );
    assert!(
        rows[head + 1].contains('↳'),
        "a continuation does not mark its sigil column:\n{}",
        rows[head + 1]
    );
}

#[test]
fn a_wash_covers_both_display_rows_of_a_wrapped_change() {
    // **A band that stopped at the first row would make a wrapped removal read as
    // ending early**, which is the same sentence §5.3 already carries about a
    // band that stops where its text stops. Read as backgrounds rather than as
    // text, because that is the only thing that can see it.
    let (scratch, mut highlighter, history) = open("shell-wrap-wash");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = wrapped(&mut frame);

    let chrome = chrome_of(&app);
    let laid = body_layout(PANE, &chrome, 1, 1);
    let view = app
        .view(&mut frame, &mut highlighter, &history, laid)
        .expect("view");
    // **A palette that actually washes a row, which `Theme::default()` at this
    // depth does not.** Probed rather than assumed: the default resolves to a
    // pane whose changed rows carry only the left bar, so
    // `Painter::line_row`'s `set_style(area, wash)` is a no-op on it and a
    // continuation that lost its band read identically. Mutation found that too,
    // after the non-vacuity guard below had already been strengthened once.
    let theme = Theme::dark().resolve(vigia::Depth::Truecolor);
    let mut buf = Buffer::empty(PANE);
    render(&mut buf, PANE, &view, &theme, Glyphs::default(), &chrome);
    let laid_regions = regions(PANE, &chrome, &view);

    // The first `Row::Wrap` the screen draws, by its index among the region's
    // rows, which is what makes this a claim about the drawn row rather than
    // about the model.
    let at = view
        .rows
        .iter()
        .position(|row| matches!(row, Row::Wrap { .. }))
        .expect("a wrapped row on this pane");
    assert!(
        at > 0,
        "a continuation is the region's first row, so it has no head"
    );
    let head = laid_regions.diff.top + at as u16 - 1;
    let tail = head + 1;

    let backgrounds = |row: u16| -> Vec<ratatui::style::Color> {
        (0..PANE.width).map(|col| buf[(col, row)].bg).collect()
    };
    let above = backgrounds(head);
    let below = backgrounds(tail);

    // **The non-vacuity guard is a *comparison*, not a presence.** The first
    // spelling asked whether the head row carried any background other than
    // `Reset`, and the pane's own background satisfies that at every column, so a
    // continuation that lost its band read identically and the gate stayed green
    // against exactly the defect it was written for. Mutation found it: gating
    // `set_style(area, wash)` on `number.is_some()` survived.
    //
    // A context row is what a row with no wash looks like on this pane, so the
    // claim is that a changed row does not look like one.
    let plain = view
        .rows
        .iter()
        .position(|row| {
            matches!(
                row,
                Row::Line {
                    kind: vigia_core::LineKind::Context,
                    ..
                }
            )
        })
        .expect("a context row on this pane");
    let unwashed = backgrounds(laid_regions.diff.top + plain as u16);
    assert_ne!(
        above, unwashed,
        "the head row is drawn like an unchanged one, so this palette washes \
         nothing and the comparison below is vacuous"
    );
    assert_eq!(
        above, below,
        "a wrapped change's band stops at its first row, so the row below reads \
         as a different line"
    );
}

#[test]
fn no_line_ever_occupies_more_than_the_cap() {
    // **`delta --wrap-max-lines` defaults to 2 and this takes that number.** The
    // ceiling is written from the ruling's own words, *a line displaces at most
    // one row*, rather than from the constant that implements it: an assertion
    // that read the cap out of the code would move with a mutation of it and stay
    // green against the regression it exists to catch.
    //
    // Swept over widths, because the pathological case is a narrow pane and a
    // long line, which is exactly where an unbounded wrap would eat the region.
    // **The fixture is taller than the pane, and it has to be.** The shared one
    // is nine rows against a region of twenty-odd, so the expansion never
    // approaches the region's last row and the over-occupation this gate exists
    // to catch cannot happen on it. Mutation found that: relaxing the room test
    // from `+ 2 <= height` to `+ 1 <= height` survived every gate in this file.
    let scratch = Scratch::new("shell-wrap-cap");
    scratch.write("src/lines.rs", "seed\n");
    scratch.commit_all("base");
    let mut body = String::new();
    for n in 0..60 {
        body.push_str(&format!(
            "let line_{n} = \"{}\"; // {TAIL}\n",
            "q".repeat(70)
        ));
    }
    scratch.write("src/lines.rs", body);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    for width in [30u16, 40, 60, 80, 120, 200] {
        let at = Rect::new(0, 0, width, 24);
        let mut app = App::new();
        let body = diff_height(at, &chrome_of(&app), 1, 1);
        app.apply(Action::ToggleWrap, &mut frame, body)
            .expect("apply");
        let chrome = chrome_of(&app);
        let laid = body_layout(at, &chrome, 1, 1);
        let view = app
            .view(&mut frame, &mut highlighter, &history, laid)
            .expect("view");

        let mut run = 0usize;
        for row in &view.rows {
            match row {
                Row::Wrap { .. } => {
                    run += 1;
                    assert!(
                        run <= 1,
                        "a line at {width} columns displaces {run} rows, where the \
                         ruling allows one"
                    );
                }
                _ => run = 0,
            }
        }
        assert!(
            view.rows.len() <= laid.diff,
            "a {width} column pane drew {} rows into a region of {}",
            view.rows.len(),
            laid.diff
        );
    }
}

#[test]
fn a_wrapped_continuation_keeps_the_text_its_floor() {
    // **Breakindent has a cap and the cap is the tail's floor.** At eighty columns
    // a sixty-space indent would leave a continuation with a handful of columns,
    // which is not a route to the end of anything. Neovim's `'breakindent'` with
    // the half-width cap `indent_of` states.
    let (scratch, mut highlighter, history) = open("shell-wrap-indent");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = wrapped(&mut frame);
    let (view, rows) = drawn(&mut app, &mut frame, &mut highlighter, &history);

    let deep = rows
        .iter()
        .position(|row| row.contains("let deep"))
        .expect("the deeply indented line");
    let indent = match &view.rows[deep] {
        Row::Line { .. } => match &view.rows[deep + 1] {
            Row::Wrap { indent, .. } => *indent,
            other => panic!("the deep line did not wrap: {other:?}"),
        },
        other => panic!("row {deep} is not a content line: {other:?}"),
    };
    assert!(
        indent > 0,
        "the continuation of an indented line is flush left, so the block shape \
         #164 ruled uniform is broken"
    );
    assert!(
        indent < DEEP,
        "the continuation took the whole {DEEP} column indent, so a deeply \
         indented line buys a second row with nothing on it"
    );
}

#[test]
fn wrapping_off_is_the_pane_a_caller_that_named_no_width_draws() {
    // **The mode is inert when it is off**, which is the claim the whole shipped
    // default rests on. Two collections of the same frame: one at the pane's real
    // width with wrapping off, one at no width at all, which is what every caller
    // before this change passed. The rows have to be the same rows.
    //
    // It is not a tautology: `wrap_rows` runs on both, takes the gutter on one and
    // not on the other, and a build that split rows regardless of the flag would
    // differ here on the first long line.
    let (scratch, mut highlighter, history) = open("shell-wrap-inert");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();
    let chrome = chrome_of(&app);
    let laid = body_layout(PANE, &chrome, 1, 1);

    // **A warm-up frame first, and it is not ceremony.** The first frame of a
    // process is drawn plain (`Viewport::highlight`), so comparing frame one
    // against frame two would report a difference in `spans` and say nothing at
    // all about wrapping.
    app.view(&mut frame, &mut highlighter, &history, laid)
        .expect("view");
    let sized = app
        .view(&mut frame, &mut highlighter, &history, laid)
        .expect("view");
    let bare = Body {
        diff_width: 0,
        ..laid
    };
    let unsized_view = app
        .view(&mut frame, &mut highlighter, &history, bare)
        .expect("view");

    assert!(
        sized.rows.iter().any(|row| matches!(row, Row::Line { .. })),
        "the fixture drew no content rows, so this compares two empty screens"
    );
    assert_eq!(
        sized.rows, unsized_view.rows,
        "wrapping is off and the rows still differ by the width they were \
         collected at"
    );
}

#[test]
fn the_bottom_of_the_diff_is_reachable_when_lines_wrap() {
    // **The units bug the split makes visible.** `View::last_screenful` and
    // `collect`'s overshoot branch both rest the diff's last **logical** row on
    // the pane's last **display** row, which is exact while the two are one
    // number. With wrapping on the clamp lands too far back, and dropping the
    // excess off the bottom the way every other frame does puts the end of the
    // diff out of reach of `G`, which is the gesture that exists to reach it.
    //
    // The fixture is deliberately taller than the pane: on a short diff there is
    // no clamp to get wrong and this passes against the defect.
    let scratch = Scratch::new("shell-wrap-bottom");
    scratch.write("src/lines.rs", "seed\n");
    scratch.commit_all("base");
    let mut body = String::new();
    for n in 0..40 {
        body.push_str(&format!("let line_{n} = \"{}\";\n", "q".repeat(70)));
    }
    body.push_str(&format!("let last = \"tail\"; // {TAIL}\n"));
    scratch.write("src/lines.rs", body);

    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let mut app = wrapped(&mut frame);

    // **Scrolled rather than jumped, because `G` unpinned is a jump to the last
    // *file*** and this fixture has one. The bottom clamp is what a reader
    // reaches by scrolling off the end, which is `View::last_screenful` and the
    // overshoot branch: the two places that compare a logical span against a
    // display height.
    let height = diff_height(PANE, &chrome_of(&app), 1, 1);
    app.apply(Action::Scroll(10_000), &mut frame, height)
        .expect("apply");
    let (view, rows) = drawn(&mut app, &mut frame, &mut highlighter, &history);

    assert!(
        view.total_rows > height,
        "the fixture fits the pane, so there is no bottom clamp to get wrong"
    );
    assert!(
        rows.iter().any(|row| row.contains(TAIL)),
        "`G` with wrapping on cannot reach the last line of the diff:\n{}",
        rows.join("\n")
    );
    assert!(
        rows.last().is_some_and(|row| !row.is_empty()),
        "the diff's last screenful ends on a blank row, which §11.1 rules it \
         does not:\n{}",
        rows.join("\n")
    );
}

#[test]
fn w_toggles_wrapping_and_leaves_the_thumb_where_it_was() {
    // **B19's own claim about the scrollbar.** The bar counts the diff's rows and
    // not the terminal's, so pressing `w` reflows the pane and moves nothing the
    // bar is drawn from. A build that made the total display-rows would pass every
    // other gate here and fail this one.
    let (scratch, mut highlighter, history) = open("shell-wrap-thumb");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);

    let mut app = App::new();
    let height = diff_height(PANE, &chrome_of(&app), 1, 1);
    let before = app
        .view(&mut frame, &mut highlighter, &history, split(&app))
        .expect("view");

    app.apply(Action::ToggleWrap, &mut frame, height)
        .expect("apply");
    let after = app
        .view(&mut frame, &mut highlighter, &history, split(&app))
        .expect("view");

    assert!(
        after.rows.iter().any(|row| matches!(row, Row::Wrap { .. })),
        "the press did not turn wrapping on, so nothing below is a comparison"
    );
    assert_eq!(
        (before.total_rows, before.rows_above, before.top),
        (after.total_rows, after.rows_above, after.top),
        "pressing `w` moved the scrollbar, so the bar is counting display rows"
    );
}

#[test]
fn a_wrapped_row_still_costs_the_pane_rather_than_the_line() {
    // **I4's shape, and the one an unbounded wrap would break first.**
    // [#45](https://github.com/breferrari/vigia/issues/45) bounded a row's walk at
    // the pane, and `SPEC.md` §10 records the measurement: a 22-row body of
    // Japanese examined 8231 characters to fill 1600 columns before the bound
    // existed. A second row per line is allowed to double the walk and is not
    // allowed to make it follow the *line*, so the ceiling is written in rows and
    // columns, which are both the pane's.
    //
    // The fixture is `Scratch::wide_lines_as`, whose lines are 531 columns: the
    // same one `tests/paint.rs` uses, so the two bounds are over the same
    // workload and the difference between them is the mode.
    let scratch = Scratch::wide_lines_as("shell-wrap-cost", 3, 12, "rs");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let mut app = wrapped(&mut frame);

    let chrome = chrome_of(&app);
    let laid = body_layout(PANE, &chrome, 3, 3);
    let view = app
        .view(&mut frame, &mut highlighter, &history, laid)
        .expect("view");
    let mut buf = Buffer::empty(PANE);
    let stats = render(
        &mut buf,
        PANE,
        &view,
        &Theme::default(),
        Glyphs::default(),
        &chrome,
    );

    assert!(
        view.rows.iter().any(|row| matches!(row, Row::Wrap { .. })),
        "the cost below was measured on a pane that wrapped nothing"
    );
    assert!(stats.examined > 0, "the counter is not being fed at all");
    let bound = stats.rows as u64 * u64::from(PANE.width);
    assert!(
        stats.examined <= bound,
        "a wrapped {}-row body examined {} source characters against the {bound} \
         a pane of this size allows, so a row is costing the line rather than the \
         pane",
        stats.rows,
        stats.examined
    );
}

#[test]
fn every_jump_still_lands_on_a_heading_when_lines_wrap() {
    // **`scroll.rs`'s own gate, in the mode that could break it.** Every jump on
    // this map resolves through `App::jump_to` to `Position { file, row: 0 }`,
    // and row zero of a file is its heading. Wrapping cannot move that by itself,
    // because a heading is not a content row and never continues; what *can* move
    // it is the front trim, which is the one place in this change that rewrites
    // `View::top` after the walk has placed it.
    //
    // So the fixture is several files and the gate jumps to each in turn, which
    // is the shape that would catch a trim firing on a frame that is not at the
    // diff's bottom.
    let scratch = Scratch::new("shell-wrap-jumps");
    scratch.write("src/a.rs", "one\n");
    scratch.write("src/b.rs", "one\n");
    scratch.write("src/c.rs", "one\n");
    scratch.commit_all("base");
    for name in ["a", "b", "c"] {
        let mut body = String::new();
        for n in 0..8 {
            body.push_str(&format!("let {name}_{n} = \"{}\";\n", "w".repeat(70)));
        }
        scratch.write(&format!("src/{name}.rs"), body);
    }

    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let mut app = wrapped(&mut frame);
    let height = diff_height(PANE, &chrome_of(&app), 3, 3);

    for file in 0..3u16 {
        app.apply(Action::ListRow(file), &mut frame, height)
            .expect("apply");
        let chrome = chrome_of(&app);
        let laid = body_layout(PANE, &chrome, 3, 3);
        let view = app
            .view(&mut frame, &mut highlighter, &history, laid)
            .expect("view");
        assert_eq!(
            view.top.row, 0,
            "a jump to file {file} with wrapping on landed {} rows into its block",
            view.top.row
        );
        assert!(
            matches!(view.rows.first(), Some(Row::File(_))),
            "a jump to file {file} with wrapping on did not put its heading on the \
             top row: {:?}",
            view.rows.first()
        );
    }
}
