//! The renderer, as text.

mod support;

use std::time::Duration;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::{Modifier, Style};
use vigia::{
    Chrome, FileEntry, Glyphs, Grabbed, HEAT_BUCKETS, HeatBucket, Hovered, ListRow, Mode, Position,
    Region, Row, Scale, Theme, View, body_layout, diff_height, regions, render,
};
use vigia_core::{Class, HISTORY_BUCKETS, LineKind, Origin, Recency, Span};

/// The `n`th drawn list row's entry, mutably, for a fixture that edits one.
fn listed_mut(view: &mut View, at: usize) -> &mut FileEntry {
    match &mut view.list[at] {
        ListRow::File(entry) => entry,
        ListRow::Group { .. } => panic!("list row {at} is a run separator, not a file"),
    }
}

/// Buckets a sparkline draws on the panes this file renders at.
const DRAWN_BUCKETS: usize = 12;

/// The mark the renderer writes where a row runs past its edge.
const CONTINUES: &str = "›";

/// What joins two facts about one subject on a line of chrome.
const FACT_JOIN: &str = " · ";

/// The sparkline's ramp, tallest last.
const RAMP: [&str; 8] = ["▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];

/// Every glyph a scrollbar's own column can carry: the track, the thumb, and the
/// step button at each end.
const BAR_GLYPHS: [char; 4] = ['│', '█', '▲', '▼'];

/// The mark on the row the diff is inside.
const CARET: &str = "▸";

/// The row a pinned list starts on, on a pane with no room for the masthead.
const LIST_TOP: u16 = 2;

/// Whether a cell's symbol is one of [`BAR_GLYPHS`].
fn is_bar_glyph(symbol: &str) -> bool {
    let mut chars = symbol.chars();
    matches!(chars.next(), Some(glyph) if BAR_GLYPHS.contains(&glyph)) && chars.next().is_none()
}

/// What a sparkline bucket nothing was written in draws.
const SPARK_TRACK: &str = "_";

/// Every foreground a written sparkline bucket can take.
fn spark_colours(theme: &Theme) -> Vec<Option<Color>> {
    [theme.spark, theme.spark_warm, theme.spark_hot]
        .into_iter()
        .map(|style| style.fg)
        .collect()
}

/// The heat strip's slice, restated rather than imported for [`CONTINUES`]'
/// reason.
const HEAT_SLICE: &str = "■";

/// Every foreground a heat slice can take.
fn heat_colours(theme: &Theme) -> Vec<Option<Color>> {
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
    .map(|style| style.fg)
    .collect()
}

/// The margin ladder, widest pane first: blank columns the pane keeps between
/// its own edge and any glyph, both sides counted together.
const MARGIN_RUNGS: [(u16, u16); 4] = [(80, 4), (79, 3), (44, 2), (43, 1)];

/// The column every row's text begins at, on a pane this wide: the margin above,
/// split evenly with the odd column going left.
fn inset_at(width: u16) -> u16 {
    let total = MARGIN_RUNGS
        .iter()
        .find(|(from, _)| width >= *from)
        .map_or(0, |(_, cells)| *cells);
    total.div_ceil(2)
}

/// A drawn row with the pane's inset taken off its head, having first checked
/// that the inset is exactly what is there.
fn content(row: &str, width: u16) -> &str {
    if row.is_empty() {
        return row;
    }
    let inset = usize::from(inset_at(width));
    let head: Vec<char> = row.chars().take(inset).collect();
    assert!(
        head.len() == inset && head.iter().all(|c| *c == ' '),
        "at {width} columns a row put a glyph inside the pane's {inset}-column \
         inset: {row:?}"
    );
    let at = row
        .char_indices()
        .nth(inset)
        .map_or(row.len(), |(at, _)| at);
    &row[at..]
}

/// Draw a view at `width` by `height` and hand back the backend to snapshot.
fn screen(width: u16, height: u16, view: &View, chrome: &Chrome) -> TestBackend {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
    let theme = Theme::default();
    terminal
        .draw(|f| {
            let area = f.area();
            render(
                f.buffer_mut(),
                area,
                view,
                &theme,
                Glyphs::default(),
                chrome,
            );
        })
        .expect("draw");
    terminal.backend().clone()
}

/// The neutral chrome, which is deliberately not the state a shell starts
/// in.
fn text_rows(drawn: &ratatui::backend::TestBackend, width: u16, height: u16) -> Vec<String> {
    (0..height)
        .map(|y| {
            (0..width)
                .map(|x| {
                    drawn
                        .buffer()
                        .cell((x, y))
                        .map_or(' ', |c| c.symbol().chars().next().unwrap_or(' '))
                })
                .collect()
        })
        .collect()
}

fn chrome() -> Chrome {
    Chrome {
        pressed: None,
        gripped: None,
        hovered: None,
        selected: None,
        scrolling: None,
        worktree: "vigia".to_owned(),
        // `None` because these views have a diff in them, and only the empty
        // state names a branch. A populated frame never asks, which is I4 and
        // which `lib.rs`'s `branch_for` gates.
        staged: None,
        icons: false,
        links: false,
        root: String::new(),
        elsewhere: 0,
        branch: None,
        mode: Mode::Watching,
        notice: None,
        voice: None,
        following: false,
        masthead: true,
        rail: false,
        sheet: None,
        // The first paint's chrome: no frame has completed, so there is no p99 to draw.
        frame: None,
        memory: None,
        notes: (0, 0),
    }
}

/// The chrome of every frame after the first, on a platform that reads memory.
fn diagnostics_chrome() -> Chrome {
    Chrome {
        pressed: None,
        gripped: None,
        scrolling: None,
        frame: Some(Duration::from_micros(800)),
        memory: Some(19 * 1024 * 1024),
        notes: (0, 0),
        ..following_chrome()
    }
}

/// The chrome of a worktree with nothing in it, which is what B3 specifies.
fn empty_chrome() -> Chrome {
    Chrome {
        pressed: None,
        gripped: None,
        scrolling: None,
        staged: None,
        elsewhere: 0,
        branch: Some("main".to_owned()),
        ..chrome()
    }
}

/// The chrome a shell actually starts with.
fn following_chrome() -> Chrome {
    Chrome {
        pressed: None,
        gripped: None,
        scrolling: None,
        following: true,
        masthead: true,
        ..chrome()
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

/// A one-line view whose single content row carries `spans`.
fn highlighted(kind: LineKind, text: &str, spans: Vec<Span>) -> View {
    // Guard the fixture.
    let covered: usize = spans.iter().map(|span| span.len).sum();
    assert!(
        spans.is_empty() || covered == text.len(),
        "the fixture's spans cover {covered} bytes of {}",
        text.len()
    );

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
            file("src/a.rs", 1, 0),
            Row::Hunk {
                old_start: 5,
                old_lines: 0,
                new_start: 5,
                new_lines: 1,
            },
            Row::Line {
                kind,
                number: 5,
                text: text.to_owned(),
                spans,
                emph: Vec::new(),
            },
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

/// What a cell is drawn *in*, as the pair a rung is actually made of.
fn weight(style: Style) -> (Option<Color>, Modifier) {
    (style.fg, style.add_modifier)
}

/// What each cell of `path`'s label on row `y` is drawn in.
fn path_weights(backend: &TestBackend, y: u16, path: &str) -> Vec<(Option<Color>, Modifier)> {
    let start = column_of(backend, y, "M") + 2;
    (start..start + path.chars().count() as u16)
        .map(|x| weight(backend.buffer()[(x, y)].style()))
        .collect()
}

/// The first column of row `y` holding `needle`.
fn column_of(backend: &TestBackend, y: u16, needle: &str) -> u16 {
    column_where(backend, y, |symbol, _| symbol == needle)
        .unwrap_or_else(|| panic!("no {needle:?} anywhere on row {y}"))
}

/// One changed file, as the pinned list carries it.
fn entry(path: &str, added: u32, removed: u32) -> FileEntry {
    FileEntry {
        origin: Origin::Unstaged,
        path: path.to_owned(),
        from: None,
        kind: 'M',
        churn: Some((added, removed)),
        spark: [0; HISTORY_BUCKETS],
        recency: Recency::Cold,
        newest: false,
        heat: [HeatBucket::default(); HEAT_BUCKETS],
    }
}

/// The same file, as a heading in the diff stream.
fn file(path: &str, added: u32, removed: u32) -> Row {
    Row::file(entry(path, added, removed))
}

/// A view with the shape a real frame produces: a file, a hunk, mixed lines.
fn one_file() -> View {
    View {
        whole: Vec::new(),
        landed: false,
        recorded: 0,
        list_span: 3,
        grouped: false,
        list: Vec::new(),
        list_top: 0,
        current_span: 0,
        total_rows: 0,
        rows_above: 0,
        rows: vec![
            file("crates/vigia-core/src/frame.rs", 3, 1),
            Row::Hunk {
                old_start: 258,
                old_lines: 7,
                new_start: 258,
                new_lines: 9,
            },
            line(LineKind::Context, 258, "    pub fn advance(&mut self) {"),
            line(
                LineKind::Context,
                259,
                "        let mut files = Vec::new();",
            ),
            line(
                LineKind::Removed,
                260,
                "        for change in self.changes() {",
            ),
            line(LineKind::Added, 260, "        for change in self.walk() {"),
            line(LineKind::Added, 261, "            // one per path"),
            line(LineKind::Added, 262, "            files.push(change?);"),
            line(LineKind::Context, 263, "        }"),
            line(LineKind::Context, 264, "    }"),
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

#[test]
fn a_screenful_of_diff() {
    let view = one_file();
    insta::assert_snapshot!(screen(80, 14, &view, &chrome()));
}

#[test]
fn the_same_screenful_at_forty_columns() {
    // The width I6 exists for: half a laptop screen beside an agent. Two content
    // lines run past the edge and say so with `›`, which is what `SPEC.md` §11.1
    // rules is not a truncated label.
    let view = one_file();
    insta::assert_snapshot!(screen(40, 14, &view, &chrome()));
}

#[test]
fn the_same_screenful_at_a_hundred_and_twenty_columns() {
    // The third of the three widths §3 names, and the one nothing covered. Wide
    // enough that nothing degrades, which is the point: it is the picture that
    // says the marks and the ladders are absent when they are not needed.
    let view = one_file();
    insta::assert_snapshot!(screen(120, 14, &view, &chrome()));
}

#[test]
fn a_content_row_stands_its_sigil_off_the_line() {
    // `SPEC.md` §5.1's fifth departure, closed.
    let view = one_file();
    // Read off the fixture rather than restated beside it, which is the move the
    // counters' gate on this same branch already makes.
    let indents: Vec<(u16, usize)> = view
        .rows
        .iter()
        .enumerate()
        .filter_map(|(i, row)| match row {
            Row::Line { text, .. } => Some((i as u16 + 1, text.len() - text.trim_start().len())),
            _ => None,
        })
        .collect();

    for width in [80u16, 40] {
        let backend = screen(width, 14, &view, &chrome());

        // Found rather than computed. Recomputing where the renderer puts a
        // sigil would be a second implementation of the gutter agreeing with the
        // first, which is the trap `column_of`'s own doc names.
        let removed = column_of(&backend, 5, "-");
        let added = column_of(&backend, 6, "+");

        // Asserted before anything reads from it, because it is a precondition rather
        // than an afterthought: every comparison below measures from `origin`, and if
        // the two sigils disagreed about their column then `origin` came from a row the
        // loop never checks and each comparison is against itself.
        assert_eq!(
            removed, added,
            "at {width} columns the two sigils sit in different columns, so the \
             origin this gate measures from is not the block's"
        );
        let origin = usize::from(removed) + 2;

        // The sigil has not moved: it still sits one space past the gutter's digits,
        // which is the half of the row this change must leave alone.
        for y in [5u16, 6] {
            let before: String = row_text(&backend, y).chars().take(origin - 2).collect();
            assert!(
                before.ends_with("260 "),
                "at {width} columns row {y}'s sigil is not one space past its \
                 line number: {before:?}"
            );
        }

        for &(y, indent) in &indents {
            let row = row_text(&backend, y);
            // Every row of the block takes its content from the same column,
            // which is the picture's other claim and the one that makes the
            // block read as a block: the two unsigilled context rows draw a
            // space where `+` and `-` go, so their content must line up with a
            // changed row's rather than sliding left into the gap.
            assert_eq!(
                row.chars().nth(origin - 1),
                Some(' '),
                "at {width} columns row {y} drew no gap between its sigil and \
                 its line: {row:?}"
            );
            let first = row
                .chars()
                .enumerate()
                .skip(origin)
                .find(|(_, c)| *c != ' ')
                .map(|(x, _)| x);
            assert_eq!(
                first,
                Some(origin + indent),
                "at {width} columns row {y}'s content does not begin {indent} \
                 columns past the origin {origin}, so either the gap is missing \
                 or the line moved: {row:?}"
            );
        }

        // The unindented line, which is the row this was reported on and the only one
        // where the gap is visible to a reader.
        let bare = highlighted(LineKind::Removed, "pub fn generated_889() {}", Vec::new());
        let flush = screen(width, 6, &bare, &chrome());
        let sigil = column_of(&flush, CONTENT_ROW, "-");
        let row = row_text(&flush, CONTENT_ROW);
        assert_eq!(
            row.chars().nth(usize::from(sigil) + 1),
            Some(' '),
            "at {width} columns an unindented removal drew its sigil against the \
             line, which is the row #164 was reported on: {row:?}"
        );
        assert_eq!(
            row.chars().nth(usize::from(sigil) + 2),
            Some('p'),
            "at {width} columns an unindented removal does not begin exactly two \
             columns past its sigil: {row:?}"
        );
    }
}

/// A worktree with nothing in it, which is the screen the tool sits on most.
fn nothing_changed() -> View {
    View {
        whole: Vec::new(),
        landed: false,
        recorded: 0,
        list_span: 0,
        grouped: false,
        list: Vec::new(),
        list_top: 0,
        current_span: 0,
        total_rows: 0,
        rows_above: 0,
        rows: Vec::new(),
        files: 0,
        top: Position::default(),
        read: 0,
        scale: Scale::flat(0),
        gutter: None,
        worktree_churn: Default::default(),
        notes: Default::default(),
    }
}

#[test]
fn a_clean_worktree_says_so_rather_than_showing_nothing() {
    // A monitor is read by glancing at it, so "nothing has changed" and "I am broken"
    // must not look identical.
    insta::assert_snapshot!(screen(40, 6, &nothing_changed(), &empty_chrome()));
}

#[test]
fn the_same_empty_state_at_eighty_columns() {
    insta::assert_snapshot!(screen(80, 6, &nothing_changed(), &empty_chrome()));
}

#[test]
fn the_same_empty_state_at_a_hundred_and_twenty_columns() {
    // The third of the three widths §3 names. Nothing degrades here, which is
    // what makes it worth keeping: it is the picture that says the ladder is
    // absent when it is not needed.
    insta::assert_snapshot!(screen(120, 6, &nothing_changed(), &empty_chrome()));
}

#[test]
fn the_header_says_which_mode_it_is_in() {
    // The mockup headers `watching · 3 files`; the two are split across the row,
    // and `assets/preview.svg` with them.
    let view = one_file();

    let live = row_text(&screen(80, 6, &view, &chrome()), 0);
    assert!(
        live.trim_end().ends_with("watching"),
        "live header: {live:?}"
    );
    assert!(!live.contains("not watching"), "live header: {live:?}");

    let stopped = Chrome {
        pressed: None,
        gripped: None,
        scrolling: None,
        mode: Mode::Lost,
        ..chrome()
    };
    let lost = row_text(&screen(80, 6, &view, &stopped), 0);
    assert!(
        lost.trim_end().ends_with("not watching"),
        "lost header: {lost:?}"
    );
}

#[test]
fn the_header_carries_no_changed_line_total() {
    // The ruling `SPEC.md` §10 closed with.
    let backend = screen(80, 6, &glancing(), &chrome());
    let header = row_text(&backend, 0);

    // What a header total would have to draw, in either form: the counters' own sigils,
    // or the bare sum if it dropped them.
    const TOTALS: [&str; 4] = ["+", "-", "55", "10"];

    // Guard the fixture, the way [`highlighted`] guards its spans.
    let worktree = chrome().worktree;
    for needle in TOTALS {
        assert!(
            !worktree.contains(needle),
            "the fixture's worktree name {worktree:?} contains {needle:?}, so the \
             assertion below would read its own left-hand side as a total"
        );
    }

    // Non-vacuity, and it is what makes the rest worth asserting.
    let height = backend.buffer().area.height;
    let body: String = (1..height).map(|y| row_text(&backend, y)).collect();
    // The two halves separately rather than as one joined string: each is
    // right-anchored in its own fixed-width column, so how many spaces sit
    // between them is a property of the pane rather than of the counts.
    assert!(
        body.contains("+42") && body.contains("-7"),
        "no per-file counter was drawn, so the header's silence proves nothing: \
         {body:?}"
    );

    // And the header is populated, so its silence is about the total rather than
    // about the row being empty.
    assert!(
        header.contains(&format!("{worktree}{FACT_JOIN}3 changed")),
        "header: {header:?}"
    );

    for needle in TOTALS {
        assert!(
            !header.contains(needle),
            "the header drew {needle:?}, which is a changed-line total or half of \
             one: {header:?}"
        );
    }
}

#[test]
fn the_header_never_lets_the_mode_word_take_the_count_as_its_object() {
    // The header's two facts are about two different subjects: the mode word says
    // whether the watch thread is live, and the count says how many files differ from
    // the index.
    let worktree = chrome().worktree;

    for (mode, word) in [(Mode::Watching, "watching"), (Mode::Lost, "not watching")] {
        // Hoisted out of the two loops below: neither pattern depends on the
        // file count or the width, and building them per iteration would be
        // eighteen allocations apiece for one constant string.
        let governs = format!("{word}{FACT_JOIN}");
        let governed = format!("{FACT_JOIN}{word}");
        let ends_row = format!(" {word}");

        for files in [1usize, 3, 100] {
            let view = View {
                files,
                ..one_file()
            };
            // Every width §3 names.
            for width in [40u16, 80, 120] {
                let header = row_text(&screen(width, 8, &view, &Chrome { mode, ..chrome() }), 0);

                assert!(
                    !header.contains(&governs),
                    "at {width} columns with {files} files the mode word is \
                     followed by a fact it reads as the object of: {header:?}"
                );
                assert!(
                    !header.contains(&governed),
                    "at {width} columns with {files} files the mode word was \
                     joined to the fact before it: {header:?}"
                );

                // The count is drawn, and it is drawn against the worktree name.
                assert!(
                    header.contains(&format!("{worktree}{FACT_JOIN}{files} changed")),
                    "at {width} columns the count is not beside the worktree: \
                     {header:?}"
                );

                // And the mode word ends the row, with blank before it, which is what
                // "the right-hand side carries it alone" means where a test can see it.
                assert!(
                    header.trim_end().ends_with(&ends_row),
                    "at {width} columns the row does not end in the mode word \
                     with a gap before it: {header:?}"
                );
            }
        }
    }
}

/// One list entry carrying every glance element, so the only thing that differs
/// between two of them is the counts width.
fn listed(path: &str, added: u32, removed: u32) -> FileEntry {
    FileEntry {
        origin: Origin::Unstaged,
        path: path.to_owned(),
        from: None,
        kind: 'M',
        churn: Some((added, removed)),
        spark: [
            0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 5, 5, 8, 8, 5, 5, 9, 9, 12, 12,
        ],
        recency: Recency::Cold,
        newest: false,
        heat: heat(&[(0, 9, 0), (5, 3, 4), (11, 0, 6)]),
    }
}

/// Three list rows whose counts cells are deliberately three different widths.
fn ragged_counts() -> View {
    let row = |path: &str, added: u32, removed: u32| Row::file(listed(path, added, removed));
    View {
        whole: Vec::new(),
        landed: false,
        recorded: 0,
        list_span: 3,
        grouped: false,
        list: vec![
            listed("src/engine/watch.rs", 139, 131).into(),
            listed("src/render/frame.rs", 42, 7).into(),
            listed("Cargo.toml", 2, 0).into(),
        ],
        list_top: 0,
        current_span: 400,
        total_rows: 400,
        rows_above: 0,
        rows: vec![row("src/engine/watch.rs", 139, 131)],
        files: 3,
        top: Position::default(),
        read: 1,
        scale: Scale::spread(12),
        gutter: None,
        worktree_churn: Default::default(),
        notes: Default::default(),
    }
}

/// The leftmost column of row `y` whose cell satisfies `is`.
fn column_where(
    backend: &TestBackend,
    y: u16,
    is: impl Fn(&str, Option<Color>) -> bool,
) -> Option<u16> {
    let buffer = backend.buffer();
    (0..buffer.area.width).find(|x| {
        let cell = &buffer[(*x, y)];
        is(cell.symbol(), cell.style().fg)
    })
}

/// The last column of the digit run that `sigil` opens on row `y`.
fn run_end(backend: &TestBackend, y: u16, sigil: &str) -> Option<u16> {
    let buffer = backend.buffer();
    let start = column_where(backend, y, |symbol, _| symbol == sigil)?;
    let mut end = start;
    for x in start + 1..buffer.area.width {
        if !buffer[(x, y)].symbol().chars().all(|c| c.is_ascii_digit()) {
            break;
        }
        end = x;
    }
    Some(end)
}

/// Every foreground the `sigil` half of row `y`'s counts cell is drawn in, sigil
/// and digits together. Empty where the row draws no such half.
fn half_ink(backend: &TestBackend, y: u16, sigil: &str) -> Vec<Option<Color>> {
    let (Some(start), Some(end)) = (
        column_where(backend, y, |symbol, _| symbol == sigil),
        run_end(backend, y, sigil),
    ) else {
        return Vec::new();
    };
    let buffer = backend.buffer();
    (start..=end).map(|x| buffer[(x, y)].style().fg).collect()
}

/// Which rows of `backend` between `rows` draw a counts cell at all.
fn counting_rows(backend: &TestBackend, rows: std::ops::Range<u16>) -> Vec<u16> {
    rows.filter(|&y| !half_ink(backend, y, "+").is_empty())
        .collect()
}

/// The file entries `view` streams below the pinned list, in draw order.
fn streamed_files(view: &View) -> impl Iterator<Item = &FileEntry> {
    view.rows.iter().filter_map(|row| match row {
        Row::File(entry) => Some(entry.as_ref()),
        _ => None,
    })
}

/// The fixture paths a counts scan would misread, which must be none.
fn guard_sigil_free_paths(view: &View) {
    for path in support::listed_files(view)
        .chain(streamed_files(view))
        .map(|entry| &entry.path)
    {
        assert!(
            !path.contains('+') && !path.contains('-'),
            "the fixture path {path:?} carries a counts sigil, so the scan would \
             read the path instead"
        );
    }
}

#[test]
fn the_counters_take_the_pictures_green_and_red() {
    // `SPEC.md` §5.1's third departure, colour half.
    let view = ragged_counts();
    let theme = Theme::default();
    guard_sigil_free_paths(&view);
    let backend = screen(80, 10, &view, &chrome());

    // Both regions, found rather than assumed, and counted separately: a gate
    // that totalled the counts cells it found would be satisfied by four list
    // rows and never reach the stream.
    let list_rows = counting_rows(&backend, LIST_TOP..LIST_TOP + 3);
    let stream_rows = counting_rows(&backend, LIST_TOP + 3..10);
    assert_eq!(
        list_rows.len(),
        3,
        "only {} of three list rows drew a counts cell, so this asserts less \
         than it reads",
        list_rows.len()
    );
    assert!(
        !stream_rows.is_empty(),
        "no row below the list drew a counts cell, so the diff heading is \
         unasserted and the ruling is gated in one region of two"
    );

    // Paired with what the fixture says each row's churn is, because the ruling is
    // value-dependent: a half takes a diff colour where it has something to say.
    let expected: Vec<(u16, (u32, u32))> = list_rows
        .iter()
        .copied()
        .zip(
            support::listed_files(&view)
                .map(|entry| entry.churn.expect("the fixture gives every list row churn")),
        )
        .chain(
            stream_rows
                .iter()
                .copied()
                .zip(streamed_files(&view).filter_map(|entry| entry.churn)),
        )
        .collect();

    for &(y, (added, removed)) in &expected {
        for (label, sigil, lines, ink) in [
            ("added", "+", added, theme.added.fg),
            ("removed", "-", removed, theme.removed.fg),
        ] {
            // A zero half is grey by a ruling of its own, and
            // `a_zero_counter_stays_grey_because_it_restates_no_change` owns it.
            if lines == 0 {
                continue;
            }
            let drawn = half_ink(&backend, y, sigil);
            assert!(
                !drawn.is_empty(),
                "row {y} says it {label} {lines} lines and drew no {sigil} half"
            );
            // The whole run against one colour repeated, rather than a walk:
            // uniformity is the claim, and a failure prints both runs with the
            // offending cell visible in them.
            assert_eq!(
                drawn,
                vec![ink; drawn.len()],
                "row {y}'s {label} half is not the picture's colour for it"
            );
        }
    }

    // Non-vacuity by *case* rather than by count: a fixture that lost every removal, or
    // every addition, would leave the loop above asserting nothing on that half while
    // the region checks stayed green.
    let greens = expected.iter().filter(|(_, (added, _))| *added > 0).count();
    let reds = expected
        .iter()
        .filter(|(_, (_, removed))| *removed > 0)
        .count();
    assert!(
        greens > 0 && reds > 0,
        "the sweep coloured {greens} added halves and {reds} removed halves, so \
         one of the two is unasserted"
    );

    // Every direction, or a screen painted one colour throughout satisfies
    // everything above. `Theme::default` is the ANSI palette, which is the
    // configuration a colour gate is likeliest to be vacuous under.
    for (what, one, other) in [
        (
            "added and the grey it replaces",
            theme.added.fg,
            theme.chrome_dim.fg,
        ),
        (
            "removed and the grey it replaces",
            theme.removed.fg,
            theme.chrome_dim.fg,
        ),
        ("added and removed", theme.added.fg, theme.removed.fg),
    ] {
        assert_ne!(
            one, other,
            "the theme draws {what} alike, so this test cannot tell them apart"
        );
    }
}

#[test]
fn a_zero_counter_stays_grey_because_it_restates_no_change() {
    // `assets/preview.svg` draws `Cargo.toml`'s `-0` in `.faint` where the two
    // rows above it draw `.red`, and §5.3 says why: green and red are loaned
    // "only where they restate the same fact", and a `-0` restates none.
    let view = ragged_counts();
    let theme = Theme::default();
    guard_sigil_free_paths(&view);

    // Guard the fixture: without a zero in it this is a test about a row that
    // removes something, and it would pass against a renderer with no rule at
    // all. Read off the view rather than off the screen, for the same reason.
    let zeroed: Vec<u16> = support::listed_files(&view)
        .enumerate()
        .filter(|(_, entry)| entry.churn.is_some_and(|(_, removed)| removed == 0))
        .map(|(index, _)| index as u16 + LIST_TOP)
        .collect();
    // Bound in the pattern rather than asserted and then iterated, so "there is
    // exactly one" is what the code says and not something it checks and then
    // loops over anyway.
    let [y] = zeroed[..] else {
        panic!(
            "the fixture has {} rows that remove nothing rather than the one it \
             is built around, so this gate is over a different screen",
            zeroed.len()
        )
    };

    let backend = screen(80, 10, &view, &chrome());
    let grey = half_ink(&backend, y, "-");
    assert!(
        !grey.is_empty(),
        "row {y} removes nothing and drew no removed half at all, so the pair \
         stopped standing or falling together"
    );
    assert_eq!(
        grey,
        vec![theme.chrome_dim.fg; grey.len()],
        "row {y}'s `-0` took a colour, and a zero restates no change"
    );

    // The same row's `+2` is still green, which is what makes the rule per half
    // rather than per cell: a renderer that greyed the whole cell whenever
    // either half was zero would satisfy the assertion above.
    let green = half_ink(&backend, y, "+");
    assert!(!green.is_empty(), "row {y} drew no added half");
    assert_eq!(
        green,
        vec![theme.added.fg; green.len()],
        "row {y}'s added half lost its colour because the half beside it is zero"
    );

    assert_ne!(
        theme.chrome_dim.fg, theme.removed.fg,
        "the theme draws the grey and the removed colour alike, so this test \
         cannot tell them apart"
    );
}

#[test]
fn the_glance_columns_agree_down_the_list() {
    // `assets/preview.svg` puts every glance element at the same x on every file row,
    // which is what makes three sparklines read as one small-multiples chart and three
    // heat strips as a comparison.
    let view = ragged_counts();

    // Guard the fixture first. If every row's counts were the same width, right
    // packing and columns would draw identically and this test would pass
    // against the defect it exists to catch.
    let widths: Vec<usize> = support::listed_files(&view)
        .map(|e| {
            let (a, r) = e.churn.expect("the fixture gives every row churn");
            format!("+{a} -{r}").chars().count()
        })
        .collect();
    assert!(
        widths
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len()
            > 1,
        "every row's counts cell is the same width ({widths:?}), so this fixture \
         cannot tell columns from right-packing"
    );

    let theme = Theme::default();
    let backend = screen(80, 10, &view, &chrome());

    // Each element by its own colour, and the counts cell by the `+` it opens with,
    // which nothing else on a list row draws.
    let spark = spark_colours(&theme);
    let heats_fg = heat_colours(&theme);

    let sparks: Vec<(u16, u16)> = (LIST_TOP..LIST_TOP + 3)
        .filter_map(|y| {
            column_where(&backend, y, |sym, fg| {
                spark.contains(&fg) && RAMP.contains(&sym)
            })
            .map(|x| (y, x))
        })
        .collect();
    let heats: Vec<(u16, u16)> = (LIST_TOP..LIST_TOP + 3)
        .filter_map(|y| {
            column_where(&backend, y, |sym, fg| {
                sym == HEAT_SLICE && heats_fg.contains(&fg)
            })
            .map(|x| (y, x))
        })
        .collect();
    // Both halves, by where each ends. The fixture's paths carry neither sigil,
    // so a run found here came from the counts cell.
    guard_sigil_free_paths(&view);
    let added: Vec<(u16, u16)> = (LIST_TOP..LIST_TOP + 3)
        .filter_map(|y| run_end(&backend, y, "+").map(|x| (y, x)))
        .collect();
    let removed: Vec<(u16, u16)> = (LIST_TOP..LIST_TOP + 3)
        .filter_map(|y| run_end(&backend, y, "-").map(|x| (y, x)))
        .collect();

    for (label, found) in [
        ("sparkline", sparks),
        ("heat", heats),
        ("added count", added),
        ("removed count", removed),
    ] {
        assert_eq!(
            found.len(),
            3,
            "{label}: only {} of three list rows drew it, so the column \
             comparison below proves nothing",
            found.len()
        );
        let (first_row, first) = found[0];
        for (y, x) in &found {
            assert_eq!(
                *x, first,
                "{label} starts at column {x} on row {y} and column {first} on \
                 row {first_row}, so the rows do not line up"
            );
        }
    }
}

/// Where each glance element sits on every list row, as one comparable value.
fn glance_columns(backend: &TestBackend) -> Vec<String> {
    let theme = Theme::default();
    let heats = heat_colours(&theme);
    let spark = spark_colours(&theme);
    let buffer = backend.buffer();

    (LIST_TOP..LIST_TOP + 3)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| {
                    let cell = &buffer[(x, y)];
                    let (sym, fg) = (cell.symbol(), cell.style().fg);
                    if sym == HEAT_SLICE && heats.contains(&fg) {
                        'h'
                    } else if RAMP.contains(&sym) && spark.contains(&fg) {
                        's'
                    } else if sym == SPARK_TRACK && fg == theme.spark_track.fg {
                        // Its own class rather than folded into `s`.
                        't'
                    } else if sym.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                        'n'
                    } else {
                        '_'
                    }
                })
                .collect()
        })
        .collect()
}

#[test]
fn a_row_missing_a_glance_element_keeps_its_column() {
    // The launch case, and the reason the fixed slots are universal rather than
    // occasional.
    for path in support::listed_files(&ragged_counts()).map(|entry| entry.path.clone()) {
        assert!(
            !path.contains(|c: char| c.is_ascii_digit()),
            "the fixture path {path:?} carries a digit, which `glance_columns` \
             reads as a counts cell"
        );
    }

    let full = ragged_counts();
    let mut gapped = ragged_counts();
    listed_mut(&mut gapped, 1).spark = [0; HISTORY_BUCKETS];
    listed_mut(&mut gapped, 2).heat = [HeatBucket::default(); HEAT_BUCKETS];

    let before = glance_columns(&screen(80, 10, &full, &chrome()));
    let after = glance_columns(&screen(80, 10, &gapped, &chrome()));

    assert_eq!(
        before[0], after[0],
        "the first row moved when a *different* row lost its sparkline"
    );

    // The load-bearing half, and it has to be read across rows of the *same* screen.
    let columns_of = |row: &str, class: char| -> Vec<usize> {
        row.char_indices()
            .filter(|(_, c)| *c == class)
            .map(|(i, _)| i)
            .collect()
    };
    // The strip and the sparkline only.
    assert_eq!(
        columns_of(&after[1], 'h'),
        columns_of(&after[0], 'h'),
        "row 1 lost its sparkline buckets and its heat strip moved with them, so \
         the slot was closed rather than kept: {:?} against row 0 {:?}",
        after[1],
        after[0]
    );

    // Non-vacuity: the gapped rows really did stop drawing those elements, or
    // the comparison above is between two identical screens.
    assert!(
        before[1].contains('s') && !after[1].contains('s'),
        "row 1 was supposed to lose its sparkline buckets: {:?} then {:?}",
        before[1],
        after[1]
    );
    // Sorted, or this asserts an ordering rather than a set.
    let mut was_slot: Vec<usize> = columns_of(&before[1], 't')
        .into_iter()
        .chain(columns_of(&before[1], 's'))
        .collect();
    was_slot.sort_unstable();
    assert_eq!(
        columns_of(&after[1], 't'),
        was_slot,
        "row 1's buckets did not become track in the columns they occupied: \
         {:?} then {:?}",
        before[1],
        after[1]
    );
    assert!(
        before[2].contains('h') && !after[2].contains('h'),
        "row 2 was supposed to lose its heat strip: {:?} then {:?}",
        before[2],
        after[2]
    );
}

#[test]
fn scrolling_the_list_does_not_move_the_columns() {
    // Reported from use, and the reason the columns are a property of the pane rather
    // than of the rows.
    let mut view = ragged_counts();
    view.list = vec![
        listed("src/small.rs", 1, 1).into(),
        listed("src/also-small.rs", 2, 0).into(),
        listed("src/huge.rs", 1500, 1500).into(),
    ];
    view.files = 3;

    let narrow = View {
        list_span: 0,
        grouped: false,
        list: view.list[..2].to_vec(),
        ..view.clone()
    };
    let wide = View {
        list_span: 0,
        grouped: false,
        list: view.list[1..].to_vec(),
        list_top: 1,
        ..view.clone()
    };

    // Non-vacuity: the two windows really do disagree about how wide a raw count
    // is, or this compares two identical screens.
    assert_ne!(
        support::listed_files(&narrow).filter_map(|e| e.churn).max(),
        support::listed_files(&wide).filter_map(|e| e.churn).max(),
        "both windows hold the same widest count, so neither layout is under \
         pressure and this proves nothing"
    );

    let before = glance_columns(&screen(80, 10, &narrow, &chrome()));
    let after = glance_columns(&screen(80, 10, &wide, &chrome()));

    // Row 1 of each window is a different *file*; what must match is where its
    // elements sit, which is what `glance_columns` reports.
    for (row, (a, b)) in before.iter().zip(after.iter()).enumerate() {
        let (a, b): (String, String) = (
            a.chars().map(|c| if c == 'n' { '_' } else { c }).collect(),
            b.chars().map(|c| if c == 'n' { '_' } else { c }).collect(),
        );
        assert_eq!(
            a, b,
            "row {row}'s glance columns moved when the list scrolled a wider \
             count into the window"
        );
    }
}

#[test]
fn a_changed_file_appearing_does_not_move_the_glance_columns() {
    // The scrollbar is the other way the contents reached the layout, and it survived
    // the fix above because it does not look like contents.
    let entries: Vec<_> = (0..8)
        .map(|n| listed(&format!("src/file{n}.rs"), 42, 7))
        .collect();
    let view_of = |files: usize| View {
        list_span: 0,
        grouped: false,
        list: entries[..files.min(entries.len())]
            .iter()
            .cloned()
            .map(ListRow::from)
            .collect(),
        rows: entries[..files.min(entries.len())]
            .iter()
            .cloned()
            .map(Row::file)
            .collect(),
        files,
        total_rows: files,
        ..ragged_counts()
    };

    // Few enough to need no bar, and enough to force one, at a height that shows
    // the list region.
    let (few, many) = (view_of(2), view_of(8));
    // Non-vacuity, and it has to be counted rather than flagged: a `differed` bool set
    // beside the assertion below can never be read, because the assertion panics first.
    let mut drew = 0usize;
    for width in 16..=120u16 {
        let quiet = glance_columns(&screen(width, 12, &few, &chrome()));
        let busy = glance_columns(&screen(width, 12, &many, &chrome()));
        // Row 0 of each is the first file row of the pinned list, and it is the
        // same file in both fixtures.
        assert_eq!(
            quiet[0], busy[0],
            "at {width} columns the list's glance columns moved because the \
             changed-file count crossed the point where a scrollbar appears"
        );
        if quiet[0].contains('h') || quiet[0].contains('s') || quiet[0].contains('n') {
            drew += 1;
        }
    }
    assert!(
        drew > 40,
        "only {drew} widths drew a glance element at all, so this swept over \
         blank rows"
    );

    // The stream, whose bar answers to the diff's height rather than the file
    // count. A separate fixture pair, because nothing about the list can make
    // the stream's bar appear.
    let short = View {
        total_rows: 2,
        rows_above: 0,
        ..view_of(2)
    };
    let tall = View {
        total_rows: 4_000,
        rows_above: 0,
        ..view_of(2)
    };
    // Compared with the bar's own column stripped, since that column is what differs by
    // construction; what must not differ is everything left of it.
    let strip = |row: String| {
        row.trim_end_matches(BAR_GLYPHS.as_slice())
            .trim_end()
            .to_owned()
    };
    for width in 16..=120u16 {
        let flat = screen(width, 12, &short, &chrome());
        let deep = screen(width, 12, &tall, &chrome());
        // The first stream heading sits below the list and its rule.
        for y in 4..8u16 {
            assert_eq!(
                strip(row_text(&flat, y)),
                strip(row_text(&deep, y)),
                "at {width} columns row {y} of the stream moved because the diff \
                 grew taller than the pane"
            );
        }
    }
}

#[test]
fn the_mark_follows_the_newest_write_and_not_the_rows_brightness() {
    // The wiring gate the pulse's separation cannot be made without.
    let mut settled = ragged_counts();
    listed_mut(&mut settled, 0).recency = Recency::Live;
    listed_mut(&mut settled, 0).newest = true;
    let row = row_text(&screen(80, 10, &settled, &chrome()), LIST_TOP);
    assert!(
        row.contains('●'),
        "the last written file lost its mark once its ink had drained, which is \
         the report this row was opened on: {row:?}"
    );

    let mut bright = ragged_counts();
    listed_mut(&mut bright, 0).recency = Recency::Pulse;
    listed_mut(&mut bright, 0).newest = false;
    let row = row_text(&screen(80, 10, &bright, &chrome()), LIST_TOP);
    assert!(
        !row.contains('●'),
        "a row that is not the newest write carries the mark anyway, so the \
         painter is still reading it out of the brightness: {row:?}"
    );
}

#[test]
fn a_pulse_does_not_move_the_columns() {
    // The pulse has a reserved slot, and that is the mechanism being asserted.
    let quiet = ragged_counts();
    let mut pulsing = ragged_counts();
    // The mark is its own field, because the ink and the dot answer different
    // questions.
    listed_mut(&mut pulsing, 0).recency = Recency::Pulse;
    listed_mut(&mut pulsing, 0).newest = true;

    let before = glance_columns(&screen(80, 10, &quiet, &chrome()));
    let drawn = screen(80, 10, &pulsing, &chrome());
    let after = glance_columns(&drawn);

    for (row, (a, b)) in before.iter().zip(after.iter()).enumerate() {
        assert_eq!(
            a, b,
            "row {row}'s columns moved when a file started pulsing"
        );
    }

    // Non-vacuity: the pulse really is drawn, so this is not comparing a screen
    // against itself. Read off the backend already rendered above rather than
    // drawing the same screen a second time.
    let row = row_text(&drawn, LIST_TOP);
    assert!(
        row.contains('●'),
        "no pulse reached the row, so nothing was asserted: {row:?}"
    );
}

#[test]
fn the_headers_two_tree_facts_are_drawn_in_one_weight() {
    // `SPEC.md` §11.1's other half of the header split.
    let view = View {
        files: 3,
        ..one_file()
    };
    let theme = Theme::default();
    let clause = format!("{}{FACT_JOIN}3 changed", chrome().worktree);

    // Non-vacuity first. If the two chrome styles were equal, every assertion
    // below would hold against a renderer that drew the clause in either, and
    // the test would be checking that a thing equals itself.
    assert_ne!(
        theme.chrome.fg, theme.chrome_dim.fg,
        "the theme draws chrome and chrome_dim alike, so this test cannot tell \
         which weight the clause got"
    );

    // Both modes and three widths, because reading one screen is how a style gate
    // passes while the case a reader actually hits is unasserted.
    let mut saw_the_clause = 0usize;
    let mut saw_a_mark = 0usize;
    for (label, mode) in [("live", Mode::Watching), ("lost", Mode::Lost)] {
        // Seventeen is the clause's own width, so it is where the left is cut and the
        // continuation mark inherits a style.
        for width in [13u16, 17, 40, 80, 120] {
            let chrome = Chrome { mode, ..chrome() };
            let backend = screen(width, 8, &view, &chrome);
            let header = row_text(&backend, 0);

            // However much of the clause reached the screen.
            let header = content(&header, width);
            let matched: String = header
                .chars()
                .zip(clause.chars())
                .take_while(|(row, want)| row == want)
                .map(|(row, _)| row)
                .collect();
            let drawn = matched.trim_end();
            if drawn.is_empty() {
                continue;
            }
            saw_the_clause += 1;

            // All ASCII, so one char is one column.
            let cut = header[drawn.len()..].starts_with(CONTINUES);
            if cut {
                saw_a_mark += 1;
            }
            let cells = drawn.chars().count() + usize::from(cut);
            for x in 0..cells {
                let cell = &backend.buffer()[(x as u16 + inset_at(width), 0)];
                assert_eq!(
                    cell.style().fg,
                    theme.chrome.fg,
                    "{label} at {width} columns: column {x} of the left clause \
                     ({:?}) is not the worktree name's weight, so the header \
                     draws two weights inside one clause",
                    cell.symbol()
                );
            }
        }
    }
    // Eight of the ten screens, because a lost watch at thirteen columns leaves
    // the left no room at all: `not watching` plus its gap is the whole row.
    assert!(
        saw_the_clause >= 8,
        "only {saw_the_clause} of the ten screens drew part of the clause, so \
         this gate is weaker than it reads"
    );
    assert!(
        saw_a_mark > 0,
        "no width cut the clause, so the continuation mark's weight is unasserted"
    );

    // And the test can tell the two apart: the blank after the clause is the background
    // style, which is the one a dimmed count would have taken.
    let backend = screen(80, 8, &view, &chrome());
    let gap = &backend.buffer()[(clause.chars().count() as u16 + 1 + inset_at(80), 0)];
    assert_eq!(
        gap.style().fg,
        theme.chrome_dim.fg,
        "the column after the clause is not the chrome background, so this test \
         cannot distinguish the two weights it is asserting between"
    );
}

#[test]
fn a_nameless_worktree_draws_no_separator_with_nothing_on_its_left() {
    // The inversion of the header split, which the count's move makes reachable.
    let view = View {
        files: 3,
        ..one_file()
    };
    // Empty is the easy half and not the reachable one.
    let names = [
        ("empty", ""),
        ("zero-width space", "\u{200B}"),
        ("zero-width joiner", "\u{200D}"),
        ("right-to-left mark", "\u{200F}"),
        ("combining acute", "\u{0301}"),
        ("variation selector", "\u{FE0F}"),
        ("one space", " "),
        ("no-break space", "\u{00A0}"),
        ("ideographic space", "\u{3000}"),
        ("tab", "\t"),
        ("space then zero-width", " \u{200B}"),
        // The fourth class, and the one no width measurement can catch: these
        // report a column each and `trim` keeps them, but `ratatui` drops any
        // grapheme containing a control before it reaches a cell, so the name
        // measures nonzero and draws nothing.
        ("escape", "\u{001B}"),
        ("bell", "\u{0007}"),
    ];

    for (label, name) in names {
        let nameless = Chrome {
            pressed: None,
            gripped: None,
            scrolling: None,
            worktree: name.to_owned(),
            ..chrome()
        };

        // Every width, and the separator looked for anywhere on the row.
        let mut saw_the_count = 0usize;
        for width in 1..=120u16 {
            let header = row_text(&screen(width, 8, &view, &nameless), 0);
            assert!(
                !header.contains(FACT_JOIN.trim()),
                "at {width} columns a {label} worktree name put a separator on \
                 the header with nothing for it to join: {header:?}"
            );
            if header.contains("3 changed") {
                saw_the_count += 1;
            }
        }
        // And the count still reaches the screen, so the fix is a guard on the
        // separator rather than on the fact. Dropping the count would satisfy
        // every assertion above by saying less.
        assert!(
            saw_the_count > 90,
            "a {label} worktree name drew the count at only {saw_the_count} of \
             120 widths, so the guard is dropping the fact rather than the \
             separator"
        );
    }
}

#[test]
fn a_lost_watch_is_loud_and_a_live_one_is_quiet() {
    // A state nobody can see at a glance has not been reported.
    let view = one_file();
    let theme = Theme::default();

    // Guard the fixture, the way `the_header_carries_no_changed_line_total` does.
    let left = format!("{}{FACT_JOIN}1 changed", chrome().worktree);
    assert!(
        !left.contains('w'),
        "the fixture's left-hand header {left:?} contains a `w`, so `column_of` \
         below would read it instead of the mode word"
    );

    let style_of = |chrome: &Chrome| {
        let backend = screen(80, 6, &view, chrome);
        let x = column_of(&backend, 0, "w");
        backend.buffer()[(x, 0)].style()
    };

    let live = style_of(&chrome());
    let lost = style_of(&Chrome {
        pressed: None,
        gripped: None,
        scrolling: None,
        mode: Mode::Lost,
        ..chrome()
    });

    assert_eq!(live.fg, theme.chrome_dim.fg, "a live watch shouted");
    assert_eq!(lost.fg, theme.alert.fg, "a lost watch was drawn quietly");
    assert_ne!(
        live.fg, lost.fg,
        "the two modes are the same colour, so the header says nothing a glance \
         can catch"
    );
}

#[test]
fn a_lost_watch_reaches_the_header_and_not_only_the_footer() {
    // One event, two halves, and they are not the same half twice. The header
    // carries what is durable, which is that the diff has stopped being live.
    // The notice carries which failure did it, which is not durable at all.
    let view = one_file();
    let stopped = Chrome {
        pressed: None,
        gripped: None,
        scrolling: None,
        mode: Mode::Lost,
        notice: Some("the watch ended; this diff is no longer live".to_owned()),
        ..chrome()
    };
    let backend = screen(80, 6, &view, &stopped);

    let header = row_text(&backend, 0);
    let footer = row_text(&backend, 5);
    assert!(header.contains("not watching"), "header: {header:?}");
    assert!(footer.contains("the watch ended"), "footer: {footer:?}");
}

#[test]
fn the_empty_state_says_what_it_did_not_find_and_leaves_the_branch_to_the_header() {
    // B3's four facts, three of which are the header's, so the body spends
    // one row on the one fact that is its own.
    let backend = screen(80, 6, &nothing_changed(), &empty_chrome());
    assert_eq!(
        content(row_text(&backend, 1).trim_end(), 80),
        "no unstaged changes"
    );
    // And the branch is on the header rather than gone from the screen, which is
    // the half that makes the move a move rather than a deletion.
    assert!(
        row_text(&backend, 0).contains("main"),
        "the branch left the body and did not arrive in the header: {:?}",
        row_text(&backend, 0)
    );
}

#[test]
fn a_detached_head_names_no_branch_anywhere() {
    // Ordinary rather than exceptional: a rebase or a bisect leaves an agent
    // here routinely. Nothing invents one, because `HEAD@abc123` would put a
    // commit id in a monitor that shows no commits.
    let backend = screen(80, 6, &nothing_changed(), &chrome());
    assert_eq!(
        content(row_text(&backend, 1).trim_end(), 80),
        "no unstaged changes"
    );
    assert!(
        !row_text(&backend, 0).contains(FACT_JOIN),
        "a detached head drew a second header fact, so a branch was invented: \
         {:?}",
        row_text(&backend, 0)
    );
}

#[test]
fn a_file_with_no_line_diff_says_why() {
    let view = View {
        whole: Vec::new(),
        landed: false,
        recorded: 0,
        list_span: 3,
        grouped: false,
        list: Vec::new(),
        list_top: 0,
        current_span: 0,
        total_rows: 0,
        rows_above: 0,
        rows: vec![
            Row::file(FileEntry {
                origin: Origin::Unstaged,
                path: "assets/banner.jpg".to_owned(),
                from: None,
                kind: 'M',
                churn: None,
                spark: [0; HISTORY_BUCKETS],
                recency: Recency::Cold,
                newest: false,
                heat: [HeatBucket::default(); HEAT_BUCKETS],
            }),
            Row::Reason("binary".to_owned()),
            Row::file(FileEntry {
                origin: Origin::Unstaged,
                path: "src/merge.rs".to_owned(),
                from: None,
                kind: 'U',
                churn: None,
                spark: [0; HISTORY_BUCKETS],
                recency: Recency::Cold,
                newest: false,
                heat: [HeatBucket::default(); HEAT_BUCKETS],
            }),
            Row::Reason("unresolved conflict".to_owned()),
            Row::file(FileEntry {
                origin: Origin::Unstaged,
                path: "crates/vigia/src/shell.rs".to_owned(),
                from: Some("crates/vigia/src/main.rs".to_owned()),
                kind: 'R',
                churn: Some((0, 0)),
                spark: [0; HISTORY_BUCKETS],
                recency: Recency::Cold,
                newest: false,
                heat: [HeatBucket::default(); HEAT_BUCKETS],
            }),
        ],
        files: 3,
        top: Position::default(),
        read: 3,
        scale: Scale::flat(0),
        gutter: None,
        worktree_churn: Default::default(),
        notes: Default::default(),
    };
    insta::assert_snapshot!(screen(60, 8, &view, &chrome()));
}

#[test]
fn a_path_too_long_to_fit_keeps_the_end_that_names_the_file() {
    // Losing the tail would leave a column of `crates/vigia-core/…`, which names
    // nothing. This is the truncated-to-useless shape I6 forbids, and it is the
    // one part of I6 the renderer decides on its own rather than by layout.
    let view = View {
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
        rows: vec![file(
            "crates/vigia-core/src/very/deeply/nested/module/frame.rs",
            12,
            3,
        )],
        files: 1,
        top: Position::default(),
        read: 1,
        scale: Scale::flat(0),
        gutter: None,
        worktree_churn: Default::default(),
        notes: Default::default(),
    };
    insta::assert_snapshot!(screen(40, 4, &view, &chrome()));
}

#[test]
fn a_hunk_covering_one_line_is_written_git_s_way() {
    // Git omits the count when a side covers exactly one line, and a reader calibrated
    // on `git diff` reads its absence as "one".
    let view = View {
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
            file("VERSION", 1, 1),
            Row::Hunk {
                old_start: 1,
                old_lines: 1,
                new_start: 1,
                new_lines: 1,
            },
            line(LineKind::Removed, 1, "0.0.0"),
            line(LineKind::Added, 1, "0.1.0"),
        ],
        files: 1,
        top: Position::default(),
        read: 1,
        scale: Scale::flat(0),
        gutter: None,
        worktree_churn: Default::default(),
        notes: Default::default(),
    };
    let rendered = format!("{}", screen(40, 6, &view, &chrome()));
    assert!(
        rendered.contains("@@ -1 +1 @@"),
        "a one-line hunk is not written the way git writes it:\n{rendered}"
    );
}

#[test]
fn a_notice_takes_the_footer_from_the_key_hints() {
    let view = one_file();
    let chrome = Chrome {
        pressed: None,
        gripped: None,
        scrolling: None,
        notice: Some("the index entry for src/lib.rs points at a missing blob".to_owned()),
        ..chrome()
    };
    insta::assert_snapshot!(screen(80, 6, &view, &chrome));
}

#[test]
fn the_footer_shows_that_follow_is_engaged() {
    // I5 is otherwise invisible.
    let view = one_file();
    insta::assert_snapshot!(screen(80, 6, &view, &following_chrome()));
}

#[test]
fn a_notice_keeps_the_follow_marker_because_state_is_not_a_hint() {
    // A notice replaces the *hints*, which is advice the reader can spare.
    // Whether what they are looking at is still live is not advice, and it is
    // most worth knowing precisely when something has just gone wrong.
    let view = one_file();
    let chrome = Chrome {
        pressed: None,
        gripped: None,
        scrolling: None,
        notice: Some("the index entry for src/lib.rs points at a missing blob".to_owned()),
        ..following_chrome()
    };
    insta::assert_snapshot!(screen(80, 6, &view, &chrome));
}

#[test]
fn the_status_bar_carries_what_a_frame_cost() {
    // `SPEC.md` §5.1's two readouts, in the picture that shows where they sit
    // relative to everything else on the row. The legibility sweep can prove the
    // layout is legal at every width; only this shows it is good at one.
    let view = one_file();
    insta::assert_snapshot!(screen(80, 6, &view, &diagnostics_chrome()));
}

/// Durations that sit either side of every boundary [`frame_cell`]'s three
/// branches have, plus the two ends of the range.
const FRAME_TIMES: [Duration; 10] = [
    Duration::ZERO,
    Duration::from_micros(1),
    Duration::from_micros(9_949),
    Duration::from_micros(9_950),
    Duration::from_millis(12),
    Duration::from_micros(999_499),
    Duration::from_micros(999_500),
    Duration::from_secs(1),
    Duration::from_secs(3600),
    Duration::MAX,
];

/// Byte counts either side of the memory cell's one boundary, plus both ends.
const MEMORY_SIZES: [u64; 6] = [
    0,
    1024 * 1024,
    19 * 1024 * 1024,
    999 * 1024 * 1024,
    1000 * 1024 * 1024,
    u64::MAX,
];

#[test]
fn the_readouts_ride_the_second_footer_line_at_forty_columns() {
    // The other half of the picture above, and the one that matters for I6.
    let view = one_file();
    insta::assert_snapshot!(screen(40, 6, &view, &diagnostics_chrome()));
}

/// Where `follow ▶` starts on the footer, for each of `chromes`.
fn follow_marker_columns(chromes: impl IntoIterator<Item = Chrome>) -> Vec<u16> {
    let view = one_file();
    chromes
        .into_iter()
        .map(|chrome| column_of(&screen(80, 6, &view, &chrome), 5, "▶"))
        .collect()
}

#[test]
fn the_frame_cell_never_shifts_what_is_beside_it() {
    // The one property that makes a per-frame readout safe to draw.
    let columns = follow_marker_columns(FRAME_TIMES.map(|cost| Chrome {
        pressed: None,
        gripped: None,
        scrolling: None,
        frame: Some(cost),
        ..diagnostics_chrome()
    }));

    assert!(
        columns.windows(2).all(|pair| pair[0] == pair[1]),
        "the follow marker moved as the frame time changed: {columns:?} for {FRAME_TIMES:?}"
    );
}

#[test]
fn the_memory_cell_never_shifts_what_is_beside_it() {
    // [`the_frame_cell_never_shifts_what_is_beside_it`]'s property, one cell over.
    let columns = follow_marker_columns(MEMORY_SIZES.map(|bytes| Chrome {
        pressed: None,
        gripped: None,
        scrolling: None,
        memory: Some(bytes),
        notes: (0, 0),
        ..diagnostics_chrome()
    }));

    assert!(
        columns.windows(2).all(|pair| pair[0] == pair[1]),
        "the follow marker moved as memory changed: {columns:?} for {MEMORY_SIZES:?}"
    );
}

#[test]
fn the_memory_readout_is_drawn_wherever_the_read_is_a_syscall() {
    // A content gate, and it is the kind `reads.rs` structurally cannot be.
    let view = one_file();
    let footer = |chrome: &Chrome| {
        let backend = screen(80, 6, &view, chrome);
        (0..80)
            .map(|x| backend.buffer()[(x, 5)].symbol().to_owned())
            .collect::<String>()
    };

    assert!(
        footer(&diagnostics_chrome()).contains("19MiB"),
        "a chrome carrying a memory reading drew none of it: {:?}",
        footer(&diagnostics_chrome())
    );

    let unavailable = Chrome {
        pressed: None,
        gripped: None,
        scrolling: None,
        memory: None,
        notes: (0, 0),
        ..diagnostics_chrome()
    };
    assert!(
        !footer(&unavailable).contains("MiB"),
        "a platform with no memory reading drew one anyway: {:?}",
        footer(&unavailable)
    );
}

#[test]
fn the_first_paint_draws_no_readouts_at_all() {
    // The state a reader actually starts in, and it is not reachable by narrowing: no
    // frame has completed, so there is no p99 of anything, and `App::chrome` reports
    // `None`.
    let view = one_file();
    let first = Chrome {
        pressed: None,
        gripped: None,
        scrolling: None,
        frame: None,
        memory: Some(19 * 1024 * 1024),
        notes: (0, 0),
        ..following_chrome()
    };
    let backend = screen(80, 6, &view, &first);
    let footer: String = (0..80)
        .map(|x| backend.buffer()[(x, 5)].symbol().to_owned())
        .collect();

    assert!(
        !footer.contains("MiB") && !footer.contains("frame"),
        "the first paint drew a readout it has no value for: {footer:?}"
    );
}

#[test]
fn the_footer_takes_two_lines_when_forty_columns_cannot_hold_it() {
    // The hardest screen the footer has to lay out, and the default one: at forty
    // columns with follow engaged, the hints and the state cannot share a line.
    let view = one_file();
    insta::assert_snapshot!(screen(40, 6, &view, &following_chrome()));
}

#[test]
fn tabs_become_columns_and_control_characters_become_visible() {
    // Not cosmetic.
    let view = View {
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
            file("Makefile", 1, 0),
            line(LineKind::Added, 1, "\tcargo build\ta\tb"),
            line(LineKind::Context, 2, "bell\u{7}esc\u{1b}[31mnul\u{0}"),
        ],
        files: 1,
        top: Position::default(),
        read: 1,
        scale: Scale::flat(0),
        gutter: None,
        worktree_churn: Default::default(),
        notes: Default::default(),
    };
    let backend = screen(60, 5, &view, &chrome());
    let rendered = format!("{backend}");
    assert!(
        !rendered.contains('\t') && !rendered.contains('\u{1b}') && !rendered.contains('\u{7}'),
        "a control character reached the buffer:\n{rendered}"
    );
    insta::assert_snapshot!(backend);
}

#[test]
fn a_double_width_character_is_never_cut_in_half() {
    // Diffs carry whatever is in the files, and a CJK ideograph or an emoji occupies
    // two columns.
    let view = View {
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
            file("docs/読み方.md", 2, 0),
            line(LineKind::Added, 1, "見出し a 見出し b 見出し c"),
            line(LineKind::Added, 2, "🙂🙂🙂 tail"),
        ],
        files: 1,
        top: Position::default(),
        read: 1,
        scale: Scale::flat(0),
        gutter: None,
        worktree_churn: Default::default(),
        notes: Default::default(),
    };

    for width in 6..48u16 {
        let backend = screen(width, 5, &view, &chrome());
        let buffer = backend.buffer();
        for y in 0..5 {
            // A two-column symbol lives in one cell and the cell after it is left as a
            // blank placeholder.
            let mut occupied = 0usize;
            let mut covered = 0usize;
            for x in 0..width {
                let cell = ratatui::text::Span::raw(buffer[(x, y)].symbol()).width();
                if covered > 0 {
                    covered -= 1;
                    continue;
                }
                occupied += cell;
                covered = cell.saturating_sub(1);
            }
            assert!(
                occupied <= usize::from(width),
                "row {y} at width {width} occupies {occupied} columns, so a \
                 double-width character was split or overflowed"
            );
        }
    }

    insta::assert_snapshot!(screen(40, 5, &view, &chrome()));
}

