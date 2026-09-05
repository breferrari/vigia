//! What the palettes actually put on the screen.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use std::collections::HashSet;

use ratatui::style::{Color, Style};
use vigia::{
    Chrome, Depth, FileEntry, Glyphs, HEAT_BUCKETS, HeatBucket, Mode, Position, Row, Scale, Theme,
    View, render,
};
use vigia_core::{HISTORY_BUCKETS, LineKind, Origin, Recency};

/// Buckets a sparkline draws on the panes this file renders at.
const DRAWN_BUCKETS: usize = 12;

/// The heat strip's slice, restated rather than imported: a test sharing the
/// renderer's constant agrees with it by construction instead of checking it.
const HEAT_SLICE: &str = "■";

/// The sparkline's ramp, shortest first, restated for [`HEAT_SLICE`]'s reason
/// and declared once for `tests/render.rs`'s: two copies in one binary check
/// each other rather than the renderer, and they drift independently.
const RAMP: [&str; 8] = ["▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];

/// Every stop of the sparkline's ramp, quietest first, with the key it came from.
fn spark_stops(theme: &Theme) -> [(&'static str, Style); 3] {
    [
        ("spark", theme.spark),
        ("spark_warm", theme.spark_warm),
        ("spark_hot", theme.spark_hot),
    ]
}

fn chrome() -> Chrome {
    Chrome {
        pressed: None,
        gripped: None,
        hovered: None,
        selected: None,
        scrolling: None,
        worktree: "vigia".to_owned(),
        staged: None,
        elsewhere: 0,
        branch: None,
        mode: Mode::Watching,
        notice: None,
        voice: None,
        following: false,
        masthead: true,
        rail: false,
        sheet: None,
        icons: false,
        links: false,
        root: String::new(),
        frame: None,
        memory: None,
        notes: (0, 0),
    }
}

fn line(kind: LineKind, number: u32, text: &str) -> Row {
    Row::Line {
        kind,
        number,
        text: text.to_owned(),
        spans: Vec::new(),
        emph: Vec::new(),
    }
}

/// A file, a hunk, and one line of each kind, in a known order.
fn three_kinds() -> View {
    View {
        whole: Vec::new(),
        landed: false,
        recorded: 0,
        list_span: 1,
        grouped: false,
        list: Vec::new(),
        list_top: 0,
        current_span: 0,
        total_rows: 0,
        rows_above: 0,
        rows: vec![
            Row::file(FileEntry {
                origin: Origin::Unstaged,
                path: "src/a.rs".to_owned(),
                from: None,
                kind: 'M',
                churn: Some((2, 1)),
                spark: [0; HISTORY_BUCKETS],
                recency: Recency::Cold,
                newest: false,
                heat: [HeatBucket::default(); HEAT_BUCKETS],
            }),
            Row::Hunk {
                old_start: 1,
                old_lines: 2,
                new_start: 1,
                new_lines: 3,
            },
            line(LineKind::Context, 1, "let a = 1;"),
            line(LineKind::Added, 2, "let b = 2;"),
            line(LineKind::Removed, 2, "let c = 3;"),
            // Below the changed rows, and that placement is the whole reason it exists.
            line(LineKind::Context, 3, "let d = 4;"),
        ],
        files: 1,
        top: Position::default(),
        read: 1,
        scale: Scale::flat(0),
        gutter: None,
        worktree_churn: Default::default(),
        notes: Default::default(),
    }
}

/// Row 0 is the header, so the body's first row is 1 and these are offset by it.
const HEADING: u16 = 1;
const CONTEXT: u16 = 3;
const ADDED: u16 = 4;
const REMOVED: u16 = 5;
/// The context row under both changed rows. See [`three_kinds`].
const CONTEXT_BELOW: u16 = 6;

fn draw(width: u16, height: u16, view: &View, theme: Theme) -> TestBackend {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
    terminal
        .draw(|f| {
            let area = f.area();
            render(
                f.buffer_mut(),
                area,
                view,
                &theme,
                Glyphs::default(),
                &chrome(),
            );
        })
        .expect("draw");
    terminal.backend().clone()
}

/// Every background on row `y`, one per cell, left to right.
fn backgrounds(backend: &TestBackend, y: u16) -> Vec<Option<Color>> {
    backgrounds_from(backend, y, 0)
}

/// The same, from a given column, so a gate can ask about the row without the
/// pane's leading cell.
fn backgrounds_from(backend: &TestBackend, y: u16, from: u16) -> Vec<Option<Color>> {
    let buffer = backend.buffer();
    (from..buffer.area.width)
        .map(|x| {
            let bg = buffer[(x, y)].style().bg;
            // `Reset` and unset are the same claim from a palette's side: neither
            // paints anything over the reader's own pane. Folded here so a gate
            // says "no wash" once rather than twice.
            bg.filter(|colour| *colour != Color::Reset)
        })
        .collect()
}

fn wash_of(theme: Theme, added: bool) -> Color {
    let style = if added {
        theme.added_row
    } else {
        theme.removed_row
    };
    style.bg.expect("this palette washes its rows")
}

#[test]
fn the_fixture_lands_where_these_say() {
    // Every gate below indexes rows by the constants above, so if the layout ever moves
    // this is the one that says so, by name, instead of five gates failing with
    // assertions about colour.
    let backend = draw(60, 9, &three_kinds(), Theme::ansi());
    let buffer = backend.buffer();
    let row = |y: u16| -> String { (0..60).map(|x| buffer[(x, y)].symbol()).collect::<String>() };

    assert!(row(HEADING).contains("src/a.rs"), "{:?}", row(HEADING));
    assert!(row(CONTEXT).contains("let a = 1;"), "{:?}", row(CONTEXT));
    assert!(row(ADDED).contains("let b = 2;"), "{:?}", row(ADDED));
    assert!(row(REMOVED).contains("let c = 3;"), "{:?}", row(REMOVED));
    assert!(
        row(CONTEXT_BELOW).contains("let d = 4;"),
        "{:?}",
        row(CONTEXT_BELOW)
    );
}

#[test]
fn a_changed_row_is_washed_to_the_pane_edge() {
    // To the edge, which is the whole assertion.
    let dark = Theme::dark().resolve(Depth::Truecolor);
    let backend = draw(60, 8, &three_kinds(), dark);

    for (row, added) in [(ADDED, true), (REMOVED, false)] {
        let want = wash_of(dark, added);
        let got = backgrounds(&backend, row);
        assert_eq!(got.len(), 60, "row {row} is not the full pane");
        for (x, bg) in got.iter().enumerate() {
            // The sigil cell is the bar and carries the diff hue instead, which is
            // the one exception and has its own gate below.
            if bg.as_ref() == Some(&want) {
                continue;
            }
            assert!(
                x < 4,
                "row {row} column {x} is {bg:?}, not the wash {want:?}"
            );
        }
        assert_eq!(
            got.last().copied().flatten(),
            Some(want),
            "row {row} stops washing before the pane edge"
        );
    }
}

#[test]
fn the_sigil_keeps_its_own_colour_on_the_wash() {
    // The sigil is not the bar, which is a ruling this branch made and then reversed on
    // seeing it.
    let dark = Theme::dark().resolve(Depth::Truecolor);
    let backend = draw(60, 12, &three_kinds(), dark);
    let buffer = backend.buffer();

    for (row, added, sigil) in [(ADDED, true, "+"), (REMOVED, false, "-")] {
        let wash = wash_of(dark, added);
        let want = if added { dark.added } else { dark.removed };
        let at = (0..60)
            .find(|x| buffer[(*x, row)].symbol() == sigil)
            .unwrap_or_else(|| panic!("no {sigil:?} on row {row}"));

        let cell = buffer[(at, row)].style();
        assert_eq!(
            cell.fg, want.fg,
            "the sigil lost its diff colour on row {row}"
        );
        assert_eq!(
            cell.bg,
            Some(wash),
            "the sigil cell is not on the row's wash on row {row}"
        );
    }
}

#[test]
fn the_readme_recipe_for_terminal_colours_plus_a_wash_draws_one() {
    // A published recipe with no gate is a promise nothing keeps.
    let theme = vigia::theme::parse(
        "base        = ansi\n\
         added_row   = on #1b3d29\n\
         removed_row = on #45222a\n",
    )
    .expect("the README's recipe parses")
    .resolve(Depth::Truecolor);

    // Still the reader's own scheme everywhere else, which is the half that would
    // be silently lost if `base` were dropped or a value replaced a whole style.
    assert_eq!(theme.added, Theme::ansi().added);
    assert_eq!(theme.context, Theme::ansi().context);

    let backend = draw(60, 8, &three_kinds(), theme);
    for (row, added) in [(ADDED, true), (REMOVED, false)] {
        let want = wash_of(theme, added);
        assert_eq!(
            backgrounds(&backend, row).last().copied().flatten(),
            Some(want),
            "row {row} draws no wash, so the recipe in the README does nothing"
        );
    }
    assert_ne!(
        wash_of(theme, true),
        wash_of(theme, false),
        "an addition and a removal wash the same, which is the one thing no rung \
         may do"
    );
}

#[test]
fn every_palette_draws_a_bar_and_draws_it_as_a_background() {
    // The opposite of what a gate here would naturally assert.
    for (name, theme) in [
        ("ansi", Theme::ansi()),
        ("dark", Theme::dark()),
        ("light", Theme::light()),
    ] {
        for (label, bar) in [("added", theme.added_bar), ("removed", theme.removed_bar)] {
            assert!(
                bar.bg.is_some(),
                "{name}'s {label} bar is unset, so the row band has no left edge"
            );
            assert!(
                bar.fg.is_none(),
                "{name}'s {label} bar sets a foreground, which is a colour on a                  blank cell and means it is riding a glyph again"
            );
        }
        assert_ne!(
            theme.added_bar.bg, theme.removed_bar.bg,
            "{name} draws an addition and a removal in one bar colour, which is              the one thing the element exists to distinguish"
        );
    }
}

#[test]
fn a_context_row_is_never_washed() {
    // The wash *is* the diff signal once the sigil column stops being it, so a context
    // line carrying one would say a line changed that did not.
    let dark = Theme::dark().resolve(Depth::Truecolor);
    let backend = draw(60, 12, &three_kinds(), dark);

    for row in [CONTEXT, CONTEXT_BELOW] {
        assert!(
            backgrounds(&backend, row).iter().all(Option::is_none),
            "context row {row} was washed"
        );
    }

    // And nothing below the diff either.
    for row in CONTEXT_BELOW + 1..11 {
        assert!(
            backgrounds(&backend, row).iter().all(Option::is_none),
            "row {row}, below the whole diff, was washed"
        );
    }
}

#[test]
fn nothing_a_reader_has_to_read_is_drawn_in_colour_eight() {
    // Colour 8 is not a colour, it is a relationship to the background.
    let ansi = Theme::ansi();
    let Theme {
        // Read by a reader, so none of these may be colour 8.
        chrome,
        chrome_dim,
        staged,
        path,
        path_live,
        path_cold,
        // Text, so it is checked with the rest, and the one field here that
        // carries a modifier as part of its meaning: `path_hover` underlines,
        // which is the whole of what keeps it apart from the recency ladder.
        path_hover,
        kind,
        hunk,
        gutter,
        context,
        note,
        alert,
        comment,

        // Exempt: a track is not text.
        heat_track: _,
        bar_track: _,
        spark_track: _,

        // Exempt: backgrounds by contract, and unset on this palette anyway.
        // The word patch and the gutter tone exist only where the wash does,
        // and `ansi` draws no wash at any depth.
        added_word: _,
        removed_word: _,
        added_gutter: _,
        removed_gutter: _,

        // Exempt: marks and fills, none of them text, none of them colour 8.
        pulse: _,
        spark: _,
        spark_warm: _,
        spark_hot: _,
        bar: _,
        // A gesture in progress, brighter than `bar` and never text.
        bar_active: _,
        // A pointer at rest on the bar, between `bar` and `bar_active`, and a
        // mark rather than text for the same reason they are.
        bar_hover: _,
        heat_added: _,
        heat_added_warm: _,
        heat_added_hot: _,
        heat_removed: _,
        heat_removed_warm: _,
        heat_removed_hot: _,
        heat_mixed: _,
        heat_mixed_warm: _,
        heat_mixed_hot: _,
        added: _,
        removed: _,
        added_row: _,
        removed_row: _,
        added_bar: _,
        removed_bar: _,
        // Exempt for the reason the washes above it are, and one further: on this
        // palette it carries no colour at all, only `REVERSED`, because a
        // background has to assume one and this is the palette that cannot.
        selection: _,

        // Exempt: syntax classes. Read, but never in grey: they carry hue, and
        // `a_ramp_that_survives_sixteen_colours_is_still_a_ramp` covers them.
        keyword: _,
        type_name: _,
        function: _,
        variable: _,
        constant: _,
        string: _,
        number: _,
    } = ansi;

    let readable = [
        ("chrome", chrome),
        ("chrome_dim", chrome_dim),
        // A run separator's word and a staged row's gutter mark, and both are read.
        ("staged", staged),
        ("path", path),
        ("path_live", path_live),
        ("path_cold", path_cold),
        ("path_hover", path_hover),
        ("gutter", gutter),
        ("kind", kind),
        ("hunk", hunk),
        ("note", note),
        ("alert", alert),
        ("context", context),
        ("comment", comment),
    ];

    for (name, style) in readable {
        assert_ne!(
            style.fg,
            Some(Color::DarkGray),
            "{name} is drawn in colour 8, which some schemes put on the background"
        );
    }

    // And it may not reach for `DIM` either, which was the first replacement and was
    // invisible on the same terminal that colour 8 was: a terminal is free to reduce
    // intensity as far as it likes.
    for (name, style) in [
        ("chrome_dim", ansi.chrome_dim),
        ("gutter", ansi.gutter),
        ("comment", ansi.comment),
    ] {
        assert_eq!(style.fg, Some(Color::Gray), "{name}");
        assert!(
            !style.add_modifier.contains(ratatui::style::Modifier::DIM),
            "{name} leans on DIM, which a terminal may render as invisible"
        );
    }
}

#[test]
fn the_ansi_palette_draws_no_wash_at_any_depth() {
    // The ruling that keeps palette and depth genuinely independent axes. A wash
    // has to assume a background; `ansi` resolves to the reader's own scheme and so
    // assumes none, and it refuses at truecolour just as firmly as at sixteen.
    for depth in [Depth::Truecolor, Depth::Ansi256, Depth::Ansi16, Depth::None] {
        let backend = draw(60, 8, &three_kinds(), Theme::ansi().resolve(depth));
        for row in [CONTEXT, ADDED, REMOVED] {
            assert!(
                backgrounds_from(&backend, row, 1)
                    .iter()
                    .all(Option::is_none),
                "ansi washed row {row} at {depth:?}"
            );
        }
    }
}

#[test]
fn sixteen_colours_draw_no_background_and_keep_the_sigil() {
    // `SPEC.md` §11.1's recorded loss, now a property of the *depth* rather than of the
    // tool.
    let dark = Theme::dark();
    for depth in [Depth::Ansi256, Depth::Ansi16, Depth::None] {
        let backend = draw(60, 8, &three_kinds(), dark.resolve(depth));
        let buffer = backend.buffer();
        for row in [ADDED, REMOVED] {
            assert!(
                backgrounds(&backend, row).iter().all(Option::is_none),
                "{depth:?} kept a background on row {row}"
            );
        }

        let at = (0..60)
            .find(|x| buffer[(*x, ADDED)].symbol() == "+")
            .expect("a sigil");
        assert_eq!(
            buffer[(at, ADDED)].style().fg,
            dark.resolve(depth).added.fg,
            "{depth:?} lost the sigil's own colour"
        );
    }
}

#[test]
fn the_wash_changes_no_symbol_and_so_cannot_move_the_layout() {
    // I6 is a claim about where things land, and a background is the one kind of change
    // that must not touch that.
    for width in [40, 60, 80, 120] {
        let washed = draw(
            width,
            8,
            &three_kinds(),
            Theme::dark().resolve(Depth::Truecolor),
        );
        let plain = draw(
            width,
            8,
            &three_kinds(),
            Theme::ansi().resolve(Depth::Truecolor),
        );
        let (a, b) = (washed.buffer(), plain.buffer());
        for y in 0..8 {
            let left: Vec<_> = (0..width).map(|x| a[(x, y)].symbol()).collect();
            let right: Vec<_> = (0..width).map(|x| b[(x, y)].symbol()).collect();
            assert_eq!(left, right, "row {y} at width {width} moved");
        }
    }
}

/// The pane the graded fixture is drawn on.
const GRADED_PANE: u16 = 140;

/// A file whose heat profile has a slice in each band.
fn graded_heat() -> View {
    let mut heat = [HeatBucket::default(); HEAT_BUCKETS];
    heat[0] = HeatBucket {
        added: 12,
        removed: 0,
    };
    heat[1] = HeatBucket {
        added: 7,
        removed: 0,
    };
    heat[2] = HeatBucket {
        added: 4,
        removed: 0,
    };
    heat[3] = HeatBucket {
        added: 3,
        removed: 0,
    };
    View {
        whole: Vec::new(),
        landed: false,
        recorded: 0,
        list_span: 1,
        grouped: false,
        list: Vec::new(),
        list_top: 0,
        current_span: 0,
        total_rows: 0,
        rows_above: 0,
        rows: vec![Row::file(FileEntry {
            origin: Origin::Unstaged,
            path: "src/a.rs".to_owned(),
            from: None,
            kind: 'M',
            churn: Some((25, 0)),
            spark: [0; HISTORY_BUCKETS],
            recency: Recency::Cold,
            newest: false,
            heat,
        })],
        files: 1,
        top: Position::default(),
        read: 1,
        scale: Scale::flat(0),
        gutter: None,
        worktree_churn: Default::default(),
        notes: Default::default(),
    }
}

/// Distinct foregrounds on the file heading, ignoring the track and the counters.
fn heat_stops(theme: Theme) -> Vec<Color> {
    let backend = draw(GRADED_PANE, 4, &graded_heat(), theme);
    let buffer = backend.buffer();
    let track = theme.heat_track.fg;
    let mut seen: Vec<Color> = Vec::new();
    for x in 0..GRADED_PANE {
        let cell = &buffer[(x, HEADING)];
        // Symbol and style together.
        if cell.symbol() != HEAT_SLICE {
            continue;
        }
        let Some(fg) = cell.style().fg else { continue };
        if Some(fg) == track || seen.contains(&fg) {
            continue;
        }
        seen.push(fg);
    }
    seen
}

/// The strip's slice colours in order, track included, so a band can be asserted
/// by position rather than by counting.
fn heat_sequence(theme: Theme) -> Vec<Color> {
    let backend = draw(GRADED_PANE, 4, &graded_heat(), theme);
    let buffer = backend.buffer();
    (0..GRADED_PANE)
        .filter(|x| buffer[(*x, HEADING)].symbol() == HEAT_SLICE)
        .filter_map(|x| buffer[(x, HEADING)].style().fg)
        .collect()
}

#[test]
fn each_slice_lands_in_the_band_its_share_puts_it_in() {
    // Counting distinct colours cannot test the thresholds, and this gate exists
    // because a mutation proved it.
    let theme = Theme::dark().resolve(Depth::Truecolor);
    let got = heat_sequence(theme);

    assert_eq!(got[0], theme.heat_added_hot.fg.unwrap(), "12 of 12 is hot");
    assert_eq!(
        got[1],
        theme.heat_added_warm.fg.unwrap(),
        "7 of 12 is above half and below two thirds, so it is warm"
    );
    assert_eq!(got[2], theme.heat_added_warm.fg.unwrap(), "4 of 12 is warm");
    assert_eq!(got[3], theme.heat_added.fg.unwrap(), "3 of 12 is low");
    assert_eq!(got[4], theme.heat_track.fg.unwrap(), "an empty slice");
}

#[test]
fn a_ramp_that_survives_sixteen_colours_is_still_a_ramp() {
    // `dark` is authored in 24-bit and quantised down, unlike `ansi`, so what its ramp
    // becomes at sixteen colours is decided by the bright-variant threshold in
    // `to_ansi16` and by nothing else.
    let flat = Theme::dark().resolve(Depth::Ansi16);
    let mut seen: Vec<Color> = Vec::new();
    for colour in [flat.heat_added, flat.heat_added_warm, flat.heat_added_hot] {
        let fg = colour.fg.expect("a colour");
        if !seen.contains(&fg) {
            seen.push(fg);
        }
    }
    assert!(
        seen.len() >= 2,
        "the added ramp collapsed to {seen:?} at sixteen colours"
    );
}

#[test]
fn the_heat_ramp_has_three_stops_where_the_depth_can_draw_them() {
    // The picture ramps its additions across three greens and the strip is the one
    // element whose intensity `assets/preview.svg` actually specifies.
    let three = heat_stops(Theme::dark().resolve(Depth::Truecolor));
    assert_eq!(three.len(), 3, "expected three stops, got {three:?}");

    let indexed = heat_stops(Theme::dark().resolve(Depth::Ansi256));
    assert_eq!(indexed.len(), 3, "256 lost a stop: {indexed:?}");
}

/// One file whose eight buckets climb, so every stop of the ramp is on one row.
fn climbing() -> View {
    View {
        rows: vec![Row::file(FileEntry {
            origin: Origin::Unstaged,
            path: "src/a.rs".to_owned(),
            from: None,
            kind: 'M',
            churn: Some((12, 0)),
            spark: [
                0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 4, 4, 5, 5, 7, 7, 9, 9, 12, 12,
            ],
            recency: Recency::Cold,
            newest: false,
            heat: [HeatBucket::default(); HEAT_BUCKETS],
        })],
        files: 1,
        scale: Scale::spread(12),
        gutter: None,
        worktree_churn: Default::default(),
        notes: Default::default(),
        ..View::default()
    }
}

#[test]
fn a_taller_sparkline_bucket_is_drawn_hotter() {
    // The ramp, read off the cells rather than off the theme, which is the half
    // `the_sparkline_ramp_has_three_stops_where_the_depth_can_draw_them` cannot reach:
    // that one holds the palette apart, this one holds that the renderer spends it in
    // the right order.
    let theme = Theme::dark().resolve(Depth::Truecolor);
    let backend = draw(60, 4, &climbing(), theme);
    let buffer = backend.buffer();

    let drawn: Vec<(usize, Option<Color>)> = (0..60)
        .map(|x| &buffer[(x, HEADING)])
        .filter_map(|cell| {
            RAMP.iter()
                .position(|glyph| *glyph == cell.symbol())
                .map(|rung| (rung, cell.style().fg))
        })
        .collect();
    assert!(
        drawn.len() >= 4,
        "the fixture drew {} buckets, which is too few to see a ramp in",
        drawn.len()
    );

    // Rank, not identity: the claim is monotone, so it survives the stops moving.
    let ramp = theme.spark_ramp();
    let stops = spark_stops(&theme);
    let ranked: Vec<(usize, usize)> = drawn
        .iter()
        .map(|&(rung, fg)| {
            let rank = match ramp.as_ref() {
                Some(ramp) => ramp.iter().position(|stop| Some(*stop) == fg),
                None => stops.iter().position(|(_, style)| style.fg == fg),
            }
            .unwrap_or_else(|| {
                panic!("the bucket at rung {rung} is drawn in {fg:?}, which is no stop of the ramp")
            });
            (rung, rank)
        })
        .collect();

    for pair in ranked.windows(2) {
        let ((short, cooler), (tall, hotter)) = (pair[0], pair[1]);
        assert!(
            hotter >= cooler,
            "the bucket at rung {tall} is drawn cooler than the shorter one at              rung {short} beside it, so the ramp runs backwards"
        );
    }
    assert!(
        ranked.last().expect("a bucket").1 > ranked[0].1,
        "every bucket on a climbing row took one stop, so the ramp is flat"
    );
}

#[test]
fn the_sparkline_ramp_has_three_stops_where_the_depth_can_draw_them() {
    for (name, base) in [("dark", Theme::dark()), ("light", Theme::light())] {
        for depth in [Depth::Truecolor, Depth::Ansi256] {
            let theme = base.resolve(depth);
            // Deduped rather than compared pairwise: `Color` is `Hash` as well as
            // `Eq`, so three-into-a-set says the same thing in one assertion and
            // keeps saying it if a fourth stop is ever added.
            let stops = spark_stops(&theme);
            let distinct: HashSet<Option<Color>> =
                stops.iter().map(|(_, style)| style.fg).collect();
            assert_eq!(
                distinct.len(),
                stops.len(),
                "{name} at {depth:?} draws two sparkline stops alike: {stops:?}"
            );
        }
    }

    // And `ansi` spends two, which is a fact about the palette rather than a shortfall,
    // exactly as `heat_added` records for itself: sixteen names hold a normal and a
    // bright of each hue and no third, so the middle stop is the normal one.
    let ansi = Theme::ansi();
    assert_eq!(
        ansi.spark.fg, ansi.spark_warm.fg,
        "ansi found a third cyan, so the two-stop note in its palette is stale"
    );
    assert_ne!(
        ansi.spark_warm.fg, ansi.spark_hot.fg,
        "ansi collapsed its ramp to one stop, so height is the only channel left"
    );
}

#[test]
fn a_sparkline_track_is_never_the_colour_of_a_bucket() {
    // The track and the bars land on the same eight columns of the same row, and the
    // track is drawn from `_` where a bar is drawn from a block, so glyph already
    // separates them for a reader.
    for (name, base) in [
        ("ansi", Theme::ansi()),
        ("dark", Theme::dark()),
        ("light", Theme::light()),
    ] {
        for depth in [Depth::Truecolor, Depth::Ansi256, Depth::Ansi16] {
            let theme = base.resolve(depth);
            for (stop, style) in spark_stops(&theme) {
                assert_ne!(
                    theme.spark_track.fg, style.fg,
                    "{name} at {depth:?} draws a track in {stop}'s own colour"
                );
            }
            assert_ne!(
                theme.spark_track.fg, theme.chrome_dim.fg,
                "{name} at {depth:?} draws the track in the chrome's dim grey"
            );
        }
    }
}

#[test]
fn a_sparkline_track_is_never_the_colour_behind_it() {
    // The failure a track has that a bucket does not: quantising into the background.
    for (name, base, behind) in [
        ("dark", Theme::dark(), Color::Black),
        ("light", Theme::light(), Color::White),
    ] {
        for depth in [Depth::Truecolor, Depth::Ansi256, Depth::Ansi16] {
            let theme = base.resolve(depth);
            assert_ne!(
                theme.spark_track.fg,
                Some(behind),
                "{name} at {depth:?} draws the sparkline track in the colour of \
                 the pane behind it, so a launched worktree draws a blank column \
                 again"
            );
        }
    }
}

#[test]
fn a_sparkline_track_is_never_the_colour_of_a_path() {
    // `track_at` in `tests/render.rs` reads a track by symbol and style, and the symbol
    // is `_`, which is also most of what a `snake_case` path is made of.
    for (name, base) in [
        ("ansi", Theme::ansi()),
        ("dark", Theme::dark()),
        ("light", Theme::light()),
    ] {
        for depth in [Depth::Truecolor, Depth::Ansi256, Depth::Ansi16] {
            let theme = base.resolve(depth);
            for (which, path) in [
                ("path", theme.path),
                ("path_live", theme.path_live),
                ("path_cold", theme.path_cold),
                // `path_hover` belongs here for the same reason the other three do: it
                // is drawn on a path, so an underscore in a file name must not resolve
                // to the sparkline's own track colour at any depth.
                ("path_hover", theme.path_hover),
            ] {
                // Only the comparison that actually collides is skipped.
                if name == "light" && depth == Depth::Ansi16 && which == "path_cold" {
                    continue;
                }
                assert_ne!(
                    theme.spark_track.fg, path.fg,
                    "{name} at {depth:?} draws {which} in the track's own colour, \
                     so an underscore in a file name is a track cell"
                );
            }
        }
    }

    // The exemption is a measurement, not a licence: if the palette ever frees a
    // grey, this fails and the `continue` above comes out.
    let light = Theme::light().resolve(Depth::Ansi16);
    assert_eq!(
        light.spark_track.fg, light.path_cold.fg,
        "`light` at sixteen colours no longer draws the track in `path_cold`'s \n         colour, so the skipped comparison above would now pass and the \n         `continue` should come out"
    );
}

#[test]
fn a_sparkline_track_is_told_from_a_bucket_with_no_colour_at_all() {
    // The depth the whole glyph choice was made for.
    let mut view = three_kinds();
    if let Row::File(entry) = &mut view.rows[0] {
        entry.spark = [
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 3, 3,
        ];
    }
    view.scale = Scale::flat(3);

    let flat = Theme::dark().resolve(Depth::None);
    assert_eq!(
        flat.spark.fg, flat.spark_track.fg,
        "this depth is supposed to have collapsed the two colours, so the \
         assertion below is not testing what it says it is"
    );

    let backend = draw(80, 8, &view, flat);
    let buffer = backend.buffer();
    let row: Vec<&str> = (0..buffer.area.width)
        .map(|x| buffer[(x, HEADING)].symbol())
        .collect();
    // Guard the fixture, the way `a_row_missing_a_glance_element_keeps_its_column` does
    // for digits: at this depth colour is gone, so `_` is counted by symbol alone
    // across the whole row, and an underscore in the path would be counted as a track
    // cell.
    let path_has_underscore = view.rows.iter().any(|row| match row {
        Row::File(entry) => entry.path.contains('_'),
        _ => false,
    });
    assert!(
        !path_has_underscore,
        "the fixture's path carries an underscore, which this gate counts as a \n         track cell"
    );

    let bars = row.iter().filter(|s| RAMP.contains(s)).count();
    let track = row.iter().filter(|s| **s == "_").count();

    assert_eq!(
        bars, 1,
        "one written bucket did not draw exactly one block with colour off"
    );
    assert_eq!(
        track,
        DRAWN_BUCKETS - 1,
        "every drawn bucket but the written one failed to draw a track with \
         colour off, so nothing separates them from it on a `NO_COLOR` terminal"
    );
}

// ---------------------------------------------------------------------------
// The theme file, which is how a palette is specified rather than what it draws.
// ---------------------------------------------------------------------------

use vigia::ThemeError;
use vigia::theme;

#[test]
fn a_theme_file_can_set_the_sparkline_track() {
    // Every palette field is a key by construction, because the `palette!` macro
    // derives `Theme::KEYS` from the struct, and
    // `every_key_the_struct_has_is_a_key_a_file_can_set` asserts that shape.
    let theme = theme::parse("base = dark\nspark_track = #ff00ff\n").expect("parses");
    assert_eq!(
        theme.spark_track.fg,
        Some(Color::Rgb(0xff, 0x00, 0xff)),
        "a theme file naming the sparkline track did not reach the field"
    );
    assert_eq!(
        theme.spark,
        Theme::dark().spark,
        "setting the track moved the bucket colour with it"
    );
    assert_eq!(
        theme.heat_track,
        Theme::dark().heat_track,
        "setting the sparkline's track moved the heat strip's, so the two are \
         one key after all"
    );
}

#[test]
fn a_theme_file_overrides_only_what_it_names() {
    // The property that makes a three-line theme file worth having. Everything not
    // named has to come from the base *unchanged*, including the thirty-odd fields
    // nobody thought about while writing it.
    let theme = theme::parse("base = dark\nadded = #ff0000\n").expect("parses");
    let base = Theme::dark();

    assert_eq!(theme.added.fg, Some(Color::Rgb(0xff, 0x00, 0x00)));
    assert_eq!(theme.removed, base.removed, "an unnamed key moved");
    assert_eq!(theme.keyword, base.keyword, "an unnamed key moved");
    assert_eq!(theme.added_row, base.added_row, "an unnamed key moved");

    // Everything but the one key, checked as a whole rather than by sampling three
    // fields and hoping. Reconstructing the base from the parsed theme by putting
    // the one override back must give the base exactly.
    let mut restored = theme;
    restored.added = base.added;
    assert_eq!(restored, base);
}

#[test]
fn the_default_base_is_ansi_so_a_file_starts_where_the_tool_does() {
    let theme = theme::parse("added = #ff0000\n").expect("parses");
    let mut restored = theme;
    restored.added = Theme::ansi().added;
    assert_eq!(restored, Theme::ansi());
}

#[test]
fn an_unknown_key_is_refused_rather_than_ignored() {
    // A silently dropped key is a theme that does nothing, and "it was discarded"
    // is the one explanation a reader cannot arrive at by looking at their screen.
    let err = theme::parse("base = dark\nadd = #ff0000\n").expect_err("refused");
    assert_eq!(
        err,
        ThemeError::UnknownKey {
            line: 2,
            key: "add".to_owned()
        }
    );
    assert!(err.to_string().contains("add"), "{err}");
}

#[test]
fn a_bad_value_names_the_line_and_the_text() {
    // The line number is the whole point of these errors. A theme file is the one
    // input here a reader wrote by hand, and "something is wrong somewhere" is not
    // something they can act on.
    let cases: &[(&str, ThemeError)] = &[
        (
            "added = #gg0000\n",
            ThemeError::UnknownColour {
                line: 1,
                value: "#gg0000".to_owned(),
            },
        ),
        (
            "added = #ff00\n",
            ThemeError::UnknownColour {
                line: 1,
                value: "#ff00".to_owned(),
            },
        ),
        (
            "\n\nadded = green blinking\n",
            ThemeError::UnknownModifier {
                line: 3,
                value: "blinking".to_owned(),
            },
        ),
        (
            "added\n",
            ThemeError::MissingSeparator {
                line: 1,
                text: "added".to_owned(),
            },
        ),
        (
            "base = solarized\n",
            ThemeError::UnknownBase {
                line: 1,
                name: "solarized".to_owned(),
            },
        ),
        (
            "added = green\nbase = dark\n",
            ThemeError::LateBase { line: 2 },
        ),
        // An unfinished line. Accepting it would set the key to no colour at all,
        // which is a theme file that changes something invisibly rather than not
        // changing it, and that is the same failure as a silently dropped key.
        ("added =\n", ThemeError::MissingValue { line: 1 }),
        // The case that made `words_of` positional. Read as a comment this parses
        // clean and leaves `added` blank, which is worse than any error here.
        (
            "added = #ff00000\n",
            ThemeError::UnknownColour {
                line: 1,
                value: "#ff00000".to_owned(),
            },
        ),
    ];

    for (source, want) in cases {
        let got = theme::parse(source).expect_err(source);
        assert_eq!(&got, want, "{source:?}");
        // Every message names its line, so a reader can go straight to it.
        let said = got.to_string();
        assert!(said.starts_with("line "), "{said:?} does not name its line");
    }
}

#[test]
fn a_value_carries_a_foreground_a_background_and_modifiers() {
    let theme = theme::parse(
        "path      = #e6edf3 bold\n\
         added_row = on #0f2c1c\n\
         context   = default\n\
         hunk      = 33 on 236 dim italic\n\
         comment   = bright-black\n",
    )
    .expect("parses");

    assert_eq!(theme.path.fg, Some(Color::Rgb(0xe6, 0xed, 0xf3)));
    assert!(
        theme
            .path
            .add_modifier
            .contains(ratatui::style::Modifier::BOLD)
    );

    // A background with no foreground, which is how a row wash is written and the
    // one shape a naive parser gets wrong by reading `on` as a colour.
    assert_eq!(theme.added_row.fg, None);
    assert_eq!(theme.added_row.bg, Some(Color::Rgb(0x0f, 0x2c, 0x1c)));

    assert_eq!(theme.context.fg, Some(Color::Reset));
    assert_eq!(theme.hunk.fg, Some(Color::Indexed(33)));
    assert_eq!(theme.hunk.bg, Some(Color::Indexed(236)));
    assert_eq!(theme.comment.fg, Some(Color::DarkGray));
}

#[test]
fn a_comment_does_not_eat_a_hex_colour() {
    // The collision every hex-colour config format has to resolve: `#` starts a
    // comment and also starts a colour. A `#` counts as a comment only when
    // whitespace precedes it, so a value's first token survives.
    let theme = theme::parse(
        "# a leading comment\n\
         \n\
         added = #3fb950 # the picture's green\n",
    )
    .expect("parses");
    assert_eq!(theme.added.fg, Some(Color::Rgb(0x3f, 0xb9, 0x50)));

    // A background with no foreground puts a space directly before its colour, and
    // it was the first shape a whitespace-based comment rule destroyed.
    let wash = theme::parse("added_row = on #0f2c1c # the picture's wash\n").expect("parses");
    assert_eq!(wash.added_row.bg, Some(Color::Rgb(0x0f, 0x2c, 0x1c)));
    assert_eq!(wash.added_row.fg, None);

    // And a comment after an ordinary value still ends it.
    let named = theme::parse("added = green # not a colour\n").expect("parses");
    assert_eq!(named.added.fg, Some(Color::Green));
}

#[test]
fn a_modifier_only_value_keeps_the_colour_it_did_not_name() {
    // `added = bold` reads as "make additions bold".
    let theme = theme::parse("base = dark\nadded = bold\n").expect("parses");
    assert_eq!(
        theme.added.fg,
        Theme::dark().added.fg,
        "a modifier-only value cleared the colour"
    );
    assert!(
        theme
            .added
            .add_modifier
            .contains(ratatui::style::Modifier::BOLD)
    );

    // And a value that does name a colour still replaces it outright.
    let named = theme::parse("base = dark\nadded = #ff0000\n").expect("parses");
    assert_eq!(named.added.fg, Some(Color::Rgb(0xff, 0x00, 0x00)));
}

#[test]
fn a_second_base_is_refused_rather_than_silently_winning() {
    // `touched` is only set by an ordinary key, so two `base` lines never tripped
    // the late-base guard and the second quietly discarded the first. Every other
    // way of writing something this parser cannot honour is an error.
    let err = theme::parse("base = dark\nbase = light\n").expect_err("refused");
    assert_eq!(err, ThemeError::RepeatedBase { line: 2 });
    assert!(err.to_string().starts_with("line 2"), "{err}");

    // One is still fine, and so is one after a comment.
    theme::parse("# a comment\nbase = dark\n").expect("parses");
}

#[test]
fn a_theme_file_saved_by_notepad_still_parses() {
    // Notepad's default UTF-8 save writes a BOM, and `str::trim` will not strip one:
    // U+FEFF is `Cf`, not `White_Space`, so it survives every trim in the parser and
    // lands inside the first key.
    let with_bom = theme::parse("\u{FEFF}base = dark\nadded = #ff0000\n").expect("parses");
    let without = theme::parse("base = dark\nadded = #ff0000\n").expect("parses");
    assert_eq!(with_bom, without);
}

#[test]
fn base_takes_a_trailing_comment_like_every_other_key() {
    // `base` was the one key read straight from the raw value rather than through
    // `words_of`, so the documented comment idiom failed on the line every theme
    // file starts with: `base = dark # the picture` reported that there is no
    // theme called "dark # the picture".
    let theme = theme::parse("base = dark # the picture's palette\n").expect("parses");
    assert_eq!(theme, Theme::dark());

    // And a base that really is unknown still says so, with the three names.
    let err = theme::parse("base = solarized # nope\n").expect_err("refused");
    assert_eq!(
        err,
        ThemeError::UnknownBase {
            line: 1,
            name: "solarized".to_owned()
        }
    );
}

#[test]
fn a_theme_reaches_the_renderer_resolved_whichever_source_it_came_from() {
    // `.resolve(depth)` written on each of three exits is gated on the built-in arm
    // alone at a lossy depth, so deleting it from either of the other two leaves the
    // whole suite green: at truecolour `resolve` is the identity, and every test
    // reaching those arms ran there.
    let home = home_with("resolved", Some("base = dark\n"));
    let file = home
        .join(".config")
        .join("vigia")
        .join("theme")
        .display()
        .to_string();
    let cases: [(&str, Vec<(String, String)>); 3] = [
        (
            "a built-in named by the variable",
            vec![("VIGIA_THEME".to_owned(), "dark".to_owned())],
        ),
        (
            "a file named by the variable",
            vec![("VIGIA_THEME".to_owned(), file)],
        ),
        (
            "the file found under HOME",
            vec![("HOME".to_owned(), home.display().to_string())],
        ),
    ];

    for (why, pairs) in cases {
        let theme = theme::from_env(Depth::Ansi16, env_of(pairs), None).expect("a theme");
        assert!(
            !matches!(theme.added.fg, Some(Color::Rgb(..))),
            "{why}: reached the renderer unresolved"
        );
    }
}

#[test]
fn every_key_the_struct_has_is_a_key_a_file_can_set() {
    // True by construction, since one macro emits the struct and the key list from the
    // same declaration.
    for key in Theme::KEYS {
        let source = format!("{key} = green\n");
        theme::parse(&source).unwrap_or_else(|e| panic!("{key} is not settable: {e}"));
    }
    assert!(
        Theme::KEYS.len() >= 30,
        "the key list looks truncated: {}",
        Theme::KEYS.len()
    );
}

#[test]
fn a_theme_is_resolved_to_the_depth_it_will_be_drawn_at() {
    // The seam that keeps quantising off the frame path: `from_env` hands back a
    // palette already in colours this terminal can show, so the renderer never
    // converts anything.
    let env = |key: &str| (key == "VIGIA_THEME").then(|| "dark".to_owned());
    let flat = theme::from_env(Depth::Ansi16, env, None).expect("a theme");
    assert!(
        !matches!(flat.added.fg, Some(Color::Rgb(..))),
        "the palette reached the renderer unresolved"
    );
}

/// A home directory holding a theme file, and an environment that points at it.
fn home_with(name: &str, contents: Option<&str>) -> std::path::PathBuf {
    let home = std::env::temp_dir().join(format!("vigia-theme-{name}"));
    let dir = home.join(".config").join("vigia");
    std::fs::create_dir_all(&dir).expect("home");
    let file = dir.join("theme");
    match contents {
        Some(text) => std::fs::write(&file, text).expect("write"),
        None => {
            let _ = std::fs::remove_file(&file);
        }
    }
    home
}

fn env_of(pairs: Vec<(String, String)>) -> impl Fn(&str) -> Option<String> {
    move |key| {
        pairs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.to_owned())
    }
}

#[test]
fn a_theme_file_under_home_is_read_when_nothing_overrides_it() {
    // The half of B6's amendment that makes a preference survive a new shell. A
    // variable has to be re-declared per shell and the instruction for making that
    // permanent differs per shell; a file is set once.
    let home = home_with(
        "default",
        Some(
            "base = dark
added = #ff0000
",
        ),
    );
    let env = env_of(vec![("HOME".to_owned(), home.display().to_string())]);

    let theme = theme::from_env(Depth::Truecolor, env, None).expect("a theme");
    assert_eq!(theme.added.fg, Some(Color::Rgb(0xff, 0x00, 0x00)));
    assert_eq!(theme.keyword, Theme::dark().keyword, "the base was ignored");
}

#[test]
fn the_variable_still_wins_over_the_file() {
    // Which is what makes the variable worth keeping: it is how a reader says "not
    // this time" without editing anything.
    let home = home_with(
        "override",
        Some(
            "base = dark
added = #ff0000
",
        ),
    );
    let env = env_of(vec![
        ("HOME".to_owned(), home.display().to_string()),
        ("VIGIA_THEME".to_owned(), "light".to_owned()),
    ]);

    let theme = theme::from_env(Depth::Truecolor, env, None).expect("a theme");
    assert_eq!(theme, Theme::light().resolve(Depth::Truecolor));
}

#[test]
fn no_file_is_not_an_error_but_an_unreadable_one_is() {
    // The distinction that matters.
    let absent = home_with("absent", None);
    let env = env_of(vec![("HOME".to_owned(), absent.display().to_string())]);
    assert_eq!(
        theme::from_env(Depth::Truecolor, env, None).expect("a theme"),
        Theme::ansi().resolve(Depth::Truecolor)
    );

    let broken = home_with(
        "broken",
        Some(
            "added = #gg0000
",
        ),
    );
    let env = env_of(vec![("HOME".to_owned(), broken.display().to_string())]);
    let err = theme::from_env(Depth::Truecolor, env, None).expect_err("refused");
    assert!(err.to_string().contains("line 1"), "{err}");
}

#[test]
fn the_home_directory_is_one_rule_rather_than_one_per_platform() {
    // `HOME` first, because it is set on every Unix and by Git Bash on Windows too,
    // then `USERPROFILE`. Two names, one rule, and no XDG matrix or discovery
    // crate: the whole cost of the amendment is a place to look.
    let home = home_with(
        "windows",
        Some(
            "base = light
",
        ),
    );
    let env = env_of(vec![("USERPROFILE".to_owned(), home.display().to_string())]);
    assert_eq!(
        theme::from_env(Depth::Truecolor, env, None).expect("a theme"),
        Theme::light().resolve(Depth::Truecolor)
    );

    // And an empty one is no home at all, rather than a lookup rooted at `/`.
    let env = env_of(vec![("HOME".to_owned(), "  ".to_owned())]);
    assert_eq!(
        theme::from_env(Depth::Truecolor, env, None).expect("a theme"),
        Theme::ansi().resolve(Depth::Truecolor)
    );

    // An empty `HOME` must not hide a good `USERPROFILE`, which is the case the first
    // version of this got wrong: filtering after the fallback means `Some("")` stops
    // `or_else` from ever firing.
    let home = home_with(
        "empty-home",
        Some(
            "base = light
",
        ),
    );
    let env = env_of(vec![
        ("HOME".to_owned(), String::new()),
        ("USERPROFILE".to_owned(), home.display().to_string()),
    ]);
    assert_eq!(
        theme::from_env(Depth::Truecolor, env, None).expect("a theme"),
        Theme::light().resolve(Depth::Truecolor),
        "an empty HOME hid a good USERPROFILE"
    );
}

#[test]
fn the_ramp_collapses_to_two_stops_where_it_must_and_says_so_in_the_palette() {
    // `ansi` writes its middle stop as the same name as its low one rather than leaving
    // the depth ladder to collapse them by accident.
    let two = heat_stops(Theme::ansi());
    assert_eq!(two.len(), 2, "expected two stops, got {two:?}");

    // And the ramp is still a ramp: the stops that survive are different colours,
    // so a hot slice is still distinguishable from a quiet one.
    assert_ne!(two[0], two[1]);
}

/// The reference background each truecolour palette is designed against.
const DARK_PANE: (u8, u8, u8) = (0x0d, 0x11, 0x17);
const LIGHT_PANE: (u8, u8, u8) = (0xff, 0xff, 0xff);

/// WCAG relative luminance.
fn luminance((r, g, b): (u8, u8, u8)) -> f64 {
    fn channel(v: u8) -> f64 {
        let v = f64::from(v) / 255.0;
        if v <= 0.03928 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b)
}

/// WCAG contrast ratio: 1.0 for two identical colours, 21.0 for black on white.
fn contrast(a: (u8, u8, u8), b: (u8, u8, u8)) -> f64 {
    let (x, y) = (luminance(a), luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// The two truecolour palettes and the pane each is authored against.
fn palettes() -> [(&'static str, Theme, (u8, u8, u8)); 2] {
    [
        ("dark", Theme::dark().resolve(Depth::Truecolor), DARK_PANE),
        (
            "light",
            Theme::light().resolve(Depth::Truecolor),
            LIGHT_PANE,
        ),
    ]
}

/// The channels of a truecolour value, named so a failure says which one.
fn channels_of(colour: Color, which: &str) -> (u8, u8, u8) {
    match colour {
        Color::Rgb(r, g, b) => (r, g, b),
        other => panic!("expected a truecolour {which}, found {other:?}"),
    }
}

/// The channels of a truecolour style's foreground.
fn rgb_of(style: ratatui::style::Style) -> (u8, u8, u8) {
    channels_of(style.fg.expect("a foreground"), "foreground")
}

/// The floor a mark meant to be *seen but subordinate* has to clear.
const TRACK_FLOOR: f64 = 2.0;

/// §11.2 B20's wash stands in for the diff wash while it is up, so its floors are
/// that wash's rather than a number from anywhere else.
///
/// **A background under a whole row and a one-cell mark on the pane are not the
/// same problem**, and `TRACK_FLOOR` is the second: the shipped row washes sit at
/// 1.27:1 to 1.57:1 against their panes, so a fixed floor drawn from a mark would
/// fail the very elements it was meant to model. Relative is also what keeps this
/// honest as those washes move.
#[test]
fn the_selection_wash_is_no_worse_than_the_diff_wash_it_stands_in_for() {
    for (name, theme, pane) in palettes() {
        let wash = channels_of(
            theme
                .selection
                .bg
                .expect("a truecolour palette washes its selection"),
            "selection wash",
        );
        let rows = [
            channels_of(theme.added_row.bg.expect("added wash"), "added wash"),
            channels_of(theme.removed_row.bg.expect("removed wash"), "removed wash"),
        ];

        // Visible: at least as far from the pane as the quieter of the two.
        let quietest = rows
            .iter()
            .map(|row| contrast(*row, pane))
            .fold(f64::INFINITY, f64::min);
        let seen = contrast(wash, pane);
        assert!(
            seen >= quietest,
            "{name}'s selection is {seen:.2}:1 against the pane where the quieter \
             diff wash is {quietest:.2}:1, so a reader can see a changed row and \
             not the rows they selected"
        );

        // Readable: it costs a content row's ink no more than those washes do.
        for (element, style) in [
            ("context", theme.context),
            ("gutter", theme.gutter),
            ("comment", theme.comment),
        ] {
            let ink = rgb_of(style);
            let floor = rows
                .iter()
                .map(|row| contrast(ink, *row))
                .fold(f64::INFINITY, f64::min);
            let ratio = contrast(ink, wash);
            assert!(
                ratio >= floor,
                "{name}'s {element} is {ratio:.2}:1 on the selection wash and \
                 {floor:.2}:1 on the diff wash beside it, so selecting a row \
                 makes it harder to read than changing one does"
            );
        }

        // And it is neither diff wash, or a selected removal reads as an addition.
        for (element, other) in [
            ("added_row", theme.added_row),
            ("removed_row", theme.removed_row),
        ] {
            assert_ne!(
                theme.selection.bg, other.bg,
                "{name}'s selection is {element} exactly, so a selected row is \
                 indistinguishable from an unselected one of that kind"
            );
        }
    }

    // `ansi` washes nothing, for the reason its row washes are unset: a background
    // has to assume one. It reverses instead, which is right on any scheme.
    assert!(
        Theme::ansi().selection.bg.is_none(),
        "the ansi palette took a background, which it has no way to be right about"
    );
    assert!(
        Theme::ansi()
            .selection
            .add_modifier
            .contains(ratatui::style::Modifier::REVERSED),
        "the ansi palette neither colours nor reverses its selection, so a \
         sixteen-colour terminal shows a reader nothing at all"
    );
}

#[test]
fn a_track_is_visible_against_the_pane_it_is_drawn_on() {
    // Reported from use, and the numbers say why nothing was visible. The
    // scrollbar's track and its step buttons drew in `bar_track`, which was
    // `#21262d` on `#0d1117`: 1.24:1, where 1.0 is the background exactly.
    // Only the thumb and a *pressed* button could be seen, because those draw in
    // `bar` at 6.15:1. The light palette had the same defect at 1.45:1, and this
    // gate then found a third nobody had reported: the sparkline's own track at
    // 1.55:1, which is the empty bucket §5.1 rules must draw something.
    for (name, theme, pane) in palettes() {
        for (element, style) in [
            ("bar_track", theme.bar_track),
            ("heat_track", theme.heat_track),
            ("spark_track", theme.spark_track),
        ] {
            let ratio = contrast(rgb_of(style), pane);
            assert!(
                ratio >= TRACK_FLOOR,
                "{name}'s {element} is {ratio:.2}:1 against the pane it is drawn \
                 on, under the {TRACK_FLOOR}:1 a mark needs to be seen at all. \
                 1.00:1 is the background exactly"
            );
        }

        // And a stroke outranks a block, which is the rule `spark_track`'s
        // own docblock always stated and could not satisfy while the block it
        // was a step above was itself invisible.
        let stroke = contrast(rgb_of(theme.spark_track), pane);
        let block = contrast(rgb_of(theme.heat_track), pane);
        assert!(
            stroke > block,
            "{name}'s sparkline track is {stroke:.2}:1 against the heat track's \
             {block:.2}:1, so the one-line glyph is dimmer than the half-block \
             it has to read level with"
        );

        // And still subordinate to what it is a track for. A track that
        // reached its own thumb would satisfy the floor above and destroy the
        // reading it exists to support.
        let thumb = contrast(rgb_of(theme.bar), pane);
        let track = contrast(rgb_of(theme.bar_track), pane);
        assert!(
            thumb > track * 1.5,
            "{name}'s thumb is {thumb:.2}:1 and its track {track:.2}:1, which is \
             not enough separation for the thumb to read as the lit one"
        );
    }
}

/// The bar's track and thumb are legible on every row the bar crosses.
#[test]
fn a_bar_track_is_visible_on_every_row_it_crosses() {
    for (name, theme, pane) in palettes() {
        let track = rgb_of(theme.bar_track);
        let thumb = rgb_of(theme.bar);

        // The pane and both washes. `Theme::row` is what the shell itself calls,
        // so these are the backgrounds that actually get painted rather than a
        // restatement of them here.
        let backgrounds = [
            ("the pane", pane),
            (
                "an added row",
                channels_of(wash_of(theme, true), "row wash"),
            ),
            (
                "a removed row",
                channels_of(wash_of(theme, false), "row wash"),
            ),
        ];

        for (place, behind) in backgrounds {
            let seen = contrast(track, behind);
            assert!(
                seen >= TRACK_FLOOR,
                "{name}'s bar_track is {seen:.2}:1 on {place}, under the \
                 {TRACK_FLOOR}:1 a mark needs to be seen at all. The bar crosses \
                 that row, so this is a background it is drawn on"
            );

            // Subordinate on the same background it was measured legible on.
            let lit = contrast(thumb, behind);
            assert!(
                lit > seen * 1.5,
                "{name}'s thumb is {lit:.2}:1 on {place} and its track \
                 {seen:.2}:1, which is not enough separation for the thumb to \
                 read as the lit one"
            );
        }
    }
}

/// Every bar style says everything [`Painter::bar_cell`] reads from it.
#[test]
fn every_bar_style_says_what_bar_cell_reads() {
    // Its own enumeration, and `ansi` is why.
    let palettes = [
        ("dark", Theme::dark()),
        ("light", Theme::light()),
        ("ansi", Theme::ansi()),
    ];

    for (name, theme) in palettes {
        for (element, style) in [
            ("bar", theme.bar),
            ("bar_track", theme.bar_track),
            ("bar_hover", theme.bar_hover),
            ("bar_active", theme.bar_active),
        ] {
            assert!(
                style.fg.is_some(),
                "{name}'s {element} carries no foreground, so under a row band the \
                 bar would draw in the wash's colour instead of its own and nothing \
                 measuring this palette would notice"
            );
            assert!(
                style.bg.is_none(),
                "{name}'s {element} carries a background, which opts this \
                 palette's own bar out of the band #239 ruled should run under \
                 it. A reader's theme may choose that; a shipped palette may not"
            );
        }
    }
}

#[test]
fn the_spark_ramp_interpolates_only_where_stops_are_rgb() {
    // The ramp is derived from the three stop keys, so its ends must BE
    // those stops, and it must refuse to exist where the stops are not RGB,
    // which is the whole of how the depth ladder gates it.
    let dark = Theme::dark().resolve(Depth::Truecolor);
    let ramp = dark
        .spark_ramp()
        .expect("a truecolour palette must interpolate");
    assert_eq!(
        Some(ramp[0]),
        dark.spark.fg,
        "the ramp's floor is not `spark`"
    );
    assert_eq!(
        Some(ramp[4]),
        dark.spark_warm.fg,
        "the ramp's middle is not `spark_warm`"
    );
    assert_eq!(
        Some(ramp[7]),
        dark.spark_hot.fg,
        "the ramp's top is not `spark_hot`"
    );

    assert!(
        Theme::dark().resolve(Depth::Ansi256).spark_ramp().is_none(),
        "a 256-resolved palette interpolated indexed colours"
    );
    assert!(
        Theme::ansi()
            .resolve(Depth::Truecolor)
            .spark_ramp()
            .is_none(),
        "`ansi` interpolated named colours"
    );
}

#[test]
fn the_detected_background_picks_the_showcase_and_never_outranks_a_word() {
    use vigia::Background;
    let none = |_: &str| None;

    // A terminal that answered picks the showcase for its side.
    assert_eq!(
        theme::from_env(Depth::Truecolor, none, Some(Background::Dark)).expect("a theme"),
        Theme::dark().resolve(Depth::Truecolor),
        "a dark answer did not pick the dark showcase"
    );
    assert_eq!(
        theme::from_env(Depth::Truecolor, none, Some(Background::Light)).expect("a theme"),
        Theme::light().resolve(Depth::Truecolor),
        "a light answer did not pick the light showcase"
    );
    // No answer keeps the palette that assumes nothing.
    assert_eq!(
        theme::from_env(Depth::Truecolor, none, None).expect("a theme"),
        Theme::ansi().resolve(Depth::Truecolor),
        "silence did not keep the fallback"
    );
    // And a reader's own word still wins over any guess.
    let named = |key: &str| (key == "VIGIA_THEME").then(|| "light".to_owned());
    assert_eq!(
        theme::from_env(Depth::Truecolor, named, Some(Background::Dark)).expect("a theme"),
        Theme::light().resolve(Depth::Truecolor),
        "detection outranked VIGIA_THEME"
    );
}

/// A depth that cannot carry a background must still show a selection. The diff
/// washes degrade onto their bars; this one has none, so it reverses instead.
#[test]
fn the_selection_survives_every_colour_depth() {
    for name in ["dark", "light", "ansi"] {
        let palette = Theme::named(name).expect("a built-in palette");
        for depth in [Depth::Truecolor, Depth::Ansi256, Depth::Ansi16] {
            let selection = palette.resolve(depth).selection;
            let seen = selection.bg.is_some()
                || selection
                    .add_modifier
                    .contains(ratatui::style::Modifier::REVERSED);
            assert!(
                seen,
                "{name} at {depth:?} draws a selection with neither a background nor \
                 a reversal, so a reader drags and sees nothing while the release still sends"
            );
        }
    }
}
