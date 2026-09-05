//! The masthead's default and its one gesture.

#[path = "../../vigia-core/tests/support/mod.rs"]
mod support;

/// The shell's own screen-reading selectors, under a second name because this
/// file's `support` is `vigia-core`'s repository fixture. `tests/rail.rs` carries
/// the same pair the other way round.
#[path = "support/mod.rs"]
mod screen;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use std::time::{Duration, Instant};
use vigia::{
    Action, App, Chrome, FileEntry, Glyphs, HEAT_BUCKETS, HeatBucket, Pointing, Row, Theme, View,
    body_layout, render,
};

use vigia_core::{
    Churn, HISTORY_BUCKETS, HISTORY_SAMPLE, HISTORY_SAMPLES, HISTORY_WINDOW, Highlighter, History,
    Origin, Recency,
};

use support::{Scratch, materialise};

/// A pane with room to spare, so nothing here is answered by a floor.
const WIDE: u16 = 80;

const TALL: u16 = 24;

/// Samples the store keeps, restated rather than imported.
const WINDOW_SAMPLES: usize = 120;

/// Columns the scrollbar reserves, restated for [`WINDOW_SAMPLES`]'s reason.
const BAR_COLUMNS: usize = 2;

/// How many heights a set of drawn columns takes, which is what a graph says with.
fn distinct(heights: &[usize]) -> usize {
    let mut seen = heights.to_vec();
    seen.sort_unstable();
    seen.dedup();
    seen.len()
}

/// Cells of any glyph in a drawn row.
fn drawn_ink(row: &str) -> usize {
    row.chars().filter(|glyph| !glyph.is_whitespace()).count()
}

/// Blank columns before a drawn row's first glyph, which is the pane's inset.
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
    app.chrome("fixture", None, Pointing::default(), 0, "")
}

#[test]
fn the_shipped_shell_starts_with_the_band_hidden() {
    // The default, read off the chrome the shell actually publishes rather
    // than off the field, because the field is private and the chrome is what
    // every drawer sees.
    let shipped = chrome(&App::new());
    assert!(
        !shipped.masthead,
        "a shell nobody has pressed a key on published a masthead"
    );

    // And the default reaches the layout, which is the half a boolean cannot
    // prove: the rows are the cost, and a default that never got as far as
    // `Body::split` would leave them reserved and blank.
    let hidden = body_layout(area(), &shipped, FILES, FILES);
    assert_eq!(
        (hidden.graph, hidden.air),
        (0, 0),
        "the shipped default still reserved masthead rows"
    );

    // Not vacuous, and this is the assertion that makes it so. The same pane
    // asked for a band draws one, so what the two above measured is the default
    // rather than a pane too small to carry the element at all.
    let asked = body_layout(
        area(),
        &Chrome {
            masthead: true,
            ..shipped
        },
        FILES,
        FILES,
    );
    assert!(
        asked.graph > 0,
        "the fixture pane cannot draw a band at any setting, so nothing above is a gate"
    );
}

#[test]
fn m_shows_the_band_and_hides_it_again() {
    // The only way to the element now.
    let scratch = Scratch::large_diff("masthead-toggle", FILES, 20);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);

    let mut app = App::new();
    // The diff's height, which is what `App::apply` takes and what a scroll would be
    // clamped against.
    let height = body_layout(area(), &chrome(&app), FILES, FILES).diff;

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

        // Through the layout as well as through the flag, and on every press
        // rather than at the end: a toggle that flipped a boolean the split had
        // stopped reading would satisfy a flag-only gate on all four.
        let body = body_layout(area(), &now, FILES, FILES);
        assert_eq!(
            body.graph > 0,
            shown,
            "press {press}: the layout disagrees with the state"
        );
    }
}

#[test]
fn the_branch_stays_on_a_pane_with_no_masthead() {
    // What keeps the every-frame `.git/HEAD` read honest.
    let scratch = Scratch::large_diff("masthead-branch", FILES, 20);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);

    const BRANCH: &str = "some-branch";

    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let drawn = app.chrome("fixture", Some(BRANCH), Pointing::default(), 0, "");
    assert!(
        !drawn.masthead,
        "the fixture asked for a masthead, so this proves nothing about a pane without one"
    );

    let body = body_layout(area(), &drawn, FILES, FILES);
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
fn banded(series: [u32; HISTORY_SAMPLES]) -> View {
    // One list row and nothing else specified, because the band draws above the list
    // and no gate here reads a glance element.
    let entry = FileEntry {
        origin: Origin::Unstaged,
        path: "crates/vigia/src/render.rs".to_owned(),
        from: None,
        kind: 'M',
        churn: None,
        spark: [0; HISTORY_BUCKETS],
        recency: Recency::Cold,
        newest: false,
        heat: [HeatBucket::default(); HEAT_BUCKETS],
    };
    View {
        list_span: 1,
        grouped: false,
        list: vec![entry.clone().into()],
        rows: vec![Row::file(entry)],
        files: 1,
        gutter: None,
        worktree_churn: Churn(series),
        notes: Default::default(),
        ..View::default()
    }
}