#[test]
fn the_gutter_gives_way_before_the_text_does() {
    // The rule is that line numbers go when they would leave the content less
    // than a readable column. Both sides are asserted, because a rule that only
    // ever fires one way is not a rule.
    let view = View {
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
        rows: vec![line(LineKind::Added, 1234, "let value = compute(input);")],
        files: 1,
        top: Position::default(),
        read: 1,
        scale: Scale::flat(0),
        gutter: None,
        worktree_churn: Default::default(),
        notes: Default::default(),
    };

    let wide = format!("{}", screen(40, 3, &view, &chrome()));
    assert!(
        wide.contains("1234"),
        "forty columns dropped the gutter, which is the width I6 is about:\n{wide}"
    );

    let narrow = format!("{}", screen(24, 3, &view, &chrome()));
    assert!(
        !narrow.contains("1234"),
        "the gutter survived into a width where it costs more than it explains:\n{narrow}"
    );
    assert!(
        narrow.contains("let value"),
        "the content lost its start, which is the wrong thing to spend:\n{narrow}"
    );
}

#[test]
fn any_area_renders_including_the_ones_that_fit_nothing() {
    // A pane being dragged narrow steps through every one of these sizes.
    let view = one_file();
    for (width, height) in [(0, 0), (1, 1), (1, 2), (80, 1), (80, 2), (80, 3), (2, 30)] {
        let backend = screen(width, height, &view, &chrome());
        let area = ratatui::layout::Rect::new(0, 0, width, height);
        assert!(
            diff_height(area, &chrome(), view.files, view.files) < usize::from(height).max(1),
            "diff_height asked for more rows than {width}x{height} has"
        );
        // Non-vacuity: the loop must actually have produced a buffer of the size
        // asked for, or it proved only that nothing was drawn.
        assert_eq!(backend.buffer().area.width, width);
        assert_eq!(backend.buffer().area.height, height);
    }
}

