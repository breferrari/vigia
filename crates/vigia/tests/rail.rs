//! The left rail: the layout where the pinned list sits **beside** the diff
//! rather than above it, `SPEC.md` §11.1's widest arrangement
//! ([#252](https://github.com/breferrari/vigia/issues/252)).
//!
//! On a wide pane a path ends near column 40 and its glance cluster is pinned to
//! the right edge, up to 150 cells away. The columnar slots
//! [#77](https://github.com/breferrari/vigia/issues/77) bought keep rows
//! comparable *down* the list and the void between path and cluster destroys the
//! association *across* one, which is the thing a reader actually looks for.
//!
//! Four claims live here and they fail in different ways.
//!
//! **The rail arrives where the stacked ladder would have climbed.** That width
//! is a derivation rather than a preference, and the gate re-derives it from a
//! drawn screen rather than restating the constant.
//!
//! **Widening into it takes nothing away.** Both regions read one glance ladder,
//! so splitting a pane costs each of them width; the arrival width is the one
//! place below three hundred columns where neither loses a rung, and that is the
//! property worth a sweep.
//!
//! **The two regions are two regions.** Separate scrollbars, separate widths,
//! separate ladders, and a pointer that is told the same geometry the painter
//! drew into. The tiling half lives in
//! `tests/legibility.rs::the_body_tiles_the_pane_with_no_gap_and_no_overlap`,
//! which was written as a partition for this layout before it existed.
//!
//! **The rail keeps a path.** A rail whose paths have lost the directory they
//! are in has given up half of what it was built to show.
//!
//! What is deliberately *not* here is the arithmetic that makes those true at
//! widths nothing draws: `render.rs` carries that in a `const` block, because a
//! claim about an unreachable case cannot be a test.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use vigia::{
    App, Chrome, FileEntry, Glyphs, HEAT_BUCKETS, HeatBucket, Position, Row, Theme, View,
    body_layout, regions, render,
};
use vigia_core::Recency;

/// The block one heat slice is drawn as, restated rather than imported.
///
/// A test sharing the renderer's own constant would agree with it by
/// construction, which is `tests/legibility.rs`'s reason for restating the same
/// two glyphs.
const HEAT_SLICE: char = '■';

/// The eighth-blocks a sparkline is drawn from, restated for [`HEAT_SLICE`]'s
/// reason.
const RAMP: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// What a sparkline bucket nothing was written in draws.
const TRACK: char = '_';

/// The narrowest a path may be drawn, restated from `render.rs`'s private
/// `MIN_PATH_WIDTH`.
const MIN_PATH_WIDTH: usize = 12;

/// The path columns the rail promises beside a settled glance cluster, restated
/// from `render.rs`'s private `RAIL_PATH`.
///
/// Twice the floor, because at exactly the floor a path elides to its bare tail
/// and a rail exists so a reader can join a path to its own numbers.
const RAIL_PATH: usize = MIN_PATH_WIDTH * 2;

/// The pane `assets/preview.svg` is measured from, which §5.1 makes the picture's
/// own width.
///
/// Used here to read the **settled** glance complement off a drawn screen rather
/// than restating a slice count: what the picture draws is what the ladder's
/// settled rung is, and a test that named twelve would be asserting a number
/// against itself.
const PICTURED_PANE: u16 = 109;

/// Well past the widest pane anyone runs, so a sweep measures the rule rather
/// than the rungs that happen to be reachable.
const WIDEST: u16 = 240;

/// A pane tall enough that neither region is the thing giving way.
const TALL: u16 = 24;

fn chrome() -> Chrome {
    App::new().chrome("fixture", None, None, None, None, None)
}

/// A file with a full history and a full heat strip, so every glance element has
/// something to draw and a rung is readable off the screen.
fn entry(path: &str) -> FileEntry {
    FileEntry {
        path: path.to_owned(),
        from: None,
        kind: 'M',
        churn: Some((42, 7)),
        spark: [
            1, 1, 1, 1, 2, 2, 2, 2, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 11, 11, 12, 12,
        ],
        recency: Recency::Pulse,
        heat: {
            let mut buckets = [HeatBucket::default(); HEAT_BUCKETS];
            for (at, bucket) in buckets.iter_mut().enumerate() {
                if at % 2 == 0 {
                    bucket.added = 3;
                } else {
                    bucket.removed = 2;
                }
            }
            buckets
        },
    }
}

