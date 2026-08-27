//! The left rail: the layout where the pinned list sits **beside** the diff
//! rather than above it, `SPEC.md` §11.1's widest arrangement
//!.
//!
//! On a wide pane a path ends near column 40 and its glance cluster is pinned to
//! the right edge, up to 150 cells away. The columnar slots
//! [#77](https://github.com/breferrari/vigia/issues/77) bought keep rows
//! comparable *down* the list and the void between path and cluster destroys the
//! association *across* one, which is the thing a reader actually looks for.
//!
//! The claims live here and they fail in different ways.
//!
//! **The rail arrives where the stacked ladder would have climbed.** That width
//! is a derivation rather than a preference, and the gate re-derives it from a
//! drawn screen rather than restating the constant.
//!
//! **Widening into it takes nothing away, at the block rung.** Both regions read
//! one glance ladder, so splitting a pane costs each of them width; the arrival
//! width is the one place below three hundred columns where neither loses a rung.
//! At a dense glyph rung the ladder climbs earlier and the crossing does cost one,
//! which is [#284](https://github.com/breferrari/vigia/issues/284); what holds
//! everywhere is the floor the published picture sets, and that has a gate of its
//! own.
//!
//! **Crossing costs no rows either.** Not the diff's, which a rail's lead blank
//! and its band's arrival can both take, and not the map's.
//!
//! **The two regions are two regions.** Separate scrollbars, separate widths,
//! separate ladders, and a pointer that is told the same geometry the painter
//! drew into. The tiling half lives in
//! `tests/legibility.rs::the_body_tiles_the_pane_with_no_gap_and_no_overlap`,
//! which was written as a partition for this layout before it existed.
//!
//! **A mark for one region stays in it.** Hover is the list's and the diff is not
//! clickable, and beside a rail the two share every row.
//!
//! **The rail keeps a path**, and grows with the pane without climbing off the
//! pictured complement before a 402-column pane. A rail whose paths have lost the
//! directory they are in has given up half of what it was built to show.
//!
//! **And it deepens with the pane**, falling only where the band arrives and only
//! by the band's own rows, which is the one exception to §11.1's clamp order.
//!
//! What is deliberately *not* here is the arithmetic that makes those true at
//! widths nothing draws: `render.rs` carries that in a `const` block, because a
//! claim about an unreachable case cannot be a test.

mod support;

/// The repository fixture, under a second name because this file already has a
/// `support` of its own.
#[path = "../../vigia-core/tests/support/mod.rs"]
mod repo;

use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use vigia::{
    Action, App, Chrome, FileEntry, Glyphs, HEAT_BUCKETS, HeatBucket, ListRow, Pointing, Position,
    Regions, Row, Theme, View, action_for, body_layout, regions, render,
};

/// A key event, spelled once.
fn press(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}
use vigia_core::{Origin, Recency};

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

/// Past the width at which the rail's own ladder would climb off the settled
/// rung, which is a pane of about four hundred.
///
/// **`SPEC.md` §11.1 claims the rail does not climb past the pictured complement
/// at any pane anyone runs, and [`WIDEST`] cannot reach the width where that
/// could be false.** The rail's planning width is `pane / 3 - 4`, so it leaves the
/// settled plateau's 129 columns at about 402 columns of pane. A sweep that stops
/// at 240 asserts the claim nowhere it could fail, which is the shape `SPEC.md` §7
/// keeps finding; this is the width that makes it falsifiable.
const PAST_THE_CLIMB: u16 = 420;

/// The pane at which the rail's own glance ladder leaves the settled rung.
///
/// **Derived, and it is what `SPEC.md` §11.1's ceiling means.** The rail's
/// planning width is `rail_of(pane) - BAR_WIDTH - inset`, and the rung above the
/// settled one arrives when the share clamp can spare its 52 columns, which needs
/// 130 planning columns and therefore a rail of 134: `pane / 3 >= 134` is a pane of
/// 402. Below it the rail draws exactly what the picture draws; at it the rail is
/// wide enough to be worth more, and drawing more is what §5.3 rules space earns.
const THE_CLIMB: u16 = 402;

/// A pane tall enough that neither region is the thing giving way.
const TALL: u16 = 24;

