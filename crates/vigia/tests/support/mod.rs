//! Reading the drawn screen: the selectors more than one test binary needs.

// Each test binary uses a different subset, and a binary that used all of it
// would be a binary asking every question this file answers.
#![allow(dead_code)]

use ratatui::buffer::Buffer;
use ratatui::style::Color;
use vigia::{FileEntry, ListRow, Theme, View};

/// Columns of row `y` between `from` and `to` whose glyph is one of `symbols`
/// **and** whose foreground is one of `colours`.
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
pub fn spark_colours(theme: &Theme) -> Vec<Color> {
    [theme.spark, theme.spark_warm, theme.spark_hot]
        .into_iter()
        .filter_map(|style| style.fg)
        .collect()
}

/// Every file the pinned list draws, skipping the run separators.
pub fn listed_files(view: &View) -> impl Iterator<Item = &FileEntry> {
    view.list.iter().filter_map(ListRow::entry)
}