#[test]
fn hostile_content_never_panics_at_any_pane_size() {
    // The sweep above drags one benign fixture through seven sizes, which is a
    // real gate and structurally cannot find an arithmetic fault: nothing in
    // `one_file` is big enough to overflow anything.
    let saturated = FileEntry {
        origin: Origin::Unstaged,
        path: "src/generated.rs".to_owned(),
        from: None,
        kind: 'M',
        churn: Some((u32::MAX, u32::MAX)),
        spark: [u32::MAX; HISTORY_BUCKETS],
        recency: Recency::Pulse,
        newest: true,
        heat: [HeatBucket {
            added: u16::MAX,
            removed: u16::MAX,
        }; HEAT_BUCKETS],
    };
    let view = View {
        whole: Vec::new(),
        landed: false,
        recorded: 0,
        list_span: 2,
        grouped: false,
        list: vec![saturated.clone().into(), listed("a.rs", 0, 0).into()],
        list_top: 0,
        current_span: 400,
        total_rows: 400,
        rows_above: 0,
        rows: vec![Row::file(saturated)],
        files: 2,
        top: Position::default(),
        read: 1,
        scale: Scale::flat(u32::MAX),
        gutter: None,
        worktree_churn: Default::default(),
        notes: Default::default(),
    };

    // Every heat and sparkline rung is reached inside this range, which is what makes
    // the grouping arithmetic exercised at all: the six-slice rung, where the fault
    // was, is drawn between 37 and 47 columns in a region with no caret, and 39 to 49
    // in the pinned list, which has one.
    for width in 0..=60u16 {
        for height in 0..=8u16 {
            let backend = screen(width, height, &view, &chrome());
            assert_eq!(backend.buffer().area.width, width);
        }
    }
}

#[test]
fn a_rename_never_names_only_the_file_it_came_from() {
    // `elide_head` cuts the head because a path's tail identifies the file.
    let renamed = FileEntry {
        origin: Origin::Unstaged,
        path: "crates/vigia/src/shell.rs".to_owned(),
        from: Some("crates/vigia/src/main.rs".to_owned()),
        kind: 'R',
        churn: Some((0, 0)),
        spark: [0; HISTORY_BUCKETS],
        recency: Recency::Cold,
        newest: false,
        heat: [HeatBucket::default(); HEAT_BUCKETS],
    };
    let view = View {
        list_span: 1,
        grouped: false,
        list: vec![renamed.clone().into()],
        rows: vec![Row::file(renamed)],
        files: 1,
        ..ragged_counts()
    };

    let mut saw_pair = 0usize;
    let mut saw_alone = 0usize;
    for width in 1..=120u16 {
        let row = row_text(&screen(width, 8, &view, &chrome()), LIST_TOP);
        if row.contains('←') {
            // Drawn as a pair: both names have to be on the row whole.
            saw_pair += 1;
            assert!(
                row.contains("shell.rs") && row.contains("main.rs"),
                "at {width} columns the pair is cut: {row:?}"
            );
            continue;
        }
        // Drawn alone, so whatever of the label reached the screen must belong
        // to the file the row is about, never to the one it came from.
        if row.contains("shell.rs") {
            saw_alone += 1;
        }
        assert!(
            !row.contains("main.rs"),
            "at {width} columns the row names only the file the rename came \
             from: {row:?}"
        );
    }

    // Both directions, or the sweep saw only the width where this cannot fail.
    assert!(
        saw_pair > 0 && saw_alone > 0,
        "the sweep saw the pair {saw_pair} times and the new name alone \
         {saw_alone} times"
    );
}

#[test]
fn a_counts_cell_never_rounds_a_change_to_nothing() {
    // The counts abbreviation had no gate, and it shipped a wrong number: a narrower
    // cell left two characters, a 250-line change has no truthful form in two, and the
    // search fell through to the thousands unit and drew `+0k`.
    const BOUNDARIES: [(u32, &str); 10] = [
        (0, "+0"),
        (1, "+1"),
        (250, "+250"),
        (9_999, "+9999"),
        (10_000, "+10k"),
        (999_999, "+999k"),
        (1_000_000, "+1M"),
        (999_999_999, "+999M"),
        (1_000_000_000, "+1G"),
        (u32::MAX, "+4G"),
    ];

    // Whole tokens, not prefixes, because `contains` is the wrong instrument for a
    // value: `contains("+1")` is satisfied by `+1k`, `+1M` and `+139`, and
    // `contains("-0")` is satisfied by `-0k`, which is precisely the wrong number this
    // test is named after.
    let draws = |row: &str, want: &str| {
        row.match_indices(want).any(|(at, _)| {
            row[at + want.len()..]
                .chars()
                .next()
                .is_none_or(|c| !c.is_ascii_digit() && !"kMG".contains(c))
        })
    };

    let mut removed_at = Vec::new();
    let mut added_at = Vec::new();
    for (lines, want) in BOUNDARIES {
        let view = View {
            list_span: 1,
            grouped: false,
            list: vec![listed("src/f.rs", lines, 0).into()],
            files: 1,
            ..ragged_counts()
        };
        let backend = screen(80, 8, &view, &chrome());
        let row = row_text(&backend, LIST_TOP);
        assert!(
            draws(&row, want),
            "a file of {lines} added lines draws something other than {want:?}: \
             {row:?}"
        );
        // The half it shares the cell with, so a fix that widened one and not
        // the other cannot pass.
        assert!(
            draws(&row, "-0"),
            "the removed half went missing at {lines} added lines: {row:?}"
        );
        removed_at.push(column_of(&backend, LIST_TOP, "-"));
        // Where the *added* half starts, which is the half a content-sized cell
        // actually moves.
        added_at.push(column_of(&backend, LIST_TOP, "+"));
    }

    // The field is fixed, which is the whole reason `COUNT_CELL` is a constant rather
    // than a rung.
    assert!(
        removed_at.windows(2).all(|pair| pair[0] == pair[1]),
        "the removed half moved as the added half grew, so the pair is not two \
         independently anchored fields: {removed_at:?} for {BOUNDARIES:?}"
    );
    let span =
        |at: &[u16]| at.iter().max().copied().unwrap_or(0) - at.iter().min().copied().unwrap_or(0);
    assert_eq!(
        span(&added_at),
        3,
        "the added half's start moved by {} columns across `+0` to `+9999`, \
         where a fixed field right-aligning a two-to-five-character token moves \
         it by exactly three: {added_at:?}",
        span(&added_at)
    );
}

#[test]
fn the_palette_reaches_the_cells() {
    // Snapshots cannot see this: `TestBackend`'s `Display` writes symbols and drops
    // styles, so every colour in the theme is invisible to the rest of this file.
    let view = one_file();
    let backend = screen(80, 14, &view, &chrome());
    let buffer = backend.buffer();
    let theme = Theme::default();

    // The pane's own inset comes before anything a row draws, so every
    // column counted here is counted from the first content column and not from
    // the pane's edge.
    let inset = inset_at(80);
    let row_of = |needle: char, y: u16| {
        let cell = &buffer[(inset, y)];
        assert_eq!(
            cell.symbol(),
            needle.to_string(),
            "row {y} does not start with {needle:?}, so this test is reading the \
             wrong line"
        );
    };

    // Row five of a listless fixture: see [`LIST_TOP`]'s third category. The
    // gutter occupies the first columns, so the sigil and its colour are found
    // past it.
    let sigil_x = inset + 4;
    let removed = &buffer[(sigil_x, 5)];
    let added = &buffer[(sigil_x, 6)];
    assert_eq!(removed.symbol(), "-", "expected the removed line at y=5");
    assert_eq!(added.symbol(), "+", "expected the added line at y=6");
    assert_eq!(removed.style().fg, theme.removed.fg);
    assert_eq!(added.style().fg, theme.added.fg);
    assert_ne!(
        removed.style().fg,
        added.style().fg,
        "added and removed lines are the same colour, which is the one thing the \
         palette exists to prevent"
    );
    assert_eq!(
        buffer[(inset, 5)].style().fg,
        theme.gutter.fg,
        "the line number is not drawn in the gutter colour"
    );
    row_of('v', 0);
}

/// The row of content this file's syntax tests read.
const CONTENT_ROW: u16 = 3;

#[test]
fn a_syntax_class_reaches_the_cells_while_the_sigil_keeps_the_diff() {
    // `SPEC.md` §11.1's ruling, as cells, and it is two claims at once.
    let theme = Theme::default();
    // l0 e1 t2 ' '3 v4 a5 l6 u7 e8 ' '9 =10 ' '11 1(12) ;13
    let text = "let value = 1;";
    let view = highlighted(
        LineKind::Added,
        text,
        vec![
            Span {
                len: 3,
                class: Class::Keyword,
            },
            Span {
                len: 9,
                class: Class::Plain,
            },
            Span {
                len: 1,
                class: Class::Number,
            },
            Span {
                len: 1,
                class: Class::Plain,
            },
        ],
    );

    let backend = screen(80, 6, &view, &chrome());
    let sigil = column_of(&backend, CONTENT_ROW, "+");
    let buffer = backend.buffer();
    // Two past the sigil, not one.
    let at = |offset: u16| buffer[(sigil + 2 + offset, CONTENT_ROW)].style().fg;

    assert_eq!(
        buffer[(sigil, CONTENT_ROW)].style().fg,
        theme.added.fg,
        "the sigil stopped carrying the diff, which at sixteen colours is the \
         only thing that still does"
    );
    assert_eq!(at(0), theme.keyword.fg, "`let` is not drawn as a keyword");
    assert_eq!(at(12), theme.number.fg, "`1` is not drawn as a number");
    assert_eq!(
        at(4),
        theme.context.fg,
        "unclassified text on an added line is not drawn plain"
    );
    assert_ne!(
        at(4),
        theme.added.fg,
        "the body of an added line is still green, which is the rule §11.1 \
         considered and rejected in favour of following the mockup"
    );
    assert_ne!(
        theme.keyword.fg, theme.number.fg,
        "two classes share a colour, which is the one thing the class set exists \
         to prevent"
    );
}

#[test]
fn a_line_with_no_spans_draws_exactly_as_it_did_before_highlighting() {
    // A file type nothing recognises, which `SPEC.md` §11.1 rules is ordinary
    // rather than an error. The renderer has to draw the whole line from the
    // uncovered-tail path, so this is the fallback's only gate.
    let theme = Theme::default();
    let text = "nothing here has a grammar";
    let view = highlighted(LineKind::Context, text, Vec::new());

    let backend = screen(80, 6, &view, &chrome());
    let buffer = backend.buffer();
    let drawn: String = (0..buffer.area.width)
        .map(|x| buffer[(x, CONTENT_ROW)].symbol())
        .collect::<String>();

    assert!(
        drawn.contains(text),
        "an unclassified line drew {drawn:?}, which does not contain its own text"
    );
    let start = column_of(&backend, CONTENT_ROW, "n");
    assert_eq!(
        buffer[(start, CONTENT_ROW)].style().fg,
        theme.context.fg,
        "an unclassified line is not drawn in the plain style"
    );
}

#[test]
fn the_continuation_mark_takes_the_colour_of_the_run_that_reached_the_edge() {
    // Untested until now, and that is how the two spellings of the marking rule
    // drifted apart: `put_marked` used the caller's style and `put_runs_marked`
    // fell back to a default at the one width where its loop writes nothing.
    let theme = Theme::default();
    let text = "let // a comment long enough to run off the end of the row";
    let view = || {
        highlighted(
            LineKind::Added,
            text,
            vec![
                Span {
                    len: 3,
                    class: Class::Keyword,
                },
                Span {
                    len: text.len() - 3,
                    class: Class::Comment,
                },
            ],
        )
    };

    // One row taller since the footer gained a rule, which takes a body row and
    // would otherwise land on `CONTENT_ROW`.
    let wide = screen(30, 7, &view(), &chrome());
    let mark = column_of(&wide, CONTENT_ROW, CONTINUES);
    assert_eq!(
        wide.buffer()[(mark, CONTENT_ROW)].style().fg,
        theme.comment.fg,
        "the mark is not drawn in the colour of the comment it cut"
    );

    let narrow = screen(1, 7, &view(), &chrome());
    assert_eq!(
        narrow.buffer()[(0, CONTENT_ROW)].symbol(),
        CONTINUES,
        "a one-column row did not mark that it continues"
    );
    assert_eq!(
        narrow.buffer()[(0, CONTENT_ROW)].style().fg,
        theme.added.fg,
        "at one column the mark fell back to a default instead of taking the \
         sigil's style, which is the divergence from `put_marked` this pins"
    );
}

#[test]
fn a_tab_counts_its_columns_from_the_line_rather_than_from_its_span() {
    // The subtle half of drawing a line as runs.
    let view = highlighted(
        LineKind::Context,
        "a\tb",
        vec![
            Span {
                len: 1,
                class: Class::Keyword,
            },
            Span {
                len: 2,
                class: Class::Plain,
            },
        ],
    );

    let backend = screen(80, 6, &view, &chrome());
    let a = column_of(&backend, CONTENT_ROW, "a");
    let b = column_of(&backend, CONTENT_ROW, "b");

    assert_eq!(
        b - a,
        4,
        "`b` landed {} columns after `a`; a tab from column zero reaches the \
         stop at four, and only a counter that restarted at the span boundary \
         would put it anywhere else",
        b - a
    );
}

/// The three rungs of the recency ladder on one screen, with churn behind them.
fn glancing() -> View {
    View {
        whole: Vec::new(),
        landed: false,
        recorded: 0,
        list_span: 3,
        grouped: false,
        list: Vec::new(),
        list_top: 0,
        current_span: 0,
        total_rows: 0,
        rows_above: 0,
        rows: vec![
            Row::file(FileEntry {
                origin: Origin::Unstaged,
                path: "src/engine/watch.rs".to_owned(),
                from: None,
                kind: 'M',
                churn: Some((42, 7)),
                spark: [
                    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 5, 5, 8, 8, 5, 5, 9, 9, 12, 12,
                ],
                recency: Recency::Pulse,
                newest: true,
                // Additions at the head, a mixed slice in the middle, removals
                // at the tail. One row carrying all three kinds plus the track,
                // which is what the colour gate below reads.
                heat: heat(&[(0, 9, 0), (1, 2, 0), (5, 3, 4), (11, 0, 6)]),
            }),
            Row::file(FileEntry {
                origin: Origin::Unstaged,
                path: "src/render/frame.rs".to_owned(),
                from: None,
                kind: 'M',
                churn: Some((11, 3)),
                spark: [
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 2, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                ],
                recency: Recency::Live,
                newest: false,
                heat: heat(&[(3, 2, 1)]),
            }),
            Row::file(FileEntry {
                origin: Origin::Unstaged,
                path: "Cargo.toml".to_owned(),
                from: None,
                kind: 'M',
                churn: Some((2, 0)),
                spark: [0; HISTORY_BUCKETS],
                recency: Recency::Cold,
                newest: false,
                heat: [HeatBucket::default(); HEAT_BUCKETS],
            }),
        ],
        files: 3,
        top: Position::default(),
        read: 3,
        scale: Scale::spread(12),
        gutter: None,
        worktree_churn: Default::default(),
        notes: Default::default(),
    }
}

/// Block glyphs on row `y` drawn in `colour`, in column order.
fn blocks_of(backend: &TestBackend, y: u16, colours: &[Option<Color>]) -> Vec<char> {
    let buffer = backend.buffer();
    (0..buffer.area.width)
        .map(|x| &buffer[(x, y)])
        .filter(|cell| colours.contains(&cell.style().fg))
        .filter_map(|cell| cell.symbol().chars().next())
        .filter(|glyph| "▁▂▃▄▅▆▇█".contains(*glyph))
        .collect()
}

/// Which columns of row `y` carry a sparkline track.
fn track_at(backend: &TestBackend, y: u16, theme: &Theme) -> Vec<u16> {
    let buffer = backend.buffer();
    (0..buffer.area.width)
        .filter(|&x| {
            let cell = &buffer[(x, y)];
            cell.symbol() == SPARK_TRACK && cell.style().fg == theme.spark_track.fg
        })
        .collect()
}

/// Which columns of row `y` carry a sparkline bucket.
fn bars_at(backend: &TestBackend, y: u16, theme: &Theme) -> Vec<u16> {
    let buffer = backend.buffer();
    // Hoisted rather than rebuilt per cell, which is what it was on the first
    // pass: the ramp is a fact about the theme and the loop is over the row.
    let spark = spark_colours(theme);
    (0..buffer.area.width)
        .filter(|&x| {
            let cell = &buffer[(x, y)];
            RAMP.contains(&cell.symbol()) && spark.contains(&cell.style().fg)
        })
        .collect()
}

/// [`glancing`], on the frame a reader actually opens the pane to.
fn launched() -> View {
    let mut view = glancing();
    for row in &mut view.rows {
        if let Row::File(entry) = row {
            entry.spark = [0; HISTORY_BUCKETS];
            entry.recency = Recency::Cold;
        }
    }
    view.scale = Scale::flat(0);
    view
}

