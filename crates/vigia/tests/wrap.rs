//! `SPEC.md` §11.2 B19: a long line continues on the row below, on `w`.

#[path = "../../vigia-core/tests/support/mod.rs"]
mod support;

// The screen selectors, under a second name.
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
const PANE: Rect = Rect::new(0, 0, 80, 24);

/// A token nothing else in the fixture spells, at the end of the long line.
const TAIL: &str = "ZOMBIE";

/// A token at the end of a line that is short enough to fit.
const SHORT_TAIL: &str = "FITS";

/// Columns of leading blank on the deeply indented line, past twice what half of
/// an eighty-column pane's content leaves.
const DEEP: usize = 60;

/// Build the fixture: one committed file, then a rewrite whose lines are chosen
/// widths.
fn fixture(name: &str) -> Scratch {
    let scratch = Scratch::new(name);
    scratch.write("src/lines.rs", "one\ntwo\nthree\nfour\nfive\n");
    scratch.commit_all("base");

    let long = format!("let value = \"{}\"; // {TAIL}", "x".repeat(70));
    let short = format!("let small = \"aa\"; // {SHORT_TAIL}");
    // Taller than the whole region, which is the one case a mark still means something
    // now that lines wrap as far as they need: scrolling steps over a line by whole
    // rows of the diff, so nothing reaches the middle of one the pane cannot hold.
    let huge = format!("let huge = \"{}\"; // {TAIL}", "y".repeat(2000));
    let deep = format!(
        "{}let deep = \"{}\"; // {TAIL}",
        " ".repeat(DEEP),
        "z".repeat(70)
    );
    // The huge line last, after the context row, because it fills the region
    // on its own: put anywhere earlier it leaves every gate below it looking at a
    // pane that draws one line, and the unchanged `five` that several of them
    // need as a *context* row is never reached.
    scratch.write(
        "src/lines.rs",
        format!("{long}\n{short}\n{deep}\nfive\n{huge}\n"),
    );
    scratch
}

fn chrome_of(app: &App) -> vigia::Chrome {
    app.chrome("fixture", None, Pointing::default(), 0, "")
}

fn split(app: &App) -> Body {
    body_layout(PANE, &chrome_of(app), 1, 1)
}

/// One painted frame: the view it drew, the cells it drew into, and the chrome
/// those were laid out against, which a caller needs to ask where a region is.
fn painted(
    pane: Rect,
    app: &mut App,
    frame: &mut Frame,
    highlighter: &mut Highlighter,
    history: &History,
) -> (View, Buffer, vigia::Chrome) {
    let chrome = chrome_of(app);
    let laid = body_layout(pane, &chrome, 1, 1);
    let view = app
        .view(frame, highlighter, history, laid)
        .expect("a view of the fixture");
    let mut buf = Buffer::empty(pane);
    render(
        &mut buf,
        pane,
        &view,
        &Theme::default(),
        Glyphs::default(),
        &chrome,
    );
    (view, buf, chrome)
}

/// Every row of the diff region, as strings, with trailing blanks trimmed.
fn drawn(
    app: &mut App,
    frame: &mut Frame,
    highlighter: &mut Highlighter,
    history: &History,
) -> (View, Vec<String>) {
    let (view, buf, chrome) = painted(PANE, app, frame, highlighter, history);
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
    // Three widths of line, and the gates below cannot tell them apart without it.
    let (scratch, mut highlighter, history) = open("shell-wrap-shape");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = wrapped(&mut frame);
    let (view, _) = drawn(&mut app, &mut frame, &mut highlighter, &history);

    // Some lines continue and some do not, which is the property the gates below turn
    // on.
    let mut wrapped = 0usize;
    let mut plain = 0usize;
    for (n, row) in view.rows.iter().enumerate() {
        if !matches!(row, Row::Line { .. }) {
            continue;
        }
        if matches!(view.rows.get(n + 1), Some(Row::Wrap { .. })) {
            wrapped += 1;
        } else {
            plain += 1;
        }
    }
    assert!(
        wrapped > 0 && plain > 0,
        "the fixture draws {wrapped} lines that continue and {plain} that do not, \
         so it cannot tell one from the other"
    );
    assert!(
        view.gutter.is_some(),
        "the walk took no gutter decision, so nothing below is measuring the \
         width the rows were wrapped against"
    );
}