/// The longest path in the fixture, and the one every path assertion reads.
///
/// Long enough that no rail width can draw it whole, which is what makes
/// `the_rail_keeps_a_path_and_not_just_a_filename` a claim about the *budget*
/// rather than about this string.
const LONG: &str = "crates/vigia-core/src/engine/incremental/watch.rs";

/// A screen with both regions carrying a file heading, which is the only screen
/// this file is about.
///
/// **The diff's first row is a heading rather than a content line**, because the
/// rung a region draws is readable off a heading and off nothing else. Both
/// regions therefore have a glance cluster on their own first row, and in the
/// rail those are the same row of the pane.
fn beside() -> View {
    View {
        list: vec![entry(LONG), entry("Cargo.toml"), entry("src/main.rs")],
        list_top: 0,
        files: 3,
        top: Position { file: 0, row: 0 },
        rows: vec![
            Row::file(entry(LONG)),
            Row::file(entry("Cargo.toml")),
            Row::file(entry("src/main.rs")),
        ],
        total_rows: 3,
        rows_above: 0,
        current_span: 3,
        ..View::default()
    }
}

/// The same screen with no pinned entries, so no rail can be drawn whatever the
/// pane is wide enough for.
///
/// This is how the *stacked* ladder is read at widths where the shipped layout
/// is a rail: `clamped_to` collapses a body whose view holds no entries back to
/// the whole-pane diff, so the heading in it is planned against the pane exactly
/// as it was before this row landed.
///
/// **`files` stays at three**, which is the difference between a view with an
/// empty list and an empty worktree: at zero, B3 replaces the whole region with a
/// sentence and there is no heading to read a rung off at all. The first draft
/// zeroed it and the gate's own non-vacuity guard caught it.
fn streamed() -> View {
    View {
        list: Vec::new(),
        list_top: 0,
        top: Position { file: 0, row: 0 },
        ..beside()
    }
}

fn drawn(width: u16, height: u16, view: &View) -> Buffer {
    let area = Rect::new(0, 0, width, height);
    let mut buf = Buffer::empty(area);
    render(
        &mut buf,
        area,
        view,
        &Theme::default(),
        Glyphs::Block,
        &chrome(),
    );
    buf
}

/// Cells of row `y` between `from` and `to` whose glyph and colour are both one
/// of `symbols` and `colours`.
///
/// **Restricted to a column range, which is the whole reason this exists beside
/// `tests/legibility.rs`'s copy.** A row can hold two regions in this layout, and
/// a strip counted across the whole row adds one region's rung to the other's:
/// twelve slices beside twelve is twenty-four, and twenty-four is a legal rung. A
/// gate that counted the row would pass on a renderer that had lost the ladder
/// entirely.
///
/// Colour as well as glyph, for that file's own recorded reason: a heat slice and
/// a full sparkline bucket are the same block, so a glyph-only match counts one
/// as the other.
fn cells_in(
    buf: &Buffer,
    y: u16,
    from: u16,
    to: u16,
    colours: &[Color],
    symbols: &[char],
) -> usize {
    (from..to)
        .filter(|x| {
            let cell = &buf[(*x, y)];
            symbols
                .iter()
                .any(|glyph| cell.symbol() == glyph.to_string())
                && cell.style().fg.is_some_and(|fg| colours.contains(&fg))
        })
        .count()
}

fn heat_colours(theme: &Theme) -> Vec<Color> {
    [
        theme.heat_track,
        theme.heat_added,
        theme.heat_added_warm,
        theme.heat_added_hot,
        theme.heat_removed,
        theme.heat_removed_warm,
        theme.heat_removed_hot,
        theme.heat_mixed,
        theme.heat_mixed_warm,
        theme.heat_mixed_hot,
    ]
    .iter()
    .filter_map(|style| style.fg)
    .collect()
}

fn spark_colours(theme: &Theme) -> Vec<Color> {
    [theme.spark, theme.spark_warm, theme.spark_hot]
        .into_iter()
        .filter_map(|style| style.fg)
        .collect()
}

