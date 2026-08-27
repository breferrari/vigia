//! I4's shape, held over the **paint** rather than over the collect.
//!
//! > Streams, never buffers. First paint is independent of total diff size.
//!
//! `reads.rs` holds the half about *files*: a screenful reads the files it draws
//! and not the rest of the worktree. This file holds the half about *lines*: a
//! drawn row costs the pane it is drawn into, not the length of the line behind
//! it. They are the same invariant one term apart, and the second half went
//! unmeasured for two phases for a reason worth stating at the top:
//!
//! **no budget gate in this repo had ever painted.** `budgets.rs` timed
//! `Frame::advance` plus `App::view` and stopped there, so `render` — and with it
//! every character walked to fill a row — sat outside both tiers. A row carrying
//! 7x more line than pane therefore passed a 16ms gate that could not see it,
//! and what found it was a reader scrolling a Japanese README
//!.
//!
//! Structural, not wall-clock: [`PaintStats`] is an exact counter, so this is
//! hardware-independent, takes no slack, and runs in every `cargo test`. The
//! bound is derived from the run — rows drawn times the pane's width — rather
//! than written as a constant, because a constant is a bound no input can
//! approach and an assertion against one is documentation.
//!
//! **Why the fixture has to be Japanese.** Over ASCII a character is a column, so
//! "bounded by the pane" and "unbounded" produce the *same* count and no fixture
//! made of `generated()` can tell them apart. The gap only opens where a
//! character is two columns wide or where a line is longer than a screen, and the
//! wide fixture is both.

#[path = "../../vigia-core/tests/support/mod.rs"]
mod support;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use vigia::{
    Action, App, Chrome, Glyphs, PaintStats, Pointing, Row, Theme, View, WHEEL_ROWS, body_layout,
    render,
};
use vigia_core::{Highlighter, History};

use support::{
    PROSE_SPANS, Scratch, WIDE_EXT, WIDE_UNIT_CHARS, WIDE_UNIT_COLUMNS, WIDE_UNITS,
    WIDE_UNPARSED_EXT, prose_generated, wide_generated,
};

/// Files in the fixtures here. Small: these gates are about one row's cost, and
/// the file count is `reads.rs`'s axis rather than this one.
const FILES: usize = 3;
/// Lines per file, comfortably taller than any pane below.
const LINES: usize = 60;

/// The mark the renderer writes where a row runs past its edge.
///
/// Restated rather than imported, for the reason `tests/render.rs` gives: it is
/// one character of published behaviour, and a test sharing the constant would
/// agree with the renderer by construction.
const CONTINUES: char = '›';

/// One screenful, and everything a gate here asks about it.
///
/// A struct rather than a tuple because the fourth member is what lets the
/// grammarless baseline use this helper at all: it needs the highlighter's
/// counters, and restating the setup to reach them cost that test `painted`'s
/// own two non-vacuity assertions.
struct Painted {
    stats: PaintStats,
    highlight: vigia_core::HighlightStats,
    buf: Buffer,
    view: View,
}

/// Collect one screenful over a fixture and paint it, at `width` by `height`.
fn painted(name: &str, ext: &str, width: u16, height: u16) -> Painted {
    let scratch = Scratch::wide_lines_as(name, FILES, LINES, ext);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    assert_eq!(frame.files().len(), FILES, "fixture is not {FILES} files");

    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let area = Rect::new(0, 0, width, height);
    let chrome = app.chrome("fixture", None, Pointing::default(), 0, "");
    let screen = body_layout(area, &chrome, FILES, FILES);
    let rows = screen.diff;
    let view = app
        .view(&mut frame, &mut highlighter, &history, screen)
        .expect("view");
    assert_eq!(
        view.rows.len(),
        rows,
        "the body drew {} of {rows} rows, so this is not a full screen and \
         anything measured over it is measured over a pane nobody has",
        view.rows.len()
    );

    let mut buf = Buffer::empty(area);
    let stats = render(
        &mut buf,
        area,
        &view,
        &Theme::default(),
        Glyphs::default(),
        &chrome,
    );
    Painted {
        stats,
        highlight: highlighter.stats(),
        buf,
        view,
    }
}

/// The longest line of file content the view actually carries.
fn longest_line(view: &View) -> usize {
    view.rows
        .iter()
        .filter_map(|row| match row {
            Row::Line { text, .. } => Some(text.chars().count()),
            _ => None,
        })
        .max()
        .unwrap_or(0)
}

