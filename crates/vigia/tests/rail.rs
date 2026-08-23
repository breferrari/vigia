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

mod support;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
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

/// The caret's own column and the kind letter's cell with its gap, restated from
/// `render.rs`'s private `CARET_WIDTH` and `KIND_WIDTH`.
///
/// A rail row opens with these and the path begins after them. Counted rather
/// than matched, for the reason `KIND_WIDTH`'s own docblock gives: the opening is
/// an allowance, and a helper that recognised the kind letter by its glyph would
/// eat the head of any path beginning with one.
const CARET_WIDTH: usize = 1;
const KIND_WIDTH: usize = 2;

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
fn rung(buf: &Buffer, theme: &Theme, y: u16, columns: std::ops::Range<u16>) -> Rung {
    let within = |colours: &[ratatui::style::Color], symbols: &[char]| {
        support::columns_in(buf, y, columns.clone(), colours, symbols).len()
    };
    let heat = within(&support::heat_colours(theme), &[HEAT_SLICE]);
    let bars = within(&support::spark_colours(theme), &RAMP);
    let track = theme.spark_track.fg.expect("the track has a colour");
    let empty = within(&[track], &[TRACK]);
    let counts = columns.clone().any(|x| buf[(x, y)].symbol() == "+");
    (counts, heat, bars + empty)
}