#[test]
fn a_worktree_already_dirty_at_launch_draws_a_track_on_every_row() {
    // The state a reader sees first, and it had no fixture.
    let theme = Theme::default();
    let spark = spark_colours(&theme);
    let backend = screen(80, 6, &launched(), &chrome());

    // Row one, not [`LIST_TOP`], and the difference is the point.
    let mut starts = Vec::new();
    for y in 1..=3u16 {
        let track = track_at(&backend, y, &theme);
        assert_eq!(
            track.len(),
            DRAWN_BUCKETS,
            "row {y} drew {} track cells where the slot is {DRAWN_BUCKETS} \
             wide, so a file with no history is still leaving part of its \
             column blank: {track:?}",
            track.len()
        );
        // Contiguous, or the cells counted above are not one strip.
        assert!(
            track.windows(2).all(|pair| pair[1] == pair[0] + 1),
            "row {y}'s track is not one run of cells: {track:?}"
        );
        // Nothing invented. The track says "no churn in the window", and a
        // single bar would be a number the store never recorded.
        assert!(
            blocks_of(&backend, y, &spark).is_empty(),
            "row {y} drew a sparkline bar for a file with no history, which is \
             churn the store cannot have"
        );
        starts.push(track[0]);
    }

    // The columnar reading the fixed slots buy, on the screen it is least able
    // to give
    // today: three tracks at one `x` are what makes three sparklines read as one
    // small-multiples chart once the files start moving.
    assert!(
        starts.windows(2).all(|pair| pair[0] == pair[1]),
        "the tracks start at different columns on different rows: {starts:?}"
    );
}

#[test]
fn the_first_tick_after_launch_moves_no_column() {
    // The transition, which is what the reserved slot and the track are both for.
    let theme = Theme::default();
    let launch = screen(80, 6, &launched(), &chrome());

    let mut view = launched();
    if let Row::File(entry) = &mut view.rows[0] {
        entry.spark[HISTORY_BUCKETS - 1] = 1;
        entry.recency = Recency::Pulse;
    }
    view.scale = Scale::flat(1);
    let after = screen(80, 6, &view, &chrome());

    // The written file: every drawn bucket but the newest is track, and that one
    // is the top of the ramp because it is the busiest thing on screen.
    assert_eq!(
        track_at(&after, 1, &theme).len(),
        DRAWN_BUCKETS - 1,
        "the file that was just written did not keep the rest of its window as \
         track"
    );
    assert_eq!(
        blocks_of(&after, 1, &spark_colours(&theme)),
        vec!['█'],
        "one write against a screen peak of one is not the top of the ramp"
    );
    // Every other file is still cold, and still says so.
    for y in [2, 3] {
        assert_eq!(
            track_at(&after, y, &theme).len(),
            DRAWN_BUCKETS,
            "row {y} stopped drawing its track when a *different* file was \
             written"
        );
    }

    // Nothing moved. The whole slot occupies the same columns before and
    // after, which is what the reserved-from-the-pane ruling promises and what a
    // reader is most likely to be looking at when it is broken.
    let slot = |backend: &TestBackend, y: u16| {
        let mut columns: Vec<u16> = track_at(backend, y, &theme)
            .into_iter()
            .chain(bars_at(backend, y, &theme))
            .collect();
        columns.sort_unstable();
        columns
    };
    for y in LIST_TOP..=LIST_TOP + 2 {
        assert_eq!(
            slot(&after, y),
            slot(&launch, y),
            "row {y}'s sparkline slot moved on the tick the first file was \
             written"
        );
    }
}

#[test]
fn a_peak_that_disagrees_with_its_buckets_draws_rather_than_dividing_by_it() {
    // A guard that no test could fail is a wish, and this one was.
    let mut view = launched();
    if let Row::File(entry) = &mut view.rows[0] {
        entry.spark = [3; HISTORY_BUCKETS];
    }
    assert_eq!(
        view.scale,
        Scale::flat(0),
        "the fixture stopped being the inconsistent one"
    );

    let theme = Theme::default();
    let backend = screen(80, 5, &view, &chrome());
    assert_eq!(
        track_at(&backend, 1, &theme).len(),
        DRAWN_BUCKETS,
        "a bucket with no scale to measure it against drew something other than \
         the track"
    );
}

#[test]
fn a_bucket_busier_than_the_screens_peak_draws_the_top_and_not_a_panic() {
    // The other inconsistent caller, and the one the clamp's upper bound is for.
    let mut view = glancing();
    if let Row::File(entry) = &mut view.rows[0] {
        entry.spark = [u32::MAX; HISTORY_BUCKETS];
    }
    view.scale = Scale::flat(1);

    let theme = Theme::default();
    let backend = screen(80, 5, &view, &chrome());
    assert_eq!(
        blocks_of(&backend, 1, &spark_colours(&theme)),
        vec!['█'; DRAWN_BUCKETS],
        "a bucket far busier than the screen's peak did not simply top out"
    );
}

#[test]
fn an_empty_bucket_draws_the_track_and_a_written_one_draws_a_bar() {
    // One rule rather than a special case for the cold file: the launch screen above is
    // just the all-empty end of *this*.
    let theme = Theme::default();
    let backend = screen(80, 6, &glancing(), &chrome());

    let mut slot: Vec<(u16, char)> = track_at(&backend, 2, &theme)
        .into_iter()
        .map(|x| (x, 't'))
        .chain(bars_at(&backend, 2, &theme).into_iter().map(|x| (x, 's')))
        .collect();
    slot.sort_unstable();

    assert!(
        slot.windows(2).all(|pair| pair[1].0 == pair[0].0 + 1),
        "the sparkline slot has a hole in it, so some bucket drew neither a bar \
         nor a track: {slot:?}"
    );
    let drawn: String = slot.iter().map(|&(_, class)| class).collect();
    assert_eq!(
        drawn, "tttttssttttt",
        "row 2's window, written only in the middle, drew {drawn:?}, so a bucket's emptiness is \
         not where the store says it is"
    );
}

#[test]
fn the_track_is_never_the_shape_of_a_written_bucket() {
    // The gate on the height channel, and the reason the track is not `▁`.
    let theme = Theme::default();
    let mut view = glancing();
    if let Row::File(entry) = &mut view.rows[0] {
        entry.spark = [
            1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12,
        ];
    }
    let backend = screen(80, 5, &view, &chrome());
    let buffer = backend.buffer();

    let slot = bars_at(&backend, 1, &theme);
    assert_eq!(
        slot.len(),
        DRAWN_BUCKETS,
        "row 1 was given a bucket in every slice so its bars would locate the \
         slot, and it drew {} of them: {slot:?}",
        slot.len()
    );

    let empty = buffer[(slot[0], 2)].symbol();
    assert!(
        !RAMP.contains(&empty),
        "row 2's oldest bucket holds no writes and drew {empty:?}, which is a \
         rung of the ramp, so the height channel no longer separates 'nothing \
         happened' from 'a little did'"
    );
    // Non-vacuity from the other side: row 2 really does draw the ramp's floor
    // somewhere, or "not a rung" is being asserted about a row with no rungs.
    assert!(
        blocks_of(&backend, 2, &spark_colours(&theme)).contains(&'▁'),
        "the fixture no longer draws the ramp's floor, so the comparison above \
         is not against the glyph the ruling is about"
    );
}

/// A scale answers by grouping, and says something for one it does not name.
#[test]
fn a_scale_answers_by_grouping_and_falls_back_to_the_finest() {
    // One, two and four source buckets to a drawn one, which is the ladder
    // twenty-four halving twice produces.
    let scale = Scale([10, 20, 40]);

    assert_eq!(scale.at(1), 10, "the widest rung took the wrong figure");
    assert_eq!(scale.at(2), 20, "the settled rung took the wrong figure");
    assert_eq!(scale.at(4), 40, "the narrowest rung took the wrong figure");

    // Three divides twenty-four and is still not on the ladder, which is the
    // case that makes the fallback worth having: a rung of eight would reach it,
    // pass a divisibility check, and quietly measure a row against a denominator
    // set for another width.
    assert_eq!(
        scale.at(3),
        10,
        "an unnamed grouping did not fall back to the finest figure"
    );
    assert_eq!(
        scale.at(0),
        10,
        "a grouping of zero did not fall back to the finest figure"
    );
}

#[test]
fn a_narrowed_sparkline_covers_the_whole_window_rather_than_its_tail() {
    // The one property of the narrow rung nothing could see, and it is the opposite of
    // what a dropping rung gives.
    let theme = Theme::default();
    let mut view = glancing();
    if let Row::File(entry) = &mut view.rows[1] {
        entry.spark = [0; HISTORY_BUCKETS];
        for at in [0, 1, HISTORY_BUCKETS - 2, HISTORY_BUCKETS - 1] {
            entry.spark[at] = 9;
        }
    }
    let backend = screen(45, 5, &view, &chrome());

    let mut slot: Vec<(u16, char)> = track_at(&backend, 2, &theme)
        .into_iter()
        .map(|x| (x, 't'))
        .chain(bars_at(&backend, 2, &theme).into_iter().map(|x| (x, 's')))
        .collect();
    slot.sort_unstable();

    assert_eq!(
        slot.len(),
        DRAWN_BUCKETS / 2,
        "45 columns is meant to be the six-bucket rung, so this fixture is no \
         longer exercising a narrowed strip at all: {slot:?}"
    );
    let drawn: String = slot.iter().map(|&(_, class)| class).collect();
    assert_eq!(
        drawn, "stttts",
        "the narrowed strip drew {drawn:?}; \"ttttss\" is the newest six source \
         buckets and \"sstttt\" the oldest six, so the window is being \
         truncated rather than re-projected"
    );
}

/// Everything on row `y`, as the reader sees it.
fn row_text(backend: &TestBackend, y: u16) -> String {
    let buffer = backend.buffer();
    (0..buffer.area.width)
        .map(|x| buffer[(x, y)].symbol().to_owned())
        .collect()
}

#[test]
fn the_glance_elements_at_eighty_columns() {
    insta::assert_snapshot!(screen(80, 5, &glancing(), &chrome()));
}

#[test]
fn the_glance_elements_at_forty_columns() {
    insta::assert_snapshot!(screen(40, 5, &glancing(), &chrome()));
}

#[test]
fn a_file_that_just_changed_is_marked_and_the_rest_dim() {
    // Two claims, and the second is invisible to every snapshot in this file
    // because `TestBackend`'s `Display` drops styles: the pulse belongs to
    // exactly one row, and the three rungs have to be three *different*
    // intensities or the gradient `SPEC.md` §5.1 asks for is not being drawn.
    let theme = Theme::default();
    let backend = screen(80, 6, &glancing(), &chrome());

    // Row one, and the reason is the fixture rather than the height.
    let pulsing = row_text(&backend, 1);
    assert!(
        pulsing.contains('●'),
        "the file named by the newest tick carries no pulse: {pulsing:?}"
    );
    for y in [2, 3] {
        let row = row_text(&backend, y);
        assert!(
            !row.contains('●'),
            "row {y} carries the pulse too, so it marks more than the newest \
             tick: {row:?}"
        );
    }

    // The path's own cell, past the pane's inset and then past the kind letter and its
    // space.
    let path_x = inset_at(80) + 2;
    let drawn = |y: u16| {
        let style = backend.buffer()[(path_x, y)].style();
        (style.fg, style.add_modifier)
    };
    let want = |recency| {
        let style = theme.recency(recency);
        (style.fg, style.add_modifier)
    };
    let rungs = [Recency::Pulse, Recency::Live, Recency::Cold];
    for (y, recency) in (1..=3).zip(rungs) {
        assert_eq!(
            drawn(y),
            want(recency),
            "row {y} is not drawn as {recency:?}"
        );
    }
    // Three rungs have to be three *different* intensities, or the gradient
    // `SPEC.md` §5.1 asks for is being claimed rather than drawn.
    assert!(
        drawn(1) != drawn(2) && drawn(2) != drawn(3) && drawn(1) != drawn(3),
        "two rungs of the recency ladder are drawn identically: {:?}",
        [drawn(1), drawn(2), drawn(3)]
    );
}

#[test]
fn a_sparkline_scales_against_the_busiest_file_not_itself() {
    // The whole reason `View::peak` exists.
    let spark = spark_colours(&Theme::default());
    let backend = screen(80, 6, &glancing(), &chrome());
    let busiest = blocks_of(&backend, 1, &spark);
    let quieter = blocks_of(&backend, 2, &spark);

    assert!(
        busiest.contains(&'█'),
        "the busiest file's tallest bucket is not the top of the ramp: \
         {busiest:?}"
    );
    assert!(
        !quieter.contains(&'█'),
        "a file whose busiest bucket is 2 against a screen peak of 12 reached \
         the top of the ramp, so each row is being scaled against itself: \
         {quieter:?}"
    );
    assert!(
        quieter.contains(&'▁'),
        "the quieter file drew no bucket at all, so a bucket with something in \
         it is rounding down to nothing: {quieter:?}"
    );
    // A file nothing has written since startup draws no *bar*.
    assert!(
        blocks_of(&backend, 3, &spark).is_empty(),
        "a file with no churn drew a sparkline bar, which is churn the store \
         cannot have"
    );
    assert_eq!(
        track_at(&backend, 3, &Theme::default()).len(),
        DRAWN_BUCKETS,
        "a file with no churn drew no track either, so its column is blank"
    );
}

/// A pane wide enough for the heat strip's widest rung.
const WHOLE_STRIP_PANE: u16 = 140;

/// A heat map from `(slice, added, removed)` triples, everything else track.
fn heat(slices: &[(usize, u16, u16)]) -> [HeatBucket; HEAT_BUCKETS] {
    let mut map = [HeatBucket::default(); HEAT_BUCKETS];
    for &(at, added, removed) in slices {
        map[at] = HeatBucket { added, removed };
    }
    map
}

