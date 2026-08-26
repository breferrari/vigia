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

// **The screen selectors, under a second name.** `support` is already bound to
// `vigia-core`'s fixture module above, and this file needs both: a repository to
// drive and a cell walk to read the pane back with. See `screen::rows_of` for why
// that walk is shared rather than written here.
#[path = "support/mod.rs"]
mod screen;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use vigia::{
    Action, App, Body, Glyphs, Pointing, Row, TRACK_SCALE, Theme, View, Viewport, body_layout,
    diff_height, regions, render,
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
    let rows = screen::rows_of(
        &buf,
        Rect::new(
            0,
            laid_regions.diff.top,
            PANE.width,
            laid_regions.diff.rows as u16,
        ),
    );
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

#[test]
fn scrolling_up_from_the_wrapped_bottom_moves() {
    // **The end of the diff must be a place a reader can leave.**
    //
    // The first draft of the front trim advanced `View::top` by the rows it
    // dropped, and `App` stores that back as the scroll position. From the
    // advanced row fewer than a screenful of logical rows remain, so the next
    // frame is short, restarts through `last_screenful`, lands on the same row
    // and trims to the same place: every `k` after that resolves to the position
    // it started from, and the end of the diff is a one-way trap. Two audit
    // remits found it independently.
    //
    // Asserted as *the position strictly decreases*, and then as *the top of the
    // diff is reachable by pressing `k` enough times*, because the first alone
    // would pass against a shell that moved one row and then stuck.
    let scratch = Scratch::new("shell-wrap-scroll-up");
    scratch.write("src/lines.rs", "seed\n");
    scratch.commit_all("base");
    let mut body = String::new();
    for n in 0..40 {
        body.push_str(&format!("let line_{n} = \"{}\";\n", "q".repeat(70)));
    }
    scratch.write("src/lines.rs", body);

    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let mut app = wrapped(&mut frame);
    let height = diff_height(PANE, &chrome_of(&app), 1, 1);

    app.apply(Action::Scroll(10_000), &mut frame, height)
        .expect("apply");
    let bottom = app
        .view(&mut frame, &mut highlighter, &history, split(&app))
        .expect("view")
        .top;
    assert!(
        bottom.row > 0,
        "the fixture never left row zero, so there is no bottom to be trapped at"
    );

    app.apply(Action::Scroll(-1), &mut frame, height)
        .expect("apply");
    let up = app
        .view(&mut frame, &mut highlighter, &history, split(&app))
        .expect("view")
        .top;
    assert!(
        up.row < bottom.row,
        "`k` at the wrapped bottom resolved back to the same row ({} then {}), \
         so the end of the diff cannot be left",
        bottom.row,
        up.row
    );

    // And all the way back, which is what says the first step was not the only one.
    for _ in 0..200 {
        app.apply(Action::Scroll(-1), &mut frame, height)
            .expect("apply");
    }
    let top = app
        .view(&mut frame, &mut highlighter, &history, split(&app))
        .expect("view")
        .top;
    assert_eq!(
        top.row, 0,
        "two hundred presses of `k` from the wrapped bottom did not reach the \
         top of the diff"
    );
}

#[test]
fn a_wide_glyph_straddling_the_break_is_drawn_on_one_of_the_two_rows() {
    // **A character is not allowed to fall down the crack between the rows.**
    //
    // The walk admits a character while `column < room` and only then charges its
    // width, so the last one admitted can be two columns wide against one column
    // of room. Splitting after it left a head one column too wide, which the
    // painter then cut and stamped `›` on: the glyph was drawn on neither row,
    // and a head that continues downward carried the mark that means rightward.
    //
    // **What this asserts is the invariant, not the symptom the audit predicted**,
    // and the difference is worth recording. The report described a head cut and
    // stamped `›` with the glyph drawn on neither row; mutated back to the old
    // boundary the pane draws that glyph intact and unmarked at every one of the
    // sixty-one widths swept here, so the symptom could not be demonstrated. The
    // bound moved anyway, because *the head never exceeds the room it was cut
    // for* is the property the row model owes the painter and the old form did
    // not guarantee it. The gate says that, which is checkable, rather than
    // restating a repro that does not reproduce.
    //
    // The fixture is ASCII padding then CJK, so the break lands on a two-column
    // glyph at some width of the sweep whatever the gutter does.
    let scratch = Scratch::new("shell-wrap-wide-break");
    scratch.write("src/wide.rs", "seed\n");
    scratch.commit_all("base");
    scratch.write(
        "src/wide.rs",
        format!("let w = \"{}日本語のテキスト\";\n", "a".repeat(40)),
    );
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    for width in 30u16..=90 {
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

        for (n, row) in view.rows.iter().enumerate() {
            let Row::Line { text: head, .. } = row else {
                continue;
            };
            let Some(Row::Wrap { text: tail, .. }) = view.rows.get(n + 1) else {
                continue;
            };
            // The whole source line, split across the two rows and nothing lost
            // between them. Read off the rows rather than off the painter,
            // because the painter is what the row model is telling what to draw.
            let whole = format!("{head}{tail}");
            assert!(
                whole.contains("日本語のテキスト"),
                "at {width} columns a wrapped line lost content across the \
                 break: head {head:?} tail {tail:?}"
            );
        }

        // And the head is never marked, at any width, which is the half the
        // painter answers.
        let (_, rows) = {
            let mut buf = ratatui::buffer::Buffer::empty(at);
            render(
                &mut buf,
                at,
                &view,
                &Theme::default(),
                Glyphs::default(),
                &chrome,
            );
            let laid_regions = regions(at, &chrome, &view);
            (
                (),
                screen::rows_of(
                    &buf,
                    Rect::new(
                        0,
                        laid_regions.diff.top,
                        width,
                        laid_regions.diff.rows as u16,
                    ),
                ),
            )
        };
        for (n, row) in rows.iter().enumerate() {
            if rows.get(n + 1).is_some_and(|below| below.contains('↳')) {
                assert!(
                    !row.ends_with('›'),
                    "at {width} columns a head row that continues downward is \
                     marked as continuing rightward: {row:?}"
                );
                assert!(
                    ratatui::text::Span::raw(row).width() <= usize::from(width),
                    "at {width} columns a head row occupies {} of them: {row:?}",
                    ratatui::text::Span::raw(row).width()
                );
            }
        }
    }
}

#[test]
fn a_line_of_zero_width_characters_does_not_buy_a_second_row() {
    // **The walk reports clipped for two reasons and only one is a full row.**
    // The other is the character bound, which a run of combining marks trips with
    // the row still empty; wrapping there spends one of the two rows the cap
    // allows on a row that draws nothing.
    let scratch = Scratch::new("shell-wrap-zero-width");
    scratch.write("src/zero.rs", "seed\n");
    scratch.commit_all("base");
    // Combining acute accents: many characters, no columns.
    scratch.write("src/zero.rs", format!("//{}\n", "\u{0301}".repeat(600)));
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let mut app = wrapped(&mut frame);
    let (view, _) = drawn(&mut app, &mut frame, &mut highlighter, &history);

    assert!(
        view.rows.iter().any(|row| matches!(row, Row::Line { .. })),
        "the fixture drew no content rows at all"
    );
    assert!(
        !view.rows.iter().any(|row| matches!(row, Row::Wrap { .. })),
        "a line that occupies no columns wrapped anyway, so one of the two rows \
         the cap allows is drawing nothing"
    );
}

/// A fixture of one file whose every line wraps, taller than the pane.
fn tall(name: &str, lines: usize) -> Scratch {
    let scratch = Scratch::new(name);
    scratch.write("src/lines.rs", "seed\n");
    scratch.commit_all("base");
    let mut body = String::new();
    for n in 0..lines {
        body.push_str(&format!("let line_{n} = \"{}\";\n", "q".repeat(70)));
    }
    body.push_str(&format!("let last = \"tail\"; // {TAIL}\n"));
    scratch.write("src/lines.rs", body);
    scratch
}

#[test]
fn the_pinned_end_is_reachable_when_lines_wrap() {
    // **`G` inside a pinned file clamps in `App`, not in the walk**, and it
    // clamps with `span.saturating_sub(height)`: a **logical** span less a
    // **display** height. With `w` on that lands the top too early, and until
    // `View::collect` derived *at the bottom* for itself the walk had no way to
    // know it should rest the last row on the last row, so the pinned file's own
    // ending was unreachable by the one gesture that names it.
    //
    // The same subtraction is in the drag on the diff's bar, which the next gate
    // drives. Both are outside `collect` and neither could be fixed by a flag
    // `collect` sets, which is why the flag is derived from the walk instead.
    let scratch = tall("shell-wrap-pinned-end", 40);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let mut app = wrapped(&mut frame);
    let height = diff_height(PANE, &chrome_of(&app), 1, 1);

    // **A frame before a gesture, which is what the shell does.** `App::shown`
    // is what the last frame drew, so it is zero until one has been; the loop
    // paints and then reads input, so a reader's first keypress always follows a
    // paint. A gate that pressed a key against a shell that had never drawn would
    // be measuring a state the program cannot be in.
    drawn(&mut app, &mut frame, &mut highlighter, &history);

    app.apply(Action::ToggleSingle, &mut frame, height)
        .expect("apply");
    drawn(&mut app, &mut frame, &mut highlighter, &history);
    app.apply(Action::Bottom, &mut frame, height)
        .expect("apply");
    let (_, rows) = drawn(&mut app, &mut frame, &mut highlighter, &history);
    assert!(
        rows.iter().any(|row| row.contains(TAIL)),
        "`G` in a pinned file with wrapping on cannot reach its last line:\n{}",
        rows.join("\n")
    );
}

#[test]
fn a_drag_to_the_end_of_the_bar_reaches_the_end_when_lines_wrap() {
    // The pointer's half of the gate above. `Action::DiffTo` clears `anchored`,
    // so nothing self-corrects on a later frame: before the derivation, dragging
    // the thumb to the bottom with `w` on hid the tail for as long as the reader
    // left it there.
    let scratch = tall("shell-wrap-drag-end", 40);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let mut app = wrapped(&mut frame);
    let height = diff_height(PANE, &chrome_of(&app), 1, 1);

    // **A frame before a gesture, which is what the shell does.** `App::shown`
    // is what the last frame drew, so it is zero until one has been; the loop
    // paints and then reads input, so a reader's first keypress always follows a
    // paint. A gate that pressed a key against a shell that had never drawn would
    // be measuring a state the program cannot be in.
    drawn(&mut app, &mut frame, &mut highlighter, &history);

    app.apply(Action::DiffTo(TRACK_SCALE), &mut frame, height)
        .expect("apply");
    let (_, rows) = drawn(&mut app, &mut frame, &mut highlighter, &history);
    assert!(
        rows.iter().any(|row| row.contains(TAIL)),
        "a drag to the bottom of the diff's bar with wrapping on does not reach \
         the end of the diff:\n{}",
        rows.join("\n")
    );
}

#[test]
fn a_page_step_skips_no_line_when_lines_wrap() {
    // **A page is a screenful, and a screenful is fewer rows of the diff when
    // they wrap.** `Space` stepped `height - 1` logical rows against a screen
    // showing about half that many, so a reader paging through a wrapped diff
    // never saw most of it. The step is taken before the frame it moves to
    // exists, so `App` remembers what the last frame drew.
    //
    // Asserted as *the first line of page two is one the reader has already
    // seen*, which is the overlap row a page keeps, rather than as an arithmetic
    // identity: the identity would restate the implementation.
    let scratch = tall("shell-wrap-page", 40);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let mut app = wrapped(&mut frame);
    let height = diff_height(PANE, &chrome_of(&app), 1, 1);

    let (_, first) = drawn(&mut app, &mut frame, &mut highlighter, &history);
    // **The line's own name, not the drawn row.** A row carries the scrollbar's
    // glyph in its last column and the thumb moves between the two frames, so
    // comparing rows compares the bar and reports a skip that did not happen.
    let named = |rows: &[String]| -> Vec<String> {
        rows.iter()
            .filter_map(|row| {
                row.split_whitespace()
                    .find(|word| word.starts_with("line_"))
                    .map(str::to_owned)
            })
            .collect()
    };
    let seen = named(&first);
    assert!(
        seen.len() > 2,
        "page one drew {} content rows, which is too few to page over",
        seen.len()
    );

    app.apply(Action::Page(1), &mut frame, height)
        .expect("apply");
    let (_, second) = drawn(&mut app, &mut frame, &mut highlighter, &history);
    let next = named(&second);
    let landed = next.first().expect("page two drew no content");
    assert!(
        seen.contains(landed),
        "page two opens on {landed:?} and page one showed {seen:?}, so a page \
         step with wrapping on walked over content nobody saw"
    );
}

#[test]
fn the_wrapped_bottom_survives_the_frame_after_the_gesture() {
    // **The gate every other one in this file was missing, and it catches the
    // same defect three ways.**
    //
    // A monitor repaints without being touched: a filesystem event, an ageing
    // wake, a pointer crossing the pane. So a screen that is only correct on the
    // frame a gesture produced is not correct at all, and every gate here drew
    // **once** after its gesture, which is exactly the frame that cannot see it.
    //
    // The defect: the bottom clamp fires on the frame a reader reaches the end
    // and stores a position from which the next walk collects exactly a
    // screenful, so nothing overshoots, nothing is short, and the second frame
    // dropped the tail off the bottom again. The reader saw the last line, did
    // nothing, and watched it go.
    //
    // Driven three ways because the three gestures reach the end by three
    // different roads: the scroll clamp, the pinned `G`, and a drag to the far
    // end of the bar.
    for way in ["scroll", "pinned G", "drag"] {
        let scratch = tall(&format!("shell-wrap-steady-{}", way.replace(' ', "-")), 40);
        let worktree = scratch.worktree();
        let mut frame = worktree.frame();
        materialise(&mut frame);
        let mut highlighter = Highlighter::eager();
        let history = History::new();
        let mut app = wrapped(&mut frame);
        let height = diff_height(PANE, &chrome_of(&app), 1, 1);
        drawn(&mut app, &mut frame, &mut highlighter, &history);

        match way {
            "scroll" => {
                app.apply(Action::Scroll(10_000), &mut frame, height)
                    .expect("apply");
            }
            "pinned G" => {
                app.apply(Action::ToggleSingle, &mut frame, height)
                    .expect("apply");
                drawn(&mut app, &mut frame, &mut highlighter, &history);
                app.apply(Action::Bottom, &mut frame, height)
                    .expect("apply");
            }
            _ => {
                app.apply(Action::DiffTo(TRACK_SCALE), &mut frame, height)
                    .expect("apply");
            }
        }

        let (_, landing) = drawn(&mut app, &mut frame, &mut highlighter, &history);
        assert!(
            landing.iter().any(|row| row.contains(TAIL)),
            "{way} did not reach the end of the diff at all:\n{}",
            landing.join("\n")
        );

        // **No gesture between the two frames**, which is the whole of it: this is
        // the repaint a monitor does on its own.
        let (_, settled) = drawn(&mut app, &mut frame, &mut highlighter, &history);
        assert!(
            settled.iter().any(|row| row.contains(TAIL)),
            "after {way} the end of the diff was drawn once and gone on the next \
             repaint, so a reader who touches nothing loses it:\n{}",
            settled.join("\n")
        );
    }
}

#[test]
fn a_press_of_w_at_the_bottom_keeps_the_last_line_on_screen() {
    // **The mode changing under a reader who is resting at the end.** Wrapping
    // means fewer of the diff's rows fit, so the clamp has to give the difference
    // back; without that the last line a reader was looking at scrolls off the
    // bottom on the keystroke that was supposed to show them more of it.
    //
    // `w_toggles_wrapping_and_leaves_the_thumb_where_it_was` presses `w` from row
    // zero, where there is nothing to trim, so it holds for a narrower reason
    // than its name and cannot see this.
    let scratch = tall("shell-wrap-press-at-bottom", 40);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let mut app = App::new();
    let height = diff_height(PANE, &chrome_of(&app), 1, 1);

    drawn(&mut app, &mut frame, &mut highlighter, &history);
    app.apply(Action::Scroll(10_000), &mut frame, height)
        .expect("apply");
    let (_, before) = drawn(&mut app, &mut frame, &mut highlighter, &history);
    assert!(
        before.iter().any(|row| row.contains(TAIL)),
        "the unwrapped pane is not at the end of the diff, so pressing `w` here \
         says nothing"
    );

    app.apply(Action::ToggleWrap, &mut frame, height)
        .expect("apply");
    let (_, after) = drawn(&mut app, &mut frame, &mut highlighter, &history);
    assert!(
        after.iter().any(|row| row.contains(TAIL)),
        "pressing `w` at the end of the diff scrolled the last line off the \
         bottom:\n{}",
        after.join("\n")
    );
}

#[test]
fn the_bar_is_drawn_from_the_travel_a_drag_is_resolved_against() {
    // **A readout and the gesture performed on it are one contract**, which this
    // project has been corrected on once already: a bar drawn from one quantity
    // and dragged against another agrees at the two ends and comes apart in the
    // middle, so a gate that checks only the ends passes against the defect.
    //
    // The first draft of `App::dragged_to` measured the track in the rows a frame
    // *drew*, while the painter goes on drawing the thumb from the region's
    // height. Pressing inside the thumb's own body then sent it to the bottom of
    // the track from under the pointer.
    //
    // Asserted at the **middle** of the track, and as a round trip: where the
    // thumb is drawn after a drag has to be where the drag was aimed.
    let scratch = tall("shell-wrap-bar-contract", 60);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let mut app = wrapped(&mut frame);
    let height = diff_height(PANE, &chrome_of(&app), 1, 1);
    drawn(&mut app, &mut frame, &mut highlighter, &history);

    let middle = TRACK_SCALE / 2;
    app.apply(Action::DiffTo(middle), &mut frame, height)
        .expect("apply");
    let view = app
        .view(&mut frame, &mut highlighter, &history, split(&app))
        .expect("view");

    assert!(
        view.total_rows > 0,
        "the frame measured no total, so there is no bar to be dragged"
    );
    // Where the thumb sits, in the same units the painter draws it in: rows above
    // over the diff's own rows.
    let travel = view.total_rows.saturating_sub(height);
    let want = travel / 2;
    assert!(
        view.rows_above.abs_diff(want) <= 1,
        "a drag to the middle of the track resolved to row {} where the thumb is \
         drawn at {want} of a travel of {travel}, so the bar and the drag are two \
         different arithmetics",
        view.rows_above
    );
}

#[test]
fn a_page_step_after_a_resize_is_bounded_by_the_pane_it_lands_in() {
    // **A screenful measured on the departing screen, spent in the arriving
    // one.** `App::shown` is what the last frame drew and `height` is this
    // batch's: `lib.rs` drains a batch of events and paints once at the end of
    // it, so a resize and a `Space` arriving together give a fresh height against
    // a stale count. Dragged from fifty rows to twelve, the step would otherwise
    // be measured in the fifty-row screen and walk over everything the twelve-row
    // one could have shown.
    //
    // Asserted as *the step is no larger than the pane it lands in*, which is the
    // claim, rather than as an exact row, which would restate the arithmetic.
    let scratch = tall("shell-wrap-resize-page", 80);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    let tall_pane = Rect::new(0, 0, 80, 50);
    let short_pane = Rect::new(0, 0, 80, 12);
    let mut app = App::new();
    let tall_body = diff_height(tall_pane, &chrome_of(&app), 1, 1);
    app.apply(Action::ToggleWrap, &mut frame, tall_body)
        .expect("apply");

    // One frame on the tall pane, which is what leaves a large `shown` behind.
    // **And it is the last frame before the step**, which is the whole case: a
    // frame drawn on the small pane first would refresh the count and the gate
    // would be measuring a state the defect cannot reach. Mutation caught that:
    // with a small-pane frame in between, removing the clamp survived.
    let chrome = chrome_of(&app);
    let before = app
        .view(
            &mut frame,
            &mut highlighter,
            &history,
            body_layout(tall_pane, &chrome, 1, 1),
        )
        .expect("view")
        .top;

    // Then the pane shrinks and a page is asked for, with no frame between.
    let short_body = diff_height(short_pane, &chrome_of(&app), 1, 1);
    app.apply(Action::Page(1), &mut frame, short_body)
        .expect("apply");
    let after = app
        .view(
            &mut frame,
            &mut highlighter,
            &history,
            body_layout(short_pane, &chrome_of(&app), 1, 1),
        )
        .expect("view")
        .top;

    let stepped = after.row.saturating_sub(before.row);
    assert!(
        stepped > 0,
        "the page step moved nothing, so this measures no step at all"
    );
    assert!(
        stepped <= short_body,
        "a page on a {short_body}-row body stepped {stepped} rows of the diff, \
         so the step was measured in the pane the reader has left"
    );
}

#[test]
fn a_diff_that_fits_unwrapped_and_not_wrapped_keeps_its_heading() {
    // **The band no other fixture in this file sits in**: a diff whose *own* rows
    // fit the pane and whose *display* rows do not. `tall()` overruns both and
    // `fixture` overruns neither, which is why twenty-two gates missed this.
    //
    // One `j` there ran the walk short, restarted it through `last_screenful` to
    // the walk's own floor, and trimmed from the front: the heading, the path,
    // the counts and the heat strip went off the top, `k` at row zero moves
    // nothing, and no scrollbar is drawn on a diff that short to say anything was
    // hidden. A trim is only honest where there is somewhere above to scroll to.
    //
    // Driven with `j` then `k`, because the position the trim leaves has to be
    // one the reader can get back from.
    let scratch = Scratch::new("shell-wrap-short-band");
    scratch.write("src/band.rs", "seed\n");
    scratch.commit_all("base");
    let mut body = String::new();
    // Ten wrapped lines: thirteen of the diff's own rows against a body of
    // eighteen, and twenty-three display rows, which is the band.
    for n in 0..10 {
        body.push_str(&format!("let band_{n} = \"{}\";\n", "b".repeat(70)));
    }
    scratch.write("src/band.rs", body);

    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let mut app = wrapped(&mut frame);
    let height = diff_height(PANE, &chrome_of(&app), 1, 1);

    let (view, _) = drawn(&mut app, &mut frame, &mut highlighter, &history);
    let logical = view
        .rows
        .iter()
        .filter(|row| !matches!(row, Row::Wrap { .. }))
        .count();
    assert!(
        logical < height && view.rows.len() >= height,
        "the fixture is {logical} of the diff's own rows in {} display rows \
         against a body of {height}, which is not the band this gate is about",
        view.rows.len()
    );

    // **Down one, then back.** Scrolling down is *supposed* to take the heading
    // off the top; the defect was that it could not be brought back, because the
    // walk restarted to its own floor and trimmed from the front, where `k` at
    // row zero moves nothing. So the claim is the return, not the departure.
    app.apply(Action::Scroll(1), &mut frame, height)
        .expect("apply");
    let (down, _) = drawn(&mut app, &mut frame, &mut highlighter, &history);
    assert_eq!(
        down.top.row, 1,
        "one `j` did not move the top, so the return below proves nothing"
    );

    app.apply(Action::Scroll(-1), &mut frame, height)
        .expect("apply");
    let (back, rows) = drawn(&mut app, &mut frame, &mut highlighter, &history);
    assert_eq!(
        back.top.row, 0,
        "`k` did not come back to the top of the diff"
    );
    assert!(
        matches!(back.rows.first(), Some(Row::File(_))),
        "the reader came back to the top of the diff and the heading was still \
         trimmed off it, with nothing above to scroll to:\n{}",
        rows.join("\n")
    );

    // **And the overshoot road to the same floor**, which the two gestures above
    // do not take. Scrolling far past the end of a diff this short overshoots the
    // last file rather than running the walk short, and that path restarts through
    // `last_screenful` too: it is the second of the two places that can land the
    // reader on the walk's own floor with more display rows than the pane holds.
    app.apply(Action::Scroll(10_000), &mut frame, height)
        .expect("apply");
    let (over, rows) = drawn(&mut app, &mut frame, &mut highlighter, &history);
    assert_eq!(
        over.top.row, 0,
        "a diff shorter than the pane in its own rows did not clamp back to its \
         own top"
    );
    assert!(
        matches!(over.rows.first(), Some(Row::File(_))),
        "scrolling past the end of a diff shorter than the pane trimmed its \
         heading off the top, and there is nothing above to scroll to:\n{}",
        rows.join("\n")
    );
}

#[test]
fn a_landing_follow_served_is_not_trimmed_off_its_own_row() {
    // **I5's frame is the one frame that must not be trimmed.** Follow puts the
    // viewport on what just changed; where that change sits exactly one screenful
    // from the end of the diff, the bottom clamp's own conditions are all met and
    // the front trim would drop the landing row itself. The single frame that
    // exists to show a reader what an agent just did would then not show it.
    //
    // **Swept over heights rather than pinned at one**, because the case is an
    // equality between the rows remaining and the rows a pane holds, and which
    // height produces it is a function of the fixture's shape rather than
    // something to compute here. The audit that raised this could not say whether
    // any real pane reaches it; the sweep is what answers that.
    let scratch = Scratch::new("shell-wrap-landing");
    scratch.write("src/deep.rs", &{
        let mut base = String::new();
        for n in 0..20 {
            base.push_str(&format!("let keep_{n} = \"{}\";\n", "k".repeat(70)));
        }
        base
    });
    scratch.commit_all("base");
    // **Two hunks, and the busy one is the deep one.** `landing_of` lands on the
    // *busiest* hunk's header, and a file with one hunk near its top always puts
    // that header on row one, where the landing is row zero's neighbour and this
    // gate's case cannot arise. One line changed at the top and eight at the
    // bottom is what makes the landing a row deep in the block.
    let mut edited = String::new();
    for n in 0..20 {
        if n == 1 || (12..16).contains(&n) {
            edited.push_str(&format!("let CHANGED_{n} = \"{}\";\n", "c".repeat(70)));
        } else {
            edited.push_str(&format!("let keep_{n} = \"{}\";\n", "k".repeat(70)));
        }
    }
    scratch.write("src/deep.rs", edited);

    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    let mut served = 0usize;
    for rows in 3usize..=40 {
        let view = View::collect(
            &mut frame,
            &mut highlighter,
            &history,
            Viewport {
                diff_rows: rows,
                width: 78,
                wrap: true,
                landing: true,
                measured: true,
                ..Viewport::default()
            },
        )
        .expect("view");
        if !view.landed || view.top.row == 0 {
            continue;
        }
        served += 1;
        // **I5's promise is that the change is drawn, not that a particular row
        // is on top**, which is #257's own ruling and the reason a landing that
        // runs short is allowed to be superseded by the back-up that fills the
        // pane. So the assertion is the promise: a frame that served a landing
        // draws the change it landed for.
        // **A changed line, either side of it.** The oracle was the added
        // lines' own text at first, and that is not the change: a hunk draws its
        // removals before its additions, so a pane resting on the removed side is
        // showing the reader exactly what the agent did and the gate called it a
        // failure. I5 says the viewport goes to what changed, not to the half of
        // it that is green.
        assert!(
            view.rows.iter().any(|row| matches!(
                row,
                Row::Line { kind, .. } | Row::Wrap { kind, .. }
                    if *kind != vigia_core::LineKind::Context
            )),
            "at {rows} rows a served landing drew no changed line at all, so the \
             frame that exists to show what just changed does not show it"
        );
    }
    assert!(
        served > 0,
        "no height in the sweep served a landing below row zero, so this gate \
         never reached the case it is about"
    );
}