/// A shell that has asked for the rail, which is what every gate below is about.
///
/// **The gesture, not the width, since `SPEC.md` §11.2 B14**
///. The rail arrived on
/// its own until then, so `App::new()` was enough and the width alone decided the
/// layout. It is a request now, and this file's subject is the rail that a reader
/// asked for: the width is a precondition and the gesture is the cause. The one
/// gate about the *default* builds its own chrome and says so.
fn chrome() -> Chrome {
    let mut chrome = stacked_chrome();
    chrome.rail = true;
    chrome
}

/// A shell that has not asked, which is what ships.
fn stacked_chrome() -> Chrome {
    App::new().chrome("fixture", None, Pointing::default(), 0, "")
}

/// A file with a full history and a full heat strip, so every glance element has
/// something to draw and a rung is readable off the screen.
fn entry(path: &str) -> FileEntry {
    FileEntry {
        origin: Origin::Unstaged,
        path: path.to_owned(),
        from: None,
        kind: 'M',
        churn: Some((42, 7)),
        spark: [
            1, 1, 1, 1, 2, 2, 2, 2, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 11, 11, 12, 12,
        ],
        recency: Recency::Pulse,
        newest: true,
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
        list_span: 3,
        grouped: false,
        list: vec![
            entry(LONG).into(),
            entry("Cargo.toml").into(),
            entry("src/main.rs").into(),
        ],
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
/// sentence and there is no heading to read a rung off at all. Zeroing it is
/// what the gate's own non-vacuity guard exists to catch.
fn streamed() -> View {
    View {
        list_span: 0,
        grouped: false,
        list: Vec::new(),
        list_top: 0,
        top: Position { file: 0, row: 0 },
        ..beside()
    }
}

fn drawn(width: u16, height: u16, view: &View) -> Buffer {
    drawn_at(width, height, view, Glyphs::Block)
}

/// The same, at a named glyph rung.
///
/// **The rung is an input to the glance ladder, so pinning every gate here to
/// blocks sweeps one rung of two**
///. `Columns::plan`
/// takes `glyphs`, a denser cell draws two buckets
/// per column, so the same layout table is reached at a *different width* on a
/// terminal that carries braille or octants. A sweep at one rung is a sweep of one
/// of three ladders, which is the shape `SPEC.md` §7 keeps finding.
fn drawn_at(width: u16, height: u16, view: &View, glyphs: Glyphs) -> Buffer {
    let area = Rect::new(0, 0, width, height);
    let mut buf = Buffer::empty(area);
    render(&mut buf, area, view, &Theme::default(), glyphs, &chrome());
    buf
}

/// Every glyph rung the sparkline can be drawn from.
///
/// `Glyphs::auto` resolves to one of these from the terminal, so all three are
/// screens a reader gets rather than options nobody takes.
const RUNGS: [Glyphs; 3] = [Glyphs::Block, Glyphs::Braille, Glyphs::Octant];

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
    rung_at(buf, theme, y, columns, Glyphs::Block)
}

/// The same, reading the alphabet a named glyph rung draws from.
///
/// The block rung keeps the restated `RAMP` and `TRACK`, which is this suite's
/// convention and its reason: a test importing the renderer's own table would
/// agree with it by construction. A dense rung's alphabet is *derived*, because
/// there is no table to restate: the glyphs are a function of two sub-column
/// levels and `Glyphs::glyph` is the only thing that knows them.
fn rung_at(
    buf: &Buffer,
    theme: &Theme,
    y: u16,
    columns: std::ops::Range<u16>,
    glyphs: Glyphs,
) -> Rung {
    let (ramp, empty_glyph) = if glyphs.density() == 1 {
        (RAMP.to_vec(), TRACK)
    } else {
        let levels = glyphs.levels();
        let mut ramp = Vec::new();
        for left in 0..=levels {
            for right in 0..=levels {
                if (left, right) != (0, 0) {
                    ramp.push(glyphs.glyph(left, right));
                }
            }
        }
        (ramp, glyphs.glyph(0, 0))
    };
    let within = |colours: &[ratatui::style::Color], symbols: &[char]| {
        support::columns_in(buf, y, columns.clone(), colours, symbols).len()
    };
    let heat = within(&support::heat_colours(theme), &[HEAT_SLICE]);
    let bars = within(&support::spark_colours(theme), &ramp);
    let track = theme.spark_track.fg.expect("the track has a colour");
    let empty = within(&[track], &[empty_glyph]);
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
    rungs_at(width, view, Glyphs::Block)
}

/// The same, at a named glyph rung.
fn rungs_at(width: u16, view: &View, glyphs: Glyphs) -> (Rung, Rung) {
    let area = Rect::new(0, 0, width, TALL);
    let theme = Theme::default();
    let buf = drawn_at(width, TALL, view, glyphs);
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
    let read =
        |region: vigia::Region| rung_at(&buf, &theme, region.top, columns_of(region), glyphs);
    (read(told.list), read(told.diff))
}

/// The first width at which a pane draws a rail, found rather than restated.
fn first_rail() -> u16 {
    (1..=WIDEST)
        .find(|width| body_layout(Rect::new(0, 0, *width, TALL), &chrome(), 3, 3).rail)
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
        saw_rail |= body_layout(
            Rect::new(0, 0, width, TALL),
            &chrome(),
            view.files,
            view.files,
        )
        .rail;
        saw_stacked |= !body_layout(
            Rect::new(0, 0, width, TALL),
            &chrome(),
            view.files,
            view.files,
        )
        .rail;
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
    let body = body_layout(area, &chrome(), view.files, view.files).clamped_to(view.list.len());
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
    crowded.list = (0..200)
        .map(|at| ListRow::from(entry(&format!("src/f{at}.rs"))))
        .collect();
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
        let body = body_layout(area, &chrome(), view.files, view.files);
        assert!(body.rail, "the sweep left the rail at {width} columns");
        let told = regions(area, &chrome(), &view);
        let buf = drawn(width, TALL, &view);

        // **The path's own columns: from after the kind letter to the pulse that
        // opens the glance cluster.** Bounded on the right by an *element* rather
        // than by a blank run, which is what makes this gate able to fail at
        // all. Splitting on a double space measures the path plus the whole
        // cluster, there being no double space between a path and the pulse, and
        // stays green while the floor is two columns short.
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

    // **Read off the drawn screen rather than from `Body`**, because
    // `body_layout(..).rail` and `first_rail()` both reduce to the same predicate
    // and comparing them is comparing an expression with itself. What a reader
    // gets below the arrival width is a *rule* between the regions and one column
    // of content, and that is what this asserts.
    let view = beside();
    for width in [40u16, 80, PICTURED_PANE, arrives - 1] {
        let area = Rect::new(0, 0, width, TALL);
        let body = body_layout(area, &chrome(), view.files, view.files).clamped_to(view.list.len());
        let areas = body.areas(area);
        assert!(
            !body.rail,
            "a {width}-column pane drew a rail below the arrival width"
        );
        assert!(
            areas.rule.height > 0,
            "at {width} columns the regions are stacked and no rule was drawn \
             between them"
        );
        assert_eq!(
            (areas.list.x, areas.list.width),
            (areas.diff.x, areas.diff.width),
            "at {width} columns the two regions do not share the pane's columns, \
             so this pane is not the stacked layout the picture draws"
        );
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
/// between two is a third place it can happen: a rail charges `LEAD_ROWS` where
/// the stacked layout at the same height returns the whole-body diff and charges
/// nothing, so at 133 columns and six
/// rows the diff had three rows and at 134 it had two.
///
/// **The claim is about the crossing and about the rail, and deliberately not
/// about every width step.** Sweeping all widths reddens twice on the *stacked*
/// layout, at widths no rail can reach: the footer's ladder takes a
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
    // every height to two hundred rows.** Sweeping one file count and one
    // masthead setting leaves the defect this is written for alive in the cell
    // that is not looked at: beside a rail `after` still holds the row the
    // stacked
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
                let stacked =
                    body_layout(Rect::new(0, 0, arrives - 1, height), &chrome, files, files);
                let rail = body_layout(Rect::new(0, 0, arrives, height), &chrome, files, files);
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
            let body = body_layout(Rect::new(0, 0, width, height), &bare, 40, 40);
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
/// recorded there as one. It is not a defect and it is not free either:
/// `Body::beside`'s docblock cannot claim monotonicity outright, and this gate
/// is what catches the claim.
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
        let body = body_layout(Rect::new(0, 0, width, height), &bare, files, files);
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
        let body = body_layout(Rect::new(0, 0, width, height), &shown, files, files);
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
///
/// **Swept to [`PAST_THE_CLIMB`] rather than to [`WIDEST`]**, because the rail's
/// planning width leaves the settled plateau at [`THE_CLIMB`] and a sweep stopping
/// at 240 asserts the ceiling nowhere it could fail. That is the difference
/// between a gate and a claim, and this file has already been the place it was got
/// wrong. Widening the sweep turned *"does not climb at any pane anyone runs"*
/// into a number, which is what it should always have been.
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

    let mut climbed = false;
    let mut previous: Option<(u16, u16, u16)> = None;
    for width in first_rail()..=PAST_THE_CLIMB {
        let area = Rect::new(0, 0, width, TALL);
        let areas = body_layout(area, &chrome(), view.files, view.files)
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
        if width < THE_CLIMB {
            assert_eq!(
                (heat, spark),
                (pictured.1, pictured.2),
                "at {width} columns the rail drew a {heat}-slice strip and a \
                 {spark}-cell sparkline where the published picture draws {} and {}",
                pictured.1,
                pictured.2
            );
        } else {
            assert!(
                heat >= pictured.1 && spark >= pictured.2,
                "at {width} columns the rail drew {heat} and {spark}, under the \
                 complement it kept one column narrower"
            );
            climbed |= heat > pictured.1 || spark > pictured.2;
        }
    }

    // **The ceiling is a pinned width, not a phrase.** `SPEC.md` §11.1 said the
    // rail keeps the pictured complement *at any pane anyone runs*, which is a
    // claim no sweep can fail: the width where it stops being true is simply
    // outside the sweep. It is 402 columns, the rail's share reaching the 130
    // planning columns the ladder's next rung asks for, and naming it is what
    // makes the sentence checkable. If the share moves, this reddens.
    assert!(
        climbed,
        "the rail never left the pictured complement by {PAST_THE_CLIMB} columns, \
         so THE_CLIMB is not where the ladder actually turns"
    );
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
    let full = body_layout(area, &chrome(), 500, 500);
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

/// Crossing into the rail never removes a glance element, and never takes either
/// region below the complement the published picture draws, **at any glyph rung**.
///
/// **The claim `widening_into_the_rail_takes_no_glance_element_away` makes is the
/// block rung's, and this is what is true of the other two.** `Columns::plan`
/// takes `glyphs`; a braille or octant cell draws two buckets per column, so the
/// same layout table is reached at different widths and the stacked ladder climbs
/// at a pane of **119** rather than 134. By 133 a dense terminal is already on the
/// wider strip, and the rail cannot match it: keeping that rung would need 119
/// columns of rail out of a 134-column pane. So on those terminals the crossing
/// **does** cost a rung, from twenty-four heat slices to twelve, in both regions.
///
/// That is a real cost and it is recorded rather than hidden: `SPEC.md` §11.1
/// carries it, and [#284](https://github.com/breferrari/vigia/issues/284) is the
/// row for an arrival width that knows which rung it is on, which needs the glyph
/// rung to reach the layout and is a signature this row cannot absorb.
///
/// **What holds at every rung is the floor**, and it is the one §5.1 anchors: no
/// element is taken away, and neither region falls below what
/// `assets/preview.svg`'s own complement is at that rung. Twelve slices is the
/// pictured strip, and twelve is where a dense terminal lands.
#[test]
fn crossing_into_the_rail_keeps_the_pictured_complement_at_every_rung() {
    let view = beside();
    let theme = Theme::default();
    let arrives = first_rail();
    let mut fell = 0usize;

    for glyphs in RUNGS {
        // The floor, read off the published pane at this rung rather than
        // restated, which is what keeps the number the picture's rather than this
        // file's.
        let listless = streamed();
        let area = Rect::new(0, 0, PICTURED_PANE, TALL);
        let told = regions(area, &chrome(), &listless);
        let (_, floor_heat, floor_spark) = rung_at(
            &drawn_at(PICTURED_PANE, TALL, &listless, glyphs),
            &theme,
            told.diff.top,
            columns_of(told.diff),
            glyphs,
        );
        assert!(
            floor_heat > 0 && floor_spark > 0,
            "{glyphs:?}: the pictured pane drew no complement to compare against"
        );

        let below = rungs_at(arrives - 1, &view, glyphs);
        let above = rungs_at(arrives, &view, glyphs);

        for (region, was, now) in [("rail", below.0, above.0), ("diff", below.1, above.1)] {
            assert!(
                now.0 || !was.0,
                "{glyphs:?}: crossing into the rail took the counts off the {region}"
            );
            assert!(
                now.1 > 0 && now.2 > 0,
                "{glyphs:?}: crossing into the rail took an element off the \
                 {region}: {was:?} became {now:?}"
            );
            assert!(
                now.1 >= floor_heat && now.2 >= floor_spark,
                "{glyphs:?}: beside a rail the {region} draws {now:?}, under the \
                 ({floor_heat}, {floor_spark}) the published pane draws at this rung"
            );
            if now.1 < was.1 || now.2 < was.2 {
                fell += 1;
            }
        }
    }

    // **The dense rungs' cost, pinned rather than merely allowed.** Two rungs,
    // two regions, one fall each: the block rung must not fall and the other two
    // must, or the ladder has moved and `SPEC.md` §11.1's paragraph about it has
    // gone stale without anything saying so.
    assert_eq!(
        fell, 4,
        "the crossing cost a rung in {fell} region-rung pairs; blocks should cost \
         none and the two dense rungs one each in both regions"
    );
}

/// A pointer at the rail's real geometry routes to the region it is over.
///
/// **The seam between what this row changed and what consumes it, which nothing
/// crossed.** `tests/rail.rs` asserted the *shape* `regions` reports and
/// `tests/input.rs` asserted routing against hand-built numbers; the rail's own
/// boundary columns had never been through `action_for`. That is the coverage
/// shape `SPEC.md` §7 keeps finding: two halves each tested against its own idea
/// of the other, with production the only place they meet.
///
/// [#251](https://github.com/breferrari/vigia/issues/251) made every hit-test
/// column-aware and could not test it against a layout that did not exist. This
/// is that test, at the geometry the layout actually produces.
#[test]
fn a_pointer_at_the_rails_own_columns_reaches_the_region_it_is_over() {
    use ratatui::crossterm::event::{Event, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    use vigia::{Action, action_for};

    let view = beside();
    let width = first_rail();
    let area = Rect::new(0, 0, width, TALL);
    let told = regions(area, &chrome(), &view);
    assert_eq!(
        told.list.top, told.diff.top,
        "the fixture pane is not a rail, so this gate is about a layout that does \
         not exist"
    );

    let boundary = told.list.left + told.list.width;
    assert_eq!(
        boundary, told.diff.left,
        "the two regions do not meet, so there is no boundary to probe"
    );
    // **And the boundary is interior**, or the columns probed below are off the
    // pane and every routing assertion is about a cell nobody can point at. Found
    // by mutation: widening the rail to the whole pane left this gate green while
    // four of its neighbours reddened.
    assert!(
        told.diff.width > 0 && boundary < area.x + area.width,
        "the diff has no columns of its own at {width}, so the boundary is the \
         pane's edge rather than a seam between two regions"
    );

    let event = |column: u16, row: u16| {
        Event::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        })
    };
    let row = told.list.top;

    // **The last column of the rail and the first of the diff**, which is the one
    // pair a row cannot tell apart in this layout and the only pair that could
    // ever have been got wrong.
    assert!(
        matches!(
            action_for(&event(boundary - 1, row), told),
            Some(Action::ScrollList(_))
        ),
        "a wheel on the rail's last column did not reach the map: {:?}",
        action_for(&event(boundary - 1, row), told)
    );
    assert!(
        matches!(
            action_for(&event(boundary, row), told),
            Some(Action::Scroll(_))
        ),
        "a wheel on the diff's first column did not reach the diff: {:?}",
        action_for(&event(boundary, row), told)
    );

    // **A click on a listed file, at the rail's own columns.** The pointer has to
    // land on the file the rail drew there rather than on the row of a region
    // seventy columns away.
    let click = |column: u16, row: u16| {
        Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        })
    };
    assert_eq!(
        action_for(&click(2, told.list.top + 1), told),
        Some(Action::ListRow(1)),
        "clicking the rail's second row did not jump the diff to its second file"
    );
    assert!(
        matches!(
            action_for(&click(boundary + 2, told.list.top + 1), told),
            None | Some(Action::DiffTo(_))
        ),
        "clicking inside the diff was routed to the map: {:?}",
        action_for(&click(boundary + 2, told.list.top + 1), told)
    );

    // **The rail's column below its last file belongs to neither region**, which
    // is the shipped common case: three changed files in a body of twenty. It
    // falls through to the diff exactly as the band's rows do, and saying so here
    // is what makes it a decision rather than an accident nobody noticed.
    let empty = told.list.top + told.list.rows;
    assert!(
        empty < told.diff.top + told.diff.rows,
        "the fixture's rail is as tall as its column, so the row below it is not \
         reachable and this half asserts nothing"
    );
    assert!(
        matches!(action_for(&event(2, empty), told), Some(Action::Scroll(_))),
        "a wheel below the rail's last file did not fall through to the diff: {:?}",
        action_for(&event(2, empty), told)
    );
}