#[test]
fn the_four_heat_kinds_reach_the_cells_and_are_distinct() {
    // Invisible to every snapshot in this file, and more so than the palette test
    // above: a heat strip draws the *same glyph* for all four kinds, so a picture of
    // one is twelve identical blocks.
    let theme = Theme::default();
    let backend = screen(WHOLE_STRIP_PANE, 5, &glancing(), &chrome());
    let buffer = backend.buffer();

    // By colour as well as by glyph: the sparkline's top rung on the same row is
    // also a full block. See [`blocks_of`].
    let palette: Vec<_> = [
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
    .collect();
    let strip: Vec<_> = (0..WHOLE_STRIP_PANE)
        .map(|x| &buffer[(x, 1)])
        .filter(|cell| cell.symbol() == HEAT_SLICE)
        .filter_map(|cell| cell.style().fg)
        .filter(|fg| palette.contains(fg))
        .collect();
    assert_eq!(
        strip.len(),
        HEAT_BUCKETS,
        "the heading drew {} slices rather than a whole strip",
        strip.len()
    );

    // The busiest slice of this file holds nine changed lines, so anything from
    // five up is heavy and slice 1's two lines are not. That covers all four
    // kinds and both intensities in one row.
    let want = |style: ratatui::style::Style| style.fg.expect("the theme sets a colour");
    assert_eq!(strip[0], want(theme.heat_added_hot), "slice 0: 9 additions");
    assert_eq!(
        strip[1],
        want(theme.heat_added),
        "slice 1: 2 additions, light"
    );
    assert_eq!(strip[5], want(theme.heat_mixed_hot), "slice 5: 3 and 4");
    assert_eq!(
        strip[11],
        want(theme.heat_removed_hot),
        "slice 11: 6 removals"
    );
    assert_eq!(strip[8], want(theme.heat_track), "slice 8 is untouched");

    // Both intensities of one kind have to differ, or `heavy` is computed and
    // then thrown away.
    assert_ne!(
        strip[0], strip[1],
        "a heavy slice and a light one of the same kind are drawn identically"
    );

    // And the four have to be four. A theme that resolved them to one colour
    // would satisfy a strip drawn entirely correctly and say nothing to a
    // reader, which is the failure this whole element exists to avoid.
    let four = [strip[0], strip[5], strip[11], strip[8]];
    let distinct = four
        .iter()
        .enumerate()
        .filter(|(at, colour)| !four[..*at].contains(colour))
        .count();
    assert_eq!(
        distinct, 4,
        "the four heat kinds resolved to {distinct} colours: {four:?}"
    );
}

// Heat-strip snapshots at 80 and 40 columns exist, and they are
// `the_glance_elements_at_*` above: the shared fixture carries heat, so those two
// pictures *are* the heat-strip pictures.

/// The two-region screen `SPEC.md` §11.1 rules: a pinned list over a diff.
fn two_regions_at(current: usize, row: usize) -> View {
    View {
        whole: Vec::new(),
        landed: false,
        recorded: 0,
        list_span: 3,
        grouped: false,
        list: vec![
            entry("src/engine/change.rs", 8, 2).into(),
            entry("src/engine/watch.rs", 42, 7).into(),
            entry("src/render/frame.rs", 11, 3).into(),
        ],
        list_top: 0,
        // Tall enough that a scroll inside one file is several rows of bar, which
        // is what makes the within-a-file half of the ruling observable at all.
        current_span: 400,
        total_rows: 0,
        rows_above: 0,
        rows: vec![
            file("src/engine/watch.rs", 42, 7),
            Row::Hunk {
                old_start: 38,
                old_lines: 8,
                new_start: 38,
                new_lines: 9,
            },
            line(LineKind::Context, 38, "fn coalesce(&mut self) {"),
            line(LineKind::Added, 39, "    if self.pending.is_empty() {"),
        ],
        files: 3,
        top: Position { file: current, row },
        read: 4,
        scale: Scale::flat(0),
        gutter: None,
        worktree_churn: Default::default(),
        notes: Default::default(),
    }
}

/// The same screen at the top of its current file.
fn two_regions(current: usize) -> View {
    two_regions_at(current, 0)
}

#[test]
fn the_caret_marks_the_file_the_diff_is_inside() {
    // The one thing on screen that says which of the listed files the diff below
    // belongs to.
    for current in 0..3usize {
        let view = two_regions(current);
        let backend = screen(64, 18, &view, &chrome());
        let buffer = backend.buffer();

        for row in 0..3u16 {
            // The list starts at [`LIST_TOP`], under the header and the body's lead
            // blank, and the caret sits on the pane's own leading column rather than at
            // its first content column.
            let marked = buffer[(0, row + LIST_TOP)].symbol() == CARET;
            assert_eq!(
                marked,
                row as usize == current,
                "with the diff in file {current}, row {row} of the list {} the \
                 caret",
                if marked { "carries" } else { "does not carry" }
            );
        }
    }
}

#[test]
fn the_rule_separates_the_regions_and_spans_the_pane() {
    // The rule is what makes two regions read as two rather than as a list that
    // ran out. It has to reach both edges: one that stopped short would read as a
    // box someone forgot to close.
    const RULE: char = '─';

    for width in [40u16, 64, 120] {
        let view = two_regions(1);
        let backend = screen(width, 18, &view, &chrome());
        let buffer = backend.buffer();

        // The header, the body's lead blank, then three list rows,
        // so the rule is row five.
        let y = LIST_TOP + 3;
        let drawn: String = (0..width).map(|x| buffer[(x, y)].symbol()).collect();
        assert_eq!(
            drawn,
            RULE.to_string().repeat(usize::from(width)),
            "at {width} columns the rule is {drawn:?}"
        );

        // And nothing above or below it is one, so the row is the separator
        // rather than a fill the renderer sprayed everywhere.
        for other in [y - 1, y + 1] {
            let row: String = (0..width).map(|x| buffer[(x, other)].symbol()).collect();
            assert!(
                !row.contains(RULE),
                "row {other} also holds the rule: {row:?}"
            );
        }
    }
}

#[test]
fn the_caret_degrades_once_and_never_flickers() {
    // I6's ladder applied to the newest glance element.
    let drawn: Vec<bool> = (1..=60u16)
        .map(|width| {
            let view = two_regions(1);
            let backend = screen(width, 18, &view, &chrome());
            let buffer = backend.buffer();
            // The list's second row, which is where `two_regions(1)` puts
            // the current file, offset past the header and the lead blank.
            (0..width)
                .map(|x| buffer[(x, LIST_TOP + 1)].symbol())
                .collect::<String>()
                .contains(CARET)
        })
        .collect();

    assert!(drawn.iter().any(|on| *on), "no width drew the caret");
    assert!(
        drawn.iter().any(|on| !*on),
        "no width was narrow enough to drop it, so the ladder is never exercised"
    );

    let first = drawn.iter().position(|on| *on).expect("a width with it");
    assert!(
        drawn[first..].iter().all(|on| *on),
        "the caret came back after being dropped: {:?}",
        &drawn[first..]
    );
}

/// The rows of column `x` that carry the scrollbar's thumb.
fn thumb_rows(backend: &TestBackend, x: u16, rows: std::ops::Range<u16>) -> Vec<u16> {
    const THUMB: &str = "█";
    let buffer = backend.buffer();
    rows.filter(|y| buffer[(x, *y)].symbol() == THUMB).collect()
}

/// The rows of a stepped bar's region that carry track rather than a step button.
fn stepped_track(region: std::ops::Range<u16>) -> std::ops::Range<u16> {
    let rows = region.end - region.start;
    assert!(
        rows >= STEP_FLOOR,
        "a {rows}-row region is below the step floor, so its bar has no buttons \
         and its track is the region itself"
    );
    region.start + 1..region.end - 1
}

#[test]
fn the_list_scrollbar_spans_the_visible_window() {
    // The list's bar is exact, because both of its numbers are free: the window
    // it shows and the changed-file count are known without reading anything.
    const TRACK: &str = "│";
    let width = 64u16;

    let mut seen = Vec::new();
    for list_top in [0usize, 3, 7] {
        let view = View {
            list_top,
            files: 10,
            ..two_regions(list_top)
        };
        let backend = screen(width, 18, &view, &chrome());
        let marks = thumb_rows(&backend, width - 1, LIST_TOP..LIST_TOP + 3);
        assert!(
            !marks.is_empty(),
            "the list bar drew no thumb at all with the window at {list_top}"
        );
        assert!(
            marks.len() < 3,
            "the thumb filled the whole bar with ten files and three rows shown"
        );
        // And the rest of the column is track, not blank: a mark with no extent
        // around it cannot be read as a position.
        let buffer = backend.buffer();
        for y in LIST_TOP..LIST_TOP + 3 {
            let symbol = buffer[(width - 1, y)].symbol();
            assert!(
                symbol == TRACK || marks.contains(&y),
                "row {y} of the list bar is {symbol:?}, neither thumb nor track"
            );
        }
        seen.push(marks[0]);
    }

    // Monotone, and moving overall.
    assert!(
        seen.windows(2).all(|pair| pair[0] <= pair[1]),
        "the thumb went back up as the window moved down: {seen:?}"
    );
    assert!(
        seen[0] < seen[seen.len() - 1],
        "the thumb never moved across the whole range: {seen:?}"
    );
}

#[test]
fn the_diff_scrollbar_is_proportional_to_the_rows_it_shows() {
    // What the bar means since I4 was narrowed: the thumb is the screen's rows over the
    // diff's rows, and it sits at the rows above the screen.
    let width = 64u16;
    let height = 24u16;
    // Asked of the layout rather than counted.
    let laid = regions(
        Rect::new(0, 0, width, height),
        &chrome(),
        &a_list_of(3, 3, 0),
    );
    let region = laid.diff.top..laid.diff.top + laid.diff.rows;
    let rows = usize::from(region.end - region.start);

    // A screen the walk could actually produce: a diff taller than the region fills
    // it, and the thumb spans the rows of the diff on screen rather than the rows of
    // the terminal, so a fixture drawing two of them would floor the thumb at one row
    // whatever the total was.
    let full: Vec<Row> = (0..rows)
        .map(|i| line(LineKind::Context, 38 + i as u32, "fn coalesce(&mut self) {"))
        .collect();

    // A thumb that halves when the diff doubles, which is the proportionality no
    // file-counting scheme can express.
    let mut lengths = Vec::new();
    for total in [rows * 2, rows * 4, rows * 8] {
        let view = View {
            total_rows: total,
            rows_above: 0,
            rows: full.clone(),
            ..a_list_of(3, 3, 0)
        };
        let marks = thumb_rows(
            &screen(width, height, &view, &chrome()),
            width - 1,
            region.clone(),
        );
        assert!(!marks.is_empty(), "a diff of {total} rows drew no thumb");
        lengths.push(marks.len());
    }
    assert!(
        lengths[0] > lengths[1] && lengths[1] > lengths[2],
        "the thumb did not shrink as the diff grew: {lengths:?}"
    );

    // And it travels the whole track, ending exactly at the bottom. The track,
    // which this region is tall enough to have step buttons on, so the two ends
    // are one row inside the region rather than the region's own.
    let track = stepped_track(region.clone());
    let total = rows * 6;
    let mut firsts = Vec::new();
    for above in [0, total / 4, total / 2, total - rows] {
        let view = View {
            total_rows: total,
            rows_above: above,
            rows: full.clone(),
            ..a_list_of(3, 3, 0)
        };
        let marks = thumb_rows(
            &screen(width, height, &view, &chrome()),
            width - 1,
            region.clone(),
        );
        assert!(!marks.is_empty(), "{above} rows above drew no thumb");
        firsts.push(marks[0]);
        if above == 0 {
            assert_eq!(marks[0], track.start, "the top of the diff is not the top");
        }
        if above == total - rows {
            assert_eq!(
                *marks.last().expect("a thumb"),
                track.end - 1,
                "the end of the diff is not the bottom"
            );
        }
    }
    assert!(
        firsts.windows(2).all(|pair| pair[0] < pair[1]),
        "the thumb did not descend as the viewport did: {firsts:?}"
    );
}

#[test]
fn a_region_with_nothing_to_scroll_spends_no_column_on_a_bar() {
    // A full bar is a column saying there is nothing to say. The list of three
    // files with three rows on screen has nowhere to scroll, so the region keeps
    // its width for the paths.
    const TRACK: &str = "│";
    const THUMB: &str = "█";
    let width = 64u16;

    let view = two_regions(1);
    assert_eq!(
        view.files,
        view.list.len(),
        "the fixture has room to scroll"
    );
    let backend = screen(width, 18, &view, &chrome());
    let buffer = backend.buffer();

    for y in LIST_TOP..LIST_TOP + 3 {
        let symbol = buffer[(width - 1, y)].symbol();
        assert!(
            symbol != TRACK && symbol != THUMB,
            "row {y} drew a bar for a list that fits entirely on screen"
        );
    }
}

#[test]
fn the_scrollbars_degrade_once_and_never_flicker() {
    // The same ladder rule the caret follows, for the same reason: a bar that
    // reappeared at a narrower width would read as the position jumping while a
    // reader dragged a pane edge.
    const TRACK: &str = "│";
    const THUMB: &str = "█";

    let drawn: Vec<bool> = (1..=60u16)
        .map(|width| {
            let view = View {
                files: 10,
                ..two_regions(1)
            };
            let backend = screen(width, 18, &view, &chrome());
            let buffer = backend.buffer();
            let symbol = buffer[(width - 1, 2)].symbol();
            symbol == TRACK || symbol == THUMB
        })
        .collect();

    assert!(drawn.iter().any(|on| *on), "no width drew a bar");
    assert!(
        drawn.iter().any(|on| !*on),
        "no width was narrow enough to drop one, so the ladder is never exercised"
    );
    let first = drawn.iter().position(|on| *on).expect("a width with one");
    assert!(
        drawn[first..].iter().all(|on| *on),
        "a bar came back after being dropped: {:?}",
        &drawn[first..]
    );

    // And the width it first appears at, which monotonicity alone cannot say.
    const BAR_WIDTH: usize = 1 + 1;
    const ROW_FLOOR: usize = 2 + 12;
    const BAR_FLOOR: usize = BAR_WIDTH + ROW_FLOOR;
    assert_eq!(
        first + 1,
        BAR_FLOOR,
        "the bar first appears at {} columns, not the {BAR_FLOOR} a row needs \
         before it can afford one",
        first + 1
    );
}

#[test]
fn a_one_row_region_with_somewhere_to_scroll_still_spends_no_column() {
    // The claim `bar_for` makes in prose, which nothing held.
    let width = 64u16;
    let view = a_stepped_screen();
    let mut seen_list = false;
    let mut seen_diff = false;

    for height in 3u16..=8 {
        let backend = screen(width, height, &view, &chrome());
        let laid = regions(Rect::new(0, 0, width, height), &chrome(), &view);

        for (name, region, one_row) in [
            ("the list", laid.list, &mut seen_list),
            ("the diff", laid.diff, &mut seen_diff),
        ] {
            if region.rows != 1 {
                continue;
            }
            *one_row = true;
            let glyph = bar_at(&backend, region.top);
            assert!(
                !is_bar_glyph(glyph),
                "at {height} rows of pane, {name} is one row and drew {glyph:?} on \
                 the bar's column, which is a mark that cannot move"
            );
        }
    }

    // Both regions reach one row by different routes, and a sweep that saw
    // neither would pass by never producing the case.
    assert!(seen_list, "no pane height gave the list exactly one row");
    assert!(seen_diff, "no pane height gave the diff exactly one row");
}

/// A pinned list of `shown` rows over `files` changed files, scrolled to `top`.
fn a_list_of(files: usize, shown: usize, top: usize) -> View {
    View {
        whole: Vec::new(),
        landed: false,
        recorded: 0,
        // A screenful is `shown`, which is what this fixture's name says and what the
        // bar is measured in: `View::list_span` is the complement of the window's
        // ceiling, and for an ungrouped list of `shown` rows that is exactly `shown`.
        list_span: shown,
        grouped: false,
        list: (0..shown)
            .map(|i| ListRow::from(entry(&format!("src/f{i}.rs"), 1, 0)))
            .collect(),
        list_top: top,
        current_span: 400,
        // A diff far taller than any pane, with the viewport at its top. Gates
        // about where the thumb sits override these two.
        total_rows: 400 * files.max(1),
        rows_above: 0,
        rows: vec![
            file("src/engine/watch.rs", 42, 7),
            line(LineKind::Context, 38, "fn coalesce(&mut self) {"),
        ],
        files,
        top: Position::default(),
        read: 2,
        scale: Scale::flat(0),
        gutter: None,
        worktree_churn: Default::default(),
        notes: Default::default(),
    }
}

#[test]
fn a_scrollbar_reaches_the_bottom_at_its_last_window() {
    // The invariant `Painter::scrollbar`'s own doc claims and neither region's
    // gate stated: the thumb's travel maps onto the track's travel, so the last
    // position fills the bottom row exactly as the first fills the top.
    let width = 64u16;
    let shown = 6usize;
    // The list's own rows, from the layout: the masthead sits above them.
    let region = {
        let laid = regions(
            Rect::new(0, 0, width, 24),
            &chrome(),
            &a_list_of(30, shown, 0),
        );
        laid.list.top..laid.list.top + laid.list.rows
    };
    // Six rows is above the step floor, so both ends of this bar are buttons and the
    // thumb's ends are one row inside them.
    let track = stepped_track(region.clone());

    for files in (shown + 1)..=30 {
        let last = files - shown;

        let bottom = a_list_of(files, shown, last);
        let backend = screen(width, 24, &bottom, &chrome());
        let marks = thumb_rows(&backend, width - 1, region.clone());
        assert!(
            !marks.is_empty(),
            "{files} files: the last window drew no thumb"
        );
        assert_eq!(
            *marks.last().expect("a thumb"),
            track.end - 1,
            "{files} files: the last window's thumb ends at row {:?}, not the \
             bottom of the track",
            marks.last()
        );

        // And the first window fills the top, so the two ends are distinguishable.
        let first = a_list_of(files, shown, 0);
        let top_marks = thumb_rows(
            &screen(width, 24, &first, &chrome()),
            width - 1,
            region.clone(),
        );
        assert_eq!(
            top_marks.first().copied(),
            Some(track.start),
            "{files} files: the first window's thumb does not start at the top"
        );
        assert_ne!(
            marks, top_marks,
            "{files} files: the bar draws the same column at both ends, so it \
             says nothing about where the window is"
        );
    }
}

/// The step buttons, spelled here rather than imported for [`RAMP`]'s reason.
const STEP_UP: &str = "▲";
const STEP_DOWN: &str = "▼";

/// The shortest region whose bar carries step buttons.
const STEP_FLOOR: u16 = 2 + 2;

/// The symbol on the bar's column at row `y`.
fn bar_at(backend: &TestBackend, y: u16) -> &str {
    let buffer = backend.buffer();
    let x = buffer.area().width - 1;
    buffer[(x, y)].symbol()
}

/// Whether a region's rows carry a bar at all.
fn has_bar(backend: &TestBackend, region: Region) -> bool {
    (region.top..region.top + region.rows).any(|y| is_bar_glyph(bar_at(backend, y)))
}

/// The fixture the step-button gates sweep: thirty changed files, six listed, a
/// diff far taller than any pane.
fn a_stepped_screen() -> View {
    View {
        total_rows: 4_000,
        rows_above: 0,
        ..a_list_of(30, 6, 0)
    }
}

#[test]
fn the_scrollbar_draws_a_step_button_at_each_end() {
    // The step buttons' own ask, on both regions, through the one drawer they share.
    let width = 64u16;
    let height = 24u16;
    let view = a_stepped_screen();
    let backend = screen(width, height, &view, &chrome());
    let seen = regions(Rect::new(0, 0, width, height), &chrome(), &view);

    for (name, region) in [("the list", seen.list), ("the diff", seen.diff)] {
        assert!(
            region.rows >= STEP_FLOOR,
            "{name} is {} rows, which is below the floor this gate is about",
            region.rows
        );
        assert!(has_bar(&backend, region), "{name} drew no bar to step");
        assert_eq!(
            bar_at(&backend, region.top),
            STEP_UP,
            "{name} has no up button on its first row"
        );
        assert_eq!(
            bar_at(&backend, region.top + region.rows - 1),
            STEP_DOWN,
            "{name} has no down button on its last row"
        );
    }
}

#[test]
fn a_held_step_button_lights_and_only_that_one() {
    // The feedback, and it is the half a reader notices most.
    let width = 64u16;
    let height = 24u16;
    let view = a_stepped_screen();
    let theme = Theme::default();
    let x = width - 1;

    let at_rest = chrome();
    let laid = regions(Rect::new(0, 0, width, height), &at_rest, &view);
    let ends = [
        laid.diff.top,
        laid.diff.top + laid.diff.rows - 1,
        laid.list.top,
        laid.list.top + laid.list.rows - 1,
    ];

    // Nothing held: every button is chrome, and the thumb is the only lit thing.
    let resting = screen(width, height, &view, &at_rest);
    for y in ends {
        assert_eq!(
            resting.buffer()[(x, y)].style().fg,
            theme.bar_track.fg,
            "row {y} is lit with no button held"
        );
    }

    // Each button in turn: it lights, and the other three do not.
    for pressed in ends {
        let held = Chrome {
            pressed: Some((x, pressed)),
            ..chrome()
        };
        let backend = screen(width, height, &view, &held);
        for y in ends {
            let want = if y == pressed {
                theme.bar_active.fg
            } else {
                theme.bar_track.fg
            };
            assert_eq!(
                backend.buffer()[(x, y)].style().fg,
                want,
                "with row {pressed} held, row {y} took the wrong style"
            );
        }
        // And the glyph is unchanged, so this is a state and not a second mark.
        let glyph = bar_at(&backend, pressed);
        assert!(
            glyph == STEP_UP || glyph == STEP_DOWN,
            "a held button drew {glyph:?} instead of staying a step button"
        );
    }

    // A cell that is not a button is unaffected, so the highlight cannot leak
    // onto the track or off the bar's column.
    let elsewhere = Chrome {
        pressed: Some((x, laid.diff.track.0)),
        ..chrome()
    };
    let backend = screen(width, height, &view, &elsewhere);
    for y in ends {
        assert_eq!(
            backend.buffer()[(x, y)].style().fg,
            theme.bar_track.fg,
            "a press on the track lit the button at row {y}"
        );
    }
}

#[test]
fn a_dragged_bar_lights_its_thumb_and_the_other_bar_stays_put() {
    // The same reading the step buttons carry, on the element a drag is actually
    // moving. Both bars share one column, so the gate that matters is that
    // gripping one leaves the other alone.
    const THUMB: &str = "█";
    let width = 64u16;
    let height = 24u16;
    let view = a_stepped_screen();
    let theme = Theme::default();
    let x = width - 1;
    let laid = regions(Rect::new(0, 0, width, height), &chrome(), &view);

    for (name, whose, gripped, other) in [
        ("the list", Grabbed::List, laid.list, laid.diff),
        ("the diff", Grabbed::Diff, laid.diff, laid.list),
    ] {
        let held = Chrome {
            gripped: Some(whose),
            ..chrome()
        };
        let backend = screen(width, height, &view, &held);
        let lit = |region: Region| {
            (region.track.0..region.track.0 + region.track.1)
                .filter(|y| bar_at(&backend, *y) == THUMB)
                .all(|y| backend.buffer()[(x, y)].style().fg == theme.bar_active.fg)
        };
        assert!(
            lit(gripped),
            "{name} was dragged and its thumb did not light"
        );
        assert!(
            !lit(other),
            "dragging {name} lit the other bar's thumb, and they share a column"
        );
    }

    // And with nothing gripped, neither is lit.
    let resting = screen(width, height, &view, &chrome());
    for region in [laid.list, laid.diff] {
        for y in region.track.0..region.track.0 + region.track.1 {
            if bar_at(&resting, y) == THUMB {
                assert_eq!(
                    resting.buffer()[(x, y)].style().fg,
                    theme.bar.fg,
                    "a thumb was lit with nothing being dragged"
                );
            }
        }
    }
}

#[test]
fn a_scroll_lights_one_arrow_on_one_bar() {
    // Reported from use: a keypress lit the matching arrow on *both* bars.
    let width = 64u16;
    let height = 24u16;
    let view = a_stepped_screen();
    let theme = Theme::default();
    let x = width - 1;
    let laid = regions(Rect::new(0, 0, width, height), &chrome(), &view);

    let ends = |region: Region| (region.top, region.top + region.rows - 1);
    let (diff_up, diff_down) = ends(laid.diff);
    let (list_up, list_down) = ends(laid.list);

    for (name, whose, way, lit, dark, other) in [
        (
            "the diff",
            Grabbed::Diff,
            -1isize,
            diff_up,
            diff_down,
            [list_up, list_down],
        ),
        (
            "the diff",
            Grabbed::Diff,
            1,
            diff_down,
            diff_up,
            [list_up, list_down],
        ),
        (
            "the list",
            Grabbed::List,
            -1,
            list_up,
            list_down,
            [diff_up, diff_down],
        ),
        (
            "the list",
            Grabbed::List,
            1,
            list_down,
            list_up,
            [diff_up, diff_down],
        ),
    ] {
        let scrolling = Chrome {
            scrolling: Some((whose, way)),
            ..chrome()
        };
        let backend = screen(width, height, &view, &scrolling);
        let fg = |y: u16| backend.buffer()[(x, y)].style().fg;

        assert_eq!(
            fg(lit),
            theme.bar_active.fg,
            "scrolling {name} by {way} did not light the arrow it moves towards"
        );
        assert_eq!(
            fg(dark),
            theme.bar_track.fg,
            "scrolling {name} by {way} lit the arrow it moves away from"
        );
        for y in other {
            assert_eq!(
                fg(y),
                theme.bar_track.fg,
                "scrolling {name} by {way} lit row {y} on the *other* bar, which answers different keys and moves a different thing"
            );
        }
    }

    // Magnitude is not direction: a half page lights what a single row lights.
    for way in [-9isize, -1, 1, 9] {
        let scrolling = Chrome {
            scrolling: Some((Grabbed::Diff, way)),
            ..chrome()
        };
        let backend = screen(width, height, &view, &scrolling);
        let expected = if way < 0 { diff_up } else { diff_down };
        assert_eq!(
            backend.buffer()[(x, expected)].style().fg,
            theme.bar_active.fg,
            "a scroll of {way} lit the wrong arrow"
        );
    }
}

#[test]
fn a_hovered_step_button_is_brighter_than_the_track_and_dimmer_than_a_press() {
    // `SPEC.md` §11.2 B10's mark, and the ordering is the load-bearing half.
    let width = 64u16;
    let height = 24u16;
    let view = a_stepped_screen();
    let theme = Theme::default();
    let x = width - 1;
    let laid = regions(Rect::new(0, 0, width, height), &chrome(), &view);
    let button = laid.diff.top;

    let fg = |chrome: &Chrome| {
        screen(width, height, &view, chrome).buffer()[(x, button)]
            .style()
            .fg
    };

    assert_eq!(fg(&chrome()), theme.bar_track.fg, "a button at rest");

    let hovered = Chrome {
        hovered: Some(Hovered::Button(x, button)),
        ..chrome()
    };
    assert_eq!(
        fg(&hovered),
        theme.bar.fg,
        "a hovered button did not brighten, so the pointer is still invisible \
         until it clicks"
    );

    let pressed = Chrome {
        pressed: Some((x, button)),
        ..chrome()
    };
    assert_eq!(fg(&pressed), theme.bar_active.fg, "a pressed button");

    let both = Chrome {
        hovered: Some(Hovered::Button(x, button)),
        pressed: Some((x, button)),
        ..chrome()
    };
    assert_eq!(
        fg(&both),
        theme.bar_active.fg,
        "a hover outranked a press, which takes away the only answer a button \
         has when pressing it moves no row"
    );

    // Three distinct rungs, asserted rather than assumed: on a palette where two
    // of them coincided this test would pass while saying nothing.
    assert_ne!(theme.bar_track.fg, theme.bar.fg);
    assert_ne!(theme.bar.fg, theme.bar_active.fg);

    // And the *other* button is untouched, so the mark is about a cell rather
    // than about the bar. This is what `Hovered::Button` carrying a cell buys.
    let other = laid.diff.top + laid.diff.rows - 1;
    assert_eq!(
        screen(width, height, &view, &hovered).buffer()[(x, other)]
            .style()
            .fg,
        theme.bar_track.fg,
        "hovering one button lit the one at the other end"
    );
}

#[test]
fn a_hovered_row_reads_as_the_pointer_and_never_as_recency() {
    // The anti-collision property, and the whole reason this design works.
    let theme = Theme::default();
    for (name, recency) in [
        ("pulse", Recency::Pulse),
        ("live", Recency::Live),
        ("cold", Recency::Cold),
    ] {
        assert_ne!(
            weight(theme.path_hover),
            weight(theme.recency(recency)),
            "a hovered row reads exactly like a {name} one, so the mark says \
             something about the worktree instead of about the pointer"
        );
    }

    // And the distinguishing channel is the modifier rather than the colour, which on
    // the default palette is the only one left.
    for recency in [Recency::Pulse, Recency::Live, Recency::Cold] {
        assert!(
            !theme
                .recency(recency)
                .add_modifier
                .contains(Modifier::UNDERLINED),
            "a recency weight underlines, so it collides with the hover mark"
        );
    }

    // Drawn, and on the list's row rather than the diff's heading. A diff
    // heading goes through the same `file_row` drawer, so a mark keyed on the
    // row alone would light one if the regions were ever confused.
    let width = 80u16;
    let height = 24u16;
    let view = a_stepped_screen();
    let laid = regions(Rect::new(0, 0, width, height), &chrome(), &view);
    // The second list row, not the first, and the caret is the reason.
    let row = laid.list.top + 1;

    // Two screens, drawn once each, rather than one per row inside the loop
    // below, which is the same picture five times over.
    let want = weight(theme.path_hover);
    let untouched = screen(width, height, &view, &chrome());
    let hovering = screen(
        width,
        height,
        &view,
        &Chrome {
            hovered: Some(Hovered::Row(row)),
            ..chrome()
        },
    );
    let path_weight = |backend: &TestBackend, y: u16| {
        (0..width)
            .filter(|x| weight(backend.buffer()[(*x, y)].style()) == want)
            .count()
    };

    assert_eq!(
        path_weight(&untouched, row),
        0,
        "a row was drawn in the hover weight with nothing hovered"
    );
    assert!(
        path_weight(&hovering, row) > 0,
        "a hovered row did not take the hover weight"
    );

    // Every other list row is untouched, so the mark is about one row and not
    // the region.
    for other in (laid.list.top..(laid.list.top + laid.list.rows)).filter(|y| *y != row) {
        assert_eq!(
            path_weight(&hovering, other),
            0,
            "hovering row {row} lit row {other} as well"
        );
    }
}

/// Every built-in palette, so a rule about the pointer is asserted on all three
/// rather than on whichever one `Theme::default` happens to be.
fn built_ins() -> [(&'static str, Theme); 3] {
    [
        ("ansi", Theme::ansi()),
        ("dark", Theme::dark()),
        ("light", Theme::light()),
    ]
}

#[test]
fn the_pointer_reads_the_same_colour_wherever_it_rests() {
    // One reading means *the pointer is here*, on a path and on a bar alike.
    for (name, theme) in built_ins() {
        assert_eq!(
            theme.path_hover.fg, theme.bar_hover.fg,
            "{name} draws a hovered path in a colour the bar's own hover does \
             not use, so the pointer reads as two marks instead of one"
        );
    }
}

#[test]
fn the_pointer_never_takes_the_weight_the_caret_row_does() {
    // The two channels, kept apart by construction.
    for (name, theme) in built_ins() {
        assert!(
            theme.path_hover.add_modifier.contains(Modifier::UNDERLINED),
            "{name}'s hover weight stopped underlining, which is the whole of \
             what keeps it off the recency ladder"
        );
        assert!(
            !theme.path_hover.add_modifier.contains(Modifier::BOLD),
            "{name}'s hover weight is bold, which is the weight the caret's row \
             carries, so a pointer resting anywhere reads as the current file"
        );
    }
}

#[test]
fn the_file_the_diff_is_inside_is_drawn_bold_beside_its_caret() {
    // A second channel on one statement rather than a second statement.
    let width = 80u16;
    let height = 24u16;
    // Every entry is `Recency::Cold` and the caret sits on the first row, so the
    // expected weight is one rung plus `BOLD` and nothing else on screen shares
    // it: the pulse rung is a different foreground.
    let view = a_stepped_screen();
    let theme = Theme::default();
    let laid = regions(Rect::new(0, 0, width, height), &chrome(), &view);
    let backend = screen(width, height, &view, &chrome());

    let plain = weight(theme.recency(Recency::Cold));
    let marked = (plain.0, plain.1 | Modifier::BOLD);
    assert_ne!(plain, marked, "the fixture's rung is already bold");

    let row = laid.list.top;
    assert!(
        (0..width).any(|x| backend.buffer()[(x, row)].symbol() == CARET),
        "the fixture drew no caret, so this gate is about nothing"
    );
    assert!(
        path_weights(&backend, row, "src/f0.rs")
            .iter()
            .all(|w| *w == marked),
        "the row the caret marks drew no bold path, so the file the diff is \
         inside is named by one channel where the region carries two"
    );

    for other in (laid.list.top + 1)..(laid.list.top + laid.list.rows) {
        let path = format!("src/f{}.rs", other - laid.list.top);
        assert!(
            path_weights(&backend, other, &path)
                .iter()
                .all(|w| *w == plain),
            "row {other} took the caret's weight without a caret"
        );
    }
}

#[test]
fn the_weight_arrives_and_leaves_with_the_caret() {
    // The tie is to `affords_caret`, not to the current file.
    let width = 16u16;
    let height = 24u16;
    let view = a_stepped_screen();
    let theme = Theme::default();
    let laid = regions(Rect::new(0, 0, width, height), &chrome(), &view);
    let backend = screen(width, height, &view, &chrome());

    let plain = weight(theme.recency(Recency::Cold));
    let marked = (plain.0, plain.1 | Modifier::BOLD);

    assert!(laid.list.rows > 0, "the pane drew no list to check");
    for y in laid.list.top..(laid.list.top + laid.list.rows) {
        assert!(
            (0..width).all(|x| backend.buffer()[(x, y)].symbol() != CARET),
            "row {y} drew a caret on a pane too narrow for the column"
        );
        assert_eq!(
            (0..width)
                .filter(|x| weight(backend.buffer()[(*x, y)].style()) == marked)
                .count(),
            0,
            "row {y} kept the caret's weight after the caret was dropped, so a \
             bold row says nothing a reader can attribute"
        );
    }
}

#[test]
fn a_diff_heading_never_takes_the_current_weight() {
    // The same confinement `a_hovered_row_reads_as_the_pointer_and_never_as_recency`
    // asserts one mark over.
    let width = 80u16;
    let height = 24u16;
    let path = "src/engine/watch.rs";
    let view = two_regions(1);
    let theme = Theme::default();
    let laid = regions(Rect::new(0, 0, width, height), &chrome(), &view);
    let backend = screen(width, height, &view, &chrome());

    let plain = weight(theme.recency(Recency::Cold));
    let marked = (plain.0, plain.1 | Modifier::BOLD);

    // The fixture drew what it claims to, or everything below is about the wrong
    // rows.
    assert!(
        path_weights(&backend, laid.list.top + 1, path)
            .iter()
            .all(|w| *w == marked),
        "the caret's own row is not marked, so this gate cannot tell a confined \
         mark from an absent one"
    );
    assert!(
        path_weights(&backend, laid.diff.top, path)
            .iter()
            .all(|w| *w == plain),
        "the diff's heading for the very file the caret marks took the list's \
         weight, so the mark is keyed on the file instead of on the row"
    );
}

#[test]
fn a_hovered_row_that_is_also_the_current_one_reads_as_both() {
    // Orthogonal on purpose.
    let width = 80u16;
    let height = 24u16;
    let view = a_stepped_screen();
    let theme = Theme::default();
    let laid = regions(Rect::new(0, 0, width, height), &chrome(), &view);
    let row = laid.list.top;
    let backend = screen(
        width,
        height,
        &view,
        &Chrome {
            hovered: Some(Hovered::Row(row)),
            ..chrome()
        },
    );

    let both = (
        theme.path_hover.fg,
        theme.path_hover.add_modifier | Modifier::BOLD,
    );
    assert!(
        (0..width)
            .filter(|x| weight(backend.buffer()[(*x, row)].style()) == both)
            .count()
            > 0,
        "the hovered caret row drew neither mark whole, so one of the two \
         overwrote the other"
    );
}

#[test]
fn a_gesture_is_always_brighter_than_a_pointer_at_rest() {
    // The rule this column is drawn to, asserted as a rule rather than per element.
    const THUMB: &str = "█";

    let width = 64u16;
    let height = 24u16;
    let view = a_stepped_screen();
    let theme = Theme::default();
    let x = width - 1;
    let laid = regions(Rect::new(0, 0, width, height), &chrome(), &view);

    // Three distinct weights, asserted rather than assumed: on a palette where two
    // coincided, every ordering below would hold while saying nothing.
    assert_ne!(
        weight(theme.bar_track),
        weight(theme.bar_hover),
        "rest vs hover"
    );
    assert_ne!(
        weight(theme.bar_hover),
        weight(theme.bar_active),
        "hover vs gesture"
    );
    assert_ne!(
        weight(theme.bar),
        weight(theme.bar_hover),
        "thumb rest vs hover"
    );

    // The button, where a press has to win.
    let button = laid.diff.top;
    let fg = |chrome: &Chrome| {
        weight(screen(width, height, &view, chrome).buffer()[(x, button)].style())
    };
    assert_eq!(fg(&chrome()), weight(theme.bar_track), "a button at rest");
    assert_eq!(
        fg(&Chrome {
            hovered: Some(Hovered::Button(x, button)),
            ..chrome()
        }),
        weight(theme.bar_hover),
        "a hovered button did not take the middle rung"
    );
    assert_eq!(
        fg(&Chrome {
            hovered: Some(Hovered::Button(x, button)),
            pressed: Some((x, button)),
            ..chrome()
        }),
        weight(theme.bar_active),
        "a hover outranked a press, which takes away the only answer a button \
         has when pressing it moves no row"
    );

    // The thumb, where a drag has to win, which a two-rung ladder cannot
    // express and is the reason `bar_hover` exists.
    let thumb_fg = |chrome: &Chrome, region: Region| {
        let backend = screen(width, height, &view, chrome);
        let seen: Vec<_> = (region.track.0..region.track.0 + region.track.1)
            .filter(|y| bar_at(&backend, *y) == THUMB)
            .map(|y| weight(backend.buffer()[(x, y)].style()))
            .collect();
        assert!(!seen.is_empty(), "the fixture drew no thumb to read");
        seen
    };

    assert!(
        thumb_fg(&chrome(), laid.diff)
            .iter()
            .all(|f| *f == weight(theme.bar)),
        "a thumb was lit with nothing touching it"
    );

    let hovering = Chrome {
        hovered: Some(Hovered::Track(Grabbed::Diff)),
        ..chrome()
    };
    assert!(
        thumb_fg(&hovering, laid.diff)
            .iter()
            .all(|f| *f == weight(theme.bar_hover)),
        "a hovered bar did not light its thumb"
    );
    assert!(
        thumb_fg(
            &Chrome {
                gripped: Some(Grabbed::Diff),
                ..hovering.clone()
            },
            laid.diff
        )
        .iter()
        .all(|f| *f == weight(theme.bar_active)),
        "a hover outranked a drag on the thumb, so a hand moving the view reads \
         the same as a pointer resting on it"
    );

    // And hovering one bar leaves the other's thumb alone, which is the
    // both-bars defect one gesture over.
    assert!(
        thumb_fg(&hovering, laid.list)
            .iter()
            .all(|f| *f == weight(theme.bar)),
        "hovering the diff's bar lit the list's thumb, and they share a column"
    );
}

#[test]
fn hovering_one_bars_button_leaves_the_other_bars_alone() {
    // The both-bars defect, one gesture over.
    let width = 64u16;
    let height = 24u16;
    let view = a_stepped_screen();
    let theme = Theme::default();
    let x = width - 1;
    let laid = regions(Rect::new(0, 0, width, height), &chrome(), &view);

    let ends = |region: Region| (region.top, region.top + region.rows - 1);
    let (diff_up, diff_down) = ends(laid.diff);
    let (list_up, list_down) = ends(laid.list);

    // Every row this sweeps is really a button, asserted rather than assumed: a
    // layout change that left the list's bar bare would quietly turn its case
    // into a comparison between two track cells, which passes and proves
    // nothing.
    for row in [diff_up, diff_down, list_up, list_down] {
        assert!(
            laid.hover_at(x, row).is_some(),
            "row {row} is not a step button, so asserting its style proves nothing"
        );
    }

    for (name, lit, others) in [
        ("the diff's up", diff_up, [diff_down, list_up, list_down]),
        ("the list's down", list_down, [list_up, diff_up, diff_down]),
    ] {
        let hovered = Chrome {
            hovered: Some(Hovered::Button(x, lit)),
            ..chrome()
        };
        let backend = screen(width, height, &view, &hovered);
        let fg = |y: u16| backend.buffer()[(x, y)].style().fg;

        assert_eq!(
            fg(lit),
            theme.bar.fg,
            "hovering {name} button did not brighten it"
        );
        for y in others {
            assert_eq!(
                fg(y),
                theme.bar_track.fg,
                "hovering {name} button lit row {y}, which is a different button \
                 and possibly a different bar"
            );
        }
    }
}

#[test]
fn the_painted_track_is_the_track_the_pointer_is_told_about() {
    // The agreement the whole shape rests on.
    let width = 64u16;
    let height = 24u16;

    for above in [0usize, 1, 800, 2_000, 3_970] {
        for list_top in [0usize, 12, 24] {
            let view = View {
                rows_above: above,
                ..a_list_of(30, 6, list_top)
            };
            let view = View {
                total_rows: 4_000,
                ..view
            };
            let backend = screen(width, height, &view, &chrome());
            let seen = regions(Rect::new(0, 0, width, height), &chrome(), &view);

            for (name, region) in [("the list", seen.list), ("the diff", seen.diff)] {
                if !has_bar(&backend, region) {
                    continue;
                }
                let (track_top, track_rows) = region.track;
                for y in region.top..region.top + region.rows {
                    let glyph = bar_at(&backend, y);
                    let on_track = y >= track_top && y < track_top + track_rows;
                    let is_button = glyph == STEP_UP || glyph == STEP_DOWN;
                    assert_eq!(
                        !is_button,
                        on_track,
                        "{name} at {above} rows above and list top {list_top}: row \
                         {y} draws {glyph:?}, and the pointer is told the track is \
                         {track_top}..{}",
                        track_top + track_rows
                    );
                    assert!(
                        is_bar_glyph(glyph),
                        "{name}: row {y} of the bar's column draws {glyph:?}, which \
                         is not a bar glyph at all"
                    );
                }
            }
        }
    }
}

#[test]
fn the_step_buttons_arrive_at_the_step_floor_and_never_leave() {
    // The boundary, not the direction.
    let width = 64u16;
    let view = a_stepped_screen();
    let chrome = chrome();
    let mut short = 0;
    let mut tall = 0;
    let mut shortest_diff: Option<(u16, u16)> = None;

    for height in 5u16..=40 {
        let backend = screen(width, height, &view, &chrome);
        let seen = regions(Rect::new(0, 0, width, height), &chrome, &view);

        for (name, region) in [("the list", seen.list), ("the diff", seen.diff)] {
            if !has_bar(&backend, region) {
                continue;
            }
            let stepped = bar_at(&backend, region.top) == STEP_UP;
            assert_eq!(
                stepped,
                region.rows >= STEP_FLOOR,
                "at {height} rows of pane, {name} is {} rows and {} step buttons",
                region.rows,
                if stepped { "has" } else { "has no" }
            );
            if region.rows >= STEP_FLOOR {
                tall += 1;
            } else {
                short += 1;
            }
        }

        if has_bar(&backend, seen.diff)
            && shortest_diff.is_none_or(|(rows, _)| seen.diff.rows < rows)
        {
            shortest_diff = Some((seen.diff.rows, height));
        }
    }

    // A sweep that only ever saw one side of the floor would pass by never
    // reaching the case it is named for.
    assert!(short > 0, "no region below the step floor was swept");
    assert!(tall > 0, "no region at or above the step floor was swept");

    let (rows, height) = shortest_diff.expect("no pane height drew a diff bar at all");
    assert!(
        rows < STEP_FLOOR,
        "the shortest diff region this sweep found is {rows} rows at a pane of \
         {height}, which never reaches below the floor, so the region `MIN_BODY` \
         squeezes hardest went unlooked at"
    );
}

#[test]
fn a_bar_below_the_step_floor_draws_what_it_drew_before() {
    // The other half of the ladder, stated as its own gate because it is the half a
    // reader on a short pane actually sees: below the floor the column is track and
    // thumb, byte for byte what it was before there were buttons.
    const TRACK: &str = "│";
    const THUMB: &str = "█";
    let width = 64u16;
    let height = 24u16;
    // Three listed rows over thirty files: a bar, and a region one row short of
    // the floor.
    let view = View {
        total_rows: 4_000,
        rows_above: 0,
        ..a_list_of(30, (STEP_FLOOR - 1) as usize, 0)
    };
    let backend = screen(width, height, &view, &chrome());
    let seen = regions(Rect::new(0, 0, width, height), &chrome(), &view);

    assert_eq!(
        seen.list.rows,
        STEP_FLOOR - 1,
        "the fixture is not below the floor"
    );
    assert!(has_bar(&backend, seen.list), "the short list drew no bar");
    assert_eq!(
        seen.list.track,
        (seen.list.top, seen.list.rows),
        "a bar with no buttons must offer its whole region as track"
    );
    for y in seen.list.top..seen.list.top + seen.list.rows {
        let glyph = bar_at(&backend, y);
        assert!(
            glyph == TRACK || glyph == THUMB,
            "row {y} of a bar below the step floor draws {glyph:?}"
        );
    }
}

#[test]
fn both_regions_reach_the_step_buttons_through_one_drawer() {
    // Asserted rather than assumed.
    let width = 64u16;
    let view = a_stepped_screen();
    let chrome = chrome();

    // A pane whose two regions are the same height, found rather than computed.
    let (backend, seen, height) = (10u16..=40)
        .find_map(|height| {
            let backend = screen(width, height, &view, &chrome);
            let seen = regions(Rect::new(0, 0, width, height), &chrome, &view);
            (seen.list.rows == seen.diff.rows && seen.list.rows >= STEP_FLOOR)
                .then_some((backend, seen, height))
        })
        .expect("no pane height gives the two regions equal height above the floor");

    for (name, region) in [("the list", seen.list), ("the diff", seen.diff)] {
        assert_eq!(
            bar_at(&backend, region.top),
            STEP_UP,
            "at {height} rows of pane, {name}'s {}-row bar has no up button",
            region.rows
        );
        assert_eq!(
            bar_at(&backend, region.top + region.rows - 1),
            STEP_DOWN,
            "at {height} rows of pane, {name}'s {}-row bar has no down button",
            region.rows
        );
    }
}

#[test]
fn the_diff_scrollbar_reaches_the_bottom_at_its_last_screenful() {
    // The same invariant on the other region, where the units are rows rather
    // than files. One file, the viewport resting on its last screenful.
    let width = 64u16;
    let height = 24u16;

    for span in 20..=60usize {
        let mut view = a_list_of(3, 3, 0);
        view.files = 1;
        view.current_span = span;
        // Asked of the layout for the reason
        // `the_diff_scrollbar_is_proportional_to_the_rows_it_shows` gives: the
        // regions above the diff are the body split's business, not this gate's.
        let laid = regions(Rect::new(0, 0, width, height), &chrome(), &view);
        let region = laid.diff.top..laid.diff.top + laid.diff.rows;
        let rows = usize::from(region.end - region.start);
        // The track, not the region.
        let track = stepped_track(region.clone());
        // The viewport actually on its last screenful, which this fixture never was.
        // Its rows are filled to the region for the reason
        // `the_diff_scrollbar_is_proportional_to_the_rows_it_shows` gives: a screenful
        // is the rows of the diff on screen, so `rows_above` below only means *the
        // last screenful* on a screen that holds one.
        view.rows = (0..rows)
            .map(|i| line(LineKind::Context, 38 + i as u32, "fn coalesce(&mut self) {"))
            .collect();
        view.total_rows = span;
        view.rows_above = span.saturating_sub(rows);
        view.top = Position {
            file: 0,
            row: span.saturating_sub(rows),
        };

        let backend = screen(width, height, &view, &chrome());
        let marks = thumb_rows(&backend, width - 1, region.clone());
        if marks.is_empty() {
            continue; // nothing to scroll at this span
        }
        assert_eq!(
            *marks.last().expect("a thumb"),
            track.end - 1,
            "span {span}: the last screenful's thumb ends at {:?}, not the \
             bottom of the track",
            marks.last()
        );
    }
}

#[test]
fn the_caret_does_not_vanish_because_another_file_changed() {
    // Two ladders that collide.
    let mut ever = false;
    for width in 1..=60u16 {
        let mut drawn = Vec::new();
        for files in [3usize, 30] {
            let view = a_list_of(files, 3, 0);
            let laid = regions(Rect::new(0, 0, width, 24), &chrome(), &view);
            let backend = screen(width, 24, &view, &chrome());
            let buffer = backend.buffer();
            drawn.push(
                (0..width)
                    .map(|x| buffer[(x, laid.list.top)].symbol())
                    .collect::<String>()
                    .contains(CARET),
            );
        }
        ever |= drawn[0] || drawn[1];
        assert_eq!(
            drawn[0],
            drawn[1],
            "at {width} columns the caret is {} with three changed files and {} \
             with thirty, so a file appearing elsewhere moved the marker",
            if drawn[0] { "drawn" } else { "absent" },
            if drawn[1] { "drawn" } else { "absent" }
        );
    }

    // Or the sweep agreed about an absence. Two screens that both draw no
    // caret satisfy the equality above at every width, which is exactly how this
    // gate spent two phases proving nothing.
    assert!(
        ever,
        "no width in the sweep drew a caret on either screen, so the equality \
         above compared two absences"
    );
}

#[test]
fn a_row_keeps_its_floor_after_both_the_bar_and_the_caret() {
    // What `affords_caret`'s `BAR_WIDTH` term buys, which is not the same property as
    // `the_caret_does_not_vanish_because_another_file_changed`.
    const ROW_FLOOR: usize = 2 + 12; // the kind letter and its gap, plus MIN_PATH_WIDTH
    const BAR_COLUMNS: usize = 2;
    const CARET_GLYPH: usize = 1;
    const TRACK: &str = "│";
    const THUMB: &str = "█";

    // Constants restated for the reason this file always restates them: sharing the
    // renderer's own would make the assertion agree with the code by construction.
    let caret_columns = |width: u16| CARET_GLYPH.saturating_sub(usize::from(inset_at(width)));

    let mut saw_both = false;
    for width in 1..=60u16 {
        // Thirty files over three rows, so the list is scrollable and the bar is
        // drawn wherever the pane can afford one.
        let view = a_list_of(30, 3, 0);
        let backend = screen(width, 24, &view, &chrome());
        let buffer = backend.buffer();

        // The list's first row, from the layout: row one is masthead air on
        // any pane that affords a band, and the band draws no caret.
        let laid = regions(Rect::new(0, 0, width, 24), &chrome(), &view);
        let row: String = (0..width)
            .map(|x| buffer[(x, laid.list.top)].symbol())
            .collect();
        let caret = row.contains(CARET);
        let bar = row.ends_with(TRACK) || row.ends_with(THUMB);

        if !caret {
            continue;
        }
        if bar {
            saw_both = true;
        }
        // The pane's inset comes off before anything else does.
        let left = usize::from(width)
            - if bar { BAR_COLUMNS } else { 0 }
            - caret_columns(width)
            - usize::from(inset_at(width));
        assert!(
            left >= ROW_FLOOR,
            "at {width} columns the row draws a caret{} leaving {left} columns, \
             below the {ROW_FLOOR} it needs to name its file",
            if bar { " and a bar," } else { "," }
        );
    }

    assert!(
        saw_both,
        "no width drew a caret and a bar together, so the term this gate is \
         about is never exercised"
    );
}

#[test]
fn render_never_writes_outside_its_area_over_a_degenerate_view() {
    // `any_area_renders_including_the_ones_that_fit_nothing` sweeps pane sizes but only
    // over `one_file()`, whose list is empty and whose `current_span` is zero — so the
    // three fields this branch added are never degenerate in it.
    let shapes: Vec<View> = vec![
        a_list_of(0, 0, 0),
        a_list_of(1, 1, 0),
        a_list_of(6, 6, 0),
        a_list_of(7, 6, 1),
        a_list_of(10_000, 6, 9_994),
        // A window past the end, which a short pane hands back untouched.
        a_list_of(3, 3, 99),
        // More entries than any pane affords.
        a_list_of(40, 20, 0),
        // Nothing to be inside, so the diff bar is told a whole of zero.
        View {
            current_span: 0,
            total_rows: 0,
            rows_above: 0,
            ..a_list_of(9, 3, 0)
        },
        // A path of wide glyphs, at the caret and bar boundary.
        View {
            list_span: 1,
            grouped: false,
            list: vec![entry("src/日本語/テスト.rs", 3, 1).into()],
            ..a_list_of(9, 1, 0)
        },
    ];

    for (shape, view) in shapes.iter().enumerate() {
        for width in 0..=44u16 {
            for height in 0..=14u16 {
                for origin in [(0u16, 0u16), (3, 2)] {
                    let area = Rect::new(origin.0, origin.1, width, height);
                    let mut buf = ratatui::buffer::Buffer::empty(area);
                    // Panics inside ratatui if anything writes out of range.
                    render(
                        &mut buf,
                        area,
                        view,
                        &Theme::default(),
                        Glyphs::default(),
                        &chrome(),
                    );
                    assert_eq!(
                        *buf.area(),
                        area,
                        "shape {shape} at {width}x{height} resized its own buffer"
                    );
                }
            }
        }
    }
}

/// A removed line's band runs under the scrollbar's own column.
fn washed_screen(width: u16, height: u16, view: &View, chrome: &Chrome) -> TestBackend {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
    let theme = vigia::Theme::dark();
    terminal
        .draw(|f| {
            let area = f.area();
            vigia::render(
                f.buffer_mut(),
                area,
                view,
                &theme,
                Glyphs::default(),
                chrome,
            );
        })
        .expect("draw");
    terminal.backend().clone()
}

#[test]
fn a_wash_runs_under_the_scrollbar_column() {
    let width = 64u16;
    let view = View {
        total_rows: 400,
        rows_above: 40,
        rows: vec![
            file("src/engine/watch.rs", 42, 7),
            Row::Hunk {
                old_start: 38,
                old_lines: 8,
                new_start: 38,
                new_lines: 9,
            },
            line(
                LineKind::Removed,
                38,
                "    let stale = self.pending.take();",
            ),
            line(LineKind::Context, 39, "    if self.pending.is_empty() {"),
        ],
        ..two_regions(1)
    };
    let backend = washed_screen(width, 18, &view, &chrome());
    let buffer = backend.buffer();

    // The washed row, found by its content rather than by a hardcoded y: the
    // regions above it move when the list's height rule does.
    let washed = (0..18u16)
        .find(|y| row_text(&backend, *y).contains("let stale"))
        .expect("the removed line was not drawn at all");

    // The wash sampled from the row's trailing blank, not from column 1: the gutter's
    // cells carry their own two-tone background, and at this width column 1 is the
    // gutter's first cell.
    let wash = buffer[(width - 3, washed)].bg;
    let bar = buffer[(width - 1, washed)].bg;
    assert_ne!(
        wash,
        ratatui::style::Color::Reset,
        "the removed line was not washed at all, so this gate proves nothing"
    );
    assert_eq!(
        bar,
        wash,
        "the band stops before the scrollbar's own column at x={}, so the bar \
         stands in a notch of pane background on every changed row",
        width - 1
    );

    // And the track's own colour survived the wash, which is the half the
    // whole fix rests on and the half a symbol check cannot reach.
    assert_eq!(
        buffer[(width - 1, washed)].fg,
        vigia::Theme::dark()
            .bar_track
            .fg
            .expect("dark's track has a colour"),
        "the wash overwrote the track's foreground, so the band was painted over \
         the bar rather than behind it"
    );

    // The glyph too, which is a different claim kept for a different reason.
    assert_eq!(
        buffer[(width - 1, washed)].symbol().chars().next(),
        Some(BAR_GLYPHS[0]),
        "the bar column carries the band but no track glyph, so something drew \
         content into the column the bar owns"
    );

    // And the gap beside it *is* washed, which the tempting assertion has the other way
    // round.
    assert_eq!(
        buffer[(width - 2, washed)].bg,
        wash,
        "the reserved column beside the bar is unwashed, so the band stops a column short and reads as notched"
    );

    // The reserve itself still holds, which is the half that must not be lost
    // in widening the wash: the cell carries no glyph, so a thumb still cannot
    // sit flush against a count.
    assert_eq!(
        buffer[(width - 2, washed)].symbol(),
        " ",
        "the column reserved beside the bar drew a glyph"
    );
}

/// Every row the bar draws on carries that row's own background, buttons included.
#[test]
fn every_row_of_the_bar_carries_its_own_rows_background() {
    let changed = |n: u32| {
        [
            line(LineKind::Added, n, "    let fresh = self.pending.take();"),
            line(LineKind::Removed, n, "    let stale = self.pending.take();"),
        ]
    };
    let many: Vec<Row> = (38..58).flat_map(changed).collect();

    // Two fixtures, and the second exists only to wash a button.
    let mut heading = vec![file("src/engine/watch.rs", 42, 7)];
    heading.extend(many.iter().cloned());

    let fixtures = [
        ("opening on a heading", heading),
        ("opening mid-file", many),
    ];

    let width = 64u16;

    /// The shortest track that can express more than one position, mirroring
    /// `render.rs`'s `MIN_TRACK`.
    const MIN_BAR_CELLS: usize = 2;

    for (what, rows) in fixtures {
        let view = View {
            total_rows: 400,
            rows_above: 40,
            rows,
            ..two_regions(1)
        };

        // Two heights, and the short one is the point. `Bar::Stepped` only
        // appears above `STEP_FLOOR`; below it the bar is bare, which is a different
        // draw path and the one the gate above never exercised.
        for (height, expect_buttons) in [(11u16, false), (18, true)] {
            let backend = washed_screen(width, height, &view, &chrome());
            let buffer = backend.buffer();

            let mut bar_rows = 0usize;
            let mut washed_rows = 0usize;
            let mut saw_buttons = false;
            let mut washed_buttons = 0usize;

            for y in 0..height {
                let cell = &buffer[(width - 1, y)];
                let Some(glyph) = cell.symbol().chars().next() else {
                    continue;
                };
                if !BAR_GLYPHS.contains(&glyph) {
                    continue;
                }
                bar_rows += 1;

                // That row's own background, read from a cell the row owns rather
                // than restated from the palette.
                let behind = buffer[(width - 3, y)].bg;
                let button = glyph == BAR_GLYPHS[2] || glyph == BAR_GLYPHS[3];
                saw_buttons |= button;
                if behind != ratatui::style::Color::Reset {
                    washed_rows += 1;
                    if button {
                        washed_buttons += 1;
                    }
                }

                assert_eq!(
                    cell.bg, behind,
                    "{what} at {width}x{height}: the bar's cell on row {y} is \
                     {:?} where the row behind it is {:?}, so the band does not \
                     reach the bar on every row it crosses",
                    cell.bg, behind
                );
            }

            // Non-vacuity in two directions: the sweep must find a drawn bar at
            // all, and at least one of its rows must actually carry a wash, or
            // every comparison above was `Reset` against `Reset`.
            assert!(
                bar_rows >= MIN_BAR_CELLS,
                "{what} at {width}x{height}: the sweep found {bar_rows} bar rows, \
                 so it is not reading a drawn scrollbar and proves nothing"
            );
            assert!(
                washed_rows > 0,
                "{what} at {width}x{height}: no row the bar crosses was washed, so \
                 every comparison above was Reset against Reset"
            );

            // The shape this height was chosen for.
            assert_eq!(
                saw_buttons, expect_buttons,
                "{what} at {width}x{height}: step buttons present={saw_buttons} \
                 where this height was chosen to draw present={expect_buttons}, so \
                 the two heights no longer cover the two bar shapes"
            );

            // Per height, not once at the end.
            if expect_buttons {
                assert!(
                    washed_buttons > 0,
                    "{what} at {width}x{height}: a stepped bar drew no button on a \
                     washed row, so the case round 1 of #239's audit named is not \
                     covered by this fixture any more"
                );
            }
        }
    }
}

/// A row wash's modifier never reaches the scrollbar's cell.
#[test]
fn a_row_washs_modifier_never_reaches_the_scrollbar() {
    use ratatui::style::Modifier;

    let view = View {
        total_rows: 400,
        rows_above: 40,
        rows: vec![
            file("src/engine/watch.rs", 42, 7),
            Row::Hunk {
                old_start: 38,
                old_lines: 8,
                new_start: 38,
                new_lines: 9,
            },
            line(
                LineKind::Removed,
                38,
                "    let stale = self.pending.take();",
            ),
            line(LineKind::Context, 39, "    if self.pending.is_empty() {"),
        ],
        ..two_regions(1)
    };

    // The palette a reader could actually write, built from the shipped one so the
    // only difference is the thing under test.
    let mut theme = vigia::Theme::dark();
    theme.removed_row = theme.removed_row.add_modifier(Modifier::REVERSED);

    let width = 64u16;
    let height = 18u16;
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
    terminal
        .draw(|f| {
            let area = f.area();
            vigia::render(
                f.buffer_mut(),
                area,
                &view,
                &theme,
                Glyphs::default(),
                &chrome(),
            );
        })
        .expect("draw");
    let backend = terminal.backend().clone();
    let buffer = backend.buffer();

    let washed = (0..height)
        .find(|y| row_text(&backend, *y).contains("let stale"))
        .expect("the removed line was not drawn at all");

    // Non-vacuity: the modifier must actually have landed on the row, or this gate
    // is asserting that a thing nobody applied did not spread.
    assert!(
        buffer[(1, washed)].modifier.contains(Modifier::REVERSED),
        "the row itself is not reversed, so the wash never carried the modifier and \
         this gate proves nothing"
    );

    assert!(
        !buffer[(width - 1, washed)]
            .modifier
            .contains(Modifier::REVERSED),
        "the row wash's REVERSED reached the scrollbar's own cell, which swaps the \
         track against its background and undoes every contrast ratio the palette \
         gates prove"
    );
}

/// A bar style's own background wins over the band, and no background yields to it.
#[test]
fn a_bar_styles_own_background_wins_over_the_band() {
    fn bar_bg_on_a_washed_row(
        theme: &vigia::Theme,
    ) -> (ratatui::style::Color, ratatui::style::Color) {
        let view = View {
            total_rows: 400,
            rows_above: 40,
            rows: vec![
                file("src/engine/watch.rs", 42, 7),
                Row::Hunk {
                    old_start: 38,
                    old_lines: 8,
                    new_start: 38,
                    new_lines: 9,
                },
                line(
                    LineKind::Removed,
                    38,
                    "    let stale = self.pending.take();",
                ),
                line(LineKind::Context, 39, "    if self.pending.is_empty() {"),
            ],
            ..two_regions(1)
        };
        let (width, height) = (64u16, 18u16);
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
        terminal
            .draw(|f| {
                let area = f.area();
                vigia::render(
                    f.buffer_mut(),
                    area,
                    &view,
                    theme,
                    Glyphs::default(),
                    &chrome(),
                );
            })
            .expect("draw");
        let backend = terminal.backend().clone();
        let y = (0..height)
            .find(|y| row_text(&backend, *y).contains("let stale"))
            .expect("the removed line was not drawn at all");
        (
            backend.buffer()[(width - 1, y)].bg,
            // The trailing blank rather than column 1: the gutter carries its
            // own tone, and column 1 is a gutter cell at this width.
            backend.buffer()[(width - 3, y)].bg,
        )
    }

    // Shipped: no background on the bar, so the band shows through.
    let plain = vigia::Theme::dark();
    let (bar, wash) = bar_bg_on_a_washed_row(&plain);
    assert_ne!(
        wash,
        ratatui::style::Color::Reset,
        "the removed row was not washed, so neither half of this gate proves anything"
    );
    assert_eq!(
        bar, wash,
        "a bar style with no background of its own did not take the band's"
    );

    // A theme that declares one: it wins, and the band stops under that column.
    let opaque = ratatui::style::Color::Rgb(0x21, 0x26, 0x2d);
    let mut gutter = vigia::Theme::dark();
    gutter.bar_track = gutter.bar_track.bg(opaque);
    let (bar, wash) = bar_bg_on_a_washed_row(&gutter);
    assert_ne!(
        wash, opaque,
        "the fixture's wash is the same colour as the gutter under test, so the \
         assertion below cannot tell them apart"
    );
    assert_eq!(
        bar, opaque,
        "a bar style declaring a background did not draw it, so the value would be \
         authored, readable in a theme file, and silently dropped"
    );
}

#[test]
fn the_follow_marker_is_green_where_the_word_beside_it_is_dim() {
    // `assets/preview.svg` draws `follow ` in `.dim` and `▶` in `.grn`, and §5.1's rule
    // is that a published artifact answering a question is the answer.
    let view = one_file();
    let theme = Theme::default();
    let backend = screen(80, 6, &view, &following_chrome());

    let at = column_of(&backend, 5, "▶");
    let mark = backend.buffer()[(at, 5)].style();
    let word = backend.buffer()[(at - 2, 5)].style();

    assert_eq!(
        mark.fg, theme.added.fg,
        "the follow marker is not the picture's green"
    );
    assert_eq!(
        word.fg, theme.chrome_dim.fg,
        "the word beside the marker is not the footer's dim grey, so the split \
         the picture draws is gone"
    );
    // Both directions, or a footer painted green throughout would pass.
    assert_ne!(
        theme.added.fg, theme.chrome_dim.fg,
        "the theme draws the marker and the word alike, so this cannot tell them \
         apart"
    );
}

#[test]
fn the_readouts_are_coloured_and_their_label_is_not() {
    // `assets/preview.svg` draws `0.8ms` and `24MiB` in `.cyn` and the word `frame`
    // beside them in `.dim`.
    let theme = Theme::default();
    let backend = screen(80, 6, &one_file(), &diagnostics_chrome());
    let footer = row_text(&backend, 5);

    // Guard the fixture: the readouts have to be on this screen at all.
    assert!(
        footer.contains("0.8ms") && footer.contains("19MiB"),
        "the fixture drew no readouts, so this proves nothing: {footer:?}"
    );

    for (label, needle) in [("frame time", "0"), ("memory", "9")] {
        let at = column_of(&backend, 5, needle);
        assert_eq!(
            backend.buffer()[(at, 5)].style().fg,
            theme.chrome.fg,
            "the {label} readout is not the picture's cyan: {footer:?}"
        );
    }

    // The label beside the frame number, which must stay dim.
    let number = column_of(&backend, 5, "0");
    let at = (number..80)
        .find(|&x| backend.buffer()[(x, 5)].symbol() == "f")
        .expect("the `frame` label is drawn after the number it labels");
    assert!(
        at > number,
        "the label was found at or before the readout, so this is reading the \
         hint bar again: {footer:?}"
    );
    assert_eq!(
        backend.buffer()[(at, 5)].style().fg,
        theme.chrome_dim.fg,
        "the `frame` label took the readout's colour, so the split the picture \
         draws is gone: {footer:?}"
    );
    assert_ne!(
        theme.chrome.fg, theme.chrome_dim.fg,
        "the theme draws the readout and its label alike, so this cannot tell \
         them apart"
    );
}

#[test]
fn the_follow_marker_is_the_last_character_of_the_state() {
    // `FOLLOW_MARK` is restated beside `FOLLOWING` rather than composed into it,
    // because `concat!` takes no `char`.
    let backend = screen(80, 6, &one_file(), &following_chrome());
    let footer = row_text(&backend, 5);
    let mark = footer
        .split_whitespace()
        .find(|word| word.contains('▶'))
        .unwrap_or_else(|| panic!("the footer carries no marker at all: {footer:?}"));
    assert!(
        mark.ends_with('▶'),
        "the marker is no longer the last character of the state's own token, \
         so `FOLLOWING` and `FOLLOW_MARK` have drifted apart: {mark:?} in \
         {footer:?}"
    );
    // The token is the mode word's, not something else that happens to carry the
    // glyph, or a notice could satisfy this.
    assert_eq!(
        mark, "▶",
        "the marker is not standing alone after the mode word: {footer:?}"
    );
}

#[test]
fn an_over_magnitude_readout_is_tinted_whole_and_terminates() {
    // This one gates a hang, not a colour, and it is the more valuable half.
    let theme = Theme::default();
    for (what, chrome, sigil) in [
        (
            "a frame over a second",
            Chrome {
                pressed: None,
                gripped: None,
                scrolling: None,
                frame: Some(Duration::from_secs(2)),
                ..diagnostics_chrome()
            },
            ">1s",
        ),
        (
            "memory over a gigabyte",
            Chrome {
                pressed: None,
                gripped: None,
                scrolling: None,
                memory: Some(2 * 1024 * 1024 * 1024),
                notes: (0, 0),
                ..diagnostics_chrome()
            },
            ">1GiB",
        ),
    ] {
        let backend = screen(80, 6, &one_file(), &chrome);
        let footer = row_text(&backend, 5);
        // Guard the fixture, or a readout that silently stopped being drawn
        // would make this vacuous.
        assert!(
            footer.contains(sigil),
            "{what} drew no {sigil:?} readout, so this proves nothing: {footer:?}"
        );

        // The sigil opens the run, so it and every column of the abbreviation after it
        // carry the measurement's colour.
        let at = column_of(&backend, 5, ">");
        for (offset, glyph) in sigil.chars().enumerate() {
            let x = at + offset as u16;
            assert_eq!(
                backend.buffer()[(x, 5)].symbol().chars().next(),
                Some(glyph),
                "{what} draws {sigil:?} broken up: {footer:?}"
            );
            assert_eq!(
                backend.buffer()[(x, 5)].style().fg,
                theme.chrome.fg,
                "{what} leaves {glyph:?} of {sigil:?} outside the readout's \
                 colour: {footer:?}"
            );
        }
    }
}

#[test]
fn a_notice_can_never_colour_the_follow_marker() {
    // A notice is an error string, an error string carries a path, and `▶` is a legal
    // character in a path on every platform this ships to.
    let theme = Theme::default();
    for following in [false, true] {
        let chrome = Chrome {
            pressed: None,
            gripped: None,
            scrolling: None,
            notice: Some("cannot read ▶.rs".to_owned()),
            following,
            ..diagnostics_chrome()
        };
        for width in 20..=120u16 {
            let backend = screen(width, 8, &one_file(), &chrome);
            let green: Vec<u16> = (7..8)
                .flat_map(|y| (0..width).map(move |x| (x, y)))
                .chain((6..7).flat_map(|y| (0..width).map(move |x| (x, y))))
                .filter(|&(x, y)| {
                    backend.buffer()[(x, y)].symbol() == "▶"
                        && backend.buffer()[(x, y)].style().fg == theme.added.fg
                })
                .map(|(x, _)| x)
                .collect();
            assert!(
                green.len() <= 1,
                "at {width} columns with following={following} more than one \
                 marker is green: {green:?}"
            );
            if following {
                // Exactly one, not at most one.
                assert_eq!(
                    green.len(),
                    1,
                    "at {width} columns follow is on and exactly one marker \
                     should be green: {green:?}"
                );
            } else {
                assert!(
                    green.is_empty(),
                    "at {width} columns a notice's own glyph was painted as the \
                     follow marker while follow is off: {green:?}"
                );
            }
        }
    }
}

#[test]
fn the_position_keeps_its_grey_where_the_readouts_take_a_colour() {
    // §11.1 rules that `N/M` stays dim: the picture gives it no colour, and it is a
    // *place* rather than a measurement, which is the whole distinction the footer's
    // three colours draw.
    let theme = Theme::default();
    let backend = screen(80, 6, &one_file(), &diagnostics_chrome());
    let footer = row_text(&backend, 5);

    // Guard the fixture: the position and the readouts must both be on screen,
    // or this proves nothing about the boundary between them.
    assert!(
        footer.contains("1/1") && footer.contains("0.8ms"),
        "the fixture drew no position beside a readout: {footer:?}"
    );

    let slash = column_of(&backend, 5, "/");
    for (offset, what) in [(-1i32, "the file number"), (1, "the file count")] {
        let x = (i32::from(slash) + offset) as u16;
        assert_eq!(
            backend.buffer()[(x, 5)].style().fg,
            theme.chrome_dim.fg,
            "{what} of the position took the readouts' colour, so the tint \
             reached past the diagnostics: {footer:?}"
        );
    }
    assert_ne!(
        theme.chrome.fg, theme.chrome_dim.fg,
        "the theme draws the readout and the position alike, so this cannot \
         tell them apart"
    );
}

#[test]
fn a_diff_taller_than_the_pane_keeps_its_line_numbers() {
    // The fixed-slot ruling one element over, and on the rows a reader actually reads.
    let body = |total_rows: usize| View {
        rows: vec![
            Row::file(listed("src/engine/watch.rs", 42, 7)),
            line(LineKind::Added, 258, "    pub fn advance(&mut self) {"),
        ],
        list_span: 1,
        grouped: false,
        list: Vec::new(),
        files: 1,
        total_rows,
        rows_above: 0,
        ..ragged_counts()
    };

    let mut compared = 0usize;
    for width in 20..=120u16 {
        let flat = row_text(&screen(width, 8, &body(2), &chrome()), 2);
        let deep = row_text(&screen(width, 8, &body(10_000), &chrome()), 2);
        if !flat.contains("258") {
            continue;
        }
        compared += 1;
        assert!(
            deep.contains("258"),
            "at {width} columns a diff taller than the pane lost its line numbers entirely, so the gutter is sized from the diff's height:              {flat:?} became {deep:?}"
        );
    }
    assert!(
        compared > 40,
        "only {compared} widths drew the line number at all, so this swept over rows with no gutter to lose"
    );
}

#[test]
fn render_clips_to_the_buffer_rather_than_the_area() {
    // `render`'s own contract is that any area is legal, and most writers here reach
    // the cells through `Buffer::set_stringn` or `set_style`, which clip.
    let theme = Theme::default();
    for (buffer, area) in [
        ((40u16, 10u16), (60u16, 10u16)),
        ((40, 10), (200, 10)),
        ((10, 6), (80, 6)),
    ] {
        for view in [
            one_file(),
            View {
                files: 0,
                list_span: 0,
                grouped: false,
                list: Vec::new(),
                rows: Vec::new(),
                ..one_file()
            },
            // A heat strip, a pinned list to put a rule on screen, and enough
            // files to make the list scrollable so the bar is drawn too.
            View {
                list_span: 2,
                grouped: false,
                list: vec![
                    listed("src/engine/watch.rs", 42, 7).into(),
                    listed("src/render/frame.rs", 11, 3).into(),
                ],
                rows: vec![Row::file(listed("src/engine/watch.rs", 42, 7))],
                files: 40,
                total_rows: 4_000,
                ..ragged_counts()
            },
        ] {
            let mut buf = Buffer::empty(Rect::new(0, 0, buffer.0, buffer.1));
            render(
                &mut buf,
                Rect::new(0, 0, area.0, area.1),
                &view,
                &theme,
                Glyphs::default(),
                &chrome(),
            );
        }
    }
}

#[test]
fn the_wash_bleeds_under_the_inset() {
    // The half of the margin ladder that makes the inset design rather than padding,
    // and the half that would be silently lost by an implementation that moved the
    // *wash* instead of the *text*.
    fn washed(width: u16, height: u16, view: &View, chrome: &Chrome) -> TestBackend {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
        let theme = vigia::Theme::dark();
        terminal
            .draw(|f| {
                let area = f.area();
                vigia::render(
                    f.buffer_mut(),
                    area,
                    view,
                    &theme,
                    Glyphs::default(),
                    chrome,
                );
            })
            .expect("draw");
        terminal.backend().clone()
    }

    // Eighty and a hundred and twenty, which are two rungs of the ladder and two
    // of the three widths `SPEC.md` §3 names.
    for width in [80u16, 120] {
        let inset = inset_at(width);
        assert!(
            inset > 0,
            "at {width} columns the pane takes no inset, so this gate has \
             nothing to be about"
        );
        let view = View {
            total_rows: 400,
            rows_above: 40,
            rows: vec![
                file("src/engine/watch.rs", 42, 7),
                Row::Hunk {
                    old_start: 38,
                    old_lines: 8,
                    new_start: 38,
                    new_lines: 9,
                },
                line(
                    LineKind::Removed,
                    38,
                    "    let stale = self.pending.take();",
                ),
                line(LineKind::Context, 39, "    if self.pending.is_empty() {"),
            ],
            ..two_regions(1)
        };
        let backend = washed(width, 18, &view, &chrome());
        let buffer = backend.buffer();

        // Found by its content rather than by a hardcoded y, since the regions
        // above it move when the list's height rule does.
        let row = (0..18u16)
            .find(|y| row_text(&backend, *y).contains("let stale"))
            .expect("the removed line was not drawn at all");

        // Sampled from the trailing blank rather than the first text cell:
        // `(inset, row)` is the gutter's first cell, and that cell carries the
        // two-tone gutter's own background, not the wash.
        let inside = buffer[(width - 3, row)].bg;
        assert_ne!(
            inside,
            ratatui::style::Color::Reset,
            "at {width} columns the removed line was not washed at all, so this \
             gate proves nothing"
        );

        // Column zero is §5.1's left bar, not the wash.
        assert_ne!(
            buffer[(0, row)].bg,
            inside,
            "at {width} columns the pane's leading cell carries the wash rather than the bar"
        );
        assert_ne!(
            buffer[(0, row)].bg,
            ratatui::style::Color::Reset,
            "at {width} columns the pane's leading cell carries nothing, so the band starts a column late"
        );

        for x in 1..inset {
            assert_eq!(
                buffer[(x, row)].bg,
                inside,
                "at {width} columns the wash starts at the text instead of the \
                 pane's edge: column {x} of a washed row carries no band, so the \
                 inset reads as a misaligned highlight"
            );
            assert_eq!(
                buffer[(x, row)].symbol(),
                " ",
                "at {width} columns column {x} carries a glyph, so the text did \
                 not stand back from the edge the wash reaches"
            );
        }
    }
}

#[test]
fn a_row_pays_its_margin_once_and_the_bars_reserve_once() {
    // The premise `planning_width` charges the inset on one side only rests on,
    // measured off a drawn row rather than derived: `SPEC.md` §11.1 rules that a glance
    // row's two trailing columns are the scrollbar's reserve and not a margin, and the
    // ladder adds the matching leading columns rather than a second set of trailing
    // ones.
    let mut read_at: Vec<u16> = Vec::new();
    let mut square_from: Option<u16> = None;
    for width in 43u16..=120 {
        let backend = screen(width, 5, &glancing(), &chrome());
        let row = row_text(&backend, 1);
        // Below the width where the path survives at all there is no heading to
        // read; `the_pane_insets_its_text_at_every_rung` owns those.
        if !row.contains("watch.rs") {
            continue;
        }
        read_at.push(width);

        let leading = row.chars().take_while(|c| *c == ' ').count();
        let trailing = row.chars().rev().take_while(|c| *c == ' ').count();
        assert_eq!(
            (leading, trailing),
            (usize::from(inset_at(width)), 2),
            "at {width} columns a file row stands {leading} columns from the left \
             and {trailing} from the right. The right is the bar's reserve and \
             never a margin (§11.1), so these agreeing is what says the inset was \
             paid once: {row:?}"
        );

        if leading == trailing {
            square_from = Some(square_from.map_or(width, |first: u16| first.min(width)));
        }
    }

    // The narrowest square pane, derived from the sweep rather than recalled.
    assert_eq!(
        square_from,
        Some(79),
        "the narrowest pane whose two blanks match has moved. It is the width \
         where the margin's leading half first reaches the bar's reserve of two, \
         which is the ladder's odd rung and not the wider width where the pair \
         itself completes"
    );

    // Named widths, not a count. A count is satisfied by the wrong widths.
    // This sweep reads 78 of 78 and its first floor was `> 60`, which tolerates
    // seventeen skips: prefixing the skip above with `width < 60 ||` still passed
    // while the whole 43 to 59 band went unread, rungs and all. The rungs are the
    // few widths where the margin's two halves differ, so they are the ones worth
    // naming.
    for rung in [43u16, 44, 79, 80] {
        assert!(
            read_at.contains(&rung),
            "the sweep never read a file heading at {rung} columns, which is a \
             boundary of the margin ladder and one of the few widths where its \
             two halves are not equal"
        );
    }
}

#[test]
fn a_diff_outgrowing_its_pane_does_not_move_the_content_rows_edge() {
    // Gated here because nothing else covers it.

    let mut read_at: Vec<u16> = Vec::new();
    for width in 30u16..=120 {
        // Long enough to reach whatever edge it is given, so the row's rightmost
        // glyph is the edge rather than the end of its text.
        let long = "    let stale = self.pending.take(); ".repeat(12);
        let view = View {
            total_rows: 4000,
            rows_above: 40,
            rows: vec![
                file("src/engine/watch.rs", 42, 7),
                Row::Hunk {
                    old_start: 38,
                    old_lines: 8,
                    new_start: 38,
                    new_lines: 9,
                },
                line(LineKind::Removed, 38, &long),
            ],
            ..two_regions(1)
        };
        let backend = screen(width, 18, &view, &chrome());
        let buffer = backend.buffer();

        // Narrow panes elide the path, so the heading stops naming `watch.rs` and
        // there is nothing to compare against. Skipped rather than asserted, with
        // the counter below standing in for coverage.
        let (Some(heading), Some(content)) = (
            (0..18u16).find(|y| row_text(&backend, *y).contains("watch.rs") && *y > 4),
            (0..18u16).find(|y| row_text(&backend, *y).contains("pending")),
        ) else {
            continue;
        };

        // The whole finding is about the screens where a bar exists, so a width
        // that draws none has nothing to say here.
        let bar_drawn = (0..18u16).any(|y| is_bar_glyph(buffer[(width - 1, y)].symbol()));
        if !bar_drawn {
            continue;
        }
        read_at.push(width);

        // Every glyph the bar can draw, not just track and thumb.
        let last_glyph = |y: u16| {
            (0..width)
                .rev()
                .find(|x| {
                    let symbol = buffer[(*x, y)].symbol();
                    symbol != " " && !is_bar_glyph(symbol)
                })
                .expect("a row with no glyph on it")
        };

        assert_eq!(
            last_glyph(content),
            last_glyph(heading),
            "at {width} columns the content line stops at column {} where the \
             heading in the same region stops at {}, so the pane's trailing \
             margin was charged on top of the scrollbar's own reserve",
            last_glyph(content),
            last_glyph(heading)
        );
    }

    // Named widths, not a count, for the reason
    // `a_row_pays_its_margin_once_and_the_bars_reserve_once` carries in full: the
    // `> 60` floor this replaced tolerated thirty skips out of ninety-one, enough
    // to lose every rung boundary while still reading green.
    for rung in [43u16, 44, 79, 80] {
        assert!(
            read_at.contains(&rung),
            "the sweep never drew a heading, a long content line and a bar \
             together at {rung} columns, which is a boundary of the margin ladder"
        );
    }
}

#[test]
fn the_band_arrives_once_and_a_taller_pane_never_removes_it() {
    // Monotone in height, which is the property a reader feels rather than
    // sees: a pane dragged taller must not lose an element it had, and a
    // threshold written as two comparisons is exactly where that breaks.
    let width = 80u16;
    let view = a_list_of(3, 3, 0);
    let mut arrived: Option<u16> = None;

    for height in 1..=80u16 {
        let body = body_layout(
            Rect::new(0, 0, width, height),
            &chrome(),
            view.files,
            view.files,
        )
        .clamped_to(view.list.len());
        match (arrived, body.graph > 0) {
            (None, true) => arrived = Some(height),
            (Some(at), false) => panic!(
                "the band arrived at {at} rows and was gone again by {height}, so a taller pane lost an element a shorter one had"
            ),
            _ => {}
        }
    }

    // The height, not merely that one exists.
    assert_eq!(
        arrived,
        Some(21),
        "the band arrived at {arrived:?} rather than where the floors add up to"
    );
}

#[test]
fn the_band_never_takes_the_diff_below_a_whole_hunk() {
    // The clamp order, from the diff's side.
    let width = 80u16;
    for files in [1usize, 3, 6, 30] {
        for height in 1..=80u16 {
            let body = body_layout(Rect::new(0, 0, width, height), &chrome(), files, files);
            if body.graph == 0 {
                continue;
            }
            assert!(
                body.diff >= 10,
                "at {width}x{height} over {files} files the band left {} diff rows, under the whole hunk it must not take the pane below:                  {body:?}",
                body.diff
            );
        }
    }
}

#[test]
fn an_empty_window_draws_no_band_at_all() {
    // The track does not reach the band, and this is the gate that says so.
    let width = 80u16;
    let height = 24u16;
    let view = a_list_of(3, 3, 0);
    let backend = screen(width, height, &view, &chrome());
    let body = body_layout(
        Rect::new(0, 0, width, height),
        &chrome(),
        view.files,
        view.files,
    )
    .clamped_to(view.list.len());
    assert!(body.graph > 0, "the fixture reserved no band");

    // Asked of the layout rather than counted, which is this branch's own
    // lesson: `regions` publishes where the list starts, and everything above it
    // is the masthead.
    let laid = regions(Rect::new(0, 0, width, height), &chrome(), &view);
    let buffer = backend.buffer();
    for y in 1..laid.list.top {
        let drawn: String = (0..width)
            .map(|x| buffer[(x, y)].symbol())
            .collect::<String>()
            .trim()
            .to_owned();
        assert!(
            drawn.is_empty(),
            "an empty window drew {drawn:?} on masthead row {y}, so the band draws furniture where it has no data"
        );
    }
}

#[test]
fn hiding_the_masthead_gives_its_rows_to_the_diff() {
    // Reported from use: *"can we add a shortcut to hide and display this thing at the
    // top? I see it is not always needed"*.
    let width = 80u16;
    let height = 24u16;
    let view = a_list_of(3, 3, 0);
    let shown = body_layout(
        Rect::new(0, 0, width, height),
        &chrome(),
        view.files,
        view.files,
    );
    let hidden = body_layout(
        Rect::new(0, 0, width, height),
        &Chrome {
            masthead: false,
            ..chrome()
        },
        view.files,
        view.files,
    );

    assert!(shown.graph > 0, "the fixture drew no masthead to hide");
    assert_eq!(hidden.graph, 0, "hiding the masthead left the band drawn");
    assert_eq!(
        hidden.air, 0,
        "hiding the masthead left its blank row behind"
    );
    // The lead blank is not the masthead's to give.
    assert_eq!(
        (hidden.lead, shown.lead),
        (1, 1),
        "the lead blank moved with the masthead, so the header lost its air"
    );
    assert_eq!(
        hidden.diff,
        shown.diff + shown.band_rows(),
        "the masthead's rows went somewhere other than the diff"
    );
    assert_eq!(
        (hidden.list, hidden.rule),
        (shown.list, shown.rule),
        "hiding the masthead moved the list, which is not what was asked"
    );
}

#[test]
fn a_nameless_worktree_on_a_branch_draws_no_leading_separator() {
    // The seam the separator's guard exists against, reached from a new direction.
    let view = a_list_of(3, 3, 0);
    let mut narrowed = false;
    for worktree in ["", " ", "\u{200b}", "\u{7}"] {
        let chrome = Chrome {
            worktree: worktree.to_owned(),
            staged: None,
            elsewhere: 0,
            branch: Some("main".to_owned()),
            ..chrome()
        };
        for width in 1..=120u16 {
            let header = row_text(&screen(width, 24, &view, &chrome), 0);
            let drawn = header.trim_start();
            if drawn.is_empty() {
                continue;
            }
            narrowed |= !drawn.contains("changed");
            assert!(
                !drawn.starts_with(FACT_JOIN.trim_start()),
                "at {width} columns a worktree drawing nothing put a separator at \
                 the head of the pane: {drawn:?}"
            );
        }
    }
    assert!(
        narrowed,
        "every width drew the widest rung, so the narrower ones this is about \
         were never reached"
    );
}

#[test]
fn a_populated_worktree_names_its_branch_in_the_header() {
    // The always-on rung, on a frame that has a diff in it, which is the
    // case the suite most easily cannot see: a populated fixture that
    // set `branch: None`, so the rung was exercised by one assertion about the
    // empty state and by nothing else at all.
    let width = 80u16;
    let view = a_list_of(3, 3, 0);
    let chrome = Chrome {
        staged: None,
        elsewhere: 0,
        branch: Some("feature/band".to_owned()),
        ..chrome()
    };
    let header = row_text(&screen(width, 24, &view, &chrome), 0);
    assert!(
        header.contains("feature/band"),
        "a populated frame drew no branch: {header:?}"
    );
    assert!(
        header.contains("vigia"),
        "the branch arrived and the worktree name left: {header:?}"
    );
}

// ---------------------------------------------------------------------------
// The staged run: `SPEC.md` §11.2 B17.
// ---------------------------------------------------------------------------

/// A view holding both runs, which is what `a` produces.
fn both_runs() -> View {
    let staged = |path: &str, added: u32, removed: u32| {
        let mut entry = listed(path, added, removed);
        entry.origin = Origin::Staged;
        entry
    };
    View {
        list_span: 6,
        grouped: true,
        // Built the way `list_plan` builds one, separators included, because that
        // is what the region actually draws. `tests/list.rs` is what holds the
        // plan itself; this fixture is the picture of its output.
        list: vec![
            ListRow::Group {
                origin: Origin::Unstaged,
                count: 2,
            },
            listed("src/render.rs", 4, 1).into(),
            listed("src/frame.rs", 11, 3).into(),
            ListRow::Group {
                origin: Origin::Staged,
                count: 2,
            },
            staged("src/render.rs", 42, 7).into(),
            staged("tests/staged.rs", 64, 0).into(),
        ],
        list_top: 0,
        files: 4,
        rows: vec![
            Row::file(listed("src/render.rs", 4, 1)),
            Row::file(staged("tests/staged.rs", 64, 0)),
        ],
        ..nothing_changed()
    }
}

/// The mark, and it is the whole of what tells one run from the other on a row.
#[test]
fn a_staged_rows_kind_letter_carries_the_mark_and_an_unstaged_rows_does_not() {
    let theme = Theme::default();
    // Not vacuous only while the two keys differ. They are *not* the same by
    // design here, where `staged` and `added` are, so this needs no perturbed
    // palette; it needs the guard that says so if that ever changes.
    assert_ne!(
        theme.staged.fg, theme.kind.fg,
        "Theme::staged and Theme::kind hold one colour, so every assertion below          passes whichever key the painter reached for"
    );

    let drawn = screen(80, 12, &both_runs(), &chrome());
    let buf = drawn.buffer();
    let rows = text_rows(&drawn, 80, 12);

    // The kind letter sits `KIND_WIDTH` cells left of the path it labels.
    let letter_fg = |needle: &str| {
        let (y, row) = rows
            .iter()
            .enumerate()
            .find(|(_, row)| row.contains(needle))?;
        let at = row.find(needle).expect("just matched");
        let x = u16::try_from(at.checked_sub(2)?).ok()?;
        buf.cell((x, u16::try_from(y).ok()?))
            .map(|cell| cell.style().fg)
    };

    assert_eq!(
        letter_fg("tests/staged.rs"),
        Some(theme.staged.fg),
        "a staged row's kind letter is not drawn from Theme::staged, so nothing          on the row says which comparison it is:\n{}",
        rows.join("\n")
    );
    assert_eq!(
        letter_fg("src/frame.rs"),
        Some(theme.kind.fg),
        "an unstaged row's kind letter took the staged mark:\n{}",
        rows.join("\n")
    );

    // And the glyph that column would hold is absent from the pane entirely,
    // rather than merely moved somewhere the assertions above cannot see.
    assert!(
        !rows.iter().any(|row| row.contains('\u{2502}')),
        "a gutter bar survives somewhere on the pane:\n{}",
        rows.join("\n")
    );
}

/// The mark takes the staged colour and never the diff's own green.
#[test]
fn the_mark_takes_the_staged_colour_and_never_the_diffs_green() {
    // A perturbed palette, because the shipped one cannot tell the two apart.
    let theme = Theme {
        staged: ratatui::style::Style::new().fg(ratatui::style::Color::Magenta),
        ..Theme::default()
    };
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 12)).expect("terminal");
    let view = both_runs();
    terminal
        .draw(|f| {
            let area = f.area();
            render(
                f.buffer_mut(),
                area,
                &view,
                &theme,
                Glyphs::default(),
                &chrome(),
            );
        })
        .expect("draw");
    let buf = terminal.backend().buffer().clone();

    // The marked cells are the kind letters of the staged rows, found through the
    // path each one labels rather than by scanning for a glyph: the mark has no
    // glyph of its own, which is the whole of the ink ruling.
    let rows = text_rows(terminal.backend(), 80, 12);
    let mut found = 0;
    for (y, row) in rows.iter().enumerate() {
        let Some(at) = row.find("tests/staged.rs").or_else(|| {
            row.contains("src/render.rs")
                .then(|| row.find("src/render.rs"))
                .flatten()
                .filter(|_| row.contains("+42"))
        }) else {
            continue;
        };
        let (Ok(x), Ok(y)) = (u16::try_from(at - 2), u16::try_from(y)) else {
            continue;
        };
        let Some(cell) = buf.cell((x, y)) else {
            continue;
        };
        found += 1;
        assert_eq!(
            cell.style().fg,
            theme.staged.fg,
            "the mark at {x},{y} is not drawn from Theme::staged"
        );
        assert_ne!(
            cell.style().fg,
            theme.added.fg,
            "the mark at {x},{y} took the diff's own green, so the two \
             roles have collapsed into one key"
        );
    }
    assert!(
        found > 0,
        "no staged row reached the screen, so nothing was asserted"
    );
}

