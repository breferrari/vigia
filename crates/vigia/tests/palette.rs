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
    Chrome, Depth, FileEntry, HEAT_BUCKETS, HeatBucket, Mode, Position, Row, Theme, View, render,
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
            Row::File(FileEntry {
                path: "src/a.rs".to_owned(),
                from: None,
                kind: 'M',
                churn: Some((2, 1)),
                spark: [0; HISTORY_BUCKETS],
                recency: Recency::Cold,
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
            // **Below** the changed rows, and that placement is the whole reason
            // it exists. A wash painted with an inherited height runs from its own
            // row to the bottom of the pane, so it can only ever spill *downwards*
            // and a context row above every changed row cannot witness it. The
            // first version of this fixture had exactly one context line, first,
            // and `a_context_row_is_never_washed` passed against a renderer that
            // washed the footer.
            line(LineKind::Context, 3, "let d = 4;"),
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
/// The context row **under** both changed rows. See [`three_kinds`].
const CONTEXT_BELOW: u16 = 6;

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
    let style = if added {
        theme.added_row
    } else {
        theme.removed_row
    };
    style.bg.expect("this palette washes its rows")
}

#[test]
fn the_fixture_lands_where_these_say() {
    // Every gate below indexes rows by the constants above, so if the layout ever
    // moves this is the one that says so, by name, instead of five gates failing
    // with assertions about colour.
    let backend = draw(60, 8, &three_kinds(), Theme::ansi());
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
fn the_sigil_keeps_its_own_colour_on_the_wash() {
    // **The sigil is not the bar**, which is a ruling this branch made and then
    // reversed on seeing it. Inverting the sigil cell (diff hue behind, wash in
    // front) reads as a solid block: it takes the one glyph carrying the diff
    // signal and turns it into a background. The mockup draws its bar as a sliver
    // *beside* a green `+`, not as a recolouring of it.
    //
    // So the sigil keeps the diff colour and sits on the row's wash like every
    // other cell in the row.
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
fn a_palette_that_declines_a_bar_leaves_the_sigil_alone() {
    // The `_bar` keys still exist so a theme file can ask for one. What must hold
    // is that leaving them unset changes nothing: patching an empty style over the
    // diff style has to be the identity, or every built-in silently loses its
    // sigil colour.
    for theme in [Theme::ansi(), Theme::dark(), Theme::light()] {
        assert_eq!(theme.added_bar, ratatui::style::Style::new());
        assert_eq!(theme.removed_bar, ratatui::style::Style::new());
    }
}

#[test]
fn a_context_row_is_never_washed() {
    // The wash *is* the diff signal once the sigil column stops being it, so a
    // context line carrying one would say a line changed that did not.
    // Drawn taller than the fixture needs, so there are blank rows and a footer
    // below the last changed line for a spill to land on.
    let dark = Theme::dark().resolve(Depth::Truecolor);
    let backend = draw(60, 12, &three_kinds(), dark);

    for row in [CONTEXT, CONTEXT_BELOW] {
        assert!(
            backgrounds(&backend, row).iter().all(Option::is_none),
            "context row {row} was washed"
        );
    }

    // And nothing below the diff either. A wash is a property of one row; a wash
    // that reached the blank rows or the footer would be a rectangle, which is
    // what an inherited `Rect` height produced before this gate could see it.
    for row in CONTEXT_BELOW + 1..12 {
        assert!(
            backgrounds(&backend, row).iter().all(Option::is_none),
            "row {row}, below the whole diff, was washed"
        );
    }
}

#[test]
fn nothing_a_reader_has_to_read_is_drawn_in_colour_eight() {
    // **Colour 8 is not a colour, it is a relationship to the background.** Most
    // schemes define "bright black" as a shade just above the pane, so text drawn
    // in it can land a few points off the background and vanish. Reported from a
    // real terminal, where the key hints, the readouts, the empty state and the
    // line numbers were all invisible at once.
    //
    // The rule is about *text*, not about the colour. `heat_track` is deliberately
    // still colour 8 and is exempt by name: a track is a solid block that should
    // sit just above the background, which is exactly what the colour is for. The
    // exemption is listed rather than inferred, so adding a field cannot join it by
    // accident.
    let ansi = Theme::ansi();
    let readable: [(&str, ratatui::style::Style); 12] = [
        ("chrome", ansi.chrome),
        ("chrome_dim", ansi.chrome_dim),
        ("path", ansi.path),
        ("path_live", ansi.path_live),
        ("path_cold", ansi.path_cold),
        ("gutter", ansi.gutter),
        ("kind", ansi.kind),
        ("hunk", ansi.hunk),
        ("note", ansi.note),
        ("alert", ansi.alert),
        ("context", ansi.context),
        ("comment", ansi.comment),
    ];

    for (name, style) in readable {
        assert_ne!(
            style.fg,
            Some(Color::DarkGray),
            "{name} is drawn in colour 8, which some schemes put on the background"
        );
    }

    // And it may not reach for `DIM` either, which was the first replacement and
    // was invisible on the same terminal that colour 8 was: a terminal is free to
    // reduce intensity as far as it likes. Both ways of saying "dim" in the
    // sixteen-colour world have now failed in the field, so this palette says it
    // in neither and takes colour 7, which is readable by construction.
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

/// A file whose heat profile has a slice in each band.
///
/// Twelve buckets, so no re-projection happens and the drawn slices are the source
/// slices. The busiest is 12, so 8 is hot (two thirds), 4 is warm (one third) and 1
/// is low, which puts one slice in every band by construction rather than by luck.
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
        rows: vec![Row::File(FileEntry {
            path: "src/a.rs".to_owned(),
            from: None,
            kind: 'M',
            churn: Some((25, 0)),
            spark: [0; HISTORY_BUCKETS],
            recency: Recency::Cold,
            heat,
        })],
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

/// The strip's slice colours in order, track included, so a band can be asserted
/// by position rather than by counting.
fn heat_sequence(theme: Theme) -> Vec<Color> {
    let backend = draw(120, 4, &graded_heat(), theme);
    let buffer = backend.buffer();
    (0..120)
        .filter(|x| buffer[(*x, HEADING)].symbol() == "█")
        .filter_map(|x| buffer[(x, HEADING)].style().fg)
        .collect()
}

#[test]
fn each_slice_lands_in_the_band_its_share_puts_it_in() {
    // **Counting distinct colours cannot test the thresholds**, and this gate
    // exists because a mutation proved it. Moving the hot cut from two thirds to
    // one half leaves the fixture with three distinct colours either way, so the
    // count is satisfied while every slice is in the wrong band. Only asserting
    // *which* band a given share falls into can tell the two apart.
    //
    // The fixture's busiest slice is 12, so the shares are 12, 7, 4 and 3 against
    // it, and two of those exist only to pin a threshold. Seven is above half and
    // below two thirds, so it is warm under the rule and hot if the hot cut slips.
    // Three is just under a third, so it is low under the rule and warm if the warm
    // cut slips. Without both, moving either constant leaves every gate green.
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
    // `dark` is authored in 24-bit and quantised down, unlike `ansi`, so what its
    // ramp becomes at sixteen colours is decided by the bright-variant threshold in
    // `to_ansi16` and by nothing else. At a lower threshold all three greens land
    // on the same name and the strip stops saying anything about where the work is.
    //
    // Two rather than three, because sixteen names hold a normal and a bright of
    // each hue. Collapsing to one is the failure.
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
    // `added = bold` reads as "make additions bold" and used to mean "make
    // additions bold and colourless": the style was built from `Style::new()` and
    // `set` replaces the whole field. That is the same invisible change
    // `MissingValue` exists to prevent, reached from the other direction.
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
    // Notepad's default UTF-8 save writes a BOM, and `str::trim` will not strip
    // one: U+FEFF is `Cf`, not `White_Space`, so it survives every trim in the
    // parser and lands inside the first key. That stopped the shell from starting
    // with an error naming an invisible byte.
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
    // `.resolve(depth)` used to be written on each of three exits. Only the
    // built-in arm was ever gated at a lossy depth, so deleting it from either of
    // the other two left the whole suite green: at truecolour `resolve` is the
    // identity, and every test reaching those arms ran there. This drives all
    // three at a depth where the difference shows.
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
        let theme = theme::from_env(Depth::Ansi16, env_of(pairs)).expect("a theme");
        assert!(
            !matches!(theme.added.fg, Some(Color::Rgb(..))),
            "{why}: reached the renderer unresolved"
        );
    }
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

/// A home directory holding a theme file, and an environment that points at it.
///
/// The lookup is injected rather than the process environment being touched,
/// because `cargo test` runs these on threads of one process and `set_var` is both
/// racy and, since Rust 2024, `unsafe`.
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

    let theme = theme::from_env(Depth::Truecolor, env).expect("a theme");
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

    let theme = theme::from_env(Depth::Truecolor, env).expect("a theme");
    assert_eq!(theme, Theme::light().resolve(Depth::Truecolor));
}

#[test]
fn no_file_is_not_an_error_but_an_unreadable_one_is() {
    // The distinction that matters. Nobody has to have a theme file, so its absence
    // is the ordinary case and falls back to the default. A reader who *wrote* one
    // and silently got the default instead would have no way to find out why, so a
    // file that exists and does not parse stops the shell before it takes the
    // screen, which is the rule §11.1 already states for a path that is not a
    // repository.
    let absent = home_with("absent", None);
    let env = env_of(vec![("HOME".to_owned(), absent.display().to_string())]);
    assert_eq!(
        theme::from_env(Depth::Truecolor, env).expect("a theme"),
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
    let err = theme::from_env(Depth::Truecolor, env).expect_err("refused");
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
        theme::from_env(Depth::Truecolor, env).expect("a theme"),
        Theme::light().resolve(Depth::Truecolor)
    );

    // And an empty one is no home at all, rather than a lookup rooted at `/`.
    let env = env_of(vec![("HOME".to_owned(), "  ".to_owned())]);
    assert_eq!(
        theme::from_env(Depth::Truecolor, env).expect("a theme"),
        Theme::ansi().resolve(Depth::Truecolor)
    );

    // **An empty `HOME` must not hide a good `USERPROFILE`**, which is the case the
    // first version of this got wrong: filtering after the fallback means `Some("")`
    // stops `or_else` from ever firing. Reachable on any Windows shell that exports
    // an empty `HOME`, and invisible everywhere else, which is the worst shape a
    // bug can have.
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
        theme::from_env(Depth::Truecolor, env).expect("a theme"),
        Theme::light().resolve(Depth::Truecolor),
        "an empty HOME hid a good USERPROFILE"
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