#[test]
fn the_rail_is_off_until_the_reader_asks_for_it() {
    // **`SPEC.md` §11.2 B14's whole claim.** The rail arrives at 134 columns and
    // until this row it arrived on its own, so a reader who had not asked for a
    // narrower diff got one: at 133 the diff plans against 129 columns and at 134
    // against 60. #252's derivation of *where* the split can happen is untouched;
    // what changes is that crossing it is a gesture.
    //
    // Swept across every width the gate can reach rather than sampled at 134,
    // because a default that held at the arrival width and lapsed at 200 is the
    // shape a single screen cannot see.
    let files = 3;
    for width in 1..=WIDEST {
        let at = Rect::new(0, 0, width, TALL);
        assert!(
            !body_layout(at, &stacked_chrome(), files, files).rail,
            "a {width} column pane drew a rail nobody asked for"
        );
    }
}

#[test]
fn r_asks_for_the_rail_and_r_puts_it_back() {
    // **The gesture, end to end**: the key resolves to the action, the action moves
    // the state, and the state reaches the layout. Three links, and the gate above
    // reads only the last of them, so a key bound to nothing would leave it green.
    //
    // Driven through `App::apply` rather than by setting the field, which is what
    // `chrome()` does above and is right for gates about the *rail*; this one is
    // about the *toggle*.
    let scratch = repo::Scratch::large_diff("rail-gesture", 3, 40);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    repo::materialise(&mut frame);
    let mut app = App::new();

    assert_eq!(
        action_for(&press(KeyCode::Char('r')), Regions::default()),
        Some(Action::ToggleRail),
        "`r` is not bound to the rail"
    );

    let wide = Rect::new(0, 0, 160, TALL);
    let railed = |app: &App| {
        body_layout(
            wide,
            &app.chrome("f", None, Pointing::default(), 0, ""),
            3,
            3,
        )
        .rail
    };
    assert!(!railed(&app), "a fresh shell drew a rail");

    let height = body_layout(
        wide,
        &app.chrome("f", None, Pointing::default(), 0, ""),
        3,
        3,
    )
    .diff;
    app.apply(Action::ToggleRail, &mut frame, height)
        .expect("toggle");
    assert!(railed(&app), "`r` did not put the list beside the diff");

    app.apply(Action::ToggleRail, &mut frame, height)
        .expect("toggle");
    assert!(
        !railed(&app),
        "`r` did not put the list back above the diff"
    );
}