/// The fixture's own arithmetic, checked rather than trusted.
///
/// Every number the gates below report is relative to the shape `wide_line`
/// documents, and that shape is three string literals in a support module: an
/// editor that reflows one of them, or a `git` config that rewrites the line
/// endings, would leave every assertion here passing against a fixture that no
/// longer has the property they are about.
#[test]
fn the_wide_fixture_is_the_shape_it_says_it_is() {
    let text = wide_generated(1, "after");
    let line = text.strip_suffix('\n').expect("a line ending");

    let chars = line.chars().count();
    let columns = ratatui::text::Span::raw(line).width();

    // The prefix is `1. after: `, so 10 characters and 10 columns on top of the
    // units. Named as an expression rather than as a total, so a change to the
    // prefix reads as a change to the prefix.
    let prefix = "1. after: ".chars().count();
    assert_eq!(
        chars,
        prefix + WIDE_UNITS * WIDE_UNIT_CHARS,
        "a wide line is {chars} characters, not the {} its own documentation \
         claims",
        prefix + WIDE_UNITS * WIDE_UNIT_CHARS
    );
    assert_eq!(
        columns,
        prefix + WIDE_UNITS * WIDE_UNIT_COLUMNS,
        "a wide line is {columns} columns, not the {} its own documentation \
         claims",
        prefix + WIDE_UNITS * WIDE_UNIT_COLUMNS
    );

    // And the property the whole file rests on: more columns than characters.
    // Over an ASCII fixture these are equal, and a bound written in one is
    // indistinguishable from a bound written in the other.
    assert!(
        columns > chars,
        "a wide line is {columns} columns over {chars} characters, so this \
         fixture is not wide and nothing below can tell a column bound from a \
         character bound"
    );
}

/// The prose fixture's own shape, checked rather than trusted, for the reason
/// the wide one is: every number the #261 gates report is relative to it.
///
/// **This one guards a steeper cliff than the wide fixture's.** A wide line that
/// lost a unit would weaken its gate proportionally. A prose line that loses two
/// code spans weakens its gate by roughly fifty times, because the cost is
/// exponential in the span count, and nothing else in the suite would notice: the
/// content still reads as prose, the file still resolves as Markdown, and every
/// downstream assertion still passes.
#[test]
fn the_prose_fixture_is_the_shape_it_says_it_is() {
    let text = prose_generated(1, "after");
    // `prose_generated` emits one line per *paragraph*, so each entry carries a
    // blank line after it. Trim the lot rather than one newline, and assert the
    // separation separately below, since it is part of the shape.
    let line = text.trim_end();
    assert!(
        text.ends_with("\n\n"),
        "a prose entry does not end in a blank line, so the fixture is one \
         continuous paragraph and only its first row reaches Markdown's \
         block-start lookahead: eleven such rows measured cheaper in the frame \
         (15.30ms) than a single one parsed alone (16.88ms)"
    );

    let spans = line.matches('`').count();
    assert_eq!(
        spans,
        PROSE_SPANS * 2,
        "a prose line carries {} backticks, so it has {} code spans rather than \
         the {PROSE_SPANS} it claims, and the cost this fixture exists to \
         measure is exponential in that number",
        spans,
        spans / 2,
    );

    // The property the #261 gates rest on, and the one an innocent edit removes.
    // A pipe anywhere on the line reaches Markdown's table-row test on its
    // merits, so the guard lets it through and the fixture silently stops
    // exercising the path it was built for.
    assert!(
        !line.contains('|'),
        "a prose line contains a pipe: {line:?}. The whole fixture is lines that \
         can never be a table row, so a pipe here makes it ordinary content"
    );
}

#[test]
fn a_drawn_row_costs_the_pane_rather_than_the_line() {
    let width = 80u16;
    let painted = painted("paint-pane", WIDE_EXT, width, 24);
    let (stats, view) = (painted.stats, &painted.view);

    // Non-vacuity first, because the bound below is trivially satisfied by a
    // screen with nothing on it, by a counter nobody increments, and by a
    // fixture whose lines fit. The middle one is the easiest to lose: `examined`
    // is counted inside the walk, and a counter left at zero would make this
    // gate pass loudest exactly when the walk is worst.
    assert!(stats.rows > 0, "no content rows were drawn");
    assert!(
        stats.examined > 0,
        "{} rows were drawn and no characters counted, so the paint counter is \
         not being fed and the bound below holds for a reason that is not the \
         code",
        stats.rows
    );
    assert!(
        longest_line(view) > usize::from(width),
        "the longest drawn line is {} characters against an {width}-column \
         pane, so this fixture never exceeds the pane and the bound below \
         cannot fail",
        longest_line(view)
    );

    let bound = stats.rows * u64::from(width);
    assert!(
        stats.examined <= bound,
        "a {}-row body examined {} source characters against the {bound} an \
         {width}-column pane can show, which is {:.1}x: a drawn row is costing \
         its whole line rather than the pane it is drawn into",
        stats.rows,
        stats.examined,
        stats.examined as f64 / bound as f64
    );
}

