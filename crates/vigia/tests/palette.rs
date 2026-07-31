//! What the palettes actually put on the screen.
//!
//! `tests/colour.rs` covers the depth ladder as arithmetic, over styles that never
//! reach a buffer. This covers the other end: a real [`View`] through the real
//! renderer, read back **cell by cell**, because the two properties #11 turns on
//! are both invisible to a snapshot. `TestBackend`'s `Display` writes symbols and
//! drops styles, so a row wash and a bar are exactly the kind of change that can
//! ship broken under a green snapshot suite.
//!
//! A separate file from `render.rs` rather than more of it: that file is about
//! what the body *draws*, this is about what the palette *colours*, and the two
//! ask different questions of the same buffer.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::style::Color;
use vigia::{
    Chrome, Depth, HEAT_BUCKETS, HeatBucket, Mode, Position, Row, Theme, View, render,
};
use vigia_core::{HISTORY_BUCKETS, LineKind, Recency};

fn chrome() -> Chrome {
    Chrome {
        worktree: "vigia".to_owned(),
        branch: None,
        mode: Mode::Watching,
        notice: None,
        following: false,
        frame: None,
        memory: None,
    }
}

fn line(kind: LineKind, number: u32, text: &str) -> Row {
    Row::Line {
        kind,
        number,
        text: text.to_owned(),
        spans: Vec::new(),
    }
}

/// A file, a hunk, and one line of each kind, in a known order.
///
/// The row indices below are counted from this: 0 is the file heading, 1 the hunk
/// header, then context, added, removed.
fn three_kinds() -> View {
    View {
        rows: vec![
            Row::File {
                path: "src/a.rs".to_owned(),
                from: None,
                kind: 'M',
                churn: Some((2, 1)),
                spark: [0; HISTORY_BUCKETS],
                recency: Recency::Cold,
                heat: [HeatBucket::default(); HEAT_BUCKETS],
            },
            Row::Hunk {
                old_start: 1,
                old_lines: 2,
                new_start: 1,
                new_lines: 3,
            },
            line(LineKind::Context, 1, "let a = 1;"),
            line(LineKind::Added, 2, "let b = 2;"),
            line(LineKind::Removed, 2, "let c = 3;"),
        ],
        files: 1,
        top: Position::default(),
        read: 1,
        peak: 0,
    }
}

/// Row 0 is the header, so the body's first row is 1 and these are offset by it.
///
/// Written as constants and asserted once, in [`the_fixture_lands_where_these_say`],
/// rather than recomputed per gate: a test that derived the layout would be a
/// second implementation of it agreeing with itself.
const HEADING: u16 = 1;
const CONTEXT: u16 = 3;
const ADDED: u16 = 4;
const REMOVED: u16 = 5;

fn draw(width: u16, height: u16, view: &View, theme: Theme) -> TestBackend {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
    terminal
        .draw(|f| {
            let area = f.area();
            render(f.buffer_mut(), area, view, &theme, &chrome());
        })
        .expect("draw");
    terminal.backend().clone()
}

/// Every background on row `y`, one per cell, left to right.
fn backgrounds(backend: &TestBackend, y: u16) -> Vec<Option<Color>> {
    let buffer = backend.buffer();
    (0..buffer.area.width)
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
    let style = if added { theme.added_row } else { theme.removed_row };
    style.bg.expect("this palette washes its rows")
}

#[test]
fn the_fixture_lands_where_these_say() {
    // Every gate below indexes rows by the constants above, so if the layout ever
    // moves this is the one that says so, by name, instead of five gates failing
    // with assertions about colour.
    let backend = draw(60, 8, &three_kinds(), Theme::ansi());
    let buffer = backend.buffer();
    let row = |y: u16| -> String {
        (0..60).map(|x| buffer[(x, y)].symbol()).collect::<String>()
    };

    assert!(row(HEADING).contains("src/a.rs"), "{:?}", row(HEADING));
    assert!(row(CONTEXT).contains("let a = 1;"), "{:?}", row(CONTEXT));
    assert!(row(ADDED).contains("let b = 2;"), "{:?}", row(ADDED));
    assert!(row(REMOVED).contains("let c = 3;"), "{:?}", row(REMOVED));
}