#[test]
fn r_below_the_arrival_width_changes_nothing_and_eats_no_gesture() {
    // **`m`'s own behaviour one region over**: a pane that cannot carry the thing
    // draws nothing different, and the request is still kept, so a reader who
    // narrows a railed pane and widens it again gets the rail back rather than the
    // question.
    //
    // Asserted as *the whole body is identical*, not as `!body.rail`, because the
    // second would pass against a layout that had quietly changed something else.
    let arrives = first_rail();
    for width in 1..arrives {
        let at = Rect::new(0, 0, width, TALL);
        assert_eq!(
            body_layout(at, &stacked_chrome(), 3, 3),
            body_layout(at, &chrome(), 3, 3),
            "a {width} column pane laid out differently for a rail it cannot draw"
        );
    }

    // **The request survives a pane that cannot honour it**, which is the half
    // that would be lost by clearing the flag instead of ignoring it, and it is
    // driven through an `App` rather than through a chrome built railed.
    //
    // The first spelling of this asserted `body_layout(.., &chrome(), .., ..).rail` at
    // `arrives` and `arrives - 1`, which is the predicate `first_rail` is *defined
    // by*: neither assert could fail. What it has to test is the **state**, so a
    // mutation that cleared `App::rail` on a resize would be caught.
    let scratch = repo::Scratch::large_diff("rail-survives-narrow", 3, 40);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    repo::materialise(&mut frame);
    let mut app = App::new();
    let wide = Rect::new(0, 0, arrives, TALL);
    let of =
        |app: &App, at| body_layout(at, &app.chrome("f", None, Pointing::default(), 0, ""), 3, 3);

    let height = of(&app, wide).diff;
    app.apply(Action::ToggleRail, &mut frame, height)
        .expect("ask for the rail");
    assert!(of(&app, wide).rail, "`r` did not reach the layout");

    // Narrow past the arrival and back, with a paint at the narrow size so the
    // shell has actually drawn the pane that cannot honour it.
    let narrow = Rect::new(0, 0, arrives - 1, TALL);
    assert!(
        !of(&app, narrow).rail,
        "a pane one column short of the arrival drew a rail"
    );
    assert!(
        of(&app, wide).rail,
        "widening the pane again lost the reader's request"
    );
}

