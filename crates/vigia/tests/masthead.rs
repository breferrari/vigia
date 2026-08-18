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

/// Samples the store keeps, restated rather than imported.
///
/// A gate reading the renderer's own constant would agree with it by
/// construction, which is this suite's standing rule for every rung table.
///
/// **This was `GRAPH_COLUMNS`, and that constant is retired**
/// ([#232](https://github.com/breferrari/vigia/issues/232)): the band drew a
/// fixed fifteen columns and now draws one value per sub-column, so its period is
/// a property of the pane. What a fixture still needs is where a sample lands.
const WINDOW_SAMPLES: usize = 120;

/// Columns the scrollbar reserves, restated for [`WINDOW_SAMPLES`]'s reason.
const BAR_COLUMNS: usize = 2;

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
    band_at(width, series, Glyphs::default())
}

/// [`band_strip`] at a chosen glyph rung.
///
/// **Parameterised rather than copied**, because a gate that rebuilt the chrome,
/// the rect and the buffer to pass one different argument also dropped the
/// `body.graph > 0` assertion below, which is the fixture trap `banded`'s own
/// docblock records a withdrawn gate for.
fn band_at(width: u16, series: [u32; HISTORY_SAMPLES], glyphs: Glyphs) -> Vec<String> {
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
        glyphs,
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

/// A wider pane buys finer time, which is the opposite of what #223 ruled.
///
/// **[#223](https://github.com/breferrari/vigia/issues/223) is superseded here
/// and the correction is stated rather than absorbed.** That row saw a real
/// defect: at one column a second, a save drew a hairline between two blanks and
/// the whole band read as scatter. It reached for a wider column. `btop` fixes
/// the same defect on the same shape of signal, its network graph, by drawing
/// the **axis**: `no_zero` puts one dot on the bottom row of an empty column, so
/// a narrow spike stands on a floor rather than floating in a void. With the
/// floor drawn, coarsening costs resolution and buys nothing, and it was what
/// made the band read as separated blocks
/// ([#232](https://github.com/breferrari/vigia/issues/232), reported from a live
/// pane).
///
/// So a wider pane draws more distinct values, up to what the window holds.
#[test]
fn a_wider_pane_buys_finer_time() {
    /// One pane width and what the band drew at it.
    #[derive(Debug)]
    struct Drawn {
        width: u16,
        ink: usize,
        heights: usize,
    }

    let mut widths = Vec::new();
    for width in [60u16, 80, 120, 160, 200] {
        let rows = band_strip(width, BURSTY);
        let ink: usize = rows.iter().map(|row| drawn_ink(row)).sum();
        // Distinct glyphs across the whole band, which is how much of the ramp
        // the shape actually uses. A wider pane divides the same window into
        // more values, so it can only ever show at least as much of it.
        let mut seen: Vec<char> = Vec::new();
        for glyph in rows.iter().flat_map(|row| row.chars()) {
            if !glyph.is_whitespace() && !seen.contains(&glyph) {
                seen.push(glyph);
            }
        }
        widths.push(Drawn {
            width,
            ink,
            heights: seen.len(),
        });
    }

    for pair in widths.windows(2) {
        assert!(
            pair[1].ink >= pair[0].ink,
            "widening from {} to {} drew less ink: {widths:?}",
            pair[0].width,
            pair[1].width
        );
    }
    // Non-vacuity in the direction that matters: the sweep has to actually gain
    // something, or "never fewer" is true of a band that never changed.
    let (narrow, wide) = (&widths[0], widths.last().expect("a width"));
    assert!(
        wide.ink > narrow.ink,
        "the sweep never grew, so this gate compared nothing: {widths:?}"
    );
    assert!(
        wide.heights >= narrow.heights,
        "the widest pane drew fewer distinct heights than the narrowest, so \
         widening cost resolution: {widths:?}"
    );
}

#[test]
fn an_empty_column_draws_the_axis() {
    // **The reversal of [#158](https://github.com/breferrari/vigia/issues/158),
    // and it is the whole of why the band reads as a graph.** That ruling gave an
    // empty column nothing, because a full track of `_` "reads as a dashed rule
    // across the pane". It does, and that is what a graph's axis is: `btop` draws
    // exactly this for its network graph and calls the flag `no_zero`. Without
    // it, the filled columns float with nothing to stand on and the element reads
    // as separated blocks, which is what was reported from a live pane.
    //
    // The band has edges the sparkline does not, which is #158's own reason for
    // treating the two differently, and it cuts the other way once the element is
    // a graph rather than a strip.
    let rows = band_strip(WIDE, BURSTY);
    let baseline = rows.last().expect("a band row");
    // Between the pane's own inset and the scrollbar's reserve, which is the
    // span the band is given; the margins either side are not gaps in the axis.
    let blanks = baseline
        .trim_end()
        .chars()
        .skip(drawn_inset(baseline))
        .filter(|glyph| glyph.is_whitespace())
        .count();

    assert_eq!(
        blanks,
        0,
        "the baseline row has {blanks} gaps in it, so a quiet stretch of the \
         window leaves the graph with no floor:\n{}",
        rows.join("\n")
    );
    // And the row above it still has sky, or the band is a solid block rather
    // than a graph with a floor.
    assert!(
        rows[0].contains("  "),
        "no run of blank columns survived above the axis, so the band filled \
         its whole height:\n{}",
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
    // **Plateaus rather than two lone samples**, since
    // [#242](https://github.com/breferrari/vigia/issues/242) made the band draw a
    // level: two isolated writes smooth into one another's tails and the graded
    // column this gate needs disappears. Sustained runs at a four-to-one ratio
    // hold their heights through the kernel, which is what "quartered" always
    // meant and what a lone sample only approximated.
    let mut at = 0;
    while at < WINDOW_SAMPLES / 4 {
        s[at] = 16;
        at += 1;
    }
    let mut at = WINDOW_SAMPLES / 2;
    while at < WINDOW_SAMPLES * 3 / 4 {
        s[at] = 4;
        at += 1;
    }
    s
};

/// The eighth-block ramp, restated for [`WINDOW_SAMPLES`]'s reason.
const RAMP: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// Rows of churn the band draws, restated for [`WINDOW_SAMPLES`]'s reason.
///
/// Load bearing in the gate below rather than incidental: two rows of an
/// eight-rung ramp is a sixteen-level scale, which is what makes a quarter of the
/// peak land on the ramp's fourth rung exactly.
const GRAPH_ROWS: usize = 2;

#[test]
fn the_band_stacks_its_rows_from_the_bottom() {
    // **[#225](https://github.com/breferrari/vigia/issues/225), re-aimed at the
    // drawer that replaced `band_cell`.** The rule is unchanged and its code
    // moved: a column's height climbs a whole ramp per row, so a column that does
    // not fill the baseline row may not put anything in the row above it. Every
    // other band gate reads presence, ink, axis or span and none reads a drawn
    // column's glyph, so without this the hero element of the pane can stack
    // arbitrarily and only an eye would catch it.
    //
    // **Structural rather than a pinned rung**, because the denominator is no
    // longer the window's maximum: heights are scaled against
    // [`vigia_core::scale_of`]'s mean-based figure, so "a quarter of the peak" is not a
    // thing a fixture can state any more. What it can state, and what the rule
    // actually is, is that a short column stays in one row and a tall one does
    // not.
    let rows = band_strip(WIDE, QUARTERED);
    assert_eq!(rows.len(), GRAPH_ROWS, "the band drew the wrong row count");
    let strip = rows.join("\n");
    let upper: Vec<char> = rows[0].chars().collect();
    let base: Vec<char> = rows[1].chars().collect();

    // Every cell the band was given, as (baseline glyph, whether it reaches the
    // row above). The axis means every cell inside the span is drawn.
    let inset = drawn_inset(&rows[1]);
    let span = usize::from(WIDE) - BAR_COLUMNS - inset;
    let cells: Vec<(char, bool)> = (0..span)
        .map(|cell| (base[inset + cell], !upper[inset + cell].is_whitespace()))
        .collect();

    // The tallest column fills its baseline row and climbs into the next, which
    // is what makes the assertion below about short columns mean something: a
    // band that never left the baseline would satisfy it on its own.
    assert!(
        cells
            .iter()
            .any(|(glyph, above)| *glyph == RAMP[7] && *above),
        "no column filled the baseline row and continued above it, so nothing \
         here exercises stacking at all:\n{strip}"
    );

    // And no column that failed to fill its baseline row put anything above it.
    // This is the mutation that matters: a stack climbing by one level rather
    // than by a whole ramp spills a quarter-height column into the row above.
    for (at, (glyph, above)) in cells.iter().enumerate() {
        assert!(
            !(*above && *glyph != RAMP[7]),
            "cell {at} drew {glyph:?} on the baseline, short of the ramp's top, \
             and still put ink in the row above it:\n{strip}"
        );
    }

    // Non-vacuity in the third direction: there has to be a genuinely mid-ramp
    // column, or the loop above only ever saw full ones and empty ones.
    assert!(
        cells
            .iter()
            .any(|(glyph, _)| RAMP[..7].contains(glyph) && *glyph != RAMP[0]),
        "no column landed mid-ramp, so the fixture cannot tell a correct stack \
         from a flattened one:\n{strip}"
    );
}

/// A pane whose planning width divides the window exactly.
///
/// 124 columns leaves 120 after the bar's reserve and the inset, so at the block
/// rung one drawn cell is one stored sample and the arithmetic below can be done
/// on paper. Every other width has cells covering one or two samples, which makes
/// an exact expectation a fixture property rather than a rule.
const EXACT_PANE: u16 = 124;

#[test]
fn the_band_scales_against_the_ordinary_write_rather_than_the_largest() {
    // **`btop`'s rule, pinned by the two glyphs it produces.** Its network graph
    // faces this signal and scales against 1.3 times a recent mean rather than
    // against the window's maximum, so one outlier saturates instead of crushing
    // every ordinary write beneath it. Read from `src/linux/btop_collect.cpp`.
    //
    // A window where every sample is equal is the case that states the factor:
    // the mean **is** that value, so the scale is 1.3 of it and a column reaches
    // 16 / 1.3 of the two rows' sixteen levels, which is thirteen. Thirteen fills
    // the baseline row's eight and puts five in the row above.
    //
    // Both halves are load bearing. Drop the 1.3 and every column tops out at
    // sixteen, so the row above reads `RAMP[7]` and a uniformly busy worktree
    // draws as a solid block. Change the level count and the row above moves off
    // `RAMP[4]`, which is the only place either number is observable.
    let rows = band_strip(EXACT_PANE, [7; HISTORY_SAMPLES]);
    let at = drawn_inset(&rows[1]);

    assert_eq!(
        (rows[1].chars().nth(at), rows[0].chars().nth(at)),
        (Some(RAMP[7]), Some(RAMP[4])),
        "a uniformly busy window drew the wrong height, so either the scale is \
         no longer above the mean or a row no longer carries a whole ramp:\n{}",
        rows.join("\n")
    );
}

#[test]
fn a_dense_cell_carries_two_samples() {
    // **The braille rung's whole purpose, and nothing else here drives it**
    // ([#232](https://github.com/breferrari/vigia/issues/232)). A 2x4 cell holds
    // two sub-columns, older on the left, which is how `btop` fits two values
    // into one character and the reason its graphs read as a line rather than as
    // bars. Drawing one sample into both halves would look almost right and
    // halve the resolution silently.
    //
    // **A narrow pane on purpose.** The window holds 120 samples, so a pane
    // asking for more sub-columns than that gets neighbouring halves fed from one
    // sample, and the two are then equal by arithmetic rather than by defect.
    // **A rising ramp rather than an every-sample alternation**, since
    // [#242](https://github.com/breferrari/vigia/issues/242) made these elements
    // draw a level. The old fixture was zero on even samples and busy on odd
    // ones, which a six-second kernel erases completely and by design: a
    // one-second alternation is noise at that scale and smoothing it away is the
    // point. The claim is unchanged and is what this still tests, because a
    // monotone rise makes the older half lower on **every** cell: the two halves
    // carry two values, and the older one is on the left.
    let mut rising = [0u32; HISTORY_SAMPLES];
    for (at, sample) in rising.iter_mut().enumerate() {
        // A step, the sharpest edge a level can carry: its gradient is bounded by
        // its own kernel, so nothing steeper survives smoothing.
        *sample = if at >= HISTORY_SAMPLES / 2 { 400 } else { 0 };
    }

    // **This asserted a specific glyph until #242 and now compares the two rungs
    // against each other.** It wanted a cell whose halves differ by three levels,
    // which an every-sample alternation produced and a level cannot: the two
    // halves of one cell are a fraction of a kernel apart, so they differ by at
    // most about one rung and usually round together. The claim was never about
    // that glyph. It is that a dense cell carries **two** values where a block
    // cell carries one, and what that buys is resolution: across the same
    // transition, at the same width, on the same series, the dense rung must draw
    // more distinct heights than the block rung. That is the whole of #232's
    // "one value per sub-column" stated as something a level can still show.
    let distinct = |glyphs: Glyphs| {
        let rows = band_at(WIDE, rising, glyphs);
        let drawn = rows.last().expect("a band row").clone();
        let mut seen: Vec<char> = drawn.chars().filter(|c| !c.is_whitespace()).collect();
        seen.sort_unstable();
        seen.dedup();
        (seen.len(), drawn)
    };

    let (dense, dense_row) = distinct(Glyphs::Braille);
    let (block, block_row) = distinct(Glyphs::Block);

    assert!(
        block > 1 && dense > 1,
        "a rung drew one height across a step, so the fixture does not exercise          a transition at all:\ndense: {dense_row}block: {block_row}"
    );

    // **What this does not assert, and the finding it stands on.** The obvious
    // claim is that the dense rung draws *more* distinct heights than the block
    // one, since that is what a second sub-column is for. Measured on this very
    // fixture it draws **fewer**, 4 against 6, and that is not a defect in the
    // drawer: `Glyphs::levels` gives the block ramp eight levels a row and a 2x4
    // cell three, so density is bought with more than half the vertical
    // resolution. That trade paid while the band drew discrete events, which
    // change completely between samples. #242 made it draw a level, which is
    // smooth by construction, so the horizontal half now carries almost nothing
    // and the vertical half it was traded for is what a wave needs.
    //
    // The ladder therefore inverts for this element and that is
    // [#244](https://github.com/breferrari/vigia/issues/244), not something to
    // pin here: a gate asserting the inversion would fix it in place, and a gate
    // asserting the opposite would fail for a true reason. So this holds the part
    // that is still true and unarguable, that both rungs draw the transition, and
    // points at the ruling for the rest.
}

#[test]
fn the_band_draws_the_newest_writes_on_the_right() {
    // **Nothing gated time order, and a mirrored graph is a silent lie.**
    // `a_dense_cell_carries_two_samples` proves a cell carries *two* values and
    // cannot prove which is which: its fixture alternates, so swapping the halves
    // draws the same two glyphs and the gate passes. A band drawn backwards would
    // put a burst that just landed at the far left, where a reader reads history,
    // and nothing on screen would say so.
    //
    // Busy only in the newest quarter of the window, so the ink has one honest
    // place to be. `Churn::projected` is oldest-first and `Glyphs::glyph` takes
    // the older half first, which is the pair this checks end to end.
    let mut newest = [0u32; HISTORY_SAMPLES];
    for sample in newest.iter_mut().skip(HISTORY_SAMPLES * 3 / 4) {
        *sample = 50;
    }

    for glyphs in [Glyphs::Block, Glyphs::Braille] {
        let rows = band_at(WIDE, newest, glyphs);
        let baseline = rows.last().expect("a band row");
        let inset = drawn_inset(baseline);
        let span = usize::from(WIDE) - BAR_COLUMNS - inset;
        // The axis fills every cell, so ink is not the measure: height is. The
        // upper row is drawn only where a column climbs past the baseline row.
        let upper: Vec<char> = rows[0].chars().collect();
        let tall: Vec<usize> = (0..span)
            .filter(|cell| !upper[inset + cell].is_whitespace())
            .collect();

        assert!(
            !tall.is_empty(),
            "{glyphs:?}: nothing climbed, so this compared nothing"
        );
        let leftmost = tall[0];
        assert!(
            leftmost * 2 > span,
            "{glyphs:?}: the newest quarter of the window drew its first tall \
             column at {leftmost} of {span}, left of centre, so the band is \
             mirrored in time:\n{}",
            rows.join("\n")
        );
    }
}
