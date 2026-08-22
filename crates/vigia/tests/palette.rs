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
use std::collections::HashSet;

use ratatui::style::{Color, Style};
use vigia::{
    Chrome, Depth, FileEntry, Glyphs, HEAT_BUCKETS, HeatBucket, Mode, Position, Row, Scale, Theme,
    View, render,
};
use vigia_core::{HISTORY_BUCKETS, LineKind, Recency};

/// Buckets a sparkline draws on the panes this file renders at.
///
/// **Restated rather than imported, and no longer [`HISTORY_BUCKETS`]**
/// ([#234](https://github.com/breferrari/vigia/issues/234)). That constant is the
/// resolution the shell projects *from*; what a row draws is the rung its pane
/// affords, which is twelve at every width here.
const DRAWN_BUCKETS: usize = 12;

/// The heat strip's slice, restated rather than imported: a test sharing the
/// renderer's constant agrees with it by construction instead of checking it.
const HEAT_SLICE: &str = "■";

/// The sparkline's ramp, shortest first, restated for [`HEAT_SLICE`]'s reason
/// and declared **once** for `tests/render.rs`'s: two copies in one binary check
/// each other rather than the renderer, and they drift independently.
const RAMP: [&str; 8] = ["▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];

/// Every stop of the sparkline's ramp, quietest first, with the key it came from.
///
/// [`heat_stops`]' sibling and named for the same reason that one is: adding a
/// stop to the theme should be one edit here rather than three, and a missed one
/// makes a gate quietly stop seeing a colour rather than fail. Three sites in
/// this file spelled the triple out before it had a name.
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
        scrolling: None,
        worktree: "vigia".to_owned(),
        branch: None,
        mode: Mode::Watching,
        notice: None,
        following: false,
        masthead: true,
        sheet: false,
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
        landed: false,
        list: Vec::new(),
        list_top: 0,
        current_span: 0,
        total_rows: 0,
        rows_above: 0,
        rows: vec![
            Row::file(FileEntry {
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
        scale: Scale::flat(0),
        worktree_churn: Default::default(),
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

/// The same, from a given column, so a gate can ask about the row **without** the
/// pane's leading cell.
///
/// **That cell stopped being part of the wash's question**
/// ([#218](https://github.com/breferrari/vigia/issues/218)): it carries §5.1's
/// left bar, which is a background on a blank column of margin and degrades on its
/// own terms. A gate asking *does this palette wash* has to skip it or it is
/// asking two questions and failing the one it did not mean.
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
    // Every gate below indexes rows by the constants above, so if the layout ever
    // moves this is the one that says so, by name, instead of five gates failing
    // with assertions about colour.
    // One row taller since the footer gained a rule, which takes a body row and
    // would otherwise land on the last line this gate reads by name.
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
fn the_readme_recipe_for_terminal_colours_plus_a_wash_draws_one() {
    // **A published recipe with no gate is a promise nothing keeps.** The README
    // answers "I want my own scheme *and* the row tint" with three lines of theme
    // file, and it is the only answer there is: `ansi` refuses a wash at every
    // depth by contract, because a wash has to assume a background and that
    // palette's whole point is that it assumes none. Overriding the two keys is
    // the reader supplying the assumption themselves.
    //
    // Restated here rather than read out of `README.md`. A test that parsed the
    // file would pass on a README that had quietly stopped saying this.
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
    // **This gate used to assert the opposite**
    // ([#218](https://github.com/breferrari/vigia/issues/218)). It read
    // `a_palette_that_declines_a_bar_leaves_the_sigil_alone` and pinned every
    // built-in's `_bar` keys as unset, which is how §5.1's *tinted row with a
    // coloured left bar* shipped as the tint alone for three phases: the key
    // existed, the renderer consumed it, and a gate held it empty.
    //
    // The refusal it encoded rested on *the bar has no terminal equivalent that
    // does not spend a column I6 forbids*, which stopped being true when
    // [#119](https://github.com/breferrari/vigia/issues/119) gave the pane a
    // margin. So the gate inverts, and it inverts on the premise rather than on
    // taste.
    //
    // **Background, never foreground**, which is the property that makes it free:
    // the bar lands on a blank column of that margin, so it recolours no glyph and
    // needs no contrast against one. A foreground here would be a colour on a
    // space, which is nothing, and would mean the element had quietly gone back to
    // riding the sigil.
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
    // sit just above the background, which is exactly what the colour is for.
    //
    // **The exemption used to be by omission and this comment used to deny it.**
    // It claimed "the exemption is listed rather than inferred, so adding a field
    // cannot join it by accident", and what was listed was the *readable* side, so
    // every field not in that array was exempt by default. `spark_track` joined
    // that way in [#78](https://github.com/breferrari/vigia/issues/78): a track
    // drawn as one stroke rather than a solid block, taking the colour this gate
    // exists to keep off anything a reader has to see, and no gate said a word.
    // Whether that value is right is [#60](https://github.com/breferrari/vigia/issues/60)'s
    // question; that it was never a decision is this gate's.
    //
    // **So the partition is made by the compiler.** Destructuring `Theme` with no
    // `..` rest pattern means a new field cannot reach this gate unclassified.
    // Verified by adding one: the three palette constructors fail first with
    // `E0063: missing field`, and once those are filled in this file fails with
    // `E0027: pattern does not mention field`, naming it. So the author is made
    // to say which side it is on, after doing the work they had to do anyway.
    //
    // Strictly better than the field count this briefly used, which could be
    // silenced by bumping a number, and available only because every field is
    // `pub`. What is *not* available is walking the styles by key string:
    // `Theme::KEYS` is public and the values behind those names are not, which is
    // why this is a destructure rather than a loop.
    let ansi = Theme::ansi();
    let Theme {
        // Read by a reader, so none of these may be colour 8.
        chrome,
        chrome_dim,
        path,
        path_live,
        path_cold,
        // Text, so it is checked with the rest, and the one field here that
        // carries a modifier as part of its meaning: `path_hover` underlines,
        // which is the whole of what keeps it apart from the recency ladder.
        //
        // **It is the quietest of the four rather than the brightest, corrected
        // 2026-08-16** ([#193](https://github.com/breferrari/vigia/issues/193)).
        // It holds `bar_hover`'s value now, which on this palette is `Gray` and
        // therefore `path_cold`'s foreground **exactly**, so the modifier is not
        // merely the last separation left, it is the only one. That is why it is
        // still checked here: `Gray` is not colour 8 and must not become it, and
        // `a_palette_never_draws_text_in_colour_8` is about the foreground
        // alone. The modifier rides along untested here and is gated in
        // `tests/render.rs`, where the row is actually drawn.
        path_hover,
        kind,
        hunk,
        gutter,
        context,
        note,
        alert,
        comment,

        // Exempt: a track is not text. It is a mark that should sit just above
        // the pane, which is what colour 8 is for. `spark_track` is the awkward
        // one and says so in its own doc: it is a single stroke rather than a
        // solid block, so the premise reaches it less well, and on this palette
        // there is nothing between colour 8 and the weight content is drawn in.
        heat_track: _,
        bar_track: _,
        spark_track: _,

        // Exempt: marks and fills, none of them text, none of them colour 8.
        pulse: _,
        // Three stops of one ramp since #196, all three exempt for the reason
        // the first always was: a sparkline bucket is a block, not a glyph a
        // reader has to make out. `a_sparkline_track_is_never_the_colour_of_a_bucket`
        // is where every stop is held apart from the track it sits on.
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
    //
    // **Read past the pane's leading cell, because the bar is not the wash**
    // ([#218](https://github.com/breferrari/vigia/issues/218)). A wash has to be a
    // *tint*, and a tint is the thing that cannot be chosen without knowing the
    // background behind it. The bar is a **saturated block on a blank column**,
    // which is what the mockup draws and what needs no such knowledge, so this
    // palette can carry one and the loss §11.1 records at sixteen colours is
    // smaller than it was. The two are separate keys precisely so they can degrade
    // apart, and a gate that read them together would report a ruling it was not
    // asked about.
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
    // `SPEC.md` §11.1's recorded loss, now a property of the *depth* rather than of
    // the tool. What must not go with the wash is the sigil's own colour: patching
    // an unset bar over the diff style has to leave the diff style alone, and
    // getting that wrong blanks the last thing distinguishing an addition.
    // **`Ansi256` is in this list and used to be in the washing one.** The cube
    // cannot hold a subtle colour: `#1b3d29` quantises to `#005f00`, and over a
    // newly added file that is a screen of flat green rather than a tint. The
    // arithmetic is in `tests/colour.rs`; this is the half that draws.
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

/// The pane the graded fixture is drawn on.
///
/// **Wide enough to take the widest heat rung**, which is what keeps
/// [`graded_heat`]'s premise true: at any narrower rung the renderer sums
/// adjacent source slices, and the four graded values would arrive as two summed
/// ones with shares nobody chose. It was 120 columns while the source resolution
/// and the widest rung were the same number
/// ([#161](https://github.com/breferrari/vigia/issues/161) separated them).
const GRADED_PANE: u16 = 140;

/// A file whose heat profile has a slice in each band.
///
/// Drawn at [`GRADED_PANE`], so no re-projection happens and the drawn slices are
/// the source slices. The busiest is 12, so 8 is hot (two thirds), 4 is warm (one
/// third) and 1 is low, which puts one slice in every band by construction rather
/// than by luck.
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
        landed: false,
        list: Vec::new(),
        list_top: 0,
        current_span: 0,
        total_rows: 0,
        rows_above: 0,
        rows: vec![Row::file(FileEntry {
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
        scale: Scale::flat(0),
        worktree_churn: Default::default(),
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
        // **Symbol and style together.** Every heat slice is a full block and so is
        // a sparkline's top rung, and the counters on the same row share the
        // track's grey. Either half alone is a coincidence waiting to happen, and
        // has already happened once in this suite.
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

/// One file whose eight buckets climb, so every stop of the ramp is on one row.
///
/// `peak` is the busiest bucket **anywhere on screen**, which is what the ramp is
/// measured against, so it is set to the tallest bucket here rather than left at
/// zero: a zero peak is the empty-store path and draws nothing but track.
fn climbing() -> View {
    View {
        rows: vec![Row::file(FileEntry {
            path: "src/a.rs".to_owned(),
            from: None,
            kind: 'M',
            churn: Some((12, 0)),
            spark: [
                0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 4, 4, 5, 5, 7, 7, 9, 9, 12, 12,
            ],
            recency: Recency::Cold,
            heat: [HeatBucket::default(); HEAT_BUCKETS],
        })],
        files: 1,
        scale: Scale::spread(12),
        worktree_churn: Default::default(),
        ..View::default()
    }
}

#[test]
fn a_taller_sparkline_bucket_is_drawn_hotter() {
    // **The ramp, read off the cells rather than off the theme**, which is the
    // half `the_sparkline_ramp_has_three_stops_where_the_depth_can_draw_them`
    // cannot reach: that one holds the palette apart, this one holds that the
    // renderer spends it in the right order. A ramp whose stops were distinct and
    // applied backwards would satisfy the other gate exactly.
    //
    // `assets/preview.svg` has drawn this since the start, tallest brightest, and
    // the shell drew one flat colour until #196.
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
    // Ranked once per bucket rather than once per comparison, which also puts the
    // "no stop of the ramp" panic on the bucket that caused it.
    let stops = spark_stops(&theme);
    let ranked: Vec<(usize, usize)> = drawn
        .iter()
        .map(|&(rung, fg)| {
            let rank = stops
                .iter()
                .position(|(_, style)| style.fg == fg)
                .unwrap_or_else(|| {
                    panic!(
                        "the bucket at rung {rung} is drawn in {fg:?}, which is no stop of the ramp"
                    )
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
    // [`the_heat_ramp_has_three_stops_where_the_depth_can_draw_them`] one element
    // over, and the same picture is what asks for it: `assets/preview.svg` ramps
    // its sparkline across five greens where the shell drew one flat colour
    // (#196). Three rather than five, because three is what `Band` distinguishes
    // and what the depth ladder can draw.
    //
    // Read off the resolved theme rather than off cells, which is where the heat
    // one differs: heat has a kind-by-band cross product worth drawing through,
    // and this is one hue at three weights.
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

    // **And `ansi` spends two, which is a fact about the palette rather than a
    // shortfall**, exactly as `heat_added` records for itself: sixteen names hold
    // a normal and a bright of each hue and no third, so the middle stop is the
    // normal one. Written out in the palette rather than left to the depth ladder
    // to collapse by accident, which is what this asserts.
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
    // The track and the bars land on the same eight columns of the same row, and
    // the track is drawn from `_` where a bar is drawn from a block, so glyph
    // already separates them for a reader. Colour is what separates them for a
    // reader who is *glancing*, which is the whole of `SPEC.md` §5, and it has to
    // survive the ladder rather than only the palette it was authored in.
    //
    // Against `chrome_dim` as well, because that is the one other style that
    // draws a `·`-weight mark on a chrome line: two dim greys carrying two
    // meanings is the collision `Theme::bar` records being hit twice already.
    //
    // **`Depth::None` is left out on purpose and it is the interesting one.**
    // There every foreground resolves to `Color::Reset`, so this property is
    // false by construction: a reader on `NO_COLOR` or `TERM=dumb` has no colour
    // channel at all. That depth is exactly why the track is `_` rather than the
    // ramp's `▁`, because the glyph is then the only thing left carrying the
    // distinction, and `a_sparkline_track_is_told_from_a_bucket_with_no_colour_at_all`
    // is where that is asserted. The depths are listed rather than iterated so
    // the omission is visible instead of silent.
    for (name, base) in [
        ("ansi", Theme::ansi()),
        ("dark", Theme::dark()),
        ("light", Theme::light()),
    ] {
        for depth in [Depth::Truecolor, Depth::Ansi256, Depth::Ansi16] {
            let theme = base.resolve(depth);
            // **Every stop of the ramp, not only its quietest.** #196 made this
            // three values where it was one, and a gate that checked the first
            // would have let the other two collide with the track they are drawn
            // beside.
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
    // **The failure a track has that a bucket does not: quantising into the
    // background.** A track is authored just above the pane, which is the point
    // of it, and the sixteen-colour rung has nothing that close to either end.
    // `dark`'s track was `#21262d` and resolves to `Color::Black` at `Ansi16` on
    // a `#0d1117` pane; `light`'s was `#d0d7de` and resolves to `Color::White` on
    // white. Either one draws a full row of track in the pane's own colour, which
    // is pixel for pixel the blank column
    // [#78](https://github.com/breferrari/vigia/issues/78) exists to remove: the
    // element would have looked exactly as broken as before, on the one frame it
    // was added for.
    //
    // Against the background rather than against `spark`, because those are two
    // different failures and the gate above catches only the second. `ansi` is
    // exempt, and stated rather than skipped: it names no background at all,
    // which is that palette's whole contract.
    //
    // `heat_track` and `bar_track` fail this today and are deliberately not
    // asserted here. Their truecolour values are read off `assets/preview.svg`
    // and are right, so the defect is in what sixteen colours do to them rather
    // than in the value, which is a different repair from this one and is filed
    // separately.
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
    // `track_at` in `tests/render.rs` reads a track by symbol and style, and the
    // symbol is `_`, which is also most of what a `snake_case` path is made of.
    // The two are drawn on the same row, so if a path style and the track style
    // ever resolve alike, that helper counts a file name as track cells and a
    // reader sees one dim underscore run where there are two things.
    //
    // **`light` at `Ansi16` is exempt, and enumerating why is the point.** That
    // rung has exactly four greys and this palette has spent all of them. The
    // occupants that matter here, since a `_` on a file row is what the track can
    // be confused with: `White` is the pane, `Black` is `path` and `path_live`,
    // `Gray` is `path_cold` **and `gutter`**, `DarkGray` is `chrome_dim`. There
    // is no fifth for a track to take, so the collision is arithmetic rather than
    // a choice, and moving the track onto `DarkGray` only trades it for the
    // `chrome_dim` one.
    //
    // Not an exhaustive census of the rung and it does not need to be: `Black`
    // also holds `context`, `DarkGray` also holds `bar` and `comment`, and
    // `White` also holds `heat_track` and `bar_track`, which is
    // [#98](https://github.com/breferrari/vigia/issues/98) showing up here of all
    // places. None of those is drawn as a `_` on a file heading row, which is the
    // only confusion this gate is about.
    //
    // Left where it is because the two collisions are not equally bad: a path's
    // underscores sit inside a word in the left column, the track is eight
    // contiguous ones in a reserved slot, and nothing in the suite reads a track
    // under this palette (`Theme::default` is `ansi`, where the track is
    // `DarkGray` and `path_cold` is `Gray`).
    //
    // `gutter` was missing from the list for one round, which mattered less for
    // the count than for the staleness check below: keyed to `path_cold` alone it
    // would have called the exemption stale while `gutter` still held `Gray`.
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
                // `path_hover` belongs here for the same reason the other three
                // do: it is drawn on a path, so an underscore in a file name
                // must not resolve to the sparkline's own track colour at any
                // depth. It read "the fourth weight" until #193, which is a
                // framing that no longer describes it: it is a fourth entry in
                // this list and no longer a fourth rung above the ladder.
                ("path_hover", theme.path_hover),
            ] {
                // **Only the comparison that actually collides is skipped.**
                // Skipping the whole rung would take `path` and `path_live` with
                // it, and both are `Black` there against a `Gray` track, so a
                // future move of either onto the track's colour would go unseen
                // behind an exemption that was never about them.
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
    //
    // **The exact negation of the one comparison that is skipped**, which is
    // narrower than it was and had to become so. While the `continue` skipped the
    // whole rung, a disjunction over `path_cold` and `gutter` was right, because
    // either one holding `Gray` kept some skipped comparison live. Now that only
    // the `path_cold` comparison is skipped, `gutter` is not among the three
    // fields the loop compares at all, so accepting it as evidence would keep the
    // exemption alive after the thing it exempts had gone: move `path_cold` off
    // `Gray` and the skipped comparison would pass, the `continue` would be dead
    // weight, and this would stay green on `gutter`'s account.
    //
    // The narrowing and the disjunction were landed in different rounds, which is
    // how the two ended up disagreeing about which fact keeps the exemption alive.
    let light = Theme::light().resolve(Depth::Ansi16);
    assert_eq!(
        light.spark_track.fg, light.path_cold.fg,
        "`light` at sixteen colours no longer draws the track in `path_cold`'s \n         colour, so the skipped comparison above would now pass and the \n         `continue` should come out"
    );
}

#[test]
fn a_sparkline_track_is_told_from_a_bucket_with_no_colour_at_all() {
    // **The depth the whole glyph choice was made for.** At `Depth::None` every
    // style is `Color::Reset`, so a track and a bucket are the same colour and
    // the shape is all a reader has. That is why `SPEC.md` §5.1 refuses the
    // ramp's floor for the track: the heat strip may draw a live slice and its
    // track from one `█` because colour is its only channel, and here the
    // sparkline has no colour channel either, so the glyph has to carry it.
    //
    // Read by symbol alone, which is correct exactly here and nowhere else:
    // every other gate in this suite matches symbol and colour together because
    // two elements can share a glyph. At this depth matching on colour would
    // match everything.
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
    // Guard the fixture, the way `a_row_missing_a_glance_element_keeps_its_column`
    // does for digits: at this depth colour is gone, so `_` is counted by symbol
    // alone across the whole row, and an underscore in the path would be counted
    // as a track cell. `three_kinds` draws `src/a.rs`; this fails loudly if that
    // ever changes rather than quietly counting one cell too many.
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
    // `every_key_the_struct_has_is_a_key_a_file_can_set` asserts that shape. What
    // that cannot assert is that a *new* key reaches the field a reader meant,
    // since it only checks each key is accepted.
    //
    // Worth its own gate for this key rather than in general: `README.md` lists
    // the theme keys by hand and omitted this one until an audit found it, so a
    // reader following the README could not reach it at all. A parse test is what
    // makes the documented key and the drawn colour the same thing.
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

/// The reference background each truecolour palette is designed against.
///
/// **A terminal does not tell this program its background**, which is why the
/// `ansi` palette exists and why `theme.rs` opens by saying a tint has to assume
/// one. So this is an assumption, stated rather than smuggled: it is the
/// background `assets/preview.svg` paints and the one each palette's greys were
/// picked against. A colour that vanishes on the background it was designed for
/// has no chance on any other.
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
///
/// **Data only, and deliberately not a shared assertion.** Two gates enumerate
/// this same triple and their bodies make different claims: one is about all three
/// tracks against the pane, the other about the bar's marks across every row the
/// bar crosses. Merging the gates would let one claim's failure read as the
/// other's, so only the enumeration is shared, which carries no rationale of its
/// own.
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
///
/// **One extractor rather than one per field.** A second copy was written for
/// backgrounds when [`a_bar_track_is_visible_on_every_row_it_crosses`] landed, and
/// the two differed only in the field read and the noun in the panic. The washes
/// this file compares against already arrive as a `Color` from [`wash_of`], so a
/// background extractor was never needed at all.
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
///
/// Well under WCAG's 4.5:1 for text, deliberately: none of these is text, and a
/// track that competed with its own thumb would break the ruling that the track
/// is context and the thumb is the reading. What it rules out is the case that
/// actually shipped, which is not a subtle colour but an **absent** one.
const TRACK_FLOOR: f64 = 2.0;

#[test]
fn a_track_is_visible_against_the_pane_it_is_drawn_on() {
    // **Reported from use, and the numbers say why nothing was visible.** The
    // scrollbar's track and its step buttons drew in `bar_track`, which was
    // `#21262d` on `#0d1117`: **1.24:1**, where 1.0 is the background exactly.
    // Only the thumb and a *pressed* button could be seen, because those draw in
    // `bar` at 6.15:1. The light palette had the same defect at 1.45:1, and this
    // gate then found a third nobody had reported: the sparkline's own track at
    // 1.55:1, which is the empty bucket §5.1 rules must draw something.
    //
    // **The gate that existed could not see any of it.** The sparkline tests
    // below assert a track is not the *same palette colour* as the background,
    // which was true of every one of these while all three were invisible.
    // Identity is not visibility, and the difference is this test.
    //
    // The earlier reading was that these truecolour values were right because
    // they are read off `assets/preview.svg`, and that the defect belonged to
    // the sixteen-colour rung. That is the picture-versus-cell-grid distinction
    // `SPEC.md` §5.1 already draws: a 1.24:1 edge across many pixels of SVG is
    // perceptible, and the same ratio inside one terminal cell is not.
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

        // **And a stroke outranks a block**, which is the rule `spark_track`'s
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

        // **And still subordinate to what it is a track for.** A track that
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

/// The bar's track and thumb are legible on **every** row the bar crosses.
///
/// [`a_track_is_visible_against_the_pane_it_is_drawn_on`] checks all three tracks
/// against the pane, and for two of them that is the whole story: the heat strip
/// and the sparkline draw on list rows and file headings, which are never washed.
///
/// **The bar is the exception, and it became one on 2026-08-18**
/// ([#239](https://github.com/breferrari/vigia/issues/239)). The row wash now runs
/// under the bar's own column, so on a changed row the track and the thumb sit on
/// `added_row` or `removed_row` rather than on the pane. Every ratio in the older
/// gate is measured against the pane, so it could not see that `#57606a` fell to
/// **1.88:1** on an added row: a value this palette calls absent at 1.24:1 and
/// chose 2.96:1 to escape. The bar would have drawn as a dashed line, solid on
/// context rows and faint on the rows a reader is looking at.
///
/// This is the gate that would have caught it, and it is why the fix could not be
/// the one-line widening it looks like. It asserts both halves on all three
/// backgrounds, because a track that clears the floor by reaching its own thumb has
/// satisfied one rule by destroying the other.
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

            // **Subordinate on the same background it was measured legible on.**
            // Clearing the floor on the wash while checking the separation only
            // against the pane would pass a value that is illegible on one and
            // indistinguishable on the other, which is the shape SPEC.md section 7
            // keeps finding: two true assertions with the failure between them.
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
///
/// This began as a claim that the bar and the wash touch **disjoint** style fields,
/// which was true of `fg` and `bg` and was the stated reason the draw order did not
/// matter. Round 2 of [#239](https://github.com/breferrari/vigia/issues/239)'s audit
/// falsified it: a **modifier** is a third field, `Cell::set_style` inserts one
/// unconditionally, and `VIGIA_THEME` lets a reader put `reverse` on a row wash. The
/// order was never free, and the reason recorded here was wrong rather than
/// incomplete.
///
/// It is free now because `bar_cell` assigns rather than merges: the band's
/// background is kept and the bar owns the symbol, the foreground and the modifier
/// outright. `tests/render.rs::a_row_washs_modifier_never_reaches_the_scrollbar` is
/// the gate over that, from the drawn side.
///
/// What is left for a palette to get wrong is the other half of that bargain, and it
/// is what this gate now holds. `bar_cell` reads exactly two things from the style it
/// is handed, so:
///
/// - **A foreground is required.** It falls back to the cell's own, which under a
///   band is the *wash's* foreground, so a bar style without one would draw the
///   diff's colour and still pass every contrast gate here, which measures the
///   palette rather than the screen.
/// - **A background is declined, not forbidden.** `bar_cell` falls back to the
///   band's only where the style carries none, so a palette declaring one opts its
///   own bar out of the band #239 ruled should run under it. That is a legitimate
///   thing for a *reader's* theme to do and the wrong default to ship.
///   `render.rs::a_bar_styles_own_background_wins_over_the_band` holds the other
///   side, that a declared background really does draw.
///
/// Both are the same failure in opposite directions, a field that looks authored and
/// is not what draws.
#[test]
fn every_bar_style_says_what_bar_cell_reads() {
    // **Its own enumeration, and `ansi` is why.** [`palettes`] cannot carry it:
    // every other gate there extracts channels, and `Theme::ansi` is named colours
    // by construction, so `rgb_of` would panic on it. This gate asks about a
    // style's *shape* rather than its values, so it can hold the palette the other
    // two structurally cannot, and `ansi` is the one that matters most: it is the
    // default when nothing is detected, so leaving it out meant the shipped
    // default was compliant by luck. Named by round 3 of #239's audit.
    //
    // **Unresolved, which is stricter than resolving.** `Depth::resolve` only ever
    // strips a background and always maps a foreground to `Some`, so a resolved
    // palette passes both assertions more easily than the authored one. The
    // authored values are also what a theme file writes and what a reader edits.
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