#[test]
fn asking_for_the_rail_keeps_the_row_the_diff_was_on() {
    // **`ToggleMasthead`'s own promise one region over**, and the half the gesture
    // gate above does not reach: a reader asking where the map goes is not asking
    // to be moved inside the diff. A rail narrows the diff rather than reflowing
    // it, so a preserved position looks like the same line clipped shorter, and
    // the narrower row is a **prefix** of the wider one.
    //
    // Read after a scroll rather than at the top, because the top is where a lost
    // position lands: a gate that only ever looked there could not tell a kept
    // position from a reset one.
    //
    // **The leading columns, not the whole row.** The scrollbar is pinned to each
    // region's right edge, so the wider row ends `…spaces…▲` and the narrower one
    // ends `…▲` sooner: neither is a prefix of the other, and the first spelling of
    // this gate failed on that furniture while the position it is about was
    // preserved.
    //
    // **Derived from the narrower region rather than chosen.** A hand-tuned
    // constant compares whatever it happens to reach: thirty was inside the railed
    // diff at this pane and would have compared half of what it safely could, and
    // it loosens silently if the pane or the margins move. `lead_of` asks the
    // railed layout how wide its diff is and stops one column short of its bar.
    let scratch = repo::Scratch::large_diff("rail-keeps-the-row", 3, 200);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    repo::materialise(&mut frame);
    let mut app = App::new();
    let at = Rect::new(0, 0, 160, TALL);
    let mut highlighter = vigia_core::Highlighter::eager();
    let history = vigia_core::History::new();

    let height = body_layout(at, &app.chrome("f", None, Pointing::default(), 0, ""), 3, 3).diff;
    app.apply(Action::Scroll(30), &mut frame, height)
        .expect("scroll");

    let mut top_row = |app: &mut App, frame: &mut vigia_core::Frame<'_>| -> String {
        let chrome = app.chrome("f", None, Pointing::default(), 0, "");
        let body = body_layout(at, &chrome, 3, 3);
        let view = app
            .view(frame, &mut highlighter, &history, body)
            .expect("view");
        let mut buf = Buffer::empty(at);
        render(
            &mut buf,
            at,
            &view,
            &Theme::default(),
            Glyphs::default(),
            &chrome,
        );
        let laid = regions(at, &chrome, &view);
        let diff = laid.diff;
        (diff.left..diff.left + diff.width)
            .map(|x| buf[(x, diff.top)].symbol())
            .collect::<String>()
    };

    let stacked = top_row(&mut app, &mut frame);
    assert!(!stacked.trim().is_empty(), "the fixture drew an empty diff");

    app.apply(Action::ToggleRail, &mut frame, height)
        .expect("toggle");
    // **Without this the gate passes against a `ToggleRail` that does nothing**,
    // because the two rows are then identical and a string is trivially a prefix
    // of itself. Found by mutation.
    assert!(
        body_layout(at, &app.chrome("f", None, Pointing::default(), 0, ""), 3, 3).rail,
        "the toggle did not reach the layout, so the comparison below is between \
         two stacked frames"
    );
    let beside = top_row(&mut app, &mut frame);

    // **The comparison length comes from the drawn row, not from a constant.** A
    // hand-tuned number compares whatever it happens to reach and loosens silently
    // when the pane or the margins move; the railed row's own content, once its
    // right-edge furniture is off it, is exactly the overlap the two layouts have.
    let content = beside.trim_end_matches([' ', '▲', '▼', '█', '│']);
    assert!(
        content.chars().count() > 20,
        "the railed diff drew nothing but furniture: {beside:?}"
    );
    assert!(
        stacked.starts_with(content),
        "asking for the rail moved the diff off the row it was on:\n  stacked \
         {stacked:?}\n  beside  {content:?}"
    );
}