#[test]
fn a_changed_row_is_washed_to_the_pane_edge() {
    // To the **edge**, which is the whole assertion. A wash that stopped where the
    // text stops would be a highlight behind some words; what `assets/preview.svg`
    // draws is a band across the row, and the trailing blanks are most of it on a
    // wide pane.
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
fn the_sigil_cell_carries_the_bar() {
    // `SPEC.md` §5.1's left bar, which is the sigil cell inverted: the diff hue
    // behind, the row's own wash in front. Found by looking for the one cell whose
    // background is not the wash rather than by recomputing the gutter's width,
    // which would be a second implementation of the layout agreeing with itself.
    let dark = Theme::dark().resolve(Depth::Truecolor);
    let backend = draw(60, 8, &three_kinds(), dark);
    let buffer = backend.buffer();

    for (row, added, sigil) in [(ADDED, true, "+"), (REMOVED, false, "-")] {
        let wash = wash_of(dark, added);
        let bar = if added { dark.added_bar } else { dark.removed_bar };
        let at = (0..60)
            .find(|x| buffer[(*x, row)].symbol() == sigil)
            .unwrap_or_else(|| panic!("no {sigil:?} on row {row}"));

        let cell = buffer[(at, row)].style();
        assert_eq!(cell.bg, bar.bg, "the bar's hue is missing on row {row}");
        assert_eq!(cell.fg, bar.fg, "the sigil is not drawn in the wash");
        assert_ne!(cell.bg, Some(wash), "the bar is not distinct from the wash");
    }
}

#[test]
fn a_context_row_is_never_washed() {
    // The wash *is* the diff signal once the sigil column stops being it, so a
    // context line carrying one would say a line changed that did not.
    let dark = Theme::dark().resolve(Depth::Truecolor);
    let backend = draw(60, 8, &three_kinds(), dark);
    assert!(
        backgrounds(&backend, CONTEXT).iter().all(Option::is_none),
        "a context row was washed"
    );
}

#[test]
fn the_ansi_palette_draws_no_wash_at_any_depth() {
    // The ruling that keeps palette and depth genuinely independent axes. A wash
    // has to assume a background; `ansi` resolves to the reader's own scheme and so
    // assumes none, and it refuses at truecolour just as firmly as at sixteen.
    for depth in [
        Depth::Truecolor,
        Depth::Ansi256,
        Depth::Ansi16,
        Depth::None,
    ] {
        let backend = draw(60, 8, &three_kinds(), Theme::ansi().resolve(depth));
        for row in [CONTEXT, ADDED, REMOVED] {
            assert!(
                backgrounds(&backend, row).iter().all(Option::is_none),
                "ansi washed row {row} at {depth:?}"
            );
        }
    }
}

#[test]
fn sixteen_colours_draw_no_background_and_keep_the_sigil() {
    // `SPEC.md` §11.1's recorded loss, now a property of the *depth* rather than of
    // the tool. What must not go with the wash is the sigil's own colour: patching
    // an unset bar over the diff style has to leave the diff style alone, and
    // getting that wrong blanks the last thing distinguishing an addition.
    let dark = Theme::dark();
    for depth in [Depth::Ansi16, Depth::None] {
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
    // I6 is a claim about where things land, and a background is the one kind of
    // change that must not touch that. Asserted by rendering the same view through
    // a palette that washes and one that does not and comparing **symbols**: if the
    // wash ever consumed a column, this is what fails.
    //
    // Swept across widths rather than asserted at one, because a layout only breaks
    // at the width where something stops fitting.
    for width in [40, 60, 80, 120] {
        let washed = draw(width, 8, &three_kinds(), Theme::dark().resolve(Depth::Truecolor));
        let plain = draw(width, 8, &three_kinds(), Theme::ansi().resolve(Depth::Truecolor));
        let (a, b) = (washed.buffer(), plain.buffer());
        for y in 0..8 {
            let left: Vec<_> = (0..width).map(|x| a[(x, y)].symbol()).collect();
            let right: Vec<_> = (0..width).map(|x| b[(x, y)].symbol()).collect();
            assert_eq!(left, right, "row {y} at width {width} moved");
        }
    }
}

/// A file whose heat profile has a slice in each band.
///
/// Twelve buckets, so no re-projection happens and the drawn slices are the source
/// slices. The busiest is 12, so 8 is hot (two thirds), 4 is warm (one third) and 1
/// is low, which puts one slice in every band by construction rather than by luck.
fn graded_heat() -> View {
    let mut heat = [HeatBucket::default(); HEAT_BUCKETS];
    heat[0] = HeatBucket { added: 12, removed: 0 };
    heat[1] = HeatBucket { added: 8, removed: 0 };
    heat[2] = HeatBucket { added: 4, removed: 0 };
    heat[3] = HeatBucket { added: 1, removed: 0 };
    View {
        rows: vec![Row::File {
            path: "src/a.rs".to_owned(),
            from: None,
            kind: 'M',
            churn: Some((25, 0)),
            spark: [0; HISTORY_BUCKETS],
            recency: Recency::Cold,
            heat,
        }],
        files: 1,
        top: Position::default(),
        read: 1,
        peak: 0,
    }
}

/// Distinct foregrounds on the file heading, ignoring the track and the counters.
fn heat_stops(theme: Theme) -> Vec<Color> {
    let backend = draw(120, 4, &graded_heat(), theme);
    let buffer = backend.buffer();
    let track = theme.heat_track.fg;
    let mut seen: Vec<Color> = Vec::new();
    for x in 0..120 {
        let cell = &buffer[(x, HEADING)];
        // **Symbol and style together.** Every heat slice is a full block and so is
        // a sparkline's top rung, and the counters on the same row share the
        // track's grey. Either half alone is a coincidence waiting to happen, and
        // has already happened once in this suite.
        if cell.symbol() != "█" {
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

#[test]
fn the_heat_ramp_has_three_stops_where_the_depth_can_draw_them() {
    // The picture ramps its additions across three greens and the strip is the one
    // element whose intensity `assets/preview.svg` actually specifies. It used to
    // draw two, because sixteen names hold a normal and a bright of each hue and no
    // third stop.
    let three = heat_stops(Theme::dark().resolve(Depth::Truecolor));
    assert_eq!(three.len(), 3, "expected three stops, got {three:?}");

    let indexed = heat_stops(Theme::dark().resolve(Depth::Ansi256));
    assert_eq!(indexed.len(), 3, "256 lost a stop: {indexed:?}");
}

// ---------------------------------------------------------------------------
// The theme file, which is how a palette is specified rather than what it draws.
// ---------------------------------------------------------------------------

use vigia::ThemeError;
use vigia::theme;

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
    assert!(theme.path.add_modifier.contains(ratatui::style::Modifier::BOLD));

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
fn every_key_the_struct_has_is_a_key_a_file_can_set() {
    // True by construction, since one macro emits the struct and the key list from
    // the same declaration. Asserted anyway, because "by construction" is a claim
    // about code that can be edited, and this is what fails if the macro is ever
    // unrolled by hand.
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
    let flat = theme::from_env(Depth::Ansi16, env).expect("a theme");
    assert!(
        !matches!(flat.added.fg, Some(Color::Rgb(..))),
        "the palette reached the renderer unresolved"
    );
}

#[test]
fn the_ramp_collapses_to_two_stops_where_it_must_and_says_so_in_the_palette() {
    // `ansi` writes its middle stop as the same name as its low one rather than
    // leaving the depth ladder to collapse them by accident. The distinction
    // matters: a collapse that happens *in the palette* is a decision a reader can
    // look up, and one that happens in the quantiser is a surprise.
    let two = heat_stops(Theme::ansi());
    assert_eq!(two.len(), 2, "expected two stops, got {two:?}");

    // And the ramp is still a ramp: the stops that survive are different colours,
    // so a hot slice is still distinguishable from a quiet one.
    assert_ne!(two[0], two[1]);
}