/// What one region draws on one row: whether the counts cell is there, how many
/// heat slices, how many sparkline cells.
///
/// Named rather than written out at each use, because it is the triple every
/// monotonicity claim below compares and a bare tuple in five signatures is five
/// places to get the order wrong.
type Rung = (bool, usize, usize);

/// What one region draws on one row: whether the counts cell is there, how many
/// heat slices, how many sparkline cells.
///
/// The triple the glance ladder walks, and the thing every monotonicity claim in
/// this repo is about.
fn rung(buf: &Buffer, theme: &Theme, y: u16, from: u16, to: u16) -> Rung {
    let heat = cells_in(buf, y, from, to, &heat_colours(theme), &[HEAT_SLICE]);
    let bars = cells_in(buf, y, from, to, &spark_colours(theme), &RAMP);
    let track = theme.spark_track.fg.expect("the track has a colour");
    let empty = cells_in(buf, y, from, to, &[track], &[TRACK]);
    let counts = (from..to).any(|x| buf[(x, y)].symbol() == "+");
    (counts, heat, bars + empty)
}

/// The rung each region draws at this width, read at each region's **own** first
/// row.
///
/// Beside a rail those are the same row of the pane and in the stacked layout
/// they are not, which is exactly why the row is asked for rather than assumed.
fn rungs(width: u16, view: &View) -> (Rung, Rung) {
    let area = Rect::new(0, 0, width, TALL);
    let theme = Theme::default();
    let buf = drawn(width, TALL, view);
    let told = regions(area, &chrome(), view);
    let list = rung(
        &buf,
        &theme,
        told.list.top,
        told.list.left,
        told.list.left + told.list.width,
    );
    let diff = rung(
        &buf,
        &theme,
        told.diff.top,
        told.diff.left,
        told.diff.left + told.diff.width,
    );
    (list, diff)
}

/// The first width at which a pane draws a rail, found rather than restated.
fn first_rail() -> u16 {
    (1..=WIDEST)
        .find(|width| body_layout(Rect::new(0, 0, *width, TALL), &chrome(), 3).rail)
        .expect("no width in the sweep draws a rail")
}

/// The rail arrives exactly where the stacked list would have left the settled
/// ladder, and that is a derivation rather than a chosen number.
///
/// **Both regions read one glance ladder**, so splitting a pane in two costs each
/// half the width the whole had. A split therefore costs no rung only where both
/// halves *and* the undivided pane one column below sit on the same plateau of
/// that ladder. The settled rung's plateau ends at 133 columns; the only other
/// plateau is the ladder's top, which needs 160 planning columns in **each** half
/// and therefore a pane of 328. So there is one width, and this is the gate that
/// says the renderer picked it.
///
/// **Derived from a drawn screen rather than from the constant.** The stacked
/// ladder's climb is read off a listless view, which draws the whole-pane diff at
/// every width and is therefore the *old* layout measured at the *new* widths.
/// The settled complement itself is read at `PICTURED_PANE`, so §5.1's picture is
/// what the comparison is anchored to and no slice count is written down here.
///
/// A renderer that moved the arrival width by a column, in either direction,
/// reddens this without any snapshot moving.
#[test]
fn the_rail_arrives_where_the_stacked_list_would_have_climbed() {
    let theme = Theme::default();
    let listless = streamed();

    // The settled complement, off the published picture rather than restated.
    let area = Rect::new(0, 0, PICTURED_PANE, TALL);
    let told = regions(area, &chrome(), &listless);
    let pictured = rung(
        &drawn(PICTURED_PANE, TALL, &listless),
        &theme,
        told.diff.top,
        told.diff.left,
        told.diff.left + told.diff.width,
    );
    assert!(
        pictured.1 > 0 && pictured.2 > 0,
        "the pictured pane drew no strip and no sparkline, so there is no settled \
         complement to compare against: {pictured:?}"
    );

    // The first width at which a whole-pane row spends more on either element
    // than the picture does. That is where the stacked list would have climbed.
    let climbs = (PICTURED_PANE..=WIDEST)
        .find(|width| {
            let area = Rect::new(0, 0, *width, TALL);
            let told = regions(area, &chrome(), &listless);
            let (_, heat, spark) = rung(
                &drawn(*width, TALL, &listless),
                &theme,
                told.diff.top,
                told.diff.left,
                told.diff.left + told.diff.width,
            );
            heat > pictured.1 || spark > pictured.2
        })
        .expect("no width in the sweep leaves the settled complement");

    assert_eq!(
        first_rail(),
        climbs,
        "the rail arrives at {} where the stacked ladder climbs at {climbs}, so \
         the pane spends a column on a wider strip that a rail cannot match, or \
         gives up a rung it did not have to",
        first_rail()
    );
}