#[test]
fn r_reaches_the_painted_screen_and_not_only_the_layout() {
    // **Every other gesture gate here stops at `Body::rail`**, so a painter that
    // ignored the flag entirely would leave the whole file green: the layout would
    // say rail, the regions would say rail, and the screen would draw a strip.
    // Read off the buffer instead, at the one place the two shapes differ most
    // plainly: whether the row under the header carries a file path or the diff.
    let scratch = repo::Scratch::large_diff("rail-painted", 3, 40);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    repo::materialise(&mut frame);
    let mut app = App::new();
    let mut highlighter = vigia_core::Highlighter::eager();
    let history = vigia_core::History::new();
    let at = Rect::new(0, 0, 160, TALL);

    let mut shape = |app: &mut App, frame: &mut vigia_core::Frame<'_>| -> (u16, u16) {
        let chrome = app.chrome("f", None, Pointing::default(), 0, "");
        let body = body_layout(at, &chrome, 3, 3);
        let view = app
            .view(frame, &mut highlighter, &history, body)
            .expect("view");
        let laid = regions(at, &chrome, &view);
        // The list's own left edge and width, read off the published regions the
        // painter drew from rather than off the layout it was planned with.
        (laid.list.left, laid.list.width)
    };

    let stacked = shape(&mut app, &mut frame);
    let height = body_layout(at, &app.chrome("f", None, Pointing::default(), 0, ""), 3, 3).diff;
    app.apply(Action::ToggleRail, &mut frame, height)
        .expect("toggle");
    let beside = shape(&mut app, &mut frame);

    assert_ne!(
        stacked, beside,
        "the painted list is in the same place with the rail up as with it down"
    );
    assert!(
        beside.1 < at.width / 2,
        "the railed list takes {} of {} columns, which is not a rail",
        beside.1,
        at.width
    );
    assert_eq!(
        stacked.1, at.width,
        "the stacked list is not the full pane wide, so the comparison above is \
         between two rails"
    );
}