#[test]
fn the_paint_narrows_with_the_pane() {
    // Two widths rather than one, and the second is not decoration: a bound
    // hardcoded to any single number satisfies a one-width gate exactly as well
    // as the real thing. What distinguishes them is that the count *moves* with
    // the pane.
    //
    // Forty is I6's floor and two hundred is a wide terminal, so the two are far
    // enough apart that the gutter and sigil, which differ between them by a
    // column or two, cannot account for the gap.
    let narrow = painted("paint-narrow", WIDE_EXT, 40, 24).stats;
    let wide = painted("paint-wide", WIDE_EXT, 200, 24).stats;

    // Per row, not in total. The two panes do not draw the same number of rows:
    // at forty columns the footer takes a second line (I6), so the body is one
    // row shorter. Comparing totals would let a real regression hide behind that
    // one row, and comparing at one width would not be this gate at all.
    assert!(narrow.rows > 0 && wide.rows > 0, "a pane drew no content");
    let per_narrow = narrow.examined / narrow.rows;
    let per_wide = wide.examined / wide.rows;
    assert!(
        per_wide > per_narrow,
        "a 200-column pane examined {per_wide} characters a row and a \
         40-column pane examined {per_narrow}, so the paint is not following \
         the pane at all"
    );
    assert!(
        narrow.examined <= narrow.rows * 40,
        "a {}-row body examined {} characters in a 40-column pane",
        narrow.rows,
        narrow.examined
    );
    assert!(
        wide.examined <= wide.rows * 200,
        "a {}-row body examined {} characters in a 200-column pane",
        wide.rows,
        wide.examined
    );
}

#[test]
fn a_clipped_wide_row_still_says_it_continues() {
    // The failure a bound introduces if it is written carelessly: stop walking at
    // the pane's edge and the renderer no longer knows there was more, so a
    // clipped row draws as one that simply ended. That is worse than the cost it
    // was fixing, because a reader cannot see it.
    //
    // Swept across widths rather than checked at one, because the case that
    // breaks is a two-column glyph landing on the last cell, and which width
    // does that depends on the gutter.
    let buf = painted("paint-mark", WIDE_EXT, 80, 24).buf;
    let area = *buf.area();

    // **The content's last column, which is not always the pane's.** `SPEC.md`
    // §11.1 gives the diff region a scrollbar, and it takes the rightmost column
    // whenever there is anywhere to scroll. Reading the pane's edge unconditionally
    // would then be reading the bar and reporting every row as unmarked, which is
    // this gate failing for a reason that has nothing to do with what it asserts.
    //
    // The last three columns rather than a computed one: which of them holds the
    // mark depends on whether the bar is drawn, and the bar reserves a gap before
    // itself so the mark can be two columns in. Recomputing that here would be
    // restating the renderer's own rule instead of checking its output.
    let mut marked = 0usize;
    for y in 1..area.height.saturating_sub(1) {
        let tail: Vec<String> = (1..=3)
            .filter(|back| area.width >= *back)
            .map(|back| buf[(area.width - back, y)].symbol().to_owned())
            .collect();
        if tail.iter().any(|symbol| symbol == &CONTINUES.to_string()) {
            marked += 1;
        }
    }
    assert!(
        marked > 0,
        "not one of the {} body rows is marked as continuing, though every \
         drawn line is several times the pane's width",
        area.height.saturating_sub(2)
    );
}