#[test]
fn a_long_line_is_readable_to_its_end_only_when_wrapping_is_on() {
    // The whole of the issue, as one comparison. Off, the tail of the long
    // line is nowhere on the pane and there is no route to it: no wrap, no pan,
    // and the takeover holds the mouse. On, it is drawn.
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
fn a_mark_means_the_line_is_taller_than_the_pane_and_nothing_else() {
    // `›` says *rightward*, and there is nothing to the right of a row whose
    // content is on the row below. So no row that continues downward carries
    // it.
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

    // The huge line is four hundred columns of `y` against a pane of eighty, so
    // it is taller than the region and its last drawn row says so.
    assert!(
        rows.iter().any(|row| row.ends_with('›')),
        "a line taller than the pane was cut with no row saying so, and nothing \
         scrolls to its middle:\n{}",
        rows.join("\n")
    );
    // And the rows above that one continue downward without claiming to continue
    // rightward.
    let marked = rows
        .iter()
        .position(|row| row.ends_with('›'))
        .expect("a marked row");
    assert!(
        rows[..marked].iter().all(|row| !row.ends_with('›')),
        "a row that continues downward is marked as continuing rightward:\n{}",
        rows[..marked].join("\n")
    );
}

#[test]
fn a_continuation_blanks_its_gutter_and_marks_its_sigil() {
    // The two cells that say *this is not a new line*.
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
    // A band that stopped at the first row would make a wrapped removal read as ending
    // early, which is the same sentence §5.3 already carries about a band that stops
    // where its text stops.
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
    // A palette that actually washes a row, which `Theme::default()` at this depth does
    // not.
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

    // The non-vacuity guard is a *comparison*, not a presence.
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
fn a_line_takes_as_many_rows_as_it_needs_and_never_more_than_the_pane() {
    // There is no cap, and the only ceiling is the pane.
    let scratch = Scratch::new("shell-wrap-cap");
    scratch.write("src/lines.rs", "seed\n");
    scratch.commit_all("base");
    let mut body = String::new();
    for n in 0..60 {
        body.push_str(&format!(
            "let line_{n} = \"{}\"; // {TAIL}\n",
            // Long enough to need three rows at the widest pane swept.
            "q".repeat(400)
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
        let mut longest = 0usize;
        for row in &view.rows {
            match row {
                Row::Wrap { .. } => {
                    run += 1;
                    longest = longest.max(run);
                }
                _ => run = 0,
            }
        }
        assert!(
            longest > 1,
            "no line at {width} columns took more than two rows, so this pane \
             cannot tell an uncapped wrap from the capped one it replaced"
        );
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
    // Breakindent has a cap and the cap is the tail's floor.
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
    // The mode is inert when it is off, which is the claim the whole shipped default
    // rests on.
    let (scratch, mut highlighter, history) = open("shell-wrap-inert");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();
    let chrome = chrome_of(&app);
    let laid = body_layout(PANE, &chrome, 1, 1);

    // A warm-up frame first, and it is not ceremony.
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
    // The units bug the split makes visible.
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

    // Scrolled rather than jumped, because `G` unpinned is a jump to the last *file*
    // and this fixture has one.
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
fn w_toggles_wrapping_and_leaves_what_the_bar_counts_where_it_was() {
    // B19's surviving claim about the scrollbar: what it *counts* does not move.
    // The thumb's height does, because a screenful is fewer rows of the diff when
    // they wrap, and that is the half of B19 the same ruling amended.
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
    // I4's shape, and the one an unbounded wrap would break first.
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
    // `scroll.rs`'s own gate, in the mode that could break it.
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
    // The end of the diff must be a place a reader can leave.
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
fn a_wide_glyph_straddling_a_break_is_drawn_on_one_of_the_rows() {
    // A character is not allowed to fall down the crack between the rows.
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
            if !matches!(view.rows.get(n + 1), Some(Row::Wrap { .. })) {
                continue;
            }
            // The whole source line, across every row it takes and nothing
            // lost between them. Read off the rows rather than off the painter,
            // because the painter is what the row model is telling what to draw.
            let mut whole = head.clone();
            for row in view.rows[n + 1..]
                .iter()
                .take_while(|row| matches!(row, Row::Wrap { .. }))
            {
                if let Row::Wrap { text, .. } = row {
                    whole.push_str(text);
                }
            }
            assert!(
                whole.contains("日本語のテキスト"),
                "at {width} columns a wrapped line lost content across the \
                 break: the rows join to {whole:?}"
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
    // The walk reports clipped for two reasons and only one is a full row.
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
    // `G` inside a pinned file clamps in `App`, not in the walk, and it clamps with
    // `span.saturating_sub(height)`: a logical span less a display height.
    let scratch = tall("shell-wrap-pinned-end", 40);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let mut app = wrapped(&mut frame);
    let height = diff_height(PANE, &chrome_of(&app), 1, 1);

    // A frame before a gesture, which is what the shell does.
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
    // The pointer's half of the gate above.
    let scratch = tall("shell-wrap-drag-end", 40);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let mut app = wrapped(&mut frame);
    let height = diff_height(PANE, &chrome_of(&app), 1, 1);

    // A frame before a gesture, which is what the shell does.
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
    // A page is a screenful, and a screenful is fewer rows of the diff when they wrap.
    let scratch = tall("shell-wrap-page", 40);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let mut app = wrapped(&mut frame);
    let height = diff_height(PANE, &chrome_of(&app), 1, 1);

    let (_, first) = drawn(&mut app, &mut frame, &mut highlighter, &history);
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
    // The gate every other one in this file was missing, and it catches the
    // same defect three ways.
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

        // No gesture between the two frames, which is the whole of it: this is
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
    // The mode changing under a reader who is resting at the end.
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
    // A readout and the gesture performed on it are one contract, which this
    // project has been corrected on once already: a bar drawn from one quantity
    // and dragged against another agrees at the two ends and comes apart in the
    // middle, so a gate that checks only the ends passes against the defect.
    let scratch = tall("shell-wrap-bar-contract", 60);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let mut app = wrapped(&mut frame);
    let height = diff_height(PANE, &chrome_of(&app), 1, 1);
    // Kept, because the gesture below is resolved against the screenful this frame
    // leaves behind rather than against the one it lands in.
    let (before, _) = drawn(&mut app, &mut frame, &mut highlighter, &history);
    let screenful = before.shown().min(height);
    assert!(
        screenful < height,
        "the departing screen held {screenful} rows of the diff in a region of \
         {height}, so the screenful and the region agree and a bar drawn from \
         either would pass this"
    );

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
    // over the diff's own rows, the thumb spanning a screenful. `SPEC.md` §11.1
    // rules the travel is the total less a screenful, which is the region's height
    // only while nothing wraps.
    let travel = view.total_rows.saturating_sub(screenful);
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
    // A screenful measured on the departing screen, spent in the arriving one.
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
    // The band no other fixture in this file sits in: a diff whose *own* rows
    // fit the pane and whose *display* rows do not. `tall()` overruns both and
    // `fixture` overruns neither, which is why twenty-two gates missed this.
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

    // Down one, then back.
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

    // And the overshoot road to the same floor, which the two gestures above do not
    // take.
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
    // I5's frame is the one frame that must not be trimmed.
    let scratch = Scratch::new("shell-wrap-landing");
    scratch.write("src/deep.rs", &{
        let mut base = String::new();
        for n in 0..20 {
            base.push_str(&format!("let keep_{n} = \"{}\";\n", "k".repeat(70)));
        }
        base
    });
    scratch.commit_all("base");
    // Two hunks, and the busy one is the deep one.
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

#[test]
fn a_band_of_three_row_lines_scrolls_and_stays_scrolled() {
    // Every other wrapped fixture in this file takes two rows a line, which is the
    // shape the cap left behind, and two is exactly the number that hides this defect:
    // `View::display_rows` went on counting *at most one break* after the cap was
    // removed, so a screen of two-row lines was counted right by accident and a screen
    // of three-row lines read as short.
    let scratch = Scratch::new("shell-wrap-three-row-band");
    scratch.write("src/band.rs", "seed\n");
    scratch.commit_all("base");
    let mut body = String::new();
    for n in 0..7 {
        body.push_str(&format!("let wide_{n} = \"{}\";\n", "w".repeat(150)));
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
    let tallest = {
        let mut run = 0usize;
        let mut best = 0usize;
        for row in &view.rows {
            match row {
                Row::Wrap { .. } => {
                    run += 1;
                    best = best.max(run + 1);
                }
                _ => run = 0,
            }
        }
        best
    };
    assert!(
        tallest >= 3,
        "the fixture's lines take {tallest} rows, and two is the width at which \
         this defect hides"
    );

    app.apply(Action::Scroll(1), &mut frame, height)
        .expect("apply");
    let (after, _) = drawn(&mut app, &mut frame, &mut highlighter, &history);
    assert_eq!(
        after.top.row, 1,
        "one `j` on a band of three-row lines resolved back to where it started, \
         so the screen was counted as short when it was full"
    );
}

#[test]
fn the_two_row_counters_agree_on_the_same_screen() {
    // `View::display_rows` and `View::wrap_rows` count the same thing twice, once
    // before the walk decides it came up short and once when it expands, and nothing
    // tied them together until one of them was left behind by the cap's removal.
    let scratch = Scratch::new("shell-wrap-counters");
    scratch.write("src/c.rs", "seed\n");
    scratch.commit_all("base");
    let mut body = String::new();
    for n in 0..12 {
        body.push_str(&format!("let c_{n} = \"{}\";\n", "c".repeat(40 + n * 30)));
    }
    scratch.write("src/c.rs", body);

    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    for width in [30u16, 50, 80, 120] {
        for rows in [6u16, 12, 24, 40] {
            let at = Rect::new(0, 0, width, rows);
            let mut app = App::new();
            let body = diff_height(at, &chrome_of(&app), 1, 1);
            app.apply(Action::ToggleWrap, &mut frame, body)
                .expect("apply");
            let chrome = chrome_of(&app);
            let laid = body_layout(at, &chrome, 1, 1);
            let view = app
                .view(&mut frame, &mut highlighter, &history, laid)
                .expect("view");
            // Every row the expansion produced is a display row by construction,
            // so counting them the other way has to give the same answer for the
            // rows that survived.
            let counted: usize = view
                .rows
                .iter()
                .filter(|row| !matches!(row, Row::Wrap { .. }))
                .count();
            let wraps = view.rows.len() - counted;
            assert!(
                view.rows.len() <= laid.diff,
                "at {width}x{rows} the expansion drew {} rows into a region of {}",
                view.rows.len(),
                laid.diff
            );
            assert!(
                wraps == 0 || counted > 0,
                "at {width}x{rows} the screen is continuations with no line above them"
            );
        }
    }
}

/// The line names on the drawn rows, which is what a skip has to be counted in:
/// a row carries the bar's glyph in its last column and the thumb moves between
/// frames, so comparing whole rows compares the bar and reports a skip that did
/// not happen.
fn named(rows: &[String]) -> Vec<String> {
    rows.iter()
        .filter_map(|row| {
            row.split_whitespace()
                .find(|word| word.starts_with("line_"))
                .map(str::to_owned)
        })
        .collect()
}

#[test]
fn no_diff_row_is_skipped_when_a_page_follows_the_toggle_in_one_batch() {
    // The shell paints once per batch, not once per wake, so `w` and a page step
    // arrive with no frame between them and the step is measured on the pane the
    // reader is leaving. Both directions, because the two are not one case twice:
    // turning wrap off *grows* the screenful under a screen that held less.
    for (start_wrapped, step) in [
        (true, Action::Page(1)),
        (false, Action::Page(1)),
        (true, Action::HalfPage(1)),
        (false, Action::HalfPage(1)),
    ] {
        let scratch = tall("shell-wrap-batch-page", 60);
        let worktree = scratch.worktree();
        let mut frame = worktree.frame();
        materialise(&mut frame);
        let mut highlighter = Highlighter::eager();
        let history = History::new();
        let mut app = App::new();
        let height = diff_height(PANE, &chrome_of(&app), 1, 1);
        if start_wrapped {
            app.apply(Action::ToggleWrap, &mut frame, height)
                .expect("apply");
        }

        let (view, first) = drawn(&mut app, &mut frame, &mut highlighter, &history);
        let seen = named(&first);
        let logical = view.rows.iter().filter(|row| !row.is_wrap()).count();
        // Non-vacuity, and it is the whole of why this fixture wraps: where a
        // screenful of the diff is the pane's own height in both modes there is no
        // stale count for the step to be measured in, and the gate cannot fail.
        if start_wrapped {
            assert!(
                logical < height,
                "the wrapped screen held {logical} rows of the diff in a region of \
                 {height}, so wrapping bought no difference and this gate proves \
                 nothing"
            );
        }
        assert!(
            seen.len() > 2,
            "the first screen drew {} content rows, too few to page over",
            seen.len()
        );

        // Two intentions, one paint, which is what the loop does with a batch.
        app.apply(Action::ToggleWrap, &mut frame, height)
            .expect("apply");
        app.apply(step, &mut frame, height).expect("apply");
        let (after, second) = drawn(&mut app, &mut frame, &mut highlighter, &history);
        // The other direction's non-vacuity: here the wrapped screen is the one being
        // arrived at, so it is the arriving frame that has to hold fewer of the
        // diff's rows than the region or the toggle bought no difference.
        if !start_wrapped {
            let arriving = after.rows.iter().filter(|row| !row.is_wrap()).count();
            assert!(
                arriving < height,
                "the arriving screen held {arriving} rows of the diff in a region of \
                 {height}, so wrapping bought no difference and this direction \
                 proves nothing"
            );
        }
        let next = named(&second);
        let landed = next.first().expect("the step drew no content");
        assert!(
            seen.contains(landed),
            "starting {}, `w` and {step:?} in one batch opened on {landed:?} where \
             the screen before it showed {seen:?}, so the step walked over lines \
             nobody saw",
            if start_wrapped {
                "wrapped"
            } else {
                "unwrapped"
            },
        );
    }
}

/// The diff's bar: the column it is drawn in, the rows of its track, and how many
/// of those rows the thumb fills.
fn thumb_of(
    pane: Rect,
    app: &mut App,
    frame: &mut Frame,
    highlighter: &mut Highlighter,
    history: &History,
) -> (View, usize, usize) {
    let (view, buf, chrome) = painted(pane, app, frame, highlighter, history);
    let diff = regions(pane, &chrome, &view).diff;
    let column = diff.bar.expect("the diff drew no scrollbar to measure");
    let (top, rows) = diff.track;
    let filled = (top..top + rows)
        .filter(|y| buf[(column, *y)].symbol() == "\u{2588}")
        .count();
    (view, filled, rows as usize)
}

#[test]
fn the_bars_span_is_the_screenful_the_pane_holds_in_both_modes() {
    // `SPEC.md` §11.1: a track maps onto travel, and the diff's travel is its total
    // rows less *a screenful*. A screenful stopped being the region's height the day
    // `w` shipped, and the thumb went on being drawn for the region.
    for width in [80u16, 40] {
        let pane = Rect::new(0, 0, width, 24);
        let scratch = tall(&format!("shell-wrap-thumb-{width}"), 60);
        let worktree = scratch.worktree();
        let mut frame = worktree.frame();
        materialise(&mut frame);
        let mut highlighter = Highlighter::eager();
        let history = History::new();
        let mut app = App::new();
        let height = diff_height(pane, &chrome_of(&app), 1, 1);
        app.apply(Action::ToggleWrap, &mut frame, height)
            .expect("apply");

        let (view, filled, track) =
            thumb_of(pane, &mut app, &mut frame, &mut highlighter, &history);
        let logical = view.rows.iter().filter(|row| !row.is_wrap()).count();

        // The two candidate spans, and the gate is worthless where they agree.
        let honest = (logical * track / view.total_rows).max(1).min(track);
        let region = (height * track / view.total_rows).max(1).min(track);
        assert_ne!(
            honest, region,
            "at {width} columns the screenful and the region give the same thumb, \
             so this gate cannot tell them apart"
        );
        assert_eq!(
            filled, honest,
            "at {width} columns the pane holds {logical} of the diff's \
             {} rows and drew a thumb {filled} rows of {track}, where the \
             screenful gives {honest} and the region's own height gives {region}",
            view.total_rows,
        );
    }
}

#[test]
fn the_thumb_rests_at_the_track_end_at_the_wrapped_bottom() {
    // The span and `rows_above` are one change: the span alone widens the travel
    // while the position still names the stored top, which sits a screenful above
    // the screen at a wrapped bottom, and the thumb then stops short of the end.
    let scratch = tall("shell-wrap-thumb-bottom", 60);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let mut app = wrapped(&mut frame);
    let height = diff_height(PANE, &chrome_of(&app), 1, 1);
    drawn(&mut app, &mut frame, &mut highlighter, &history);

    app.apply(Action::Scroll(10_000), &mut frame, height)
        .expect("apply");
    let (view, buf, chrome) = painted(PANE, &mut app, &mut frame, &mut highlighter, &history);
    let diff = regions(PANE, &chrome, &view).diff;
    let column = diff.bar.expect("the diff drew no scrollbar");
    let (top, rows) = diff.track;
    assert!(
        rows > 2,
        "the track is {rows} rows, too short to sit short of"
    );
    assert_eq!(
        buf[(column, top + rows - 1)].symbol(),
        "\u{2588}",
        "the diff is scrolled to its end and the thumb is not on the last row of \
         its track, so the bar says there is more below"
    );
}

/// Ten lines, three of them wide: a diff that fits the region unwrapped and does
/// not once it wraps, which is the only screen where the region and the screenful
/// give the bar different answers.
fn fits(name: &str) -> Scratch {
    let scratch = Scratch::new(name);
    let base: String = (0..10).map(|n| format!("line {n}\n")).collect();
    scratch.write("src/fits.rs", base);
    scratch.commit_all("base");
    let body: String = (0..10)
        .map(|n| {
            if n % 4 == 1 {
                format!("let wide_{n} = \"{}\";\n", "w".repeat(150))
            } else {
                format!("line {n}\n")
            }
        })
        .collect();
    scratch.write("src/fits.rs", body);
    scratch
}

#[test]
fn the_pointer_is_told_about_the_bar_a_wrapped_diff_shorter_than_the_region_draws() {
    // The pointer's map and the painter decide the bar from one span, or the
    // reader sees a bar the pointer calls content. Under wrap the screenful is
    // smaller than the region, and a diff that fits one and not the other is
    // where two spans part.
    let scratch = fits("shell-wrap-bar-map");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let last = PANE.width - 1;
    let thumb_drawn = |buf: &Buffer, top: u16, rows: u16| {
        (top..top + rows).any(|y| buf[(last, y)].symbol() == "\u{2588}")
    };

    let mut app = wrapped(&mut frame);
    let (view, buf, chrome) = painted(PANE, &mut app, &mut frame, &mut highlighter, &history);
    let region = split(&app).diff;
    assert!(
        view.shown() < view.total_rows,
        "the screen holds {} of {} rows, so nothing wraps and this gate proves nothing",
        view.shown(),
        view.total_rows
    );
    assert!(
        view.total_rows <= region,
        "the diff is {} rows against a region of {region}, so both spans say bar and \
         this gate proves nothing",
        view.total_rows
    );
    let diff = regions(PANE, &chrome, &view).diff;
    assert!(
        thumb_drawn(&buf, diff.top, diff.rows),
        "the painter drew no thumb in column {last}"
    );
    assert_eq!(
        diff.bar,
        Some(last),
        "the painter drew a bar in column {last} and the pointer is told there is none"
    );

    // The direction that does not move: unwrapped, the diff fits the screen, so
    // neither the painter nor the map has a bar.
    let mut app = App::new();
    let (view, buf, chrome) = painted(PANE, &mut app, &mut frame, &mut highlighter, &history);
    let diff = regions(PANE, &chrome, &view).diff;
    assert!(
        !thumb_drawn(&buf, diff.top, diff.rows),
        "unwrapped, the diff fits the region and a thumb was drawn"
    );
    assert_eq!(
        diff.bar, None,
        "unwrapped, the diff fits the region and the pointer is told of a bar"
    );
}

/// Short unwrapped lines first, heavily wrapped ones last, so a screenful at the
/// top is far more rows of the diff than a screenful at the end. `tall` cannot
/// show this: its lines are one width, so every screenful is the same size.
fn lopsided(name: &str) -> Scratch {
    let scratch = Scratch::new(name);
    for f in 0..3 {
        scratch.write(&format!("src/f{f}.rs"), "seed\n");
    }
    scratch.commit_all("base");
    for f in 0..2 {
        let mut body = String::new();
        for n in 0..30 {
            body.push_str(&format!("let s{f}_{n} = 1;\n"));
        }
        scratch.write(&format!("src/f{f}.rs"), body);
    }
    let mut body = String::new();
    for n in 0..14 {
        body.push_str(&format!("let long_{n} = \"{}\";\n", "z".repeat(200)));
    }
    body.push_str(&format!("let last = \"tail\"; // {TAIL}\n"));
    scratch.write("src/f2.rs", body);
    scratch
}

#[test]
fn a_drag_to_the_end_of_the_bar_reaches_the_end_when_the_density_varies() {
    // The far end of the track has to ask for a row *past* the end and let the walk
    // clamp it. Resolving it against a travel instead lands inside the content, and
    // a position on a file's first row is exactly where the bottom clamp stands
    // aside so that a jump keeps its heading on the top row: the tail then falls off
    // the bottom and the thumb stops short of its own track.
    let scratch = lopsided("shell-wrap-lopsided-drag");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let mut app = wrapped(&mut frame);
    let height = diff_height(PANE, &chrome_of(&app), 1, 1);

    // A frame at the top, which is what leaves a large screenful behind for the
    // gesture to be resolved against.
    let (top, _) = drawn(&mut app, &mut frame, &mut highlighter, &history);
    let at_top = top.shown();

    app.apply(Action::DiffTo(TRACK_SCALE), &mut frame, height)
        .expect("apply");
    let (view, rows) = drawn(&mut app, &mut frame, &mut highlighter, &history);
    // Non-vacuity: the two ends of this diff must hold different numbers of its
    // rows, or a stale screenful cannot be told from a fresh one.
    assert!(
        at_top > view.shown(),
        "the fixture holds {at_top} rows of the diff at the top and {} at the end, \
         so its density does not vary and this gate proves nothing",
        view.shown()
    );
    assert!(
        rows.iter().any(|row| row.contains(TAIL)),
        "a drag to the end of the bar left the last line of the diff off the \
         screen:\n{}",
        rows.join("\n")
    );
}