/// Drawing both runs costs the path no column at all.
#[test]
fn drawing_both_runs_spends_no_column_on_the_mark() {
    let mut ungrouped = both_runs();
    ungrouped.grouped = false;
    ungrouped.list.retain(|row| {
        row.entry()
            .is_some_and(|entry| entry.origin == Origin::Unstaged)
    });
    ungrouped.files = 2;

    let grouped = text_rows(&screen(80, 12, &both_runs(), &chrome()), 80, 12);
    let plain = text_rows(&screen(80, 12, &ungrouped, &chrome()), 80, 12);

    let path_at = |rows: &[String]| {
        rows.iter()
            .find(|row| row.contains("src/frame.rs"))
            .map(|row| row.find("src/frame.rs").expect("just matched"))
    };
    let (with, without) = (path_at(&grouped), path_at(&plain));
    assert!(
        with.is_some() && without.is_some(),
        "the fixture lost its row"
    );
    assert_eq!(
        with, without,
        "drawing both runs moved the path, so the mark is costing a column again"
    );
}

/// The header names both runs, and the staged half is owed even at zero.
#[test]
fn the_header_counts_both_runs() {
    let with = Chrome {
        staged: Some(2),
        ..chrome()
    };
    let rows = text_rows(&screen(120, 12, &both_runs(), &with), 120, 12);
    assert!(
        rows[0].contains("4 changed") && rows[0].contains("2 staged"),
        "the header does not carry both totals: {:?}",
        rows[0]
    );

    // Zero is drawn, because it is the only acknowledgment pressing `a` on a
    // worktree with nothing staged can give. A key that does nothing a reader can
    // see is the defect B17 is named for, one layer down.
    let empty = Chrome {
        staged: Some(0),
        ..chrome()
    };
    let rows = text_rows(&screen(120, 12, &both_runs(), &empty), 120, 12);
    assert!(
        rows[0].contains("0 staged"),
        "the header drops a staged total of zero, so `a` is a key that says \
         nothing on the tree where saying something matters most: {:?}",
        rows[0]
    );

    // And with the run off there is no second fact at all.
    let rows = text_rows(&screen(120, 12, &both_runs(), &chrome()), 120, 12);
    assert!(
        !rows[0].contains("staged"),
        "the header names the staged run on a pane that is not drawing it: {:?}",
        rows[0]
    );
}

