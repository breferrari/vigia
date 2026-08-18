//! The masthead's default and its one gesture.
//!
//! `SPEC.md` §11.1 rules the worktree churn band **hidden until a reader asks
//! for it**, which is [#204](https://github.com/breferrari/vigia/issues/204)
//! reversing the default the toggle shipped under.
//!
//! > `m` shows the masthead and hides it again, and it starts hidden.
//!
//! A separate binary from `render.rs` for the reason that file's own header
//! gives about `input.rs`: the question is different. `render.rs` builds a
//! [`vigia::Chrome`] by hand and asks what the drawer does with one, which is
//! exactly what cannot answer this, because a hand-built chrome says whatever
//! the test wrote in it. What is gated here is the **shipped** answer: the
//! chrome an untouched [`App`] produces, and what `m` does to it.
//!
//! That pair is worth its own file now rather than a line in another because of
//! which way the default points. The toggle's app side had no gate at all when
//! it landed, and while the band was drawn by default a broken `m` cost a reader
//! four rows they could not get back. It now costs them the element entirely.

#[path = "../../vigia-core/tests/support/mod.rs"]
mod support;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use vigia::{
    Action, App, Chrome, FileEntry, Glyphs, HEAT_BUCKETS, HeatBucket, Row, Theme, View,
    body_layout, render,
};
use vigia_core::{Churn, HISTORY_BUCKETS, HISTORY_SAMPLES, Highlighter, History, Recency};

use support::{Scratch, materialise};

/// A pane with room to spare, so nothing here is answered by a floor.
///
/// `GRAPH_KEEP` and `GRAPH_FLOOR` both decline the band on a pane that cannot
/// afford it, and a gate written at either edge would pass against a build that
/// had no default at all. Every assertion below is made where the band **is**
/// affordable, and `the_shipped_shell_starts_with_the_band_hidden` says so in an
/// assertion rather than in this comment.
const WIDE: u16 = 80;
const TALL: u16 = 24;

/// Columns of churn the band draws, restated rather than imported.
///
/// A gate reading the renderer's own constant would agree with it by
/// construction, which is this suite's standing rule for every rung table.
const GRAPH_COLUMNS: usize = 15;

/// Columns the scrollbar reserves, restated for [`GRAPH_COLUMNS`]'s reason.
const BAR_COLUMNS: usize = 2;

/// Runs of one glyph in a drawn row, which is how many bars it carries.
///
/// **Walked rather than windowed.** The first version used `windows(2)`, which
/// has no window before the first character and therefore under-counted a bar
/// starting at column zero; it passed only because the pane's inset happens to
/// blank that column, which is a property of the fixture rather than of the
/// band. It also collected into a `Vec` to get the windows at all.
fn drawn_runs(row: &str) -> usize {
    let mut runs = 0;
    let mut previous = ' ';
    for glyph in row.chars() {
        if glyph != previous && !glyph.is_whitespace() {
            runs += 1;
        }
        previous = glyph;
    }
    runs
}

/// Cells of any glyph in a drawn row.
fn drawn_ink(row: &str) -> usize {
    row.chars().filter(|glyph| !glyph.is_whitespace()).count()
}

/// Blank columns before a drawn row's first glyph, which is the pane's inset.
///
/// **Named because two gates recover it and one of them had hand-ported the
/// other.** What counts as blank is one definition, and a copy adapted from
/// `&str` to `&[char]` is the surface it drifts across: change it in the gate
/// that reads the band's left edge and the gate that locates a column by index
/// keeps the old answer, reading the wrong cell without failing.
fn drawn_inset(row: &str) -> usize {
    row.chars()
        .take_while(|glyph| glyph.is_whitespace())
        .count()
}

/// Changed files in every fixture here, which is what `assets/preview.svg` draws.
const FILES: usize = 3;

fn area() -> Rect {
    Rect::new(0, 0, WIDE, TALL)
}

fn chrome(app: &App) -> Chrome {
    app.chrome("fixture", None, None, None, None, None)
}

#[test]
fn the_shipped_shell_starts_with_the_band_hidden() {
    // **The default, read off the chrome the shell actually publishes** rather
    // than off the field, because the field is private and the chrome is what
    // every drawer sees.
    let shipped = chrome(&App::new());
    assert!(
        !shipped.masthead,
        "a shell nobody has pressed a key on published a masthead"
    );

    // And the default reaches the **layout**, which is the half a boolean cannot
    // prove: the rows are the cost, and a default that never got as far as
    // `Body::split` would leave them reserved and blank.
    let hidden = body_layout(area(), &shipped, FILES);
    assert_eq!(
        (hidden.graph, hidden.air),
        (0, 0),
        "the shipped default still reserved masthead rows"
    );

    // **Not vacuous**, and this is the assertion that makes it so. The same pane
    // asked for a band draws one, so what the two above measured is the default
    // rather than a pane too small to carry the element at all.
    let asked = body_layout(
        area(),
        &Chrome {
            masthead: true,
            ..shipped
        },
        FILES,
    );
    assert!(
        asked.graph > 0,
        "the fixture pane cannot draw a band at any setting, so nothing above is a gate"
    );
}