#[test]
fn a_row_of_zero_width_characters_still_costs_the_pane() {
    // The hole a column bound leaves on its own, and the reason `printable`
    // carries a second one in characters.
    //
    // `unicode-width` measures a combining mark, a zero-width joiner, a
    // variation selector and `U+200B` as **zero columns**. A run of them never
    // advances `column`, so a bound written only in columns is satisfied
    // forever and the walk runs to the end of the line: the exact unbounded
    // shape the bound removes, still present for content that is ordinary
    // rather than hostile. Decomposed Unicode, emoji built from joiners, and
    // anything pasted out of a web page all reach it.
    //
    // Built by hand rather than from a fixture, for the reason the tab row
    // below gives: `wide_line` has none of these, and a fixture that had them
    // would be testing the fixture.
    let area = Rect::new(0, 0, 80, 6);
    let zero_width = "\u{200b}\u{200d}\u{fe0f}\u{0301}".repeat(500);
    let chars = zero_width.chars().count();
    assert_eq!(
        ratatui::text::Span::raw(&zero_width).width(),
        0,
        "the fixture is not zero-width, so it cannot defeat a column bound"
    );

    let view = View {
        rows: vec![Row::Line {
            kind: vigia_core::LineKind::Context,
            number: 1,
            text: zero_width,
            spans: Vec::new(),
            emph: Vec::new(),
        }],
        files: 1,
        ..View::default()
    };
    let chrome = App::new().chrome("fixture", None, Pointing::default(), 0, "");
    let mut buf = Buffer::empty(area);
    let stats = render(
        &mut buf,
        area,
        &view,
        &Theme::default(),
        Glyphs::default(),
        &chrome,
    );

    // A looser bound than `a_drawn_row_costs_the_pane_rather_than_the_line`'s,
    // deliberately, and the two are not in tension. That gate holds real text to
    // one character a column and the wide fixture reaches 76% of it. Nothing can
    // hold *this* row to that, because a zero-width character produces no column
    // to be bounded by: what is claimed here is only that the walk stops at a
    // constant multiple of the pane instead of running to the end of the line.
    // Restated as a literal rather than imported, for the reason `CONTINUES` is:
    // a test sharing the constant would agree with the renderer by construction.
    const CHARS_PER_COLUMN: u64 = 4;
    let bound = stats.rows * u64::from(area.width) * CHARS_PER_COLUMN;
    assert!(
        stats.examined <= bound,
        "a row of {chars} zero-width characters examined {} of them against the \
         {bound} a {}-column pane can be asked for: the column bound cannot see \
         this row, so the walk is unbounded",
        stats.examined,
        area.width
    );

    // And it has to be pressed, or the assertion above passes for a row that
    // simply ran out of characters rather than one the bound stopped.
    assert!(
        u64::try_from(chars).expect("a sane fixture") > bound,
        "the fixture is {chars} characters against a bound of {bound}, so it \
         never reaches it"
    );
}

#[test]
fn a_tab_stop_after_the_bound_still_counts_from_the_line_start() {
    // Tab stops are counted from the start of the line, across span boundaries,
    // so the counter the bound is written in terms of is the same counter tab
    // expansion reads. Getting that wrong misaligns indentation on every row of
    // a tab-indented file, and it is invisible until one is drawn.
    //
    // Built by hand rather than from a fixture: `wide_line` has no tabs, and a
    // fixture that had them would be testing the fixture.
    let area = Rect::new(0, 0, 40, 6);
    let view = View {
        rows: vec![Row::Line {
            kind: vigia_core::LineKind::Context,
            number: 1,
            text: "\tab\tcd\tefgh\tij".to_owned(),
            spans: Vec::new(),
            emph: Vec::new(),
        }],
        files: 1,
        ..View::default()
    };
    let chrome = Chrome {
        pressed: None,
        gripped: None,
        scrolling: None,
        worktree: "fixture".to_owned(),
        ..App::new().chrome("fixture", None, Pointing::default(), 0, "")
    };
    let mut buf = Buffer::empty(area);
    render(
        &mut buf,
        area,
        &view,
        &Theme::default(),
        Glyphs::default(),
        &chrome,
    );

    // `\tab\tcd\tefgh\tij` expands to four columns, `ab`, two to the next stop,
    // `cd`, two more, `efgh`, four more, `ij`: the row reads
    // `    ab  cd  efgh    ij` from the first content column.
    let row: String = (0..area.width)
        .map(|x| buf[(x, 1)].symbol().to_owned())
        .collect();
    assert!(
        row.contains("    ab  cd  efgh    ij"),
        "the tab-expanded row is {row:?}, so stops are not being counted from \
         the start of the line"
    );
}