/// Widening a pane into the rail takes no glance element away, from either
/// region.
///
/// **The hardest case the layout table has**, which #252 says in its own words:
/// the two shapes allocate differently and the boundary is where they meet. Every
/// other monotonicity gate in this repo sweeps one shape.
///
/// Asserted per region and per element, over a sweep that crosses the boundary
/// with room on both sides. The non-vacuity guard at the end is what stops it
/// passing on a build where the rail is never drawn at all: a sweep that only
/// ever saw one shape would be asserting the stacked ladder a second time.
#[test]
fn widening_into_the_rail_takes_no_glance_element_away() {
    let view = beside();
    let (mut saw_stacked, mut saw_rail) = (false, false);
    let mut last: Option<(u16, (Rung, Rung))> = None;

    for width in 100..=WIDEST {
        saw_rail |= body_layout(Rect::new(0, 0, width, TALL), &chrome(), view.files).rail;
        saw_stacked |= !body_layout(Rect::new(0, 0, width, TALL), &chrome(), view.files).rail;
        let now = rungs(width, &view);
        if let Some((below, was)) = last {
            for (region, (was, now)) in [("list", (was.0, now.0)), ("diff", (was.1, now.1))] {
                assert!(
                    (now.0 || !was.0) && now.1 >= was.1 && now.2 >= was.2,
                    "widening from {below} to {width} columns took something away \
                     from the {region}: {was:?} became {now:?}"
                );
            }
        }
        last = Some((width, now));
    }

    assert!(
        saw_stacked && saw_rail,
        "the sweep saw a stacked pane = {saw_stacked} and a rail = {saw_rail}, so \
         it did not cross the boundary it is about"
    );
}

/// Beside a rail the two regions are two regions: same rows, different columns,
/// each with its own bar and its own ladder.
///
/// **The claim `tests/list.rs::each_region_reports_its_own_bar_column` records as
/// unmakeable until this row landed.** On every layout that shipped before it,
/// `Body::areas` spread `..area` to both regions, so their rects shared an `x`
/// and a `width` and therefore a bar column: a `regions` handing `Bar::region` the
/// wrong rect produced the right two numbers. Here it does not.
#[test]
fn the_two_regions_are_two_regions() {
    let view = beside();
    let width = first_rail();
    let area = Rect::new(0, 0, width, TALL);
    let body = body_layout(area, &chrome(), view.files).clamped_to(view.list.len());
    assert!(body.rail, "the first rail width did not draw a rail");

    let areas = body.areas(area);
    assert_eq!(
        areas.list.y, areas.diff.y,
        "the two regions do not start on the same row, so this is not a rail"
    );
    assert_eq!(
        areas.list.x, area.x,
        "the rail does not begin at the pane's own leading column"
    );
    assert_eq!(
        areas.list.x + areas.list.width,
        areas.diff.x,
        "the diff does not begin where the rail ends"
    );
    assert_eq!(
        areas.diff.x + areas.diff.width,
        area.x + area.width,
        "the diff does not reach the pane's own trailing column"
    );
    assert_eq!(
        (areas.rule.width, areas.rule.height),
        (0, 0),
        "a rule was drawn between regions that are side by side"
    );
    assert_eq!(
        areas.band.width, area.width,
        "the band stopped following the pane once the regions split"
    );

    // A view tall enough to overflow neither region reports no bar, so the two
    // assertions below are about a drawn bar rather than a field that is always
    // `Some`. `beside` holds three files and three rows against a pane of TALL.
    let told = regions(area, &chrome(), &view);
    assert_eq!(
        (told.list.bar, told.diff.bar),
        (None, None),
        "a screen with nothing to scroll told the pointer about a bar"
    );

    // Now overflow both, and the two bars are in different columns for the first
    // time in this program's history.
    let mut crowded = view.clone();
    crowded.list = (0..200).map(|at| entry(&format!("src/f{at}.rs"))).collect();
    crowded.files = 200;
    crowded.total_rows = 5_000;
    let told = regions(area, &chrome(), &crowded);
    assert_eq!(
        told.list.bar,
        Some(areas.list.x + areas.list.width - 1),
        "the rail's bar is not on the rail's own right edge"
    );
    assert_eq!(
        told.diff.bar,
        Some(area.x + area.width - 1),
        "the diff's bar is not on the pane's right edge"
    );
    assert_ne!(
        told.list.bar, told.diff.bar,
        "both bars are still in one column, which is the assumption this layout \
         exists to break"
    );
}