#[test]
fn m_shows_the_band_and_hides_it_again() {
    // **The only way to the element now.** With the default hidden there is no
    // config file, no flag and nothing that persists between runs, so a `m` that
    // stopped flipping the state would put the band out of reach with every
    // other gate in the suite staying green: they all build their own chrome.
    let scratch = Scratch::large_diff("masthead-toggle", FILES, 20);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);

    let mut app = App::new();
    // The diff's height, which is what `App::apply` takes and what a scroll
    // would be clamped against. The masthead's own arm reads neither, and this
    // gate deliberately hands it the real number anyway rather than a zero:
    // an arm that grew a dependency on either should be caught by the gate that
    // presses the key, not by the next reader.
    let height = body_layout(area(), &chrome(&app), FILES).diff;

    assert!(!chrome(&app).masthead, "the shell did not start hidden");

    for press in 1..=4 {
        let running = app
            .apply(Action::ToggleMasthead, &mut frame, height)
            .expect("m");
        assert!(running, "press {press}: m asked the shell to quit");

        let shown = press % 2 == 1;
        let now = chrome(&app);
        assert_eq!(
            now.masthead,
            shown,
            "press {press}: the band should be {}",
            if shown { "drawn" } else { "gone" }
        );

        // Through the layout as well as through the flag, and on **every** press
        // rather than at the end: a toggle that flipped a boolean the split had
        // stopped reading would satisfy a flag-only gate on all four.
        let body = body_layout(area(), &now, FILES);
        assert_eq!(
            body.graph > 0,
            shown,
            "press {press}: the layout disagrees with the state"
        );
    }
}

#[test]
fn the_branch_stays_on_a_pane_with_no_masthead() {
    // **What keeps the every-frame `.git/HEAD` read honest.** `Shell::paint`
    // reads the branch on every frame under the rule *never touch a file the
    // frame does not draw*, and what satisfies that rule is the header's ladder
    // rather than the masthead: #158 moved the branch to the header, and #204
    // makes the difference load bearing, since the masthead is now absent unless
    // a reader asks for it. A branch that had stayed up there would make most
    // frames read a file they draw nothing from.
    let scratch = Scratch::large_diff("masthead-branch", FILES, 20);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);

    const BRANCH: &str = "some-branch";

    let mut app = App::new();
    let mut highlighter = Highlighter::new();
    let history = History::new();
    let drawn = app.chrome("fixture", Some(BRANCH), None, None, None, None);
    assert!(
        !drawn.masthead,
        "the fixture asked for a masthead, so this proves nothing about a pane without one"
    );

    let body = body_layout(area(), &drawn, FILES);
    let view = app
        .view(&mut frame, &mut highlighter, &history, body)
        .expect("view");
    let mut buf = Buffer::empty(area());
    render(
        &mut buf,
        area(),
        &view,
        &Theme::default(),
        Glyphs::default(),
        &drawn,
    );

    let header: String = (0..WIDE).map(|x| buf[(x, 0)].symbol()).collect();
    assert!(
        header.contains(BRANCH),
        "a pane with the masthead hidden drew no branch: {header:?}"
    );
}

/// A view whose band has a known series, drawn on a pane that allocates one.
///
/// **The pinned list is what allocates the band**, and that is the fixture trap
/// worth recording rather than rediscovering: `render` clamps the body to
/// `view.list.len()`, so a view carrying diff rows and an empty list draws no
/// masthead however tall the pane or however true the flag. An earlier attempt
/// at a band gate was withdrawn for exactly this, having read the file list's
/// sparkline and concluded the band drew nothing.
fn banded(series: [u32; HISTORY_SAMPLES]) -> View {
    // **One list row and nothing else specified**, because the band draws above
    // the list and no gate here reads a glance element. The two sibling fixtures
    // in `tests/render.rs` and `tests/legibility.rs` are empty in the same
    // fields for the same reason.
    let entry = FileEntry {
        path: "crates/vigia/src/render.rs".to_owned(),
        from: None,
        kind: 'M',
        churn: None,
        spark: [0; HISTORY_BUCKETS],
        recency: Recency::Cold,
        heat: [HeatBucket::default(); HEAT_BUCKETS],
    };
    View {
        list: vec![entry.clone()],
        rows: vec![Row::File(entry)],
        files: 1,
        worktree_churn: Churn(series),
        ..View::default()
    }
}