#[test]
fn a_gesture_costs_one_screenful_however_many_events_it_arrived_as() {
    // The premise behind coalescing input, held where the cost actually is.
    //
    // A trackpad reports one flick as a stream of scroll events. The shell drains
    // them and paints once (`drain` in `lib.rs`), and this is why that is worth
    // doing: moving the viewport is free, and *drawing* is what costs, so one
    // gesture is one screenful of work however many events carried it. Painting
    // per event walks every position in between, and over a large diff most of
    // those positions enter a hunk nothing has parsed.
    //
    // Structural rather than timed: the comparison is lines highlighted and
    // characters painted, so it holds on any machine.
    // Many short files rather than the wide fixture's usual few tall ones, and
    // that is the term that decides the whole comparison. A forward-only parse
    // is *cumulative within a hunk*: drawing row 90 of one hunk needs the ninety
    // above it whether that took one frame or thirty. The saving is entirely in
    // the hunks a batched gesture never enters, so the travel has to cross them.
    const BURST_FILES: usize = 24;
    const BURST_LINES: usize = 6;
    let scratch = Scratch::wide_lines_as("paint-burst", BURST_FILES, BURST_LINES, WIDE_EXT);
    let worktree = scratch.worktree();
    let area = Rect::new(0, 0, 80, 24);
    let notches = 100;

    let paint = |batched: bool| -> (u64, PaintStats, vigia::Position) {
        let mut frame = worktree.frame();
        frame.advance().expect("advance");
        // `past_first_paint` rather than `new`, matching `reads.rs` and
        // `viewport.rs`: a reader flicking a trackpad is by definition past the
        // plain opening frame (`Viewport::highlight`, I7), and the batched arm
        // draws exactly once, so without this its single draw *is* that frame and
        // the non-vacuity guard below fires on a run that measured nothing.
        let mut app = App::past_first_paint();
        let mut highlighter = Highlighter::eager();
        let history = History::new();
        let chrome = app.chrome("fixture", None, Pointing::default(), 0, "");
        let screen = body_layout(area, &chrome, BURST_FILES, BURST_FILES);
        let rows = screen.diff;
        let mut buf = Buffer::empty(area);
        let mut total = PaintStats::default();

        for at in 0..notches {
            app.apply(Action::Scroll(WHEEL_ROWS), &mut frame, rows)
                .expect("scroll");
            if batched && at + 1 < notches {
                continue;
            }
            let fresh = app
                .view(&mut frame, &mut highlighter, &history, screen)
                .expect("view");
            total += render(
                &mut buf,
                area,
                &fresh,
                &Theme::default(),
                Glyphs::default(),
                &chrome,
            );
        }
        // `App::view` writes the resolved top back as the position, and both
        // arms always draw on the last notch, so this is where the gesture ended
        // without carrying a `View` out of the loop to ask it.
        (highlighter.stats().lines, total, app.position())
    };

    let (each_lines, per_event, landed) = paint(false);
    let (batch_lines, once, batch_landed) = paint(true);

    assert!(
        each_lines > 0 && batch_lines > 0,
        "one of the two runs highlighted nothing, so there is nothing to compare"
    );
    assert!(
        batch_lines * 4 < each_lines,
        "drawing {notches} scroll events as one frame highlighted {batch_lines} \
         lines against {each_lines} for drawing each of them, which is not the \
         saving the drain exists for"
    );
    assert!(
        once.rows * 4 < per_event.rows,
        "one frame painted {} rows against {} for {notches} frames",
        once.rows,
        per_event.rows
    );

    // And it is the *same* gesture rather than a shorter one, which is the half
    // that makes the saving legitimate: coalescing the paint must not coalesce
    // the travel.
    assert_eq!(
        batch_landed, landed,
        "the batched gesture ended somewhere the per-event one did not"
    );
}

#[test]
fn an_unparsed_extension_costs_no_parse() {
    // The baseline the wall-clock gates attribute the parse by subtracting. It
    // is only a baseline while `syntect` really has no grammar for the
    // extension, and that is a property of a dependency rather than of this
    // repo, so it is asserted rather than assumed: the day a grammar appears
    // this fails instead of quietly becoming a second measurement of the same
    // thing.
    // Through the same helper as every gate above, so the baseline is the
    // measurement they take minus one term rather than a second setup that
    // happens to resemble it. It also inherits `painted`'s own two non-vacuity
    // assertions, which a hand-rolled copy of it silently did without.
    let plain = painted("paint-unparsed", WIDE_UNPARSED_EXT, 80, 24);

    assert_eq!(
        plain.highlight.lines, 0,
        "{} lines were highlighted over `.{WIDE_UNPARSED_EXT}`, so it is no \
         longer a zero-parse baseline and the attribution that subtracts it is \
         measuring a difference of two parses",
        plain.highlight.lines
    );

    // And it still draws, which is what makes it the *same* measurement minus
    // one term rather than a different one.
    assert!(
        plain.stats.rows > 0 && plain.stats.examined > 0,
        "the unparsed fixture drew nothing, so it is not a baseline for \
         anything"
    );
}