/// The rail keeps a path, not just a filename.
///
/// `MIN_PATH_WIDTH` outranks every glance element and is what stops a row naming
/// nothing; `RAIL_PATH` is the stronger promise the rail's own floor makes, and it
/// is what the rail is *for*. A rail at the floor that had spent its columns on a
/// wider strip would draw `…watch.rs` beside a cluster, which is a row that has
/// given up the association the rail was built to restore.
///
/// Measured as drawn columns rather than as arithmetic, and against a path no
/// rail width can draw whole, so the number is the budget rather than the string.
#[test]
fn the_rail_keeps_a_path_and_not_just_a_filename() {
    let view = beside();
    let mut widths = 0usize;

    for width in first_rail()..=WIDEST {
        let area = Rect::new(0, 0, width, TALL);
        let body = body_layout(area, &chrome(), view.files);
        assert!(body.rail, "the sweep left the rail at {width} columns");
        let told = regions(area, &chrome(), &view);
        let buf = drawn(width, TALL, &view);

        // **The path's own columns: from after the kind letter to the pulse that
        // opens the glance cluster.** Bounded on the right by an *element* rather
        // than by a blank run, which is the correction that made this gate able to
        // fail at all. The first draft split on a double space, and there is no
        // double space between a path and the pulse, so it measured the path plus
        // the whole cluster and stayed green while the floor was two columns short.
        //
        // Read against `LONG`, which no rail width can draw whole, so the count is
        // the budget rather than the string. Where a rail is wide enough to draw it
        // whole the count is the string and is comfortably over the floor, which is
        // the direction that cannot hide a defect.
        let row: String = (told.list.left..told.list.left + told.list.width)
            .map(|x| buf[(x, told.list.top)].symbol().to_owned())
            .collect::<Vec<_>>()
            .join("");
        let cluster = row
            .char_indices()
            .find(|(_, glyph)| *glyph == '●')
            .map(|(at, _)| at)
            .expect("the rail's first row draws a pulse");
        let path = row[..cluster]
            .trim_start_matches(['▸', ' '])
            .trim_start_matches('M')
            .trim();
        assert!(
            path.chars().count() >= RAIL_PATH,
            "at {width} columns the rail drew {} columns of path where its floor \
             promises {RAIL_PATH}: {path:?}",
            path.chars().count()
        );
        widths += 1;
    }

    assert!(
        widths > 50,
        "the sweep covered {widths} rail widths, which is too few to be about the \
         floor rather than about one pane"
    );
}

/// No pane below the rail's arrival width changes shape, including the one the
/// published picture is measured from.
///
/// **The half that says every other fixture in this repo is untouched.**
/// `assets/preview.svg` is a stacked screen and §5.1 makes it a specification, so
/// a rail reaching it would make the picture false. I6's forty-column pane is
/// three rungs of ladder below that again.
#[test]
fn the_rail_never_reaches_the_widths_the_picture_and_i6_pin() {
    let arrives = first_rail();
    assert!(
        arrives > PICTURED_PANE,
        "the rail arrives at {arrives}, at or below the {PICTURED_PANE} columns \
         `assets/preview.svg` is measured from, so the published picture no longer \
         describes what the tool draws"
    );

    for width in 1..arrives {
        for height in [1u16, 6, TALL, 60] {
            let body = body_layout(Rect::new(0, 0, width, height), &chrome(), 3);
            assert!(
                !body.rail,
                "a {width}x{height} pane drew a rail below the arrival width"
            );
        }
    }
}