/// The blank pane says where the work went, which is the whole of the
/// report: an agent that stages its own work empties the pane, and
/// `no unstaged changes` reads the same on a clean tree and on a fully staged one.
#[test]
fn an_empty_view_says_where_the_work_went() {
    let signposted = Chrome {
        elsewhere: 3,
        ..empty_chrome()
    };
    let rows = text_rows(&screen(80, 6, &nothing_changed(), &signposted), 80, 6);
    assert!(
        rows[1].contains("no unstaged changes") && rows[1].contains("3 staged"),
        "the empty state does not say where the work went: {:?}",
        rows[1]
    );
}

/// And it says only what is true: a genuinely clean tree gains no second fact.
#[test]
fn an_empty_view_with_nothing_anywhere_says_only_that() {
    let rows = text_rows(&screen(80, 6, &nothing_changed(), &empty_chrome()), 80, 6);
    // `trim` and an equality, not a `contains`.
    assert_eq!(
        rows[1].trim(),
        "no unstaged changes",
        "a clean tree's empty state grew a second fact"
    );

    // With the run on and nothing anywhere, both comparisons are named,
    // because both were asked about and the line has to say it looked.
    let both = Chrome {
        staged: Some(0),
        ..empty_chrome()
    };
    let rows = text_rows(&screen(80, 6, &nothing_changed(), &both), 80, 6);
    assert!(
        rows[1].contains("no staged or unstaged changes"),
        "with both runs drawn the empty state names only one of them: {:?}",
        rows[1]
    );
}