/// The columns one region holds, from the geometry the painter drew into.
///
/// Asked of `regions` rather than derived, so a gate reading a rung is reading
/// the same rect the renderer painted. Four hand-spelled copies of this
/// expression were what it replaced.
fn columns_of(region: vigia::Region) -> std::ops::Range<u16> {
    region.left..region.left + region.width
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
    // **Both regions have to hold rows**, or a "list" rung read at the diff's own
    // first row across the whole pane would pass for a rung this layout never
    // drew. Asserted rather than assumed: it is a property of the fixture, and a
    // fixture is the thing most likely to change under a gate.
    assert!(
        told.list.rows > 0 && told.diff.rows > 0,
        "at {width} columns a region drew no rows, so its rung is being read off \
         the other one"
    );
    let read = |region: vigia::Region| rung(&buf, &theme, region.top, columns_of(region));
    (read(told.list), read(told.diff))
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
        columns_of(told.diff),
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
                columns_of(told.diff),
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
        let row: String = columns_of(told.list)
            .map(|x| buf[(x, told.list.top)].symbol().to_owned())
            .collect::<Vec<_>>()
            .join("");
        let cluster = row
            .char_indices()
            .find(|(_, glyph)| *glyph == '●')
            .map(|(at, _)| at)
            .expect("the rail's first row draws a pulse");
        // **The kind letter is skipped by position, never by glyph.** Trimming a
        // leading `M` would eat the first character of a path that happens to
        // begin with one, and `KIND_WIDTH`'s own docblock in `render.rs` is about
        // exactly this: the row's opening cell is a fixed allowance, not something
        // to be recognised.
        let opening: String = row.chars().take(CARET_WIDTH + KIND_WIDTH).collect();
        let path = row[opening.len()..cluster].trim();
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

/// A hover in the rail underlines the rail's own row and nothing in the diff.
///
/// **The fourth surface of the same defect, and the one that reaches a reader.**
/// [#251](https://github.com/breferrari/vigia/issues/251) made every hit-test in
/// `input.rs` column-aware and [#254](https://github.com/breferrari/vigia/issues/254)
/// did the paint marks; `Hovered::Row` still carries a bare screen row, and
/// `Painter::file_row` compares it against the row it is drawing on. That
/// comparison is written for **both** regions, and the comment over it justified
/// itself with *"a diff heading's row is never inside"* the list region. True
/// while the two were stacked. Beside a rail they share every row, so hovering
/// the third file in the rail underlined the third file heading in the diff.
///
/// `SPEC.md` §5.3's B10 adopted hover on the list's rows alone, because the diff
/// is not clickable and a mark there would imply it is. This gate is that ruling
/// asserted rather than described.
#[test]
fn a_hover_in_the_rail_does_not_light_the_diff() {
    use ratatui::style::Modifier;
    use vigia::Hovered;

    let view = beside();
    let width = first_rail();
    let area = Rect::new(0, 0, width, TALL);
    let told = regions(area, &chrome(), &view);
    // The rail's own first row, which beside a rail is also the diff's.
    let row = told.list.top;
    assert_eq!(
        row, told.diff.top,
        "the two regions do not share a row, so this gate is about a layout that \
         does not exist"
    );

    let hovered = Chrome {
        hovered: Some(Hovered::Row(row)),
        ..chrome()
    };
    let mut buf = Buffer::empty(area);
    render(
        &mut buf,
        area,
        &view,
        &Theme::default(),
        Glyphs::Block,
        &hovered,
    );

    let underlined = |columns: std::ops::Range<u16>| {
        columns
            .filter(|x| {
                buf[(*x, row)]
                    .style()
                    .add_modifier
                    .contains(Modifier::UNDERLINED)
            })
            .count()
    };

    let rail = underlined(columns_of(told.list));
    let diff = underlined(columns_of(told.diff));

    // Non-vacuity first: a gate that found no underline anywhere would pass on a
    // build where hover had been deleted outright.
    assert!(
        rail > 0,
        "the hovered rail row drew no underline at all, so this gate is not \
         measuring hover"
    );
    assert_eq!(
        diff, 0,
        "hovering the rail underlined {diff} cells of the diff heading drawn on \
         the same row, which `SPEC.md` §5.3 B10 rules is a mark on a surface the \
         pointer cannot act on"
    );
}

/// Crossing into the rail never hands the diff fewer rows.
///
/// **The bigger-container-holds-less failure, at the one boundary this row
/// introduces.** The margin ladder is written out as a table to refuse it and
/// `list_cap`'s step is argued for it; both are about *one* layout, and crossing
/// between two is a third place it can happen. The first draft of this row did
/// it: a rail charges `LEAD_ROWS` where the stacked layout at the same height
/// returns the whole-body diff and charges nothing, so at 133 columns and six
/// rows the diff had three rows and at 134 it had two.
///
/// **The claim is about the crossing and about the rail, and deliberately not
/// about every width step.** A first draft swept all widths and reddened twice on
/// the *stacked* layout, at widths no rail can reach: the footer's ladder takes a
/// second line at eight columns and shortens the whole body, and at forty-five
/// columns the body gains a row, the pinned list takes its first, and the diff
/// pays that row plus the rule's. The second is the shipped design of regions
/// arriving as space appears; the first is a real defect and is filed as
/// [#283](https://github.com/breferrari/vigia/issues/283), on the shelf with the
/// evidence and the reason it was not taken here. Asserting them here would have
/// been asserting something the product does not do, which is how a gate ends up
/// being weakened until it says nothing.
///
/// **Swept with the masthead off**, which is what keeps the assertion strict: the
/// band's arrival is a genuine step back for the diff, bounded by `GRAPH_KEEP`,
/// and `the_rail_is_monotone_in_pane_height` owns it on the axis it belongs to.
#[test]
fn crossing_into_the_rail_never_costs_the_diff_a_row() {
    let arrives = first_rail();
    let mut heights = 0usize;
    let mut banded = 0usize;

    // **Both masthead settings, every file count that changes the answer, and
    // every height to two hundred rows.** A first draft swept one file count and
    // one masthead setting and the defect it was written for survived in the cell
    // it did not look at: beside a rail `after` still holds the row the stacked
    // list would have taken and the rule's, so the band's fit test sees more rows
    // and the band can arrive *earlier* than on the strip the rail replaces. With
    // one changed file the strip costs two rows and the band costs three, so the
    // rail came out a row short.
    //
    // The sample is written out rather than narrowed to the boundary that failed,
    // because the bound `Body::beside` rests on is a claim about *every* file
    // count: the file counts here are the ones where `files`, `list_cap` and
    // `affordable` each take their turn at deciding the strip's height, and two
    // hundred rows is well past any pane that could saturate them.
    for masthead in [false, true] {
        let chrome = Chrome {
            masthead,
            ..chrome()
        };
        for files in [0usize, 1, 2, 3, 5, 6, 7, 12, 40, 200, 5000] {
            for height in 1..=200u16 {
                let stacked = body_layout(Rect::new(0, 0, arrives - 1, height), &chrome, files);
                let rail = body_layout(Rect::new(0, 0, arrives, height), &chrome, files);
                assert!(
                    !stacked.rail,
                    "the width below the arrival already draws a rail at {height} rows"
                );
                assert!(
                    rail.diff >= stacked.diff,
                    "with masthead {masthead} over {files} files at {height} rows, \
                     widening from {} to {arrives} columns took the diff from {} \
                     rows to {}",
                    arrives - 1,
                    stacked.diff,
                    rail.diff
                );

                // **The map's own crossing**, which nothing asserted until the
                // exhaustive sweep went looking for it. A rail exists to show more
                // of the changed set, so a pane widened into one handing back
                // *fewer* files would be the same failure on the region the layout
                // is named for.
                assert!(
                    rail.list >= stacked.list,
                    "with masthead {masthead} over {files} files at {height} rows, \
                     widening from {} to {arrives} columns took the map from {} \
                     rows to {}",
                    arrives - 1,
                    stacked.list,
                    rail.list
                );

                // **And the same failure mirrored.** `Body::beside` charges the
                // band against fewer rows than it has, so it can only ever band
                // *later* than the stacked layout; if it ever banded so much later
                // that the stacked layout had one and the rail did not, a reader
                // narrowing the pane would gain a band. The delay is bounded by
                // two rows and this is what says so.
                assert!(
                    !(stacked.graph > 0 && rail.rail && rail.graph == 0),
                    "with masthead {masthead} over {files} files at {height} rows, \
                     the stacked layout draws a band and the rail one column wider \
                     does not, so narrowing the pane would gain one"
                );

                if rail.rail {
                    heights += 1;
                }
                if rail.graph > 0 || stacked.graph > 0 {
                    banded += 1;
                }
            }
        }
    }

    // Or the sweep never drew a band and the axis this gate was widened for was
    // never reached.
    assert!(
        banded > 20,
        "a band was drawn at {banded} of the sizes swept, too few for the \
         masthead axis to be under test"
    );

    let bare = Chrome {
        masthead: false,
        ..chrome()
    };

    // Or the crossing was never drawn as a rail and every comparison above was
    // between two stacked layouts.
    assert!(
        heights > 1000,
        "the arrival width drew a rail at {heights} of the sizes swept, which is \
         too few to be about the boundary"
    );

    // And inside the rail, widening never takes a row: both regions' rows come out
    // of one body, and the body does not change with width once the footer has
    // settled.
    for height in 1..=48u16 {
        let mut previous: Option<(u16, usize)> = None;
        for width in arrives..=WIDEST {
            let body = body_layout(Rect::new(0, 0, width, height), &bare, 40);
            if let Some((below, was)) = previous {
                assert!(
                    body.diff >= was,
                    "at {height} rows, widening from {below} to {width} columns \
                     inside the rail took the diff from {was} rows to {}",
                    body.diff
                );
            }
            previous = Some((width, body.diff));
        }
    }
}

/// The rail deepens with the pane and falls only where the band arrives.
///
/// **Two claims, because the honest property has two halves.** With the masthead
/// off the rail's rows are the body's less one lead blank, so the map is strictly
/// monotone in pane height and a taller pane always shows more files. With it on
/// the band spans the pane above both columns, so its rows are unavailable to the
/// map as well as to the diff, and the map falls by exactly the band's rows at
/// the one height the band arrives at.
///
/// That second half is an exception to `SPEC.md` §11.1's clamp order and it is
/// recorded there as one. It is not a defect and it is not free either: an
/// earlier draft of `Body::beside`'s docblock claimed monotonicity outright, and
/// this gate is what would have caught the claim.
#[test]
fn the_rail_is_monotone_in_pane_height() {
    let width = first_rail();
    let bare = Chrome {
        masthead: false,
        ..chrome()
    };
    let shown = Chrome {
        masthead: true,
        ..chrome()
    };
    // More files than any height in the sweep can draw, so the pane is always the
    // thing deciding and never the changed-file count.
    let files = 500;

    let mut previous = 0usize;
    for height in 1..=80u16 {
        let body = body_layout(Rect::new(0, 0, width, height), &bare, files);
        assert!(
            body.list >= previous,
            "with no masthead, a pane grown to {height} rows drew {} files where \
             the row below drew {previous}",
            body.list
        );
        previous = body.list;
    }
    assert!(
        previous > 60,
        "the masthead-off sweep topped out at {previous} files, so it never \
         reached the depths this layout is for"
    );

    let (mut previous, mut falls) = (0usize, 0usize);
    let mut band_rows = 0usize;
    for height in 1..=80u16 {
        let body = body_layout(Rect::new(0, 0, width, height), &shown, files);
        if body.list < previous {
            falls += 1;
            let took = body.graph + body.air;
            assert!(
                took > 0 && previous - body.list <= took,
                "with the masthead on, a pane grown to {height} rows drew {} files \
                 where the row below drew {previous}, a fall of {} against the \
                 band's own {took} rows",
                body.list,
                previous - body.list
            );
        }
        band_rows = band_rows.max(body.graph + body.air);
        previous = body.list;
    }

    // Exactly one fall, and it is the band's arrival. More than one would mean
    // the band comes and goes; none would mean the sweep never drew a band and
    // the loop above asserted nothing.
    assert_eq!(
        falls, 1,
        "the map fell {falls} times across the height sweep; the band arrives once"
    );
    assert!(
        band_rows > 0,
        "no height in the sweep drew a band, so the fall this gate is about was \
         never reachable"
    );
}

/// Both regions grow with the pane, and the rail draws the pictured complement
/// at every width it is drawn at.
///
/// **The width axis of the two claims above.** `rail_of` is a share floored at a
/// constant and its docblock says both halves are monotone by construction; that
/// is an argument, and this is the evidence. What it also pins is the ceiling:
/// `SPEC.md` §11.1 rules the rail draws twelve slices and twelve buckets, exactly
/// what `assets/preview.svg` draws, and does not climb past it at any pane anyone
/// runs. A change to the share that let it climb early would make the picture and
/// the rail disagree with nothing else noticing.
#[test]
fn the_rail_grows_with_the_pane_and_keeps_the_pictured_complement() {
    let view = beside();
    let theme = Theme::default();
    let pictured = {
        let area = Rect::new(0, 0, PICTURED_PANE, TALL);
        let told = regions(area, &chrome(), &streamed());
        rung(
            &drawn(PICTURED_PANE, TALL, &streamed()),
            &theme,
            told.diff.top,
            columns_of(told.diff),
        )
    };

    let mut previous: Option<(u16, u16, u16)> = None;
    for width in first_rail()..=WIDEST {
        let area = Rect::new(0, 0, width, TALL);
        let areas = body_layout(area, &chrome(), view.files)
            .clamped_to(view.list.len())
            .areas(area);
        if let Some((below, rail, diff)) = previous {
            assert!(
                areas.list.width >= rail && areas.diff.width >= diff,
                "widening from {below} to {width} columns narrowed a region: rail \
                 {rail} to {}, diff {diff} to {}",
                areas.list.width,
                areas.diff.width
            );
        }
        previous = Some((width, areas.list.width, areas.diff.width));

        let (_, heat, spark) = rungs(width, &view).0;
        assert_eq!(
            (heat, spark),
            (pictured.1, pictured.2),
            "at {width} columns the rail drew a {heat}-slice strip and a \
             {spark}-cell sparkline where the published picture draws {} and {}",
            pictured.1,
            pictured.2
        );
    }
}

/// A stale view shortens the rail and moves nothing else.
///
/// `render` and `regions` both take `Body::split(..).clamped_to(view.list.len())`,
/// and beside a rail the rows the list does not use are in its own column with
/// nothing below them: handing them back to the diff would draw the diff twice,
/// once in each region. The tiling gate in `tests/legibility.rs` only ever sees a
/// view whose entries match what the pane afforded, which is the shipped path and
/// not the one this is about.
#[test]
fn a_stale_view_shortens_the_rail_and_moves_nothing_else() {
    let width = first_rail();
    let area = Rect::new(0, 0, width, TALL);
    let full = body_layout(area, &chrome(), 500);
    assert!(full.rail, "the fixture pane does not draw a rail");
    assert!(
        full.list > 3,
        "the rail affords {} rows, too few for a shortened view to differ from it",
        full.list
    );

    for have in [0usize, 1, 3, full.list - 1, full.list, full.list + 7] {
        let body = full.clamped_to(have);
        let areas = body.areas(area);
        assert_eq!(
            usize::from(areas.list.height),
            have.min(full.list),
            "a view holding {have} entries drew {} rail rows",
            areas.list.height
        );
        // The diff keeps its own column and its own rows whatever the map holds,
        // except where an empty map collapses the body to one whole-pane region.
        if have == 0 {
            assert_eq!(
                (areas.diff.x, areas.diff.width),
                (area.x, area.width),
                "an empty map left the diff in a column rather than giving it the \
                 pane"
            );
        } else {
            assert_eq!(
                usize::from(areas.diff.height),
                full.diff,
                "a view holding {have} entries changed the diff's height"
            );
        }
        assert_eq!(
            usize::from(area.y + 1) + body.rows(),
            usize::from(areas.diff.y + areas.diff.height),
            "a view holding {have} entries left `Body::rows` disagreeing with the \
             rows the diff is drawn into"
        );
    }
}