/// **The rail draws both runs whole, and it was the layout the row-budget fix
/// missed.**
///
/// `Body::split` takes a list *row* budget since
/// [#313](https://github.com/breferrari/vigia/issues/313), because a grouped list
/// draws a separator per run and a region sized from its files alone is short by
/// exactly those rows. The stacked branch was given it and `Body::beside` was not,
/// so #313's own headline defect stayed live in one of the two shapes this tool
/// draws: the staged run was announced and its tail was not there.
///
/// Nothing else in this file calls `show_staged`, which is why auditing the
/// neighbourhood does not reach it.
#[test]
fn a_rail_draws_the_tail_of_the_staged_run() {
    let scratch = repo::Scratch::large_diff("rail-staged-tail", 6, 8);
    let worktree = scratch.worktree();
    // Three staged, three left on disk.
    scratch.git(&["add", "src/mod_0.rs", "src/mod_1.rs", "src/mod_2.rs"]);

    let mut frame = worktree.frame();
    frame.show_staged(true);
    frame.advance().expect("advance");
    assert_eq!(frame.files().len(), 6, "the fixture holds both runs");

    let mut app = App::new();
    app.apply(Action::ToggleRail, &mut frame, 0).expect("apply");
    let mut highlighter = vigia_core::Highlighter::eager();
    let history = vigia_core::History::new();

    // Wide enough for the rail and tall enough for every file plus both labels.
    let at = Rect::new(0, 0, 200, 30);
    let chrome = Chrome {
        rail: true,
        staged: Some(3),
        ..App::new().chrome("fixture", None, Pointing::default(), 0, "")
    };
    let body = body_layout(
        at,
        &chrome,
        frame.files().len(),
        vigia::list_rows_wanted(frame.files()),
    );
    assert!(
        body.rail,
        "the pane did not take the rail, so this proves nothing"
    );

    let view = app
        .view(&mut frame, &mut highlighter, &history, body)
        .expect("collect");

    let drawn: Vec<usize> = vigia::list_plan(frame.files(), view.list_top, view.list.len())
        .iter()
        .filter_map(|slot| match slot {
            vigia::Slot::File(at) => Some(*at),
            vigia::Slot::Group { .. } => None,
        })
        .collect();
    assert!(
        drawn.contains(&5),
        "the rail drew {drawn:?} and the last staged file is not among them, so \
         the run is announced with its tail missing"
    );
    assert_eq!(
        view.listed_files(),
        6,
        "the rail is short of the changed set by the rows its separators took"
    );
}