#[test]
fn the_staged_run_at_eighty_columns() {
    insta::assert_snapshot!(screen(
        80,
        12,
        &both_runs(),
        &Chrome {
            staged: Some(2),
            ..chrome()
        }
    ));
}

#[test]
fn the_staged_run_at_a_hundred_and_twenty_columns() {
    insta::assert_snapshot!(screen(
        120,
        12,
        &both_runs(),
        &Chrome {
            staged: Some(2),
            ..chrome()
        }
    ));
}

#[test]
fn the_staged_run_at_forty_columns() {
    // I6's floor, where the gutter gives way before the path does and the run
    // separators are what still carry the fact.
    insta::assert_snapshot!(screen(
        40,
        12,
        &both_runs(),
        &Chrome {
            staged: Some(2),
            ..chrome()
        }
    ));
}

/// The body's split is the same whatever the staged facts say.
#[test]
fn the_layout_is_the_same_whatever_the_staged_facts_say() {
    let view = both_runs();
    let plain = chrome();
    let told = Chrome {
        staged: Some(7),
        elsewhere: 4,
        ..chrome()
    };

    for width in [40u16, 60, 80, 120, 200] {
        for height in 4..=40u16 {
            let at = ratatui::layout::Rect::new(0, 0, width, height);
            let a = body_layout(at, &plain, view.files, view.list.len());
            let b = body_layout(at, &told, view.files, view.list.len());
            assert_eq!(
                a, b,
                "at {width}x{height} the body splits differently once the chrome \
                 carries a staged count, so the layout is reading a field that is \
                 one frame stale when it is asked"
            );
        }
    }
}

/// Like [`washed_screen`], with the palette chosen by the caller: the ladder
/// tests below need a theme already resolved at a depth.
fn themed_screen(
    width: u16,
    height: u16,
    view: &View,
    chrome: &Chrome,
    theme: &vigia::Theme,
) -> TestBackend {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
    terminal
        .draw(|f| {
            let area = f.area();
            vigia::render(f.buffer_mut(), area, view, theme, Glyphs::default(), chrome);
        })
        .expect("draw");
    terminal.backend().clone()
}

/// A removed line whose pair diff marked one word, beside a context line:
/// the fixture every word-emphasis gate below reads.
fn emphasised_view() -> View {
    let text = "    let stale = self.pending.take();";
    let stale = text.find("stale").expect("fixture") as u32;
    View {
        total_rows: 400,
        rows_above: 40,
        rows: vec![
            file("src/engine/watch.rs", 42, 7),
            Row::Hunk {
                old_start: 38,
                old_lines: 8,
                new_start: 38,
                new_lines: 9,
            },
            Row::Line {
                kind: LineKind::Removed,
                number: 38,
                text: text.to_owned(),
                spans: Vec::new(),
                // One range today; the type holds many per line.
                #[allow(clippy::single_range_in_vec_init)]
                emph: vec![stale..stale + 5],
            },
            line(LineKind::Context, 39, "    if self.pending.is_empty() {"),
        ],
        ..two_regions(1)
    }
}

/// `SPEC.md` §11.2 B18: the bytes the pair diff marked take the hotter wash, and
/// the bytes around
/// them keep the row's own.
#[test]
fn a_paired_rows_changed_words_take_the_hotter_wash() {
    let view = emphasised_view();
    let backend = washed_screen(64, 18, &view, &chrome());
    let buffer = backend.buffer();
    let theme = vigia::Theme::dark();

    let row = (0..18u16)
        .find(|y| row_text(&backend, *y).contains("let stale"))
        .expect("the removed line was not drawn");
    let drawn = row_text(&backend, row);
    let at = drawn.find("stale").expect("the word was not drawn") as u16;

    for x in at..at + 5 {
        assert_eq!(
            buffer[(x, row)].bg,
            theme.removed_word.bg.expect("dark declares a word patch"),
            "column {x} of the marked word does not wear the hotter wash"
        );
    }
    let outside = drawn.find("let").expect("fixture") as u16;
    assert_eq!(
        buffer[(outside, row)].bg,
        theme.removed_row.bg.expect("dark declares a wash"),
        "a column outside the marked word lost the row's own wash"
    );
}

/// The two-tone gutter marks the changed row's number cells and leaves a
/// context row's alone.
#[test]
fn the_gutter_tone_marks_changed_rows_and_leaves_context_alone() {
    let view = emphasised_view();
    let backend = washed_screen(64, 18, &view, &chrome());
    let buffer = backend.buffer();
    let theme = vigia::Theme::dark();

    let removed = (0..18u16)
        .find(|y| row_text(&backend, *y).contains("let stale"))
        .expect("the removed line was not drawn");
    let context = (0..18u16)
        .find(|y| row_text(&backend, *y).contains("is_empty"))
        .expect("the context line was not drawn");

    let drawn = row_text(&backend, removed);
    let number = drawn.find("38").expect("the line number was not drawn") as u16;
    assert_eq!(
        buffer[(number, removed)].bg,
        theme
            .removed_gutter
            .bg
            .expect("dark declares a gutter tone"),
        "the changed row's number cells do not wear the tone"
    );
    let drawn = row_text(&backend, context);
    let number = drawn.find("39").expect("the context number was not drawn") as u16;
    assert_eq!(
        buffer[(number, context)].bg,
        ratatui::style::Color::Reset,
        "a context row's gutter took a tone it has no wash to be a tone of"
    );
}

/// Below truecolour the word patch and the gutter tone drop with the wash they
/// belong to, through the same resolve that drops every background: nothing
/// here is a second ladder.
#[test]
fn the_word_patch_and_gutter_tone_drop_out_below_truecolor() {
    let view = emphasised_view();
    let theme = vigia::Theme::dark().resolve(vigia::Depth::Ansi256);
    let backend = themed_screen(64, 18, &view, &chrome(), &theme);
    let buffer = backend.buffer();

    let row = (0..18u16)
        .find(|y| row_text(&backend, *y).contains("let stale"))
        .expect("the removed line was not drawn");
    for x in 0..64u16 {
        assert_eq!(
            buffer[(x, row)].bg,
            ratatui::style::Color::Reset,
            "column {x} kept a background at a depth whose quantiser drops them all"
        );
    }
}

/// The sheet's corners follow the glyph rung, and the dense rungs splice the title into
/// the border btop's way.
#[test]
fn the_sheets_corners_follow_the_glyph_rung() {
    let view = emphasised_view();
    let mut shown = chrome();
    shown.sheet = Some(0);
    let theme = vigia::Theme::dark();

    for (glyphs, arc) in [(Glyphs::Braille, true), (Glyphs::Block, false)] {
        let mut terminal = Terminal::new(TestBackend::new(64, 18)).expect("terminal");
        terminal
            .draw(|f| {
                let area = f.area();
                vigia::render(f.buffer_mut(), area, &view, &theme, glyphs, &shown);
            })
            .expect("draw");
        let backend = terminal.backend().clone();
        let screen: Vec<String> = (0..18u16).map(|y| row_text(&backend, y)).collect();
        let joined = screen.join("\n");
        assert!(
            joined.contains("gestures"),
            "no sheet drew at all, so nothing here is a gate:\n{joined}"
        );
        if arc {
            assert!(
                joined.contains('╭') && joined.contains('╯'),
                "a dense rung drew square corners:\n{joined}"
            );
            assert!(
                joined.contains("┐ gestures"),
                "the dense rung lost the border splice:\n{joined}"
            );
        } else {
            assert!(
                !joined.contains('╭') && joined.contains('┌'),
                "the Block rung drew an arc its console cannot:\n{joined}"
            );
            assert!(
                joined.contains("─ gestures"),
                "the Block rung lost its inline title:\n{joined}"
            );
        }
    }
}

/// File-type icons are opt-in, every row gets one when any does, and off
/// leaves not a single private-use glyph on the screen.
#[test]
fn icons_are_opt_in_and_every_row_gets_one() {
    let view = View {
        rows: vec![
            file("src/engine/watch.rs", 42, 7),
            file("assets/blob.bin", 1, 0),
        ],
        files: 2,
        ..two_regions(2)
    };
    let pua =
        |c: char| ('\u{e000}'..='\u{f8ff}').contains(&c) || ('\u{f0000}'..'\u{ffffe}').contains(&c);

    let off = washed_screen(64, 18, &view, &chrome());
    for y in 0..18u16 {
        assert!(
            !row_text(&off, y).chars().any(pua),
            "icons are off and row {y} still drew a private-use glyph"
        );
    }

    let mut shown = chrome();
    shown.icons = true;
    let mut terminal = Terminal::new(TestBackend::new(64, 18)).expect("terminal");
    let theme = vigia::Theme::dark();
    terminal
        .draw(|f| {
            let area = f.area();
            vigia::render(
                f.buffer_mut(),
                area,
                &view,
                &theme,
                Glyphs::default(),
                &shown,
            );
        })
        .expect("draw");
    let on = terminal.backend().clone();

    let rows: Vec<(u16, String)> = (0..18u16)
        .map(|y| (y, row_text(&on, y)))
        .filter(|(_, t)| t.contains("watch.rs") || t.contains("blob.bin"))
        .collect();
    assert!(rows.len() >= 2, "the fixture drew fewer than two file rows");
    let mut origins = Vec::new();
    for (y, text) in &rows {
        let icon_at = text
            .char_indices()
            .find(|(_, c)| pua(*c))
            .map(|(at, _)| at)
            .unwrap_or_else(|| panic!("row {y} drew no icon with icons on: {text}"));
        origins.push(icon_at);
    }
    assert!(
        origins.windows(2).all(|pair| pair[0] == pair[1]),
        "rows put their icons at different columns, so the paths slide: {origins:?}"
    );
    // The row the table does not know still gets a mark, the generic one.
    let unknown = rows
        .iter()
        .find(|(_, t)| t.contains("blob.bin"))
        .expect("fixture");
    assert!(
        unknown.1.contains(vigia::icons::GENERIC),
        "an unknown extension drew something other than the generic mark"
    );
}

/// A linked path is one cell carrying the whole OSC 8 wrapper with its
/// width forced to the label's, tui-link's shape; rootless or switched off it
/// is exactly the plain cells it always was.
#[test]
fn a_linked_path_is_one_cell_carrying_the_uri() {
    let view = View {
        rows: vec![
            file("src/engine/watch.rs", 42, 7),
            file("src/we ird%.rs", 1, 0),
        ],
        files: 2,
        ..two_regions(2)
    };
    let mut shown = chrome();
    shown.links = true;
    shown.root = "/home/reader/tree".to_owned();
    let theme = vigia::Theme::dark();
    let backend = themed_screen(64, 18, &view, &shown, &theme);
    let buffer = backend.buffer();

    let row = (0..18u16)
        .find(|y| (0..64u16).any(|x| buffer[(x, *y)].symbol().contains("watch.rs")))
        .expect("the linked path was not drawn");
    let (x, cell) = (0..64u16)
        .map(|x| (x, &buffer[(x, row)]))
        .find(|(_, c)| c.symbol().contains("watch.rs"))
        .expect("fixture");
    let symbol = cell.symbol();
    assert!(
        symbol.contains("\x1b]8;;file:///home/reader/tree/src/engine/watch.rs\x1b\\"),
        "the cell does not open the link: {symbol:?}"
    );
    assert!(
        symbol.ends_with("\x1b]8;;\x1b\\"),
        "the cell does not close the link: {symbol:?}"
    );
    // The width the cell forces is the label's: the fixture's label is the
    // path spelled whole, nineteen columns of ASCII.
    assert_eq!(
        cell.diff_option,
        ratatui::buffer::CellDiffOption::ForcedWidth(
            std::num::NonZeroU16::new("src/engine/watch.rs".len() as u16).expect("nonzero")
        ),
        "the forced width is not the label's"
    );
    assert_eq!(
        buffer[(x + 1, row)].symbol(),
        " ",
        "the covered cell behind the link is not blank"
    );

    // Encoding: the space and the percent are escaped, nothing else invented.
    let odd = (0..18u16)
        .flat_map(|y| (0..64u16).map(move |x| (x, y)))
        .map(|(x, y)| buffer[(x, y)].symbol().to_owned())
        .find(|s| s.contains("ird%25.rs"))
        .expect("the odd path was not linked");
    assert!(
        odd.contains("file:///home/reader/tree/src/we%20ird%25.rs"),
        "the URI is not minimally encoded: {odd:?}"
    );

    // And the default fixture, rootless, draws not one escape anywhere.
    let plain = washed_screen(64, 18, &view, &chrome());
    for y in 0..18u16 {
        assert!(
            !row_text(&plain, y).contains('\x1b'),
            "a rootless chrome leaked an escape into row {y}"
        );
    }
}

/// A linked path claims its columns with `ForcedWidth`, and ratatui's differ walks
/// straight past every column that claim covers: it emits the first cell and advances,
/// with none of the shrink protection its `None` arm carries.
#[test]
fn a_linked_paths_covered_columns_record_what_the_terminal_shows() {
    let area = Rect::new(0, 0, 80, 18);
    let long = "src/engine/a-very-long-module-name.rs";
    let short = "src/a.rs";
    let mut shown = chrome();
    shown.links = true;
    shown.root = "/tree".to_owned();
    let theme = vigia::Theme::dark();

    let drawn = |path: &str, added: u32| {
        let view = View {
            rows: vec![file(path, added, 0)],
            files: 1,
            ..two_regions(1)
        };
        let mut buf = Buffer::empty(area);
        vigia::render(&mut buf, area, &view, &theme, Glyphs::default(), &shown);
        buf
    };

    let buf = drawn(long, 42);
    let (at, row) = (0..18u16)
        .flat_map(|y| (0..80u16).map(move |x| (x, y)))
        .find(|at| buf[*at].symbol().contains(long))
        .expect("the long path was not linked at all");
    let claimed = match buf[(at, row)].diff_option {
        ratatui::buffer::CellDiffOption::ForcedWidth(width) => width.get(),
        other => panic!("a linked path claimed nothing: {other:?}"),
    };
    // Every column the claim covers records the character the terminal shows
    // there, so the frame after this one can tell what is on screen.
    let shadow: String = (at + 1..at + claimed)
        .map(|x| buf[(x, row)].symbol())
        .collect();
    let label: String = buf[(at, row)].symbol().to_owned();
    let label = label
        .rsplit_once("\u{1b}]8;;")
        .expect("a closing wrapper")
        .0
        .rsplit_once("\u{1b}\\")
        .expect("an opening wrapper")
        .1
        .to_owned();
    assert_eq!(
        shadow,
        label.chars().skip(1).collect::<String>(),
        "the covered columns do not record the label the terminal is showing"
    );

    // And a shorter path in the same slot leaves nothing of a longer one
    // behind it: blanks in both frames are what the differ cannot see.
    let buf = drawn(short, 1);
    let (at, row) = (0..18u16)
        .flat_map(|y| (0..80u16).map(move |x| (x, y)))
        .find(|at| buf[*at].symbol().contains(short))
        .expect("the short path was not linked at all");
    for x in at + short.chars().count() as u16..at + claimed {
        assert!(
            buf[(x, row)].symbol().trim().is_empty(),
            "column {x} still records a longer path's tail"
        );
    }
}

/// The sheet composites over the regions, so nothing may claim columns inside
/// it: a claim reaching in makes the differ skip the sheet's own cells, and the
/// row underneath shows through. Reported from a real pane.
#[test]
fn nothing_claims_columns_the_sheet_is_drawn_over() {
    let view = View {
        rows: vec![
            file("src/engine/a-very-long-module-name-indeed.rs", 42, 7),
            file("src/render/another-quite-long-name.rs", 11, 3),
        ],
        files: 2,
        ..two_regions(2)
    };
    let mut shown = chrome();
    shown.links = true;
    shown.root = "/tree".to_owned();
    shown.sheet = Some(0);
    let theme = vigia::Theme::dark();
    let backend = themed_screen(120, 30, &view, &shown, &theme);
    let buffer = backend.buffer();

    let sheet = vigia::regions(Rect::new(0, 0, 120, 30), &shown, &view)
        .sheet
        .expect("the fixture pane drew no sheet");
    for y in sheet.top..sheet.top + sheet.height {
        for x in 0..sheet.left {
            let claimed = match buffer[(x, y)].diff_option {
                ratatui::buffer::CellDiffOption::ForcedWidth(width) => u32::from(width.get()),
                _ => 1,
            };
            assert!(
                u32::from(x) + claimed <= u32::from(sheet.left),
                "the cell at {x},{y} claims {claimed} columns and reaches into the \
                 sheet, so the differ will skip the sheet's own cells there"
            );
        }
    }
}

/// The continuation mark says *this line has more than the pane can show*, so it may
/// only ever sit against content that reached it.
#[test]
fn a_continuation_mark_only_sits_against_content_that_reached_it() {
    let theme = vigia::Theme::dark();
    let shown = chrome();
    let width = 60u16;
    let mut terminal = Terminal::new(TestBackend::new(width, 14)).expect("terminal");

    // Lengths either side of the pane's own width, plus multibyte content: a
    // VS16 emoji, an em dash, and a wide CJK character, because each measures
    // differently from its byte length.
    let bodies: Vec<String> = (0..40)
        .map(|i| {
            let filler = "x".repeat(i * 3 % 90);
            match i % 4 {
                0 => format!("plain {filler}"),
                1 => format!("warn \u{26a0}\u{fe0f} {filler}"),
                2 => format!("dash \u{2014} {filler}"),
                _ => format!("wide \u{5e83}\u{5e83} {filler}"),
            }
        })
        .collect();

    let mut marks: Vec<(usize, u16, usize, String)> = Vec::new();
    for start in 0..bodies.len().saturating_sub(5) {
        let rows: Vec<Row> = std::iter::once(file("src/a.rs", 42, 7))
            .chain(std::iter::once(Row::Hunk {
                old_start: 1,
                old_lines: 5,
                new_start: 1,
                new_lines: 5,
            }))
            .chain(
                bodies[start..start + 5]
                    .iter()
                    .enumerate()
                    .map(|(n, body)| {
                        // Real spans, chunked on character boundaries the way a
                        // grammar emits them: the mark is decided while walking
                        // spans, so a fixture with none never reaches the code
                        // that decides it.
                        let mut spans = Vec::new();
                        let mut taken = 0usize;
                        for (at, ch) in body.char_indices() {
                            if at >= taken + 7 {
                                spans.push(Span {
                                    len: at - taken,
                                    class: Class::Plain,
                                });
                                taken = at;
                            }
                            let _ = ch;
                        }
                        if taken < body.len() {
                            spans.push(Span {
                                len: body.len() - taken,
                                class: Class::Keyword,
                            });
                        }
                        Row::Line {
                            kind: LineKind::Added,
                            number: n as u32 + 1,
                            text: body.clone(),
                            spans,
                            emph: Vec::new(),
                        }
                    }),
            )
            .collect();
        let view = View {
            rows,
            files: 1,
            ..two_regions(1)
        };
        terminal
            .draw(|f| {
                let area = f.area();
                vigia::render(
                    f.buffer_mut(),
                    area,
                    &view,
                    &theme,
                    Glyphs::default(),
                    &shown,
                );
            })
            .expect("draw");
        let backend = terminal.backend().clone();

        for y in 0..14u16 {
            let text = row_text(&backend, y);
            let Some(at) = text.find('\u{203a}') else {
                continue;
            };
            let before: String = text[..at].chars().rev().take(2).collect();
            assert!(
                !before.starts_with(' '),
                "at scroll {start}, row {y} shows a continuation mark with blank \
                 space before it, so it survived a line that no longer needs \
                 one:\n{text:?}"
            );

            // The reported symptom is the mark *moving*: every content row is clipped
            // at the same column, so every mark a frame draws has to land in that one
            // column.
            marks.push((start, y, text[..at].chars().count(), text.clone()));
        }
    }

    let first = marks.first().map_or(0, |(_, _, at, _)| *at);
    assert!(
        marks.len() > 20,
        "the loop drew {} marks, too few to be testing the invariant",
        marks.len()
    );
    if let Some((start, y, at, text)) = marks.iter().find(|(_, _, at, _)| *at != first) {
        panic!(
            "continuation marks do not share a column: row {y} at scroll {start} \
             puts one at column {at} where every earlier mark used {first}, so \
             the mark drifts as the reader scrolls:\n{text:?}"
        );
    }
}

/// No emoji presentation selector reaches the buffer.
#[test]
fn no_emoji_presentation_selector_reaches_the_buffer() {
    let theme = vigia::Theme::dark();
    let shown = chrome();
    let mut terminal = Terminal::new(TestBackend::new(60, 14)).expect("terminal");

    let body = "  ? `\u{26a0}\u{fe0f} **Gym cashback**: no row yet in [[x]]";
    let rows = vec![
        file("a.ts", 1, 0),
        Row::Hunk {
            old_start: 1,
            old_lines: 1,
            new_start: 1,
            new_lines: 1,
        },
        Row::Line {
            kind: LineKind::Added,
            number: 47,
            text: body.to_string(),
            spans: Vec::new(),
            emph: Vec::new(),
        },
    ];
    let view = View {
        rows,
        files: 1,
        ..two_regions(1)
    };
    terminal
        .draw(|f| {
            let area = f.area();
            vigia::render(
                f.buffer_mut(),
                area,
                &view,
                &theme,
                Glyphs::default(),
                &shown,
            );
        })
        .expect("draw");
    let backend = terminal.backend().clone();

    let all: Vec<String> = (0..14u16).map(|y| row_text(&backend, y)).collect();
    let drawn = all
        .iter()
        .find(|text| text.contains("Gym"))
        .unwrap_or_else(|| panic!("the emoji row was never drawn:\n{all:#?}"));
    assert!(
        !drawn.contains('\u{fe0f}'),
        "a presentation selector reached the buffer, so `ratatui` claims a \
         second cell for the pair and its diff writes that cell without \
         moving the cursor, shifting every column after it:\n{drawn:?}"
    );
    assert!(
        drawn.contains('\u{26a0}'),
        "the selector was dropped and took its glyph with it:\n{drawn:?}"
    );
}