/// The band's own rows, drawn at `width`, as strings.
///
/// **Not `band_rows`, which this crate already uses for something else**:
/// `Body::band_rows` returns a row *count*. One grep, two meanings, and the
/// count is the older claim.
fn band_strip(width: u16, series: [u32; HISTORY_SAMPLES]) -> Vec<String> {
    let shown = Chrome {
        masthead: true,
        ..chrome(&App::new())
    };
    let area = Rect::new(0, 0, width, TALL);
    let body = body_layout(area, &shown, 1);
    assert!(
        body.graph > 0,
        "the fixture drew no band at {width} columns, so the gate would prove \
         nothing"
    );
    let mut buf = Buffer::empty(area);
    render(
        &mut buf,
        area,
        &banded(series),
        &Theme::default(),
        Glyphs::default(),
        &shown,
    );
    let top = 1 + body.lead as u16;
    (top..top + body.graph as u16)
        .map(|y| {
            (0..width)
                .map(|x| buf[(x, y)].symbol().to_owned())
                .collect::<String>()
        })
        .collect()
}

/// A worktree written in bursts, which is the shape #223 was reported on.
const BURSTY: [u32; HISTORY_SAMPLES] = {
    let mut s = [0; HISTORY_SAMPLES];
    s[8] = 6;
    s[26] = 9;
    s[55] = 2;
    s[70] = 11;
    s[98] = 5;
    s[115] = 8;
    s
};

#[test]
fn a_wider_pane_buys_wider_bars_and_not_finer_time() {
    /// One pane width and what the band drew at it.
    #[derive(Debug)]
    struct Drawn {
        width: u16,
        ink: usize,
        bars: usize,
    }

    // **The defect, stated as a property.** `Churn::projected` clamped the drawn
    // width to the sample count, so past 120 columns one column was one second
    // and a save drew a hairline between two blanks. Widening must buy bar
    // width; the time resolution is the element's own and does not move.
    let mut widths = Vec::new();
    for width in [60u16, 80, 120, 160, 200] {
        let rows = band_strip(width, BURSTY);
        // Summed over every row the band was given rather than two named ones,
        // which restated `GRAPH_ROWS` silently.
        let ink: usize = rows.iter().map(|row| drawn_ink(row)).sum();
        let bars = rows.iter().map(|row| drawn_runs(row)).max().unwrap_or(0);
        widths.push(Drawn { width, ink, bars });
    }

    // Ink grows with the pane.
    for pair in widths.windows(2) {
        assert!(
            pair[1].ink >= pair[0].ink,
            "widening from {} to {} drew less ink: {widths:?}",
            pair[0].width,
            pair[1].width
        );
    }
    // And the column count never exceeds the element's own resolution, however
    // wide the pane gets. This is what stops a wide pane sampling per second.
    for drawn in &widths {
        assert!(
            drawn.bars <= GRAPH_COLUMNS,
            "at {} columns the band drew {} bars, past its own resolution",
            drawn.width,
            drawn.bars
        );
    }
    // Non-vacuity: the widest pane must actually be wider in ink than the
    // narrowest, or "never exceeds" is true of a band that never grew.
    assert!(
        widths.last().expect("a width").ink > widths[0].ink,
        "the sweep never grew, so this gate compared nothing: {widths:?}"
    );
}

#[test]
fn a_burst_and_its_neighbour_land_in_one_column() {
    // Two writes six seconds apart are one burst, and at the old one-second
    // columns they drew as two separate spikes with five blanks between them.
    let mut near = [0u32; HISTORY_SAMPLES];
    near[40] = 5;
    near[46] = 5;
    let rows = band_strip(WIDE, near);
    let bars = rows[1]
        .split(|c: char| c.is_whitespace())
        .filter(|run| !run.is_empty())
        .count();
    assert_eq!(
        bars,
        1,
        "two writes six seconds apart drew {bars} bars rather than one:\n{}",
        rows.join("\n")
    );
}

#[test]
fn an_empty_column_still_draws_nothing() {
    // #158, unchanged by the coarser period and worth a gate precisely because
    // coarser columns could look like an excuse to revisit it.
    let rows = band_strip(WIDE, BURSTY);
    assert!(
        rows.iter().any(|row| row.contains("  ")),
        "no run of blank columns survived, so the band filled its gaps:\n{}",
        rows.join("\n")
    );
}