/// The band's own rows, drawn at `width`, as strings.
fn band_strip(width: u16, series: [u32; HISTORY_SAMPLES]) -> Vec<String> {
    band_at(width, series, Glyphs::default())
}

/// [`band_strip`] at a chosen glyph rung.
fn band_at(width: u16, series: [u32; HISTORY_SAMPLES], glyphs: Glyphs) -> Vec<String> {
    let shown = Chrome {
        masthead: true,
        ..chrome(&App::new())
    };
    let area = Rect::new(0, 0, width, TALL);
    let body = body_layout(area, &shown, 1, 1);
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

/// The band's period against a drawn sparkline bucket's, read off one screen.
#[test]
fn the_band_is_never_coarser_than_a_drawn_sparkline_bucket() {
    /// The sparkline's empty bucket at the block rung, restated for
    /// `WINDOW_SAMPLES`' reason.
    const TRACK: char = '_';

    let shown = Chrome {
        masthead: true,
        ..chrome(&App::new())
    };
    assert!(
        !screen::listed_files(&banded(BURSTY))
            .next()
            .expect("the fixture lists a file")
            .path
            .contains(TRACK),
        "the fixture's path carries an underscore, which this gate counts as a \
         sparkline bucket"
    );

    let mut compared = 0usize;
    for glyphs in [Glyphs::Block, Glyphs::Braille] {
        for width in 40u16..=200 {
            let area = Rect::new(0, 0, width, TALL);
            let body = body_layout(area, &shown, 1, 1);
            if body.graph == 0 {
                continue;
            }
            let mut buf = Buffer::empty(area);
            render(
                &mut buf,
                area,
                &banded(BURSTY),
                &Theme::default(),
                glyphs,
                &shown,
            );

            // The bottom band row, which is the axis and is solid since
            // #232. Every row above it is a height and carries only the columns
            // tall enough to reach it, so counting one of those would measure the
            // series rather than the graph.
            let top = 1 + body.lead as u16;
            let axis = top + body.graph as u16 - 1;
            let cells = (0..width)
                .filter(|x| !buf[(*x, axis)].symbol().trim().is_empty())
                .count();
            // The first row under the band that draws a track, which is the
            // fixture's one file: its history is empty, so its whole slot is
            // track and nothing else on that row can be.
            let empty = glyphs.glyph(0, 0).to_string();
            let Some(buckets) = (axis + 1..area.height)
                .map(|y| {
                    (0..width)
                        .filter(|x| buf[(*x, y)].symbol() == empty)
                        .count()
                })
                .find(|found| *found > 0)
            else {
                continue;
            };
            compared += 1;

            // Each side times its own element's density, which stopped cancelling on
            // 2026-08-22.
            let (columns, samples) = (cells * glyphs.density(), buckets * glyphs.density());
            assert!(
                columns >= samples,
                "{glyphs:?} at {width} columns: the band carries {columns} \
                 sub-columns where the sparkline draws {samples}, so the \
                 worktree graph is coarser than a per-file bucket and one burst \
                 can split in one element while merging in the other"
            );
        }
    }

    assert!(
        compared > 0,
        "no width drew both elements, so this gate compared nothing"
    );
}

/// A worktree written in bursts, which is the shape the hairline was reported
/// on.
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

/// A wider pane buys finer time, which is the opposite of coarsening.
#[test]
fn a_wider_pane_buys_finer_time() {
    /// One pane width and what the band drew at it.
    #[derive(Debug)]
    struct Drawn {
        width: u16,
        ink: usize,
        glyphs: usize,
    }

    let mut widths = Vec::new();
    for width in [60u16, 80, 120, 160, 200] {
        let rows = band_strip(width, BURSTY);
        let ink: usize = rows.iter().map(|row| drawn_ink(row)).sum();
        // Distinct glyphs, and this field is named for that now.
        let mut seen: Vec<char> = Vec::new();
        for glyph in rows.iter().flat_map(|row| row.chars()) {
            if !glyph.is_whitespace() && !seen.contains(&glyph) {
                seen.push(glyph);
            }
        }
        widths.push(Drawn {
            width,
            ink,
            glyphs: seen.len(),
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
        wide.glyphs >= narrow.glyphs,
        "the widest pane used less of the ramp than the narrowest, so widening \
         cost resolution: {widths:?}"
    );
}

#[test]
fn an_empty_column_draws_the_axis() {
    // The reversal of the empty-column rule, and it is the whole of why the band reads
    // as a graph.
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
        // Exact rather than within a fudge, which is what `+ 2` was: the last column's
        // span ends at the width itself, so the band stops precisely where the
        // scrollbar's reserve begins.
        let inset = drawn_inset(rows.last().expect("a band row"));
        assert_eq!(
            widest,
            usize::from(width) - BAR_COLUMNS,
            "at {width} columns the band did not end at the bar's reserve,              inset {inset}"
        );
    }
}

/// Two columns of known height against one peak.
const QUARTERED: [u32; HISTORY_SAMPLES] = {
    let mut s = [0; HISTORY_SAMPLES];
    // Plateaus rather than two lone samples, the band drawing a level: two isolated
    // writes smooth into one another's tails and the graded column this gate needs
    // disappears.
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
const GRAPH_ROWS: usize = 2;

#[test]
fn the_band_stacks_its_rows_from_the_bottom() {
    // Aimed at the drawer that replaced `band_cell`.
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
const EXACT_PANE: u16 = 124;

#[test]
fn the_band_scales_against_the_ordinary_write_rather_than_the_largest() {
    // The scale rule, pinned by the two glyphs it produces. The denominator
    // sits above the ordinary write rather than at the window's maximum, so one
    // outlier saturates instead of crushing every ordinary write beneath it.
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
fn the_band_draws_the_newest_writes_on_the_right() {
    // Nothing gated time order, and a mirrored graph is a silent lie.
    let mut newest = [0u32; HISTORY_SAMPLES];
    for sample in newest.iter_mut().skip(HISTORY_SAMPLES * 3 / 4) {
        *sample = 50;
    }

    // Both rungs, which draw the same band, kept here as the cheapest possible
    // statement of that: if the ruling is ever undone in
    // the drawer, this asserts time order on whatever replaces it rather than on
    // one rung only.
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

/// A wave, which is the shape the picture specifies and the levelling makes
/// these elements draw: monotone up then monotone down, with every sample
/// non-zero so the
/// series exercises the ramp rather than the axis.
fn wave() -> [u32; HISTORY_SAMPLES] {
    let mut series = [0u32; HISTORY_SAMPLES];
    for (at, sample) in series.iter_mut().enumerate() {
        let turn = at as f64 / HISTORY_SAMPLES as f64 * std::f64::consts::TAU;
        // Floored at one, because a level divides by its kernel's weight and a
        // zero sample is the axis rather than the ramp's bottom rung.
        *sample = (((turn.sin() + 1.0) * 200.0) as u32).max(1);
    }
    series
}

/// Every drawn sub-column's height, summed across the band's rows.
fn column_heights(width: u16, series: [u32; HISTORY_SAMPLES], pane: Glyphs) -> Vec<usize> {
    let glyphs = pane;
    let mut inverse = std::collections::HashMap::new();
    for left in 0..=glyphs.levels() {
        for right in 0..=glyphs.levels() {
            inverse.insert(glyphs.glyph(left, right), (left, right));
        }
    }

    let rows = band_at(width, series, pane);
    let cells = rows[0].chars().count();
    let mut heights = vec![0usize; cells * glyphs.density()];
    for row in &rows {
        for (cell, drawn) in row.chars().enumerate() {
            // Sky above a bar is left unwritten, which is the drawer's own rule
            // and is a height of zero rather than a glyph to look up.
            let (left, right) = if drawn.is_whitespace() {
                (0, 0)
            } else {
                *inverse.get(&drawn).unwrap_or_else(|| {
                    panic!(
                        "the band drew {drawn:?} on a {pane:?} pane, which \
                         {glyphs:?} cannot spell, so it is following the ladder \
                         again and #244 has been undone"
                    )
                })
            };
            for (sub, level) in [left, right][..glyphs.density()].iter().enumerate() {
                heights[cell * glyphs.density() + sub] += level;
            }
        }
    }
    heights
}

/// A worktree written in one burst and then edited ordinarily, which is the
/// shape the collapsing wave was reported on.
const BURST_THEN_ORDINARY: [u32; HISTORY_SAMPLES] = {
    let mut s = [0; HISTORY_SAMPLES];
    let mut at = 4;
    while at < 28 {
        s[at] = 2_400;
        at += 2;
    }
    s[58] = 190;
    s[72] = 240;
    s[88] = 150;
    s[104] = 210;
    s[116] = 170;
    s
};

/// A burst filling a third of the window, with ordinary edits after it.
const LONG_BURST_THEN_ORDINARY: [u32; HISTORY_SAMPLES] = {
    let mut s = [0; HISTORY_SAMPLES];
    let mut at = 2;
    while at < 42 {
        s[at] = 3_000;
        at += 3;
    }
    let mut at = 48;
    while at < HISTORY_SAMPLES {
        s[at] = 180;
        at += 11;
    }
    s
};

/// One loud burst does not press every ordinary write onto the floor.
#[test]
fn a_burst_does_not_press_the_ordinary_writes_onto_the_floor() {
    // Two traces, and the second is not a duplicate.
    for (name, series) in [
        ("reported", BURST_THEN_ORDINARY),
        ("long burst", LONG_BURST_THEN_ORDINARY),
    ] {
        // Both rungs, because the band follows the pane, so a braille reader's band is
        // a different picture.
        for pane in [Glyphs::Block, Glyphs::Braille] {
            let ceiling = GRAPH_ROWS * pane.levels();
            for width in [40u16, 60, 80, 109, 124] {
                let heights = column_heights(width, series, pane);

                // Non-vacuity first, and it is two claims.
                assert!(
                    heights.contains(&ceiling),
                    "{name}, {pane:?} at {width}: nothing in the trace saturated, so \
                 there is no loud write and this gate is about a fixture \
                 that cannot show the defect"
                );
                let ordinary = heights.len() * 2 / 3;
                assert!(
                    heights[ordinary..].iter().any(|height| *height > 0),
                    "{name}, {pane:?} at {width}: the newest third of the band is \
                 empty, so the ordinary writes never reached the drawn series"
                );

                // The defect itself: nothing that was written may sit on the lowest
                // level the band has.
                let floored = heights.iter().filter(|height| **height == 1).count();
                // Zero where the rung can express it, and bounded where it cannot.
                let allowed = if ceiling >= GRAPH_ROWS * RAMP.len() {
                    0
                } else {
                    heights.len() / 4
                };
                assert!(
                    floored <= allowed,
                    "{name}, {pane:?} at {width}: {floored} of {} columns are \
                     pinned on the band's lowest level where {allowed} is the \
                     most this rung's quantisation explains, so the burst \
                     pressed them onto the floor:\n{}",
                    heights.len(),
                    band_at(width, series, pane).join("\n")
                );

                // And the shape is back, not merely off the floor, which is a separate
                // claim: a band lifted off the axis and drawn flat would satisfy the
                // assertion above and still say nothing.
                let drawn = distinct(&heights);

                // Scaled to the rung, because the rungs do not offer the same number of
                // heights.
                assert!(
                    drawn * 4 > ceiling,
                    "{name}, {pane:?} at {width}: the band drew {drawn} distinct \
                 heights of a possible {ceiling}, so it is off the floor and \
                 flat instead of on it"
                );
            }
        }
    }
}

/// The band divides by the store's own figure, and by nothing else.
#[test]
fn the_band_divides_by_the_stores_own_figure() {
    let mut compared = 0usize;
    for series in [
        BURST_THEN_ORDINARY,
        LONG_BURST_THEN_ORDINARY,
        QUARTERED,
        wave(),
    ] {
        // Every rung, because the band follows the pane. Pinned to blocks
        // one rung is enough; a braille
        // reader's band is a different picture and is the one that row is about.
        for pane in [Glyphs::Block, Glyphs::Braille] {
            let ceiling = GRAPH_ROWS * pane.levels();
            for width in [40u16, 60, 80, 109, 124] {
                // `column_heights` is sized to the pane and carries the margin's
                // cells as zeroes, so the band's own span is read off the axis row,
                // which is solid, and the heights are sliced to it.
                let rows = band_at(width, series, pane);
                let axis = rows.last().expect("a band row");
                let inset = drawn_inset(axis) * pane.density();
                let slots = drawn_ink(axis) * pane.density();
                let drawn = column_heights(width, series, pane)[inset..inset + slots].to_vec();
                let scale = Churn(series).scale_at(slots);
                let values = Churn(series).levels(slots);
                let expected: Vec<usize> = values
                    .iter()
                    .map(|value| {
                        if scale == 0 || *value == 0 {
                            return 0;
                        }
                        ((u64::from(*value) * ceiling as u64).div_ceil(u64::from(scale)) as usize)
                            .clamp(1, ceiling)
                    })
                    .collect();
                assert_eq!(
                    drawn, expected,
                    "{pane:?} at {width} columns: the band drew heights that {scale} \
                 does not produce, so it is dividing by something else"
                );
                // Non-vacuity: a band of all zeroes or all ceilings would match any
                // scale of the same shape, so the fixture has to exercise the ramp.
                assert!(
                    expected.iter().any(|level| *level > 0 && *level < ceiling),
                    "{pane:?} at {width} columns: nothing landed mid-ramp, so this \
                 compared nothing that could tell two yardsticks apart"
                );
                compared += 1;
            }
        }
    }
    assert!(compared > 0, "this gate compared nothing");
}

/// The band's yardstick does not lurch when the pane is resized by a column.
///
/// The defect this catches came in with the cut itself and was found by
/// measuring rather than by a gate. The cut needs a population, and the
/// band's first
/// shape took it over the series it draws. That series is a *projection*:
/// `Churn::projected` sums where the pane holds fewer columns than the window
/// holds samples, so which values were outlying changed with the pane. A second
/// shape cut the projection against a threshold taken at source, which is a units
/// mismatch, since a drawn column is a sum of several samples and can pass a
/// threshold none of its parts would.
///
/// `Churn::scale_at` cuts the samples and projects what is left, which is the
/// order `History::repeak` takes one element over. Measured, worst single-column
/// step over widths 36 to 200:
///
/// | fixture | shipped | plain mean | cut on the projection |
/// |---|---|---|---|
/// | burst then ordinary | 6.5% | 2.9% | 41.3% |
/// | long burst | 7.3% | 2.9% | 91.4% |
/// | quartered | 14.3% | 14.3% | 14.3% |
/// | wave | 2.9% | 2.9% | 2.9% |
///
/// So the gate is two claims and neither needs a tuned number. Where nothing is
/// outlying the figure is the plain mean's exactly, at every width. Where
/// something is, the step is at most half what cutting the projection would have
/// cost, and the measurement above says the real margin is six times that.
///
/// Some movement is the projection's own and predates this row entirely: a
/// narrower column sums more samples and must be measured against more, which is
/// the whole 14.3% on `QUARTERED`.
#[test]
fn the_bands_yardstick_does_not_lurch_when_the_pane_resizes() {
    /// Thirteen tenths of the mean of the non-empty values, with no cut in it.
    fn plain(values: &[u32]) -> u32 {
        let busy: Vec<u64> = values
            .iter()
            .map(|value| u64::from(*value))
            .filter(|value| *value > 0)
            .collect();
        if busy.is_empty() {
            return 0;
        }
        u32::try_from(busy.iter().sum::<u64>() * 13 / (busy.len() as u64 * 10)).expect("a scale")
    }
    /// The most one column of resize moves a figure, as a percentage.
    fn step(before: u32, after: u32) -> f64 {
        let (low, high) = (
            f64::from(before.min(after)).max(1.0),
            f64::from(before.max(after)),
        );
        (high / low - 1.0) * 100.0
    }

    for (name, series, outlying) in [
        ("burst then ordinary", BURST_THEN_ORDINARY, true),
        ("long burst", LONG_BURST_THEN_ORDINARY, true),
        ("quartered", QUARTERED, false),
        ("wave", wave(), false),
    ] {
        let shown = Chrome {
            masthead: true,
            ..chrome(&App::new())
        };
        // Both rungs, because the band follows the pane, and a dense cell
        // resizes on a different grid: its sub-column
        // count is twice the pane's, so it crosses the window's sample count at
        // half the width blocks do.
        for pane in [Glyphs::Block, Glyphs::Braille] {
            let (mut worst, mut worst_on_projection) = (0.0f64, 0.0f64);
            let mut previous: Option<(u32, u32)> = None;
            let mut compared = 0usize;
            for width in 36u16..=200 {
                let area = Rect::new(0, 0, width, TALL);
                if body_layout(area, &shown, 1, 1).graph == 0 {
                    continue;
                }
                // The band's own span, read off the axis row, which is solid since
                // #232. Planning it from `width` would divide a projection no pane
                // produces.
                let axis = band_at(width, series, pane)
                    .last()
                    .expect("a band row")
                    .clone();
                let slots = drawn_ink(&axis) * pane.density();
                assert!(
                    slots > 0,
                    "{name}: the band drew no axis at {width} columns"
                );

                let drawn = Churn(series).levels(slots);
                // The rule's own figure, exactly. That the *drawer* uses it is
                // `the_band_divides_by_the_stores_own_figure`'s claim, made cell for
                // cell, and keeping the two apart is what lets this one be exact.
                let shipped = Churn(series).scale_at(slots);
                let unmoved = plain(&drawn);
                // What the withdrawn shape would have answered: the cut taken over
                // the drawn series rather than over the window.
                let on_projection = vigia_core::scale_of(drawn.iter().copied());

                // Where nothing is outlying, exactly the figure the plain rule
                // sets, at every width rather than at the worst of them.
                if !outlying {
                    assert_eq!(
                        shipped, unmoved,
                        "{name}: at {width} columns the cut fired on a window with \
                     nothing outlying in it"
                    );
                }

                if let Some((was, was_on_projection)) = previous {
                    // Only where the two shapes differ.
                    if outlying {
                        worst = worst.max(step(was, shipped));
                        worst_on_projection =
                            worst_on_projection.max(step(was_on_projection, on_projection));
                    }
                    compared += 1;
                }
                previous = Some((shipped, on_projection));
            }

            assert!(
                compared > 100,
                "{name}, {pane:?}: only {compared} widths compared"
            );
            // Only where the two shapes differ, and with no equality to hide
            // behind. This carried `|| worst == worst_on_projection` so that the
            // fixtures where nothing is outlying, and all three rules agree, would
            // pass. That disjunct made the gate blind to the defect it is named for:
            // rewrite `Churn::scale_at` as the withdrawn shape and the two numbers
            // become the same float, the equality fires, and the mutation walks
            // through. Where nothing is outlying the exact `shipped == unmoved`
            // assertion above already pins every width, so this arm is not needed
            // there and is scoped away instead of excused.
            if outlying {
                // Eight, below all four measured values rather than at one of
                // them: 23.24 and 29.70 at the block rung, 9.54 and 10.32 at the
                // dense one. Every drawn column covers the same span of the
                // window, so the only thing left for the withdrawn shape to react
                // to is where the cut is taken, and the fixture therefore
                // discriminates by less than an uneven projection would.
                assert!(
                    worst_on_projection > 8.0,
                    "{name}, {pane:?}: cutting the projection moved only \
                 {worst_on_projection:.1}%, so this fixture cannot tell the two \
                 shapes apart and the comparison below says nothing"
                );
                assert!(
                    worst <= worst_on_projection / 2.0,
                    "{name}, {pane:?}: the shipped yardstick moves {worst:.1}% on \
                 one column of resize where cutting the projection moves \
                 {worst_on_projection:.1}%, so taking the cut before the \
                 projection has stopped buying the stability it exists for"
                );
            }
        }
    }
}

/// A window with a legitimate dynamic range is scaled exactly as it always was.
#[test]
fn a_window_with_a_wide_range_is_scaled_as_it_always_was() {
    /// Thirteen tenths of the mean of the non-empty values, with no cut in it.
    fn plain(values: &[u32]) -> u32 {
        let busy: Vec<u64> = values
            .iter()
            .map(|value| u64::from(*value))
            .filter(|value| *value > 0)
            .collect();
        if busy.is_empty() {
            return 0;
        }
        u32::try_from(busy.iter().sum::<u64>() * 13 / (busy.len() as u64 * 10)).expect("a scale")
    }

    for series in [QUARTERED, BURSTY, wave(), [7; HISTORY_SAMPLES]] {
        // Both rungs: the band follows the pane again, and the no-op claim is
        // about the rule rather than about one glyph set.
        for pane in [Glyphs::Block, Glyphs::Braille] {
            for width in [40u16, 60, 80, 109, 124] {
                // The drawn series, which is what the band divides, and the span is
                // read off the axis row rather than off `width`.
                let axis = band_at(width, series, pane)
                    .last()
                    .expect("a band row")
                    .clone();
                let slots = drawn_ink(&axis) * pane.density();
                // Non-vacuity, and it is the whole gate.
                assert!(
                    slots > 0,
                    "the band drew no axis at {width} columns, so this compared \
                 nothing"
                );
                let levelled = Churn(series).levels(slots);
                // `Churn::scale_at`, which is what the band calls, and not `scale_of`
                // over the same series.
                assert_eq!(
                    Churn(series).scale_at(slots),
                    plain(&levelled),
                    "{pane:?} at {width} columns: the outlier cut fired on a window \
                 with nothing outlying in it"
                );
            }
        }
    }
}

#[test]
fn the_band_follows_the_rung_the_pane_detects() {
    // The reverse of pinning the band to blocks. Taking it off the glyph
    // ladder pins it there; the band follows the pane
    // again, so a reader whose font carries braille gets a braille band.
    let series = wave();
    for width in [40u16, 60, 80, 109, 124] {
        let blocks = band_at(width, series, Glyphs::Block);
        for dense in [Glyphs::Braille, Glyphs::Octant] {
            assert_ne!(
                band_at(width, series, dense),
                blocks,
                "at {width} columns the band drew the same at {dense:?} as at the \
                 block rung, so the pane's detected glyphs are not reaching it"
            );
        }
    }
}

#[test]
fn a_dense_band_is_drawn_in_the_glyphs_its_pane_detected() {
    // The half that says the band is decodable where it is drawn.
    let series = wave();
    for pane in [Glyphs::Block, Glyphs::Braille, Glyphs::Octant] {
        for width in [40u16, 60, 80, 109, 124] {
            let heights = column_heights(width, series, pane);
            let ceiling = GRAPH_ROWS * pane.levels();
            assert!(
                heights.iter().all(|height| *height <= ceiling),
                "on a {pane:?} pane at {width} columns the band drew past the \
                 {ceiling} heights that rung can carry"
            );
            // Non-vacuity: the rung has to be exercised rather than left on the
            // axis, or "every height fits" is true of a blank band.
            assert!(
                heights.contains(&ceiling),
                "on a {pane:?} pane at {width} columns nothing reached the \
                 rung's ceiling, so this compared nothing"
            );
        }
    }
}

/// A store holding one six-second burst, the last write one sample before `now`.
fn burst_at(now: Instant) -> History {
    let began = now - HISTORY_SAMPLE * 6;
    let mut history = History::starting_at(began);
    for second in 0..6u32 {
        history.record_sized(
            [("src/a.rs", Some(6_000u64))],
            began + HISTORY_SAMPLE * second,
        );
    }
    history
}

#[test]
fn a_quiet_window_slides_left_rather_than_freezing() {
    // The reported defect.
    let now = Instant::now();
    let mut history = burst_at(now);

    let fresh = band_at(WIDE, history.worktree_churn().0, Glyphs::default());
    // Columns, not byte offsets: the ramp's glyphs are three bytes each and the
    // axis is one, so `char_indices` moves the answer whenever the mix of glyphs
    // in a row changes, which is a property of the data rather than of where the
    // ink is.
    let ink_at = |rows: &[String]| -> Vec<usize> {
        rows.iter()
            .flat_map(|row| row.chars().enumerate())
            .filter(|(_, glyph)| !glyph.is_whitespace() && *glyph != '_')
            .map(|(at, _)| at)
            .collect()
    };
    let before = ink_at(&fresh);
    assert!(
        !before.is_empty(),
        "the fixture drew no ink, so nothing here could move"
    );

    // Thirty seconds of nothing at all, which is the wake the ageing clock now
    // produces and the tick path never would.
    history.record_sized([], now + Duration::from_secs(30));
    let aged = band_at(WIDE, history.worktree_churn().0, Glyphs::default());
    let after = ink_at(&aged);

    assert_ne!(
        aged, fresh,
        "thirty seconds passed with no writes and the band drew the identical \
         picture, so the graph is frozen rather than quiet"
    );
    assert!(
        !after.is_empty(),
        "the burst left the window entirely in thirty seconds of a two-minute \
         window, so this measured a drain rather than a slide"
    );
    assert!(
        after.iter().max() < before.iter().max(),
        "the newest ink did not move left, so the window is redrawing rather \
         than ageing:\nbefore {before:?}\nafter  {after:?}"
    );
}

#[test]
fn the_band_and_the_sparklines_age_together() {
    // The coherence requirement, stated as a gate rather than left as a mechanism.
    let now = Instant::now();
    let mut history = burst_at(now);

    let band = |h: &History| h.worktree_churn().0;
    let spark = |h: &History| h.level("src/a.rs");

    let (band_before, spark_before) = (band(&history), spark(&history));
    assert!(
        spark_before.is_some_and(|buckets| buckets.iter().any(|bucket| *bucket > 0)),
        "the fixture's file has no sparkline, so this compares one element"
    );

    history.record_sized([], now + HISTORY_WINDOW / 2);

    assert_ne!(
        band(&history),
        band_before,
        "sixty seconds of quiet left the band where it was"
    );
    assert_ne!(
        spark(&history),
        spark_before,
        "sixty seconds of quiet moved the band and left the sparkline where it \
         was, so the two elements disagree about what time it is"
    );
}

#[test]
fn the_band_climbs_the_ramp_toward_the_top() {
    // btop's multi-row rule: one colour per row against the vertical axis, quiet at the
    // baseline and hot at the top, so a column that climbs reads hotter as it does.
    let theme = vigia::Theme::dark().resolve(vigia::Depth::Truecolor);
    let ramp = theme.spark_ramp().expect("dark at truecolour interpolates");

    let shown = Chrome {
        masthead: true,
        ..chrome(&App::new())
    };
    let area = Rect::new(0, 0, WIDE, TALL);
    let body = body_layout(area, &shown, 1, 1);
    assert!(
        body.graph > 1,
        "the fixture band is one row, so nothing climbs"
    );
    let mut buf = Buffer::empty(area);
    render(
        &mut buf,
        area,
        &banded(QUARTERED),
        &theme,
        Glyphs::default(),
        &shown,
    );

    let top = 1 + body.lead as u16;
    let base = top + body.graph as u16 - 1;
    // A column with ink in both rows: the tallest of the fixture's quarters.
    let full = (0..WIDE).find(|x| {
        !buf[(*x, top)].symbol().trim().is_empty() && !buf[(*x, base)].symbol().trim().is_empty()
    });
    let x = full.expect("no column filled the band, so nothing here is a gate");
    let rank = |fg: Option<ratatui::style::Color>| {
        ramp.iter()
            .position(|stop| Some(*stop) == fg)
            .unwrap_or_else(|| panic!("{fg:?} is no stop of the ramp"))
    };
    assert!(
        rank(buf[(x, top)].style().fg) > rank(buf[(x, base)].style().fg),
        "the top row of a full column is not hotter than its baseline"
    );
}

/// Widths the band is drawn at here, spanning the rungs the ladder serves.
const LADDER: [u16; 5] = [46, 60, WIDE, 109, 134];

/// Sub-column heights of the band alone, with the pane's margins cut away.
///
/// [`band_at`] hands back whole pane rows, and an untouched margin cell reads as
/// a height of zero exactly as an axis column does, so a gate over the raw vector
/// compares the inset against the graph.
fn band_heights(width: u16, series: [u32; HISTORY_SAMPLES]) -> Vec<usize> {
    let glyphs = Glyphs::default();
    let rows = band_at(width, series, glyphs);
    let axis = rows.last().expect("a band row");
    // The band is contiguous and its axis row is solid, so the inset and the ink
    // between them name the span without looking at any other row.
    let (from, span) = (drawn_inset(axis), drawn_ink(axis));
    let heights = column_heights(width, series, glyphs);
    heights[from * glyphs.density()..(from + span) * glyphs.density()].to_vec()
}

/// The worktree the defect was reported from: three files, written once each.
///
/// Driven through the store rather than written as a series, because what makes
/// this population what it is comes from `Track::wrote`: a path seen for the
/// first time has no earlier size to differ from, so its write weighs the floor.
fn three_files_written_once() -> [u32; HISTORY_SAMPLES] {
    let now = Instant::now();
    let began = now - HISTORY_SAMPLE * (HISTORY_SAMPLES as u32 - 1);
    let mut history = History::starting_at(began);
    for (second, path, bytes) in [
        (10u32, "src/a.rs", 2_500u64),
        (14, "src/b.rs", 1_200),
        (20, "src/c.rs", 640),
    ] {
        history.record_sized([(path, Some(bytes))], began + HISTORY_SAMPLE * second);
    }
    // Age to now with nothing written, which is what the reported pane had done.
    history.record_sized(core::iter::empty::<(&str, Option<u64>)>(), now);
    history.worktree_churn().0
}

/// A signal that never moves has to draw a level band.
#[test]
fn a_steady_worktree_draws_a_level_band_rather_than_a_comb() {
    // Every sample the same, so any variation across the drawn columns is the
    // projection rather than the worktree.
    let flat = [600u32; HISTORY_SAMPLES];

    for width in LADDER {
        let heights = band_heights(width, flat);
        let (low, high) = (
            heights.iter().min().copied().expect("a column"),
            heights.iter().max().copied().expect("a column"),
        );
        assert!(
            high > 0,
            "the fixture drew nothing at {width} columns, so this compared \
             nothing"
        );
        // One apart rather than equal, because the ramp quantises and the
        // window's two ends carry less kernel than its middle.
        assert!(
            high - low <= 1,
            "a worktree written at one constant rate drew heights from {low} to \
             {high} at {width} columns, so neighbouring columns carry different \
             amounts of time and the band reads as a comb: {heights:?}"
        );
    }
}

/// The reported defect: a band with only two heights cannot draw a shape.
#[test]
fn a_small_worktrees_band_is_not_axis_or_ceiling() {
    let reported = three_files_written_once();

    for width in LADDER {
        // Read off the band the pane actually gave rather than restated, so a
        // layout that hands down a different depth cannot make this vacuous.
        let ceiling = Glyphs::default().levels() * band_strip(width, reported).len();
        let heights = band_heights(width, reported);
        let drawn = distinct(&heights);
        let between = heights
            .iter()
            .filter(|height| **height > 0 && **height < ceiling)
            .count();
        assert!(
            drawn > 2,
            "the reported worktree drew {drawn} distinct height(s) at {width} \
             columns, so the band has the axis and the ceiling and nothing in \
             between to make a shape out of: {heights:?}"
        );
        // The stronger half, and the one a count of heights cannot give: a
        // column that is neither on the floor nor pinned at the top.
        assert!(
            between > 0,
            "every drawn column at {width} is either the axis or the ceiling, \
             so the ramp between them is unreachable: {heights:?}"
        );
    }
}
