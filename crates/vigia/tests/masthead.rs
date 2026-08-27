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
    app.chrome("fixture", None, Pointing::default(), 0, "")
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
    let hidden = body_layout(area(), &shipped, FILES, FILES);
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

        // Through the layout as well as through the flag, and on **every** press
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
///
/// **The comparison the retired ceiling made, in the form that survives #232**
/// ([#234](https://github.com/breferrari/vigia/issues/234)).
/// `vigia-core` once asserted `GRAPH_PERIOD < HISTORY_BUCKET`: two elements read
/// one store over one window and a reader has both on screen, so a burst that
/// splits in a file's sparkline and merges in the worktree band at the same
/// instant is incoherent. That gate went with the constants it read, because the
/// band's period stopped being a constant and became a property of the pane, and
/// nothing has compared the two since.
///
/// So it is compared where both are now decided, on the screen. The band plots
/// one value per sub-column and the sparkline one per drawn bucket, both across
/// the same window, so **finer** means *more of them*: the band may never carry
/// fewer sub-columns than the sparkline carries buckets.
///
/// Read from the drawn cells rather than from either ladder, for this suite's
/// standing reason. The band's row is solid because #232 gave it an axis, so its
/// ink is its cells; the fixture's file has no history, so its whole slot is
/// track and `TRACK` counts it. The path in `banded` carries no underscore, which
/// is what makes that count the slot rather than the row, and this asserts it
/// rather than trusting it.
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

            // **The bottom band row, which is the axis and is solid since
            // #232.** Every row above it is a height and carries only the columns
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

            // **Each side times its own element's density, which stopped
            // cancelling on 2026-08-22.** The claim is sub-columns against
            // buckets. While both elements followed one ladder a dense glyph
            // packed two of each into one cell and the factor stood on both
            // sides, so it was left out;
            // [#244](https://github.com/breferrari/vigia/issues/244) took the
            // band off the ladder and the sparkline kept it, so on a dense pane
            // the densities are now 1 and 2 and the bare comparison claims
            // something twice as weak as the sentence above it.
            //
            // **It changes no outcome today and is a truth fix rather than a
            // strength one**, which is worth saying so nobody reads it as a
            // tightened gate: measured across this sweep the slackest pair is a
            // braille pane at 41 columns, 39 sub-columns against 6 samples, so
            // the margin is 6.5x where the correction is 2x. Deleting the two
            // density terms leaves the suite green.
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
/// **[#223](https://github.com/breferrari/vigia/issues/223) is replaced here
/// and the correction is stated rather than absorbed.** That row saw a real
/// defect: at one column a second, a save drew a hairline between two blanks and
/// the whole band read as scatter. It reached for a wider column. What fixes the
/// same defect on the same shape of signal is the **axis**: one mark on the
/// bottom row of an empty column, so a narrow spike stands on a floor rather
/// than floating in a void. With the
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
        glyphs: usize,
    }

    let mut widths = Vec::new();
    for width in [60u16, 80, 120, 160, 200] {
        let rows = band_strip(width, BURSTY);
        let ink: usize = rows.iter().map(|row| drawn_ink(row)).sum();
        // **Distinct glyphs, and this field is named for that now.** It was
        // called `heights`, which is the conflation
        // [#244](https://github.com/breferrari/vigia/issues/244) corrects in
        // `SPEC.md` §5.1: a glyph is one cell of one row, so counting glyphs is
        // not counting column heights, and at a dense rung it is not even
        // counting one column. What it does measure is how much of the ramp the
        // shape uses, which is what the assertion below wants.
        //
        // **Not switched to counting distinct column heights, and the attempt
        // is worth
        // recording.** Counted as column heights this fixture draws three at
        // sixty columns and two at two hundred, so the assertion would fail for a
        // true reason: on an impulse series a wider pane divides the window past
        // what the level kernel varies over, and amplitude resolution genuinely
        // stops growing. What this gate claims is about *time* resolution and
        // ink. Column heights are
        // `the_bands_heights_are_the_block_rungs_and_not_a_dense_cells`, on a
        // wave, which is the series that has amplitude to resolve.
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
    // **The reversal of [#158](https://github.com/breferrari/vigia/issues/158),
    // and it is the whole of why the band reads as a graph.** That ruling gave an
    // empty column nothing, because a full track of `_` "reads as a dashed rule
    // across the pane". It does, and that is what a graph's axis is, and what
    // every graph of a signal that is zero most of the time draws. Without
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
    // **The scale rule, pinned by the two glyphs it produces.** The denominator
    // sits above the ordinary write rather than at the window's maximum, so one
    // outlier saturates instead of crushing every ordinary write beneath it.
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
fn the_band_draws_the_newest_writes_on_the_right() {
    // **Nothing gated time order, and a mirrored graph is a silent lie.** A band
    // drawn backwards would put a burst that just landed at the far left, where a
    // reader reads history, and nothing on screen would say so. Every other gate
    // here reads presence, ink, axis, span, stacking or resolution, and a
    // mirrored band has all six.
    //
    // Busy only in the newest quarter of the window, so the ink has one honest
    // place to be. `Churn::projected` is oldest-first and `Glyphs::glyph` takes
    // the older half first, which is the pair this checks end to end.
    let mut newest = [0u32; HISTORY_SAMPLES];
    for sample in newest.iter_mut().skip(HISTORY_SAMPLES * 3 / 4) {
        *sample = 50;
    }

    // Both rungs, which draw the same band since
    // [#244](https://github.com/breferrari/vigia/issues/244) and are kept here as
    // the cheapest possible statement of that: if the ruling is ever undone in
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

/// A wave, which is the shape the picture specifies and
/// [#242](https://github.com/breferrari/vigia/issues/242) made these elements
/// draw: monotone up then monotone down, with every sample non-zero so the
/// series exercises the ramp rather than the axis.
///
/// **Not the step [`the_band_stacks_its_rows_from_the_bottom`] uses.** A step is
/// two heights and a transition, which is the right fixture for a stacking rule
/// and the wrong one for a resolution claim: it cannot ask for more heights than
/// it has. This asks for a whole ramp's worth.
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
///
/// **Inverts [`Glyphs::glyph`] rather than decoding dot bits**, so this reads the
/// shipped drawer's own answer and cannot drift from it: a table built by asking
/// that function for every level pair is exact at every rung, where a decoder
/// would have to know braille's dot numbering and the eighth-block ramp
/// separately and could be wrong about either.
///
/// **`pane` is both what the terminal detected and what the band draws with**,
/// which is where [#244](https://github.com/breferrari/vigia/issues/244) put it
/// back. It briefly was not: that row took the band off the ladder and this
/// decoded with a fixed rung, so a braille pane's heights were read out of block
/// glyphs. Decoding with the pane's own rung is what lets a caller ask what a
/// *braille* reader sees, which is the question this element was reported on.
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
/// shape [#256](https://github.com/breferrari/vigia/issues/256) was reported on.
///
/// **Not [`BURSTY`], whose values are all within an order of magnitude of each
/// other.** That fixture is bursty in *time* and flat in magnitude, which is the
/// signal the mean-based rule was chosen for and cannot show this defect at all.
/// The distinction is the whole of #256: agent work is heavy tailed, a test run
/// rewriting thousands of bytes sits in the same window as the ordinary edits
/// around it, and it is the **ratio** rather than the spacing that collapses the
/// graph.
///
/// The burst is the older third of the window and the edits follow it, which is
/// the order the reader described: *"after that first burst wave with the ai
/// doing plan work, as it became more sparse"*.
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
///
/// **The fixture that binds the *upper* end of the outlier multiple.** Sweeping
/// that constant, this shape puts the floor back at eighteen times the median,
/// where [`BURST_THEN_ORDINARY`] tolerates far more. A
/// gate written only against the reported shape would leave the interval the
/// constant sits in the middle of unasserted, and a number defended only by a
/// docblock is a number that drifts.
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
///
/// **[#256](https://github.com/breferrari/vigia/issues/256), reported from a live
/// pane**: *"It has a nice wave, but the wave goes missing after"*, then *"Just
/// spikes"*. The band's yardstick was thirteen tenths of the mean of the window's
/// non-empty values, and a mean is not robust: one write an order of magnitude
/// above the rest raised the denominator until every ordinary edit rounded onto
/// the lowest of the two rows' sixteen levels.
///
/// **The floor is level one and not level zero, and the difference is why this
/// gate counts rather than looks for the axis.** `level_to` clamps a non-zero
/// count to at least one, so an ordinary write was never drawn *as* the axis: it
/// was drawn `▁` on the baseline row, one eighth of one row of two, which beside
/// an axis of `_` is the same picture and is not the same assertion. A gate
/// written against the axis would pass today and mean nothing.
///
/// Measured before the fix at eighty columns: **36 of 76 columns at level one,
/// seven distinct heights**.
#[test]
fn a_burst_does_not_press_the_ordinary_writes_onto_the_floor() {
    // **Two traces, and the second is not a duplicate.** They bind opposite ends
    // of the multiple the cut is taken at: a burst filling a third of the window
    // is what puts the floor back if the multiple is raised, and the reported
    // shape, whose burst covers about a fifth, is what the multiple was fixed
    // for.
    for (name, series) in [
        ("reported", BURST_THEN_ORDINARY),
        ("long burst", LONG_BURST_THEN_ORDINARY),
    ] {
        // **Both rungs, because the band follows the pane again** since #244 was
        // reopened, so a braille reader's band is a different picture and is the
        // one that row is about. This swept one rung while the band was pinned to
        // blocks, correctly: the two vectors were identical then.
        for pane in [Glyphs::Block, Glyphs::Braille] {
            let ceiling = GRAPH_ROWS * pane.levels();
            for width in [40u16, 60, 80, 109, 124] {
                let heights = column_heights(width, series, pane);

                // **Non-vacuity first, and it is two claims.** The trace has to carry
                // a genuinely loud event, or there is no yardstick to be dragged; and
                // it has to carry ordinary writes after it, or there is nothing the
                // dragging could have flattened. Both are read off the drawn band
                // rather than off the fixture, so a projection that dropped the tail
                // fails here rather than passing quietly.
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
                // **Zero where the rung can express it, and bounded where it
                // cannot.** The defect is the *denominator*: a burst dragging the
                // mean up until every ordinary write rounds onto the lowest
                // level. A rung answers a different question, and a dense cell
                // carries six levels over the band's two rows, so everything
                // under a sixth of the scale lands on level one by quantisation
                // whatever the denominator is. Asserting zero there would be
                // asserting the rung away.
                //
                // So the strict claim is made at the rung with sixteen levels,
                // which is where the defect was reported and measured, and a
                // dense rung is held to a quarter of its columns. Measured on the
                // shipped rule the worst dense case is 8 of 80, a tenth.
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

                // **And the shape is back, not merely off the floor**, which is a
                // separate claim: a band lifted off the axis and drawn flat would
                // satisfy the assertion above and still say nothing. A flat band
                // is one or two distinct heights. Measured on the shipped rule,
                // the reported trace draws 11 to 14 across these widths and the
                // long burst draws 7 at the narrowest, so four is below every one
                // of them with room and is not a number tuned to pass.
                //
                // Not compared against the reported picture's seven, which was
                // measured at eighty columns: a forty-column pane has fewer
                // sub-columns to be distinct in, so that comparison crosses
                // widths and is not one claim.
                //
                // Counted off the vector already in hand rather than by
                // rendering the same band a second time.
                let drawn = {
                    let mut seen = heights.clone();
                    seen.sort_unstable();
                    seen.dedup();
                    seen.len()
                };

                // **Scaled to the rung, because the rungs do not offer the same
                // number of heights.** Blocks carry sixteen over the band's two rows
                // and a dense cell carries six, so a fixed count would ask the two
                // for different fractions of what they have. A quarter of the rung
                // is the claim: four of sixteen, two of six.
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
///
/// **The gate that stops the yardstick's wiring drifting from its rule.** Every
/// other band gate here reads shape: heights, floors, spans, distinct counts. All
/// of them are satisfied by a band dividing by *some* plausible number, and a
/// mutation proved it, walking straight through the whole file with
/// `Painter::band` pointed back at `scale_of` over its own projection, which is
/// the exact defect [#256](https://github.com/breferrari/vigia/issues/256)'s
/// second half removed.
///
/// So this reproduces the drawer's arithmetic from `Churn::scale_at` and compares
/// cell for cell. `Painter::band` draws `ceil(value * levels / scale)` clamped
/// into `1..=levels` and stacks a whole ramp per row, which is three lines here;
/// the alternative, reading the figure back out of the drawn heights, was tried
/// and is not exact enough to assert against, since a column only bounds the
/// scale rather than naming it.
#[test]
fn the_band_divides_by_the_stores_own_figure() {
    let mut compared = 0usize;
    for series in [
        BURST_THEN_ORDINARY,
        LONG_BURST_THEN_ORDINARY,
        QUARTERED,
        wave(),
    ] {
        // **Every rung, because the band follows the pane again** since #244 was
        // reopened. While it was pinned to blocks one rung was enough; a braille
        // reader's band is a different picture and is the one that row is about.
        for pane in [Glyphs::Block, Glyphs::Braille] {
            let ceiling = GRAPH_ROWS * pane.levels();
            for width in [40u16, 60, 80, 109, 124] {
                // `column_heights` is sized to the pane and carries the margin's
                // cells as zeroes, so the band's own span is read off the axis row,
                // which is solid since #232, and the heights are sliced to it.
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
/// **The defect this catches was introduced by
/// [#256](https://github.com/breferrari/vigia/issues/256) and found by measuring
/// rather than by a gate.** The cut needs a population, and the band's first
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
/// | burst then ordinary | **6.5%** | 2.9% | 41.3% |
/// | long burst | **7.3%** | 2.9% | 91.4% |
/// | quartered | **14.3%** | 14.3% | 14.3% |
/// | wave | **2.9%** | 2.9% | 2.9% |
///
/// So the gate is two claims and neither needs a tuned number. Where nothing is
/// outlying the figure is the plain mean's **exactly**, at every width. Where
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
        // Both rungs, because the band follows the pane again since #244 was
        // reopened, and a dense cell resizes on a different grid: its sub-column
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
                // **The rule's own figure, exactly.** That the *drawer* uses it is
                // `the_band_divides_by_the_stores_own_figure`'s claim, made cell for
                // cell, and keeping the two apart is what lets this one be exact.
                let shipped = Churn(series).scale_at(slots);
                let unmoved = plain(&drawn);
                // What the withdrawn shape would have answered: the cut taken over
                // the drawn series rather than over the window.
                let on_projection = vigia_core::scale_of(drawn.iter().copied());

                // **Where nothing is outlying, exactly the figure the plain rule
                // sets**, at every width rather than at the worst of them.
                if !outlying {
                    assert_eq!(
                        shipped, unmoved,
                        "{name}: at {width} columns the cut fired on a window with \
                     nothing outlying in it"
                    );
                }

                if let Some((was, was_on_projection)) = previous {
                    // Only where the two shapes differ. The fixtures with nothing
                    // outlying are pinned exactly by the assertion above at every
                    // width, so accumulating a worst step for them would be
                    // arithmetic that nothing reads.
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
            // **Only where the two shapes differ, and with no equality to hide
            // behind.** This carried `|| worst == worst_on_projection` so that the
            // fixtures where nothing is outlying, and all three rules agree, would
            // pass. That disjunct made the gate blind to the defect it is named for:
            // rewrite `Churn::scale_at` as the withdrawn shape and the two numbers
            // become the same float, the equality fires, and the mutation walks
            // through. Where nothing is outlying the exact `shipped == unmoved`
            // assertion above already pins every width, so this arm is not needed
            // there and is scoped away instead of excused.
            if outlying {
                // **Fifteen, below both measured values rather than at one of
                // them.** Cutting the projection moves 41.3% at the block rung
                // and 20.0% at the dense one, where the shipped rule moves 6.5%
                // at both; a dense cell asks for twice the sub-columns, so it
                // crosses the window's sample count at half the width and
                // repeats over more of the range. The guard exists to catch a
                // fixture that stopped being heavy tailed, not to pin either
                // number.
                assert!(
                    worst_on_projection > 15.0,
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
///
/// **The fixture that binds the *lower* end of the outlier multiple**, and the
/// promise that keeps [#256](https://github.com/breferrari/vigia/issues/256) a
/// repair rather than a redesign: the cut is a no-op wherever the values are
/// within an order of magnitude of each other. [`QUARTERED`] is a deliberate
/// four-to-one series, and below eight times the median it stops drawing what it
/// drew before, at sixty and a hundred and nine columns.
///
/// Asserted against the **plain** rule written out here rather than against a
/// pinned strip, so it says *the cut did not fire* rather than *the picture
/// happens to match*.
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
                // **The drawn series, which is what the band divides**, and the span
                // is read off the axis row rather than off `width`. The band is
                // planned inside the pane's margin and the scrollbar's reserve, so
                // levelling onto the raw width would divide a projection no pane ever
                // produces: at forty columns the band draws about thirty-seven
                // sub-columns. The axis is solid since #232, so its ink is exactly
                // that span.
                let axis = band_at(width, series, pane)
                    .last()
                    .expect("a band row")
                    .clone();
                let slots = drawn_ink(&axis) * pane.density();
                // **Non-vacuity, and it is the whole gate.** A band that drew nothing
                // makes `slots` zero, `levels(0)` empty, and the comparison below
                // `0 == 0`, so twenty series-and-width pairs would agree about
                // nothing at all.
                assert!(
                    slots > 0,
                    "the band drew no axis at {width} columns, so this compared \
                 nothing"
                );
                let levelled = Churn(series).levels(slots);
                // **`Churn::scale_at`, which is what the band calls**, and not
                // `scale_of` over the same series. The two are the whole subject of
                // #256's second half: one cuts the samples and projects what is left,
                // the other cuts the projection, and only the first is a no-op here.
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
    // **[#244](https://github.com/breferrari/vigia/issues/244), reopened, and
    // this gate is the reverse of the one it replaces.** That row took the band
    // off the glyph ladder and pinned it to blocks; the band follows the pane
    // again, so a reader whose font carries braille gets a braille band.
    //
    // **The evidence the removal rested on was a misquote.** It cited a live
    // report as "the masthead read as scattered dots", where what was reported
    // was scattered *waves* that had become spikes. Dots are glyph texture and
    // point at a rung; waves becoming spikes are the signal's shape and point at
    // the denominator, which is
    // [#256](https://github.com/breferrari/vigia/issues/256) and is the rest of
    // this branch.
    //
    // **Blocks are more faithful and that did not decide it.** Measured on a
    // fixed denominator, mean absolute error between the drawn column and the
    // true level over five series at ten widths from 36 to 124 is 1.2% to 4.0%
    // for blocks against 4.3% to 8.8% for a dense cell. The rung is a
    // reader-facing option and its removal was never asked for; what the
    // measurement decides is how the dense cell is spent, not whether it is
    // offered.
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
    // **The half that says the band is decodable where it is drawn.** Drawing
    // *differently* per rung would also be true of a band emitting glyphs the
    // pane cannot render, which is the defect rather than the feature: the rung
    // ladder exists because a font that has no braille draws a question mark.
    //
    // So every cell the band paints has to come from the detected rung's own
    // glyph set. `column_heights` panics on a glyph its rung cannot spell, which
    // is what makes this a gate rather than a comment.
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
///
/// Not "ending at `now`", which is what this said: the window opens at
/// `now - HISTORY_SAMPLE * 6` and the six writes land on `0..6` samples from
/// there, so the last one is at `now - HISTORY_SAMPLE`. The difference is one
/// sample and it is exactly the quantum every gate below measures in.
///
/// Built through `History` rather than by writing a `Churn` array directly,
/// because what [#243](https://github.com/breferrari/vigia/issues/243) is about
/// is the *store* moving: a hand-built series is already whatever shape the test
/// wanted and cannot show that anything aged.
///
/// The burst's span was a parameter and both callers passed the same six
/// seconds, which is also the span itself, so it is a constant here instead.
///
/// **`starting_at` rather than `new`, and the difference is the whole fixture.**
/// `History::new` opens its window at `Instant::now()`, which is *after* the
/// instants below, so `roll` saturated to zero on every one of them and the
/// six-second burst was a single sample: `0..1` would have satisfied both gates.
/// Opening the window before the first write is what makes the six samples six.
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
    // **The defect [#243](https://github.com/breferrari/vigia/issues/243) was
    // reported for.** The window's axis is time, so a burst that has not moved is
    // a burst still claiming to be happening now. Rolling it thirty seconds with
    // nothing written has to move the ink left, and the gate reads the drawn
    // band rather than the store, because a store that rolls while the paint
    // reads a snapshot taken earlier would pass a store-level assertion and still
    // freeze the screen.
    //
    // **What this does not gate, said out loud: that anything ever rolls it.**
    // The defect was never that the store cannot age, it is that on a quiet
    // worktree nothing wakes to ask it to, and that half is the shell's loop,
    // which owns a terminal and three threads. `lib.rs`'s own source gate holds
    // it, by reading that `Shell::draw` rolls the window before it paints and on
    // the turn's own clock, and that gate is the one that was red before this
    // change. This one would pass without it.
    let now = Instant::now();
    let mut history = burst_at(now);

    let fresh = band_at(WIDE, history.worktree_churn().0, Glyphs::default());
    let ink_at = |rows: &[String]| -> Vec<usize> {
        rows.iter()
            .flat_map(|row| row.char_indices())
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
    // **[#234](https://github.com/breferrari/vigia/issues/234)'s coherence
    // requirement, stated as a gate rather than left as a mechanism.** One store
    // and one roll is *why* they agree; this is what fails if the band ever gets
    // a clock of its own. Both elements read the same window, so a roll that
    // moved one and not the other would leave the pane saying two different
    // things about what time it is.
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
    // #322, btop's multi-row rule: one colour per row against the vertical
    // axis, quiet at the baseline and hot at the top, so a column that climbs
    // reads hotter as it does. Read off the cells at a truecolour palette; the
    // gate below it holds the ladder's other half.
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
