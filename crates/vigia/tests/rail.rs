//! The left rail: the layout where the pinned list sits **beside** the diff
//! rather than above it, `SPEC.md` §11.1's widest arrangement.

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
const CARET_WIDTH: usize = 1;
const KIND_WIDTH: usize = 2;

/// The path columns the rail promises beside a settled glance cluster, restated
/// from `render.rs`'s private `RAIL_PATH`.
const RAIL_PATH: usize = MIN_PATH_WIDTH * 2;

/// The pane `assets/preview.svg` is measured from, which §5.1 makes the picture's
/// own width.
const PICTURED_PANE: u16 = 109;

/// Well past the widest pane anyone runs, so a sweep measures the rule rather
/// than the rungs that happen to be reachable.
const WIDEST: u16 = 240;

/// Past the width at which the rail's own ladder would climb off the settled
/// rung, which is a pane of about four hundred.
const PAST_THE_CLIMB: u16 = 420;

/// The pane at which the rail's own glance ladder leaves the settled rung.
const THE_CLIMB: u16 = 402;

/// A pane tall enough that neither region is the thing giving way.
const TALL: u16 = 24;

/// A shell that has asked for the rail, which is what every gate below is about.
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
const LONG: &str = "crates/vigia-core/src/engine/incremental/watch.rs";

/// A screen with both regions carrying a file heading, which is the only screen
/// this file is about.
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
fn drawn_at(width: u16, height: u16, view: &View, glyphs: Glyphs) -> Buffer {
    let area = Rect::new(0, 0, width, height);
    let mut buf = Buffer::empty(area);
    render(&mut buf, area, view, &Theme::default(), glyphs, &chrome());
    buf
}

/// Every glyph rung the sparkline can be drawn from.
const RUNGS: [Glyphs; 3] = [Glyphs::Block, Glyphs::Braille, Glyphs::Octant];

/// What one region draws on one row: whether the counts cell is there, how many
/// heat slices, how many sparkline cells.
type Rung = (bool, usize, usize);

/// What one region draws on one row: whether the counts cell is there, how many
/// heat slices, how many sparkline cells.
fn rung(buf: &Buffer, theme: &Theme, y: u16, columns: std::ops::Range<u16>) -> Rung {
    rung_at(buf, theme, y, columns, Glyphs::Block)
}

/// The same, reading the alphabet a named glyph rung draws from.
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
fn columns_of(region: vigia::Region) -> std::ops::Range<u16> {
    region.left..region.left + region.width
}

/// The rung each region draws at this width, read at each region's **own** first
/// row.
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
