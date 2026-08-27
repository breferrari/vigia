//! Reading the drawn screen: the selectors more than one test binary needs.
//!
//! **Here because a Rust integration test is its own binary**, so two test files
//! that ask the same question of a `Buffer` either share a module or keep two
//! copies. Until [#252](https://github.com/breferrari/vigia/issues/252) there was
//! one file counting glance elements and the question of sharing never arose;
//! `tests/rail.rs` is the second, and it arrived needing the same theme lists and
//! the same symbol-and-colour predicate.
//!
//! **What lives here is the selector, never the alphabet.** `RAMP`, `HEAT_SLICE`
//! and `TRACK` stay restated in each file that reads them, which is a settled
//! convention in this suite with its reason recorded: a test importing the
//! renderer's own glyph table would agree with it by construction and gate
//! nothing. A *colour* is different. `Theme` has ten heat fields and three
//! sparkline fields, none of them a claim about the renderer's behaviour, and
//! `spark_colours`'s own docblock says a fourth ramp stop must not have to be
//! remembered in two places. It was about to be three.
//!
//! Not a `#[path]` module like `vigia-core`'s: a directory under `tests/` is not
//! compiled as a test binary and `tests/package.rs` enumerates `.rs` files
//! directly under `tests/`, so a plain `mod support;` costs that gate nothing.

// Each test binary uses a different subset, and a binary that used all of it
// would be a binary asking every question this file answers.
#![allow(dead_code)]

use ratatui::buffer::Buffer;
use ratatui::style::Color;
use vigia::{FileEntry, ListRow, Theme, View};

/// Columns of row `y` between `from` and `to` whose glyph is one of `symbols`
/// **and** whose foreground is one of `colours`.
///
/// **Symbol and colour together, because neither alone separates the elements
/// that share a glyph.** The sparkline's top rung is `█` and so is every heat
/// slice, so a symbol-only match counts one as the other; and since
/// [#157](https://github.com/breferrari/vigia/issues/157) a counts cell is drawn
/// in `Theme::added`/`Theme::removed`, which collide with the heat ramp, so a
/// colour-only match counts a counter as a slice. The symbol term separates
/// those: a slice is a block and a counter is digits.
///
/// **Bounded by a column range, which is what the rail made necessary**
/// ([#252](https://github.com/breferrari/vigia/issues/252)). A row can hold two
/// regions in that layout, and a strip counted across the whole row adds one
/// region's rung to the other's. That does not merely inflate the number, it
/// lands on a *legal* one: twelve slices beside twelve is twenty-four, which is
/// the widest rung, so a whole-row count passes a whole-rungs assertion on a
/// renderer that had lost the ladder entirely. Callers that want the whole row
/// pass its whole width and say so.
/// The rows a reader would see, as plain strings with trailing blanks trimmed.
///
/// **Rebuilt from the cells rather than parsed out of `TestBackend`'s
/// `Display`.** That `Display` appends `Hidden by multi-width symbols: [...]` to
/// any row holding a two-column glyph, so every assertion about how a row *ends*
/// was silently reading that note instead of the row. Walking the cells the way a
/// terminal does, skipping what the previous symbol already covered, gives back
/// exactly what a reader would see.
///
/// **Here rather than in each file that reads a screen**
/// ([#272](https://github.com/breferrari/vigia/issues/272)), which is this
/// module's own reason one paragraph up. `tests/wrap.rs` arrived with a second
/// copy that did the naive thing, concatenating every cell's symbol: correct on
/// an ASCII fixture and one wide glyph away from reintroducing the bug the
/// paragraph above records fixing, in the file whose own subject is a pane
/// drawing `↳` and `›`.
pub fn rows_of(buf: &Buffer, area: ratatui::layout::Rect) -> Vec<String> {
    (area.top()..area.bottom())
        .map(|y| {
            let mut row = String::new();
            let mut covered = 0usize;
            for x in area.left()..area.right() {
                if covered > 0 {
                    covered -= 1;
                    continue;
                }
                let symbol = buf[(x, y)].symbol();
                row.push_str(symbol);
                covered = ratatui::text::Span::raw(symbol).width().saturating_sub(1);
            }
            row.trim_end().to_owned()
        })
        .collect()
}

pub fn columns_in(
    buf: &Buffer,
    y: u16,
    columns: std::ops::Range<u16>,
    colours: &[Color],
    symbols: &[char],
) -> Vec<u16> {
    columns
        .filter(|x| {
            let cell = &buf[(*x, y)];
            symbols
                .iter()
                .any(|glyph| cell.symbol() == glyph.to_string())
                && cell.style().fg.is_some_and(|fg| colours.contains(&fg))
        })
        .collect()
}

/// Every colour the heat strip can draw a slice in, its track included.
///
/// The track is in the list because an empty slice is still a slice and the
/// ladder is about how many columns the element was given, not how many of them
/// had something in them.
pub fn heat_colours(theme: &Theme) -> Vec<Color> {
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

/// Every colour a sparkline **bar** can be drawn in.
///
/// The track is deliberately not here: it is a different glyph as well as a
/// different colour, and counting the two together would accept the cross
/// products, a track glyph in a bar's colour or the reverse, neither of which is
/// ever drawn. A caller wanting the whole slot asks twice.
pub fn spark_colours(theme: &Theme) -> Vec<Color> {
    [theme.spark, theme.spark_warm, theme.spark_hot]
        .into_iter()
        .filter_map(|style| style.fg)
        .collect()
}

/// Every file the pinned list draws, skipping the run separators.
///
/// **The list stopped being one file per row in
/// [#313](https://github.com/breferrari/vigia/issues/313)**, so a test that wants
/// the files has to say so. `ListRow::entry` makes that a filter rather than an
/// unwrap nobody would notice going wrong, and it lives here for this module's own
/// reason: three files were about to spell the row-to-file rule three ways, and it
/// is a rule about the *renderer's* row model rather than an alphabet a test
/// should restate.
pub fn listed_files(view: &View) -> impl Iterator<Item = &FileEntry> {
    view.list.iter().filter_map(ListRow::entry)
}