#[test]
fn the_band_reaches_both_edges_of_its_slot() {
    // §5.3: furniture runs full bleed. The span arithmetic distributes the
    // remainder rather than leaving a ragged tail, so a width that does not
    // divide by the column count still reaches the last cell.
    let full = [7u32; HISTORY_SAMPLES];
    for width in [60u16, 83, 120, 137] {
        let rows = band_strip(width, full);
        let widest = rows
            .iter()
            .map(|row| row.trim_end().chars().count())
            .max()
            .unwrap_or(0);
        // **Exact rather than within a fudge**, which is what `+ 2` was: the
        // last column's span ends at the width itself, so the band stops
        // precisely where the scrollbar's reserve begins. Counted in `char`s on
        // both sides, since a byte length would disagree the moment a glyph is
        // not ASCII.
        let inset = drawn_inset(rows.last().expect("a band row"));
        assert_eq!(
            widest,
            usize::from(width) - BAR_COLUMNS,
            "at {width} columns the band did not end at the bar's reserve,              inset {inset}"
        );
    }
}

/// Two columns of known height against one peak, which is what #225 needed.
///
/// **Mixed rather than uniform, and that is the whole point of the fixture.**
/// The gate withdrawn during [#159](https://github.com/breferrari/vigia/issues/159)
/// used a series that was full everywhere, where every row sits at its ceiling
/// and flattening the stack changes nothing, so it passed against the very
/// mutation it was written for.
///
/// `Churn::projected` maps a column onto its own share of the samples, so one
/// sample per column is enough to set that column's total: the first is the peak
/// and the second is a **quarter** of it. The index is that share rather than the
/// eight it happens to be, since a change to either constant would otherwise
/// slide the quarter into a neighbouring column and leave the fixture reading a
/// bar it was not written for.
const QUARTERED: [u32; HISTORY_SAMPLES] = {
    let mut s = [0; HISTORY_SAMPLES];
    s[0] = 16;
    s[HISTORY_SAMPLES / GRAPH_COLUMNS] = 4;
    s
};

/// The eighth-block ramp, restated for [`GRAPH_COLUMNS`]'s reason.
const RAMP: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// Rows of churn the band draws, restated for [`GRAPH_COLUMNS`]'s reason.
///
/// Load bearing in the gate below rather than incidental: two rows of an
/// eight-rung ramp is a sixteen-level scale, which is what makes a quarter of the
/// peak land on the ramp's fourth rung exactly.
const GRAPH_ROWS: usize = 2;

#[test]
fn the_band_stacks_its_rows_from_the_bottom() {
    // **[#225](https://github.com/breferrari/vigia/issues/225).** Every other
    // band gate reads presence, row count, run count or blankness, and none of
    // them reads what a drawn column's glyph *is*, so the hero element of the
    // pane could stack arbitrarily and only an eye would catch it. Three
    // mutations survived the whole suite on that.
    let rows = band_strip(WIDE, QUARTERED);
    // The precondition the rungs below are derived from, asserted rather than
    // assumed: a third row would make a quarter of the peak level six of
    // twenty-four and every expected glyph here wrong.
    assert_eq!(rows.len(), GRAPH_ROWS, "the band drew the wrong row count");
    let strip = rows.join("\n");
    // `band` draws bottom up, so `row` counts from the baseline while the buffer
    // counts down from the top: the last string is the baseline.
    let upper: Vec<char> = rows[0].chars().collect();
    let base: Vec<char> = rows[1].chars().collect();

    // Derived from the drawn row rather than restated, the way
    // `the_band_reaches_both_edges_of_its_slot` already derives it: the band
    // runs from the pane's inset to where the scrollbar's reserve begins.
    let span = usize::from(WIDE) - BAR_COLUMNS - drawn_inset(&rows[1]);
    let at = |column: usize| drawn_inset(&rows[1]) + column * span / GRAPH_COLUMNS;

    // The peak column fills both rows, which is what makes the assertions below
    // about the quarter column mean something: a band that drew nothing at all
    // would satisfy "the row above is blank" on its own. Compared as a pair so a
    // failure prints both halves rather than stopping at the first.
    assert_eq!(
        (base[at(0)], upper[at(0)]),
        (RAMP[7], RAMP[7]),
        "the tallest column did not fill both rows:\n{strip}"
    );

    // A quarter of the peak over two rows of an eight-rung ramp is level four of
    // sixteen, so it draws the ramp's fourth rung on the baseline and **nothing
    // at all** above it. Both halves are load bearing: the glyph catches a ramp
    // shifted by one, and the blank catches a stack that climbs by a level
    // instead of by a whole ramp.
    assert_eq!(
        base[at(1)],
        RAMP[3],
        "a column at a quarter of the peak drew the wrong rung:\n{strip}"
    );
    assert!(
        upper[at(1)].is_whitespace(),
        "a column at a quarter of the peak spilled into the row above it:\n{strip}"
    );
}
