//! I6, as assertions rather than as a picture.
//!
//! > **Legible at 40 columns.** No horizontal overflow, no truncated-to-useless
//! > labels.
//!
//! `tests/render.rs` holds the snapshots at 40, 80 and 120 columns. A snapshot
//! records *a* width, which is why they were only ever a baseline: nothing in
//! one says the rule holds at 39, or at 41, and the failures here live exactly
//! at boundaries. So this file sweeps **every width from 1 to 120** and asserts
//! the rule instead.
//!
//! `SPEC.md` §11.1 states the rule these tests are derived from: **a thing made
//! of items breaks, a thing made of characters marks its edge, and content is
//! neither.** Each half gets its own gate, because they fail differently. Too
//! little marking is silent and looks fine; too much is loud and looks broken.
//!
//! Every test that asserts a mark asserts **both directions** — that it appears
//! where the text does not fit and is absent where it does. A rule that only
//! ever fires one way is not a rule, and a gate that only checks the firing
//! direction passes against code that marks everything unconditionally.

use std::time::Duration;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::text::Span;
use vigia::{
    Chrome, FileEntry, HEAT_BUCKETS, HINT_SEPARATOR, HeatBucket, Mode, Position, Row, Theme, View,
    body_layout, diff_height, render,
};
use vigia_core::{HISTORY_BUCKETS, LineKind, Recency};

/// The mark meaning "this continues past the right edge".
const CONTINUES: char = '›';
/// The mark meaning "the beginning of this is gone".
const ELIDED: char = '…';
/// The follow indicator's own glyph, which no hint contains.
///
/// Matched on rather than the word `follow`, because `f follow` is a hint and
/// would make every state assertion pass against a footer showing only advice.
const FOLLOW_MARK: char = '▶';

/// What joins two facts about one subject on a line of chrome.
///
/// Restated rather than imported, for [`CONTINUES`]' reason and one more. The
/// renderer keeps this separate from `HINT_SEPARATOR` on purpose, so that a
/// change to how *hints* are joined cannot silently reshape the header, and a
/// test that reached for the exported one would undo exactly that separation.
const FACT_JOIN: &str = " · ";

/// Widths every sweep covers. One column to well past the widest snapshot.
const WIDTHS: std::ops::RangeInclusive<u16> = 1..=120;

fn theme() -> Theme {
    Theme::default()
}

/// Draw one screen and hand back the backend, which is both a picture and a
/// grid of cells.
fn drawn(width: u16, height: u16, view: &View, chrome: &Chrome) -> TestBackend {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
    let theme = theme();
    terminal
        .draw(|f| {
            let area = f.area();
            render(f.buffer_mut(), area, view, &theme, chrome);
        })
        .expect("draw");
    terminal.backend().clone()
}

/// Draw and hand back the rows as plain strings, trailing blanks trimmed.
///
/// Rebuilt from the cells rather than parsed out of `TestBackend`'s `Display`.
/// That `Display` appends `Hidden by multi-width symbols: [...]` to any row
/// holding a two-column glyph, so every assertion about how a row *ends* was
/// silently reading that note instead of the row. Walking the cells the way a
/// terminal does, skipping what the previous symbol already covered, gives back
/// exactly what a reader would see.
fn rows_at(width: u16, height: u16, view: &View, chrome: &Chrome) -> Vec<String> {
    let backend = drawn(width, height, view, chrome);
    let buffer = backend.buffer();
    (0..height)
        .map(|y| {
            let mut row = String::new();
            let mut covered = 0usize;
            for x in 0..width {
                if covered > 0 {
                    covered -= 1;
                    continue;
                }
                let symbol = buffer[(x, y)].symbol();
                row.push_str(symbol);
                covered = Span::raw(symbol).width().saturating_sub(1);
            }
            row.trim_end().to_owned()
        })
        .collect()
}

/// Columns a rendered row actually occupies.
///
/// A two-column symbol lives in one cell and leaves the next as a blank
/// placeholder, so counting cells over-counts and counting symbols
/// under-counts. The row has to be walked the way a terminal walks it, skipping
/// what the previous symbol already covered.
fn occupied(width: u16, height: u16, view: &View, chrome: &Chrome, y: u16) -> usize {
    let backend = drawn(width, height, view, chrome);
    let buffer = backend.buffer();

    let mut total = 0usize;
    let mut covered = 0usize;
    for x in 0..width {
        let cell = Span::raw(buffer[(x, y)].symbol()).width();
        if covered > 0 {
            covered -= 1;
            continue;
        }
        total += cell;
        covered = cell.saturating_sub(1);
    }
    total
}

/// The eighth-blocks a sparkline is drawn from, restated rather than imported.
///
/// A test sharing the renderer's own table would agree with it by construction.
const RAMP: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// The block one heat slice is drawn as, restated for the same reason as
/// [`RAMP`].
const HEAT_BLOCK: char = '█';

/// Cells on row `y` whose foreground is one of `colours`, in column order.
///
/// **Symbol and colour together, because neither alone separates the two strips
/// any more.** The sparkline's top rung is `█` and so is every heat slice, so a
/// symbol-only match counts one as the other; the heat track is the same dim
/// grey as the `+42 -7` counters, so a colour-only match counts those. Both
/// gates below were symbol-only and one of them silently started counting
/// eighteen buckets the moment the heat strip landed.
fn cells_coloured(
    backend: &TestBackend,
    y: u16,
    colours: &[ratatui::style::Color],
    symbols: &[char],
) -> Vec<ratatui::style::Style> {
    let buffer = backend.buffer();
    (0..buffer.area.width)
        .map(|x| &buffer[(x, y)])
        .filter(|cell| {
            let symbol = cell.symbol();
            symbols.iter().any(|glyph| symbol == glyph.to_string())
                && cell.style().fg.is_some_and(|fg| colours.contains(&fg))
        })
        .map(|cell| cell.style())
        .collect()
}

/// Every foreground the heat strip can draw a slice in.
fn heat_colours(theme: &Theme) -> Vec<ratatui::style::Color> {
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

fn line(kind: LineKind, number: u32, text: &str) -> Row {
    Row::Line {
        kind,
        number,
        text: text.to_owned(),
        spans: Vec::new(),
    }
}

/// The base fixture, and its worktree name is load bearing.
///
/// `vigia` cannot end in any prefix of `watching` or of `not watching`, which is
/// what lets [`the_header_ladder_keeps_the_mode_word_last`] tell a dropped rung
/// from a cut one by looking at how the row ends. A name ending in `w`, `n` or
/// `no` would make that sweep pass against a header that marked the word instead
/// of dropping it.
fn chrome() -> Chrome {
    Chrome {
        worktree: "vigia".to_owned(),
        // Only the empty state names a branch, so every populated fixture leaves
        // this `None`. That is not tidiness: it is what a real frame carries,
        // because `branch_for` never reads HEAD for a frame with a diff in it.
        branch: None,
        mode: Mode::Watching,
        notice: None,
        following: false,
        // Absent in the base fixture, so every sweep that inherits it keeps
        // measuring the chrome it measured before the status readouts existed.
        // [`diagnostics`] is the fixture that carries them, and it is added to
        // [`cases`] rather than replacing anything: a reader on the first paint
        // has no frame time, so both shapes are real screens and both need the
        // width sweep.
        frame: None,
        memory: None,
    }
}

/// The status bar with both readouts on it, which is every frame after the first.
///
/// The values are chosen to be the *widest* each cell can be rather than typical,
/// which is what makes this fixture worth sweeping: `999ms` and `999MiB` are
/// five and six columns, the maximum either can occupy, so a width where this
/// fits is a width where any value fits. A fixture at `0.8ms` and `19MiB` would
/// pass at widths the real thing overflows.
fn diagnostics() -> Chrome {
    Chrome {
        frame: Some(Duration::from_millis(999)),
        memory: Some(999 * 1024 * 1024),
        ..following()
    }
}

fn following() -> Chrome {
    Chrome {
        following: true,
        ..chrome()
    }
}

/// A watch that has stopped, which widens the mode word from 8 columns to 12.
///
/// Swept rather than snapshotted, because the extra four columns move every
/// width at which the header's ladder changes rung. A matrix of one mode word
/// exercises one column-width class and reads as though it covered them all,
/// which is the same trap the hundred-file case exists for below.
fn lost() -> Chrome {
    Chrome {
        mode: Mode::Lost,
        ..chrome()
    }
}

/// A worktree with nothing in it, on a branch.
fn on_a_branch() -> Chrome {
    Chrome {
        branch: Some("main".to_owned()),
        ..chrome()
    }
}

fn with_notice() -> Chrome {
    Chrome {
        notice: Some("the index entry for src/lib.rs points at a missing blob".to_owned()),
        ..following()
    }
}

/// A view carrying one of every row kind, so a sweep covers them all at once.
fn every_row_kind() -> View {
    View {
        list: Vec::new(),
        list_top: 0,
        current_span: 0,
        total_rows: 0,
        rows_above: 0,
        rows: vec![
            Row::File(FileEntry {
                path: "crates/vigia-core/src/frame.rs".to_owned(),
                from: None,
                kind: 'M',
                churn: Some((3, 1)),
                spark: [0; HISTORY_BUCKETS],
                recency: Recency::Cold,
                heat: [HeatBucket::default(); HEAT_BUCKETS],
            }),
            Row::Hunk {
                old_start: 258,
                old_lines: 7,
                new_start: 258,
                new_lines: 9,
            },
            line(LineKind::Context, 258, "    pub fn advance(&mut self) {"),
            line(
                LineKind::Removed,
                260,
                "        for change in self.changes() {",
            ),
            line(LineKind::Added, 260, "        for change in self.walk() {"),
            Row::File(FileEntry {
                path: "assets/banner.jpg".to_owned(),
                from: None,
                kind: 'M',
                churn: None,
                spark: [0; HISTORY_BUCKETS],
                recency: Recency::Cold,
                heat: [HeatBucket::default(); HEAT_BUCKETS],
            }),
            Row::Note("binary"),
            Row::File(FileEntry {
                path: "crates/vigia/src/shell.rs".to_owned(),
                from: Some("crates/vigia/src/main.rs".to_owned()),
                kind: 'R',
                churn: Some((0, 0)),
                spark: [0; HISTORY_BUCKETS],
                recency: Recency::Cold,
                heat: [HeatBucket::default(); HEAT_BUCKETS],
            }),
        ],
        files: 3,
        top: Position::default(),
        read: 3,
        peak: 0,
    }
}

/// A view with content nobody wrote for a display: double-width, and a path
/// longer than any pane.
fn awkward() -> View {
    View {
        list: Vec::new(),
        list_top: 0,
        current_span: 0,
        total_rows: 0,
        rows_above: 0,
        rows: vec![
            Row::File(FileEntry {
                path: "crates/vigia-core/src/very/deeply/nested/module/frame.rs".to_owned(),
                from: None,
                kind: 'M',
                churn: Some((12, 3)),
                spark: [0; HISTORY_BUCKETS],
                recency: Recency::Cold,
                heat: [HeatBucket::default(); HEAT_BUCKETS],
            }),
            line(LineKind::Added, 1, "見出し a 見出し b 見出し c"),
            line(LineKind::Added, 2, "🙂🙂🙂 tail"),
        ],
        files: 1,
        top: Position::default(),
        read: 1,
        peak: 0,
    }
}

fn empty() -> View {
    View {
        list: Vec::new(),
        list_top: 0,
        current_span: 0,
        total_rows: 0,
        rows_above: 0,
        rows: Vec::new(),
        files: 0,
        top: Position::default(),
        read: 0,
        peak: 0,
    }
}

/// `n` uniquely labelled body rows over a diff of `files` files, so the rows
/// actually drawn can be counted.
///
/// Four columns each including the sigil, which is why the sweep that uses this
/// starts at eight rather than one: a marker clipped to nothing cannot be
/// counted, and a test that silently counted zero would pass against a renderer
/// that drew no body at all.
///
/// **`files` is a parameter because the footer's height depends on it**, through
/// the width of the widest position that count can produce. Every fixture here
/// once reported one or three files, and `1/1` and `3/3` are the same width, so
/// a renderer that ignored the count entirely and assumed one file was
/// indistinguishable from a correct one. Found by mutation.
/// `n` numbered content rows, plus a pinned list of `listed` files.
///
/// **`listed` is not optional detail.** `SPEC.md` §11.1 makes the body two
/// regions, and a view carrying files but no list is a screen `View::collect`
/// cannot produce: it fills the list to exactly the height `body_layout` asked
/// for. Handing the renderer one anyway would let a sweep assert against a
/// layout that never ships, which is the vacuous-fixture shape §7 already
/// records twice.
fn numbered(n: usize, files: usize, listed: usize) -> View {
    View {
        list: (0..listed)
            .map(|i| entry(&format!("src/f{i}.rs")))
            .collect(),
        list_top: 0,
        current_span: 0,
        total_rows: 0,
        rows_above: 0,
        rows: (0..n)
            .map(|i| line(LineKind::Added, 1, &format!("R{i:02}")))
            .collect(),
        files,
        top: Position::default(),
        read: 1,
        peak: 0,
    }
}

/// One list entry, with nothing on it but a path.
///
/// The glance elements are gated by their own tests; what these sweeps need is a
/// row of the right *shape* at every width.
fn entry(path: &str) -> FileEntry {
    FileEntry {
        path: path.to_owned(),
        from: None,
        kind: 'M',
        churn: Some((1, 0)),
        spark: [0; HISTORY_BUCKETS],
        recency: Recency::Cold,
        heat: [HeatBucket::default(); HEAT_BUCKETS],
    }
}

/// Whether `row` treats `label` honestly: drawn whole, dropped entirely, or cut
/// with the mark. Silently cut is the one illegal outcome, and it is the shape
/// I6 calls truncated-to-useless.
///
/// Dropped entirely is legal because the right-hand text is placed first: a
/// header at eight columns spends all of them on `watching` and shows no name at
/// all, which `Painter::put_right` documents as deliberate.
fn label_is_honest(row: &str, label: &str) -> bool {
    let common: String = row
        .chars()
        .zip(label.chars())
        .take_while(|(drawn, wanted)| drawn == wanted)
        .map(|(drawn, _)| drawn)
        .collect();
    if common.chars().count() == label.chars().count() || common.is_empty() {
        return true;
    }
    row[common.len()..].starts_with(CONTINUES)
}

/// Every combination a sweep runs over, named so a failure says which.
fn cases() -> Vec<(&'static str, View, Chrome)> {
    // The hundred-file case is not a bigger version of the three-file one. The
    // position it produces is `100/100` rather than `1/3`, which widens the
    // state by four columns and moves every width at which the footer changes
    // shape. A matrix of single-digit counts exercises one column-width class
    // and reads as though it covered them all.
    let many = View {
        list: Vec::new(),
        list_top: 0,
        current_span: 0,
        total_rows: 0,
        rows_above: 0,
        files: 100,
        top: Position { file: 41, row: 0 },
        ..every_row_kind()
    };
    // **The two-region screen, which is the one a reader actually gets.**
    // `SPEC.md` §11.1 pins a file list above the diff, and its rows are drawn by
    // the same `Painter::file_row` as a heading but through a *narrower* area,
    // because the caret column is an inset. Every sweep in this file would
    // otherwise measure the wide path only and report I6 as holding on a row
    // shape that never ships alone.
    let pinned = View {
        list: vec![
            entry("crates/vigia-core/src/frame.rs"),
            entry("src/engine/watch.rs"),
            entry("Cargo.toml"),
        ],
        list_top: 0,
        top: Position { file: 1, row: 0 },
        ..every_row_kind()
    };
    vec![
        ("every row kind, idle", every_row_kind(), chrome()),
        ("every row kind, following", every_row_kind(), following()),
        ("every row kind, notice", every_row_kind(), with_notice()),
        ("a pinned list, following", pinned.clone(), following()),
        ("a pinned list, notice", pinned, with_notice()),
        ("a hundred files, following", many, following()),
        ("awkward content, following", awkward(), following()),
        (
            "clean worktree on a branch, following",
            empty(),
            Chrome {
                following: true,
                ..on_a_branch()
            },
        ),
        ("clean worktree on a branch, idle", empty(), on_a_branch()),
        // A detached HEAD, which is the shorter empty-state line and therefore
        // the one where a width-dependent bug hides. Kept beside the branch case
        // rather than replacing it: the two differ by seven columns, which moves
        // where the line is cut.
        ("clean worktree, detached head", empty(), chrome()),
        // A lost watch is four columns wider on the header than a live one, so
        // it moves every width at which the ladder changes rung. Added to the
        // shared list rather than given gates of its own, which is the point:
        // every structural rule already swept here now covers the mode word too.
        ("every row kind, watch lost", every_row_kind(), lost()),
        (
            "clean worktree, watch lost",
            empty(),
            Chrome {
                mode: Mode::Lost,
                ..on_a_branch()
            },
        ),
        // Added to the *shared* list rather than given gates of its own, which
        // is the point: every structural rule already swept here now covers the
        // glance elements too. A sparkline and a pulse are new things competing
        // for the same row as the path and the counters, so "no row
        // over-occupies" and "a label that lost characters says so" are exactly
        // the assertions they can break.
        ("glance elements, idle", glancing(), chrome()),
        ("glance elements, following", glancing(), following()),
        // Same reasoning again, one row down. The status readouts add seventeen
        // columns to the footer's right-hand side, which is more than the state
        // and the hints put together at some widths, so "no row over-occupies"
        // is the assertion most likely to catch a mistake in the arithmetic that
        // decides whether they fit. Both cases, because the notice one is where
        // the fit is measured against something other than what is drawn.
        ("readouts, following", every_row_kind(), diagnostics()),
        (
            "readouts and a notice",
            every_row_kind(),
            Chrome {
                notice: Some("src/lib.rs vanished between being named and being read".to_owned()),
                ..diagnostics()
            },
        ),
        // A hundred files widens the position to `100/100`, and the readouts
        // widen the row again on top of that, so this is the widest right-hand
        // side the footer can be asked to lay out anywhere in the suite.
        (
            "readouts at a hundred files",
            View {
                list: Vec::new(),
                list_top: 0,
                files: 100,
                top: Position { file: 41, row: 0 },
                ..every_row_kind()
            },
            diagnostics(),
        ),
    ]
}

/// A file changed at both ends and untouched through the middle.
///
/// The one shape that separates a re-projection from a truncation. Any strip
/// showing a prefix of this still colours its **first** bucket, so only the last
/// one can catch it, and only if the file's tail really did change.
///
/// Additions at the head and removals at the tail, so the two ends are also
/// distinguishable by colour rather than only by position.
const ENDS_CHANGED: [HeatBucket; HEAT_BUCKETS] = {
    let mut heat = [HeatBucket {
        added: 0,
        removed: 0,
    }; HEAT_BUCKETS];
    heat[0] = HeatBucket {
        added: 9,
        removed: 0,
    };
    heat[1] = HeatBucket {
        added: 3,
        removed: 0,
    };
    heat[HEAT_BUCKETS - 1] = HeatBucket {
        added: 0,
        removed: 6,
    };
    heat
};

/// A file heading at each rung of the recency ladder, with churn and heat behind
/// it.
///
/// **Every bucket is non-zero on purpose.** An empty bucket draws as a space, so
/// a fixture with gaps in it makes the strip's *width* unobservable from the
/// rendered row, and the ladder gate below could not tell four buckets from
/// eight. The counts still differ, so the shared scale is exercised as well.
///
/// The heat strips are the mirror image: **the two ends of the file are changed
/// and the middle is not**, which is the only shape that can tell a
/// re-projection from a truncation. A strip that dropped its tail would still
/// colour its first bucket and would leave its last one cool.
fn glancing() -> View {
    View {
        list: Vec::new(),
        list_top: 0,
        current_span: 0,
        total_rows: 0,
        rows_above: 0,
        rows: vec![
            Row::File(FileEntry {
                path: "crates/vigia-core/src/watch.rs".to_owned(),
                from: None,
                kind: 'M',
                churn: Some((42, 7)),
                spark: [1, 2, 4, 6, 8, 9, 11, 12],
                recency: Recency::Pulse,
                heat: ENDS_CHANGED,
            }),
            Row::File(FileEntry {
                path: "crates/vigia/src/render.rs".to_owned(),
                from: None,
                kind: 'M',
                churn: Some((11, 3)),
                spark: [1, 1, 2, 2, 1, 3, 2, 1],
                recency: Recency::Live,
                heat: ENDS_CHANGED,
            }),
            Row::File(FileEntry {
                path: "Cargo.toml".to_owned(),
                from: None,
                kind: 'M',
                churn: Some((2, 0)),
                spark: [0; HISTORY_BUCKETS],
                recency: Recency::Cold,
                heat: [HeatBucket::default(); HEAT_BUCKETS],
            }),
        ],
        files: 3,
        top: Position::default(),
        read: 3,
        peak: 12,
    }
}

#[test]
fn no_row_ever_occupies_more_columns_than_the_screen() {
    // The overflow half of I6, and the half that would corrupt the screen rather
    // than merely read badly: a row wider than the pane wraps in the terminal,
    // which pushes every row below it down and makes the shape meaningless.
    let mut widest_seen = 0usize;
    for (name, view, chrome) in cases() {
        for width in WIDTHS {
            for height in [3u16, 6, 24] {
                for y in 0..height {
                    let columns = occupied(width, height, &view, &chrome, y);
                    assert!(
                        columns <= usize::from(width),
                        "{name}: row {y} at {width}x{height} occupies {columns} columns"
                    );
                    widest_seen = widest_seen.max(columns);
                }
            }
        }
    }
    // Non-vacuity: `columns <= width` is satisfied by drawing nothing at all, so
    // a renderer that returned early everywhere would pass this without a single
    // assertion firing.
    assert!(
        widest_seen > 40,
        "the widest row anywhere in the sweep was {widest_seen} columns, so the \
         renderer drew almost nothing and this proves nothing"
    );
}

#[test]
fn a_wide_glyph_at_the_edge_does_not_swallow_the_mark() {
    // Why `put_marked` reserves a column for the mark instead of writing it over
    // the last one, and the reason is not the one it looks like.
    //
    // Overwriting cannot corrupt the row: ratatui refuses to write into the
    // continuation cell a two-column glyph covers. What it does instead is
    // **drop the mark silently**. Fill the line to its last column and the mark
    // has nowhere left to go, so a row that continues past the edge is drawn as
    // one that simply ends, which is the single thing the mark exists to
    // prevent.
    //
    // A plain ASCII line never reaches this: it ends on a one-column character,
    // so the mark always lands. It needs a glyph that ends exactly on the edge,
    // which is why this sweeps the double-width fixture. Found by mutation,
    // where `limit - 1` becoming `limit` left all eleven other gates green.
    let view = awkward();
    let mut saw_swallowable = false;
    for width in WIDTHS {
        let rows = rows_at(width, 6, &view, &chrome());
        for (y, full) in [(2usize, "見出し a 見出し b 見出し c"), (3, "🙂🙂🙂 tail")]
        {
            let row = &rows[y];
            if row.is_empty() {
                continue;
            }
            // The sigil costs a column beyond the text itself.
            if Span::raw(full).width() < usize::from(width) {
                continue;
            }
            assert!(
                row.ends_with(CONTINUES),
                "at {width} columns a clipped line of wide glyphs was drawn as \
                 one that ends: {row:?}"
            );
            // Non-vacuity that matters more than the usual kind: only the widths
            // where a glyph lands on the final column can lose the mark, so a
            // sweep that never hit one would pass against the defect.
            if Span::raw(row.trim_end_matches(CONTINUES)).width() + 1 == usize::from(width) {
                saw_swallowable = true;
            }
        }
    }
    assert!(
        saw_swallowable,
        "no width put a glyph against the final column, so the sweep never \
         reached the case this test is about"
    );
}

#[test]
fn the_header_never_takes_a_second_line() {
    // The footer is allowed to grow and the header is not, so the difference has
    // to be gated rather than assumed. `SPEC.md` §11.1: a worktree name is not a
    // list and has nowhere to break, so a second line could not guarantee a fit
    // and would spend a body row on a maybe. The right-hand side drops whole
    // rungs instead, which is what makes one row always enough.
    //
    // Observed by finding the first body row rather than by asking the renderer
    // where it put one, which would be its own arithmetic agreeing with itself.
    //
    // From eight columns for the reason `numbered` documents: below that the
    // four-column marker is clipped to nothing and cannot be counted, so a
    // narrower sweep would silently observe an empty screen and pass.
    //
    // **Both region shapes**, because the row under the header is a different
    // one in each and a sweep over either alone would miss a header that grew on
    // the other. `listed: 0` is the short pane, where `body_layout` gives the
    // whole body to the diff; `listed: 3` is the pinned list of §11.1, where the
    // first row under the header belongs to the map rather than to the diff.
    let mut saw_a_body = false;
    for chrome in [chrome(), following(), lost(), with_notice()] {
        for width in 8..=120u16 {
            for height in [6u16, 24] {
                for listed in [0usize, 3] {
                    let view = numbered(4, 3, listed);
                    let marker = if listed > 0 { "src/f0.rs" } else { "R00" };
                    let rows = rows_at(width, height, &view, &chrome);
                    let Some(first) = rows.iter().position(|row| row.contains(marker)) else {
                        continue;
                    };
                    saw_a_body = true;
                    assert_eq!(
                        first, 1,
                        "at {width}x{height} with {listed} listed, the body \
                         started on row {first}, so the header took more than one"
                    );
                }
            }
        }
    }
    assert!(
        saw_a_body,
        "no width drew a body row, so this proves nothing"
    );
}

#[test]
fn the_header_ladder_keeps_the_mode_word_last() {
    // The header's own version of `the_state_outlives_the_hints_at_every_width`,
    // and the same rule: the count summarises a body that is on screen and can
    // be counted by looking, while whether the pane is still live is recoverable
    // from nowhere at all.
    //
    // **Since #67 the two rungs are on opposite sides**, and the order is the
    // half that did not change. The count drops first and the mode word is last
    // standing; what moved is which side each is dropped from, so this reads the
    // row as two halves rather than as one right-hand ladder.
    //
    // The two words are restated here rather than read from `Mode::word`. A test
    // that imported them would agree with the renderer by construction, which is
    // why the hint ladder is observed by rendering too.
    for (word, chrome) in [("watching", chrome()), ("not watching", lost())] {
        let view = every_row_kind();
        // `every_row_kind` carries three changed files, so this is the count the
        // header has to draw whole or not at all.
        let full = format!("{}{FACT_JOIN}3 changed", chrome.worktree);
        let (mut saw_both, mut saw_word_only, mut saw_neither) = (false, false, false);

        for width in WIDTHS {
            let header = rows_at(width, 8, &view, &chrome)[0].clone();
            // What is left once the mode word is off the row, which is the only
            // honest way to ask what the *left* drew: the word is right-aligned
            // and the blanks between the two halves belong to neither.
            //
            // One match rather than a `strip_suffix` beside an `ends_with`. Two
            // spellings of one predicate can drift under edit, and every
            // assertion below is keyed on the answer.
            let (left, has_word) = match header.strip_suffix(word) {
                Some(left) => (left.trim_end(), true),
                None => (header.trim_end(), false),
            };
            // The separator rather than the count, because a renderer that drew
            // `vigia · ` and dropped the number would still have to answer for
            // it. The rung is the whole clause or nothing.
            let has_count = left.contains(FACT_JOIN);

            if has_count {
                assert!(
                    has_word,
                    "at {width} columns the count outlived {word:?}: {header:?}"
                );
                // **The count is never cut**, and this is where that is caught.
                // A ladder drops whole rungs, so the left is either the whole
                // clause or the name without it; `vigia · 3 chan›` is neither,
                // and it is what a left-hand side that marked its edge instead
                // of dropping its rung would draw.
                assert_eq!(
                    left, full,
                    "at {width} columns the left-hand side is neither the whole \
                     clause nor the name alone: {header:?}"
                );
                saw_both = true;
            } else if has_word {
                saw_word_only = true;
            } else {
                saw_neither = true;
            }

            // **The mode word is never cut either**, which is stricter than the
            // marking rule the rest of the header follows. A fragment of the word
            // reaching the screen means someone replaced it with a token that
            // truncates, and `wat›` is a state nobody can read.
            //
            // Both spellings of cut, because they fail differently and only one
            // of them looks broken. Silently truncated ends in the fragment;
            // marked ends in the fragment and the continuation mark. A check for
            // the bare fragment alone passes against `wat›`, which is the very
            // shape this rule exists to forbid.
            //
            // Reading how the row *ends* is sound only because neither the
            // fixture's worktree name nor the count can end in a prefix of either
            // word: the name is guarded by `chrome`, and a count ends in
            // `changed`, which shares no prefix with `watching` past `w`, and
            // `wat` is three characters longer than anything `changed` ends in.
            for cut in 1..word.chars().count() {
                let fragment: String = word.chars().take(cut).collect();
                let marked = format!("{fragment}{CONTINUES}");
                assert!(
                    !header.ends_with(&fragment) && !header.ends_with(&marked),
                    "at {width} columns the header ended in {fragment:?}, which is \
                     {word:?} cut: {header:?}"
                );
            }
        }

        assert!(
            saw_both && saw_word_only && saw_neither,
            "{word}: the sweep saw both={saw_both} word-only={saw_word_only} \
             neither={saw_neither}, so it did not cover the whole ladder"
        );
    }
}

#[test]
fn the_header_count_sits_with_the_worktree_at_every_width() {
    // #67's structural half, and the one a snapshot cannot assert: a picture
    // shows one width, and what has to hold is that the count never drifts away
    // from the noun it modifies at *any* of them.
    //
    // The count is a fact about the **tree**, so it is drawn against the tree's
    // name. Beside the mode word it fused into `watching 3 files`, a verb with
    // an object naming a curated set that does not exist; beside the worktree
    // name it reads as what it is, which is how much of that tree has moved.
    //
    // Swept over both mode words, because the two differ by four columns and so
    // move every width at which the ladder changes rung, and a fixture of one
    // word exercises one column-width class while reading as though it covered
    // both.
    let view = every_row_kind();
    let mut saw_the_count = 0usize;
    let mut saw_it_dropped = 0usize;

    // Guard the fixture, the way `a_lost_watch_is_loud_and_a_live_one_is_quiet`
    // does for its own `w`. The sweep finds the count by locating the first
    // digit on the row, which is sound only while nothing else on the row can
    // supply one. A worktree named `vigia2` would point every assertion below at
    // its own left-hand side, silently and in the passing direction.
    assert!(
        !chrome().worktree.contains(|c: char| c.is_ascii_digit()),
        "the fixture's worktree name {:?} contains a digit, so the sweep below \
         would read it as the count",
        chrome().worktree
    );

    for chrome in [chrome(), lost()] {
        let joined = format!("{}{FACT_JOIN}", chrome.worktree);
        for width in WIDTHS {
            let header = rows_at(width, 8, &view, &chrome)[0].clone();
            // The count is the only number the header can draw: `#49` ruled
            // there is no changed-line total, the worktree name has no digit in
            // it, and neither mode word does. So the first digit on this row is
            // where the count starts, wherever the renderer chose to put it.
            //
            // Found rather than computed, because where the left clause ends
            // depends on what the right-hand side took, and a test that
            // recomputed that would be the renderer's own arithmetic agreeing
            // with itself.
            let Some(at) = header.find(|c: char| c.is_ascii_digit()) else {
                saw_it_dropped += 1;
                continue;
            };
            saw_the_count += 1;
            assert!(
                header[..at].ends_with(&joined),
                "at {width} columns the count is not joined to the worktree \
                 name: {header:?}"
            );
        }
    }

    // Both directions, or the sweep proved only that one of them can happen.
    assert!(
        saw_the_count > 0 && saw_it_dropped > 0,
        "the sweep saw the count {saw_the_count} times and saw it dropped \
         {saw_it_dropped} times"
    );
}

#[test]
fn the_empty_state_line_marks_its_edge() {
    // One token, and `SPEC.md` §11.1 lists it by name among them: it cannot drop
    // an item the way the hint bar can, and it has no identifying half the way a
    // path does, so the honest thing is to fill the room and mark the cut.
    //
    // Both directions, because a rule that only ever fires one way is not a rule.
    const LINE: &str = "no unstaged changes · main";
    let view = empty();
    let (mut fitted, mut cut) = (0usize, 0usize);

    for width in WIDTHS {
        let rows = rows_at(width, 6, &view, &on_a_branch());
        let body = &rows[1];
        assert!(
            label_is_honest(body, LINE),
            "the empty state was cut at {width} columns without saying so: {body:?}"
        );

        if Span::raw(LINE).width() <= usize::from(width) {
            fitted += 1;
            assert!(
                body.starts_with(LINE),
                "the empty state fits at {width} columns but was not drawn whole: {body:?}"
            );
            assert!(
                !body.contains(CONTINUES),
                "the empty state fits at {width} columns and was marked anyway: {body:?}"
            );
        } else if body.contains(CONTINUES) {
            cut += 1;
        }
    }

    assert!(
        fitted > 0 && cut > 0,
        "the sweep saw {fitted} empty states fit and {cut} cut"
    );
}

#[test]
fn the_body_gets_exactly_the_rows_the_caller_was_promised() {
    // `diff_height` is what a caller asks `View::collect` for, and the renderer
    // decides where the footer starts. Those are two computations of one number,
    // and this is what stops them drifting: before I6 the agreement was the
    // constant 2 written in two places, and now the footer's height varies with
    // the width and the follow state.
    //
    // Counted by giving the view more rows than can fit and seeing how many come
    // back, rather than by locating the footer. Locating it would mean restating
    // the renderer's own arithmetic, which agrees with itself by construction.
    let mut saw_a_body = false;
    for chrome in [chrome(), following(), with_notice()] {
        // Three file counts, and the hundred is the one that earns its place:
        // the position is `100/100` rather than `1/1`, which widens the state by
        // four columns and moves the width at which the footer takes its second
        // line. A sweep over single-digit counts alone cannot tell a renderer
        // that reads the count from one that ignores it.
        for files in [1usize, 3, 100] {
            for width in 8..=120u16 {
                for height in [3u16, 5, 6, 24] {
                    let area = Rect::new(0, 0, width, height);
                    // **The whole split, not just the diff half.** A view is
                    // built the way `View::collect` builds one, with its list
                    // filled to exactly the height the layout asked for. Handing
                    // the renderer a view that carries files but no list would
                    // make this compare a promise against a screen that never
                    // ships, and it would pass while the two regions disagreed.
                    let split = body_layout(area, &chrome, files);
                    let promised = split.diff;
                    let view = numbered(promised + 3, files, split.list);
                    let rows = rows_at(width, height, &view, &chrome);
                    let painted = rows.join("\n");

                    let drawn = (0..promised + 3)
                        .filter(|i| painted.contains(&format!("R{i:02}")))
                        .count();
                    assert_eq!(
                        drawn, promised,
                        "at {width}x{height} over {files} files the caller was \
                         promised {promised} diff rows and the renderer drew \
                         {drawn}, with a {} row list",
                        split.list
                    );
                    if promised > 0 {
                        saw_a_body = true;
                    }
                }
            }
        }
    }
    assert!(
        saw_a_body,
        "every screen in the sweep had a zero-row body, so this proves nothing"
    );
}

#[test]
fn the_footer_takes_a_second_line_only_when_one_line_cannot_hold_both() {
    // Both directions at the width I6 is named for. Following is the default
    // state, so the two-line case is the ordinary one rather than the exotic
    // one, which is what made the collision worth solving instead of tolerating.
    let view = every_row_kind();
    let tall = 24u16;

    // **The whole body, not the diff half.** This gate is about how many rows
    // the *footer* takes, and since §11.1 split the body in two, `diff_height`
    // answers a narrower question: it is the diff's share, which also moves when
    // the file list grows. Summing the split back up isolates the footer again,
    // which is what this has always been measuring.
    let body = |width: u16, chrome: &Chrome| {
        let split = body_layout(Rect::new(0, 0, width, tall), chrome, view.files);
        split.list + usize::from(split.rule) + split.diff
    };

    assert_eq!(
        body(80, &following()),
        usize::from(tall) - 2,
        "eighty columns hold the hints and the state on one line"
    );
    assert_eq!(
        body(120, &following()),
        usize::from(tall) - 2,
        "so do a hundred and twenty"
    );

    assert_eq!(
        body(40, &chrome()),
        usize::from(tall) - 2,
        "forty columns hold them too once the follow marker is gone"
    );

    assert_eq!(
        body(40, &following()),
        usize::from(tall) - 3,
        "forty columns following cannot, so the footer takes a second line"
    );

    // And the second line is real: the state is on it, above the hints.
    let rows = rows_at(40, tall, &view, &following());
    let hints = rows.last().expect("a footer");
    let state = &rows[rows.len() - 2];
    assert!(
        state.contains(FOLLOW_MARK) && !state.contains("quit"),
        "the upper footer row is not the state alone: {state:?}"
    );
    assert!(
        hints.contains("quit") && !hints.contains(FOLLOW_MARK),
        "the lower footer row is not the hints alone: {hints:?}"
    );
}

#[test]
fn a_notice_never_moves_the_diff() {
    // A notice is transient: a file that vanished between being named and being
    // read, a repository mid-`git gc`. If it could grow the footer, the reader's
    // diff would jog down a row and back every time one flickered. `SPEC.md`
    // §11.1 rules that the footer's height depends on width, follow state and
    // file count only.
    //
    // This is also what lets `Shell::draw` sample the chrome before the collect
    // that may raise the notice, so it is load bearing rather than cosmetic.
    let view = every_row_kind();
    let mut saw_two_line_footer = false;
    for width in WIDTHS {
        for height in [5u16, 24] {
            let area = Rect::new(0, 0, width, height);
            let quiet = diff_height(area, &following(), view.files);
            assert_eq!(
                quiet,
                diff_height(area, &with_notice(), view.files),
                "a notice changed the body height at {width}x{height}"
            );
            if quiet == usize::from(height) - 3 {
                saw_two_line_footer = true;
            }
        }
    }
    // Non-vacuity: two heights are trivially equal if the footer never takes a
    // second line anywhere in the sweep, which is the only state a notice could
    // have moved it into.
    assert!(
        saw_two_line_footer,
        "no width in the sweep produced a two-line footer, so nothing here could \
         have been moved by a notice"
    );
}

#[test]
fn the_status_readouts_never_move_the_diff() {
    // [`a_notice_never_moves_the_diff`]'s rule, and the readouts need it for two
    // reasons a notice does not have.
    //
    // **The frame cell does not exist on the first paint.** No frame has
    // completed, so there is no p99 of anything, and it appears on the second.
    // If it could grow the footer, every session would jog its diff down a row
    // the first time a file changed, which is the worst possible moment: the
    // reader is looking at exactly that.
    //
    // **The memory cell does not exist on a platform with no cheap read.** That
    // would make the body height platform-dependent, so a snapshot taken on one
    // tier-1 target would be a row out on another.
    let view = every_row_kind();
    let mut saw_two_line_footer = false;
    for width in WIDTHS {
        for height in [5u16, 24] {
            let area = Rect::new(0, 0, width, height);
            let bare = diff_height(area, &following(), view.files);
            assert_eq!(
                bare,
                diff_height(area, &diagnostics(), view.files),
                "the status readouts changed the body height at {width}x{height}"
            );
            if bare == usize::from(height) - 3 {
                saw_two_line_footer = true;
            }
        }
    }
    // Non-vacuity, for the reason the notice sweep gives: two heights are
    // trivially equal if the footer never takes a second line anywhere.
    assert!(
        saw_two_line_footer,
        "no width in the sweep produced a two-line footer, so nothing here could \
         have been moved by a readout"
    );
}

#[test]
fn the_readouts_take_every_width_that_can_hold_them() {
    // The ladder's other direction, and the one no other gate here asserts.
    // Every sweep in this file checks that nothing **overflows**; a footer that
    // drew no readouts at any width would satisfy all of them. This checks the
    // ladder is not leaving a whole rung's worth of room unspent.
    //
    // Written against what is drawn rather than against the arithmetic that
    // decides it, which would be the formula agreeing with itself. The rung
    // widths are restated for the reason `RAMP` and `HEAT_BLOCK` are: a test
    // sharing the renderer's own constants cannot disagree with it.
    const PAIR: usize = 11 + 2 + 6;
    const GAP: usize = 2;

    let view = every_row_kind();
    for width in WIDTHS {
        let rows = rows_at(width, 24, &view, &diagnostics());
        // Whichever row carries the state is the one the readouts share.
        let carrying = if rows[22].contains(FOLLOW_MARK) {
            22
        } else if rows[23].contains(FOLLOW_MARK) {
            23
        } else {
            continue;
        };
        if rows[carrying].contains("frame") {
            continue;
        }
        // **The blank run inside the row, not the columns left at its end.**
        // [`occupied`] counts every cell including spaces, so it always returns
        // the full width and a gate written against it asserts nothing; that was
        // this test's first form and two mutations walked straight through it.
        // What the readouts would actually occupy is the gap between the hints
        // and the right-aligned state, so that gap is what has to be measured.
        let spare = rows[carrying]
            .split(|c: char| c != ' ')
            .map(str::len)
            .max()
            .unwrap_or(0);
        assert!(
            spare < PAIR + GAP,
            "at {width} columns the footer drew no readouts with a {spare} column \
             gap on row {carrying}, which is room for both of them: {:?}",
            rows[carrying]
        );
    }
}

#[test]
fn the_status_readouts_go_before_the_hints_and_the_state_do() {
    // `SPEC.md` §11.1's drop order, and the direction it is asserted in matters.
    // The hints are how a reader operates the tool and the state is what the
    // tree is doing; these two describe `vigia` itself, so a narrowing pane owes
    // them last. A ladder that dropped a hint to keep a frame time would be
    // legal by every other gate in this file, and wrong.
    //
    // Swept rather than checked at a chosen width, because the claim is about
    // every width and the interesting ones are wherever a rung changes.
    let view = every_row_kind();
    let mut saw_readouts = false;
    let mut saw_them_dropped = false;

    for width in WIDTHS {
        let rows = rows_at(width, 24, &view, &diagnostics());
        let footer = rows[23].clone();
        let upper = rows[22].clone();
        // Either row may hold them: at narrow widths the footer takes a second
        // line and the readouts ride with the state on the upper one.
        let drawn = footer.contains("frame") || upper.contains("frame");

        if drawn {
            saw_readouts = true;
        } else {
            saw_them_dropped = true;
            // The bare state at its narrowest is `follow ▶`, and a hint bar at
            // its narrowest is `f follow`. Below the width that holds either,
            // both are gone for reasons of their own and there is nothing left
            // for this to compare against.
            let bare = rows_at(width, 24, &view, &following());
            let bare_footer = format!("{}{}", bare[22], bare[23]);
            let with = format!("{upper}{footer}");
            assert!(
                bare_footer.contains(FOLLOW_MARK) <= with.contains(FOLLOW_MARK),
                "the readouts cost the follow marker at {width} columns: \
                 {with:?} against {bare_footer:?}"
            );
            assert!(
                bare_footer.contains("quit") <= with.contains("quit"),
                "the readouts cost a hint at {width} columns: \
                 {with:?} against {bare_footer:?}"
            );
        }
    }

    // Both directions, because a ladder that never draws them and one that never
    // drops them each satisfy half of this and neither is the rule.
    assert!(
        saw_readouts,
        "no width in the sweep drew the readouts at all"
    );
    assert!(
        saw_them_dropped,
        "no width in the sweep dropped the readouts, so nothing was compared"
    );
}

#[test]
fn a_short_screen_keeps_its_body_rather_than_growing_the_footer() {
    // The footer would collide at forty columns following, and at these heights
    // taking a second line would leave one body row or none. A monitor with no
    // diff left in it has stopped being one, so the hints give way instead.
    let view = every_row_kind();
    for height in [3u16, 4] {
        let area = Rect::new(0, 0, 40, height);
        assert_eq!(
            diff_height(area, &following(), view.files),
            usize::from(height) - 2,
            "the footer grew at 40x{height} and spent a body row it could not spare"
        );
    }
    // Non-vacuity: one row taller and it does grow, so the guard is what stops
    // it rather than the collision never happening at these widths.
    let area = Rect::new(0, 0, 40, 5);
    assert_eq!(
        diff_height(area, &following(), view.files),
        2,
        "at five rows the footer should take its second line"
    );
}

/// Every distinct hint bar the renderer draws, widest first.
///
/// Observed by rendering rather than read off the table in `render.rs`. A test
/// that compared the rung table against itself would agree with any table,
/// including one that shipped a truncated rung.
///
/// Read off a clean, idle worktree, where the state is empty and the bar is
/// therefore the whole footer row. On any other screen the two share a line and
/// trimming the row would hand back the state as if it were a hint, which is
/// what the first version of this did.
fn observed_rungs() -> Vec<String> {
    let view = empty();
    let mut rungs: Vec<String> = Vec::new();
    for width in WIDTHS.rev() {
        let rows = rows_at(width, 24, &view, &chrome());
        let bar = rows.last().expect("a footer").trim().to_owned();
        if rungs.last().map(String::as_str) != Some(bar.as_str()) {
            rungs.push(bar);
        }
    }
    rungs
}

#[test]
fn the_hint_bar_drops_whole_hints_and_never_half_of_one() {
    // The truncated-to-useless shape I6 forbids, stated as the thing that must
    // never appear: a bar reading `q quit · f follow · jk scr`, where the last
    // hint names a key without naming what it does.
    let rungs = observed_rungs();
    let widest = rungs.first().expect("at least one bar").clone();
    let whole: Vec<&str> = widest.split(HINT_SEPARATOR).collect();
    assert!(
        whole.len() > 1,
        "the widest bar is not a list, so this test proves nothing: {widest:?}"
    );

    for rung in &rungs {
        if rung.is_empty() {
            continue;
        }
        for hint in rung.split(HINT_SEPARATOR) {
            assert!(
                whole.contains(&hint),
                "the bar {rung:?} contains {hint:?}, which is not one of the \
                 hints {whole:?}"
            );
        }
    }

    // And the same holds on the two-line footer, where the bar has the bottom
    // row to itself. That is the shape forty columns actually draws in the
    // default state, so leaving it to the clean-worktree sweep above would test
    // every screen except the one I6 is named for.
    let bottom = rows_at(40, 24, &every_row_kind(), &following())
        .last()
        .expect("a footer")
        .trim()
        .to_owned();
    assert!(
        rungs.contains(&bottom),
        "the forty-column bar {bottom:?} is not one of the rungs {rungs:?}"
    );
}

#[test]
fn every_rung_of_the_hint_ladder_is_the_one_above_it_minus_whole_hints() {
    // Nothing is invented on the way down. A ladder that reworded a hint to make
    // it fit would pass the test above, and would teach a second dialect at
    // exactly the width where the reader has least to go on.
    let rungs = observed_rungs();
    assert!(
        rungs.len() >= 3,
        "the ladder has {} rungs, so the sweep never narrowed it",
        rungs.len()
    );

    for pair in rungs.windows(2) {
        let (above, below) = (&pair[0], &pair[1]);
        assert!(
            Span::raw(below).width() < Span::raw(above).width(),
            "the ladder does not narrow: {above:?} then {below:?}"
        );
        if below.is_empty() {
            continue;
        }
        let kept: Vec<&str> = below.split(HINT_SEPARATOR).collect();
        let had: Vec<&str> = above.split(HINT_SEPARATOR).collect();
        // A subsequence, not merely a subset: dropping a hint must not reorder
        // the ones that stay.
        let mut remaining = had.iter();
        for hint in &kept {
            assert!(
                remaining.any(|candidate| candidate == hint),
                "{below:?} is not {above:?} minus whole hints"
            );
        }
    }
    assert!(
        rungs.last().expect("rungs").is_empty(),
        "the narrowest bar is {:?} rather than nothing",
        rungs.last()
    );
}

#[test]
fn the_state_outlives_the_hints_at_every_width() {
    // State is not advice. A reader who has lost the follow marker cannot tell a
    // view that has not moved because nothing changed from one that has not
    // moved because following was switched off, and at the narrowest widths that
    // is precisely when guessing is most expensive.
    let view = every_row_kind();
    let mut saw_hints_drop = false;
    for width in WIDTHS {
        let rows = rows_at(width, 24, &view, &following());
        let footer: String = rows[rows.len() - 2..].join(" ");
        let has_hints = footer.contains("quit") || footer.contains("follow ·");
        let has_state = footer.contains(FOLLOW_MARK);
        if !has_hints {
            saw_hints_drop = true;
        }
        assert!(
            has_state || !has_hints,
            "at {width} columns the hints survived without the state: {footer:?}"
        );
    }
    assert!(
        saw_hints_drop,
        "the hints never dropped over the whole sweep, so this proves nothing"
    );
}

#[test]
fn a_label_cut_at_the_right_edge_says_so() {
    // Both directions, over every single-token label on the screen. The hunk
    // header is the one that matters most: `@@ -258,7 +25` is not a shortened
    // header, it is a header naming a different line.
    let view = View {
        list: Vec::new(),
        list_top: 0,
        current_span: 0,
        total_rows: 0,
        rows_above: 0,
        rows: vec![
            Row::Hunk {
                old_start: 258,
                old_lines: 7,
                new_start: 258,
                new_lines: 9,
            },
            Row::Note("unresolved conflict"),
        ],
        files: 1,
        top: Position::default(),
        read: 1,
        peak: 0,
    };
    let long_name = Chrome {
        worktree: "a-worktree-with-a-very-long-name-indeed".to_owned(),
        ..chrome()
    };

    let mut fitted = 0usize;
    let mut cut = 0usize;
    for width in WIDTHS {
        let rows = rows_at(width, 8, &view, &long_name);
        // The header shares its line with the mode word and its own side with
        // the count, so how much room the name actually gets is the renderer's
        // business and `label_is_honest` is the whole assertion for it. Since
        // #67 the count is what gives way first, so a name this long is drawn
        // alone long before it is cut, and the ladder that decides the order is
        // gated by `the_header_ladder_keeps_the_mode_word_last`. The two body
        // rows own their full width, so for those the width alone decides and
        // both directions can be checked.
        assert!(
            label_is_honest(&rows[0], &long_name.worktree),
            "the worktree name was cut at {width} columns without saying so: {:?}",
            rows[0]
        );

        for (label, row, full) in [
            ("the hunk header", &rows[1], "@@ -258,7 +258,9 @@"),
            ("the note", &rows[2], "  unresolved conflict"),
        ] {
            assert!(
                label_is_honest(row, full),
                "{label} was cut at {width} columns without saying so: {row:?}"
            );
            if Span::raw(full).width() <= usize::from(width) {
                fitted += 1;
                assert!(
                    row.starts_with(full),
                    "{label} fits at {width} columns but was not drawn whole: {row:?}"
                );
                assert!(
                    !row.contains(CONTINUES),
                    "{label} fits at {width} columns and was marked anyway: {row:?}"
                );
            } else if row.contains(CONTINUES) {
                cut += 1;
            }
        }
    }
    // Both directions have to have been exercised, or the rule was only ever
    // checked where it could not fail.
    assert!(
        fitted > 0 && cut > 0,
        "the sweep saw {fitted} labels fit and {cut} cut"
    );
}

#[test]
fn a_path_says_its_head_is_missing_rather_than_its_tail() {
    // The one label on the screen whose direction is opposite, and the reason
    // there are two marks rather than one. A column of `crates/vigia-core/…`
    // names nothing; the tail is what identifies the file.
    const PATH: &str = "crates/vigia-core/src/very/deeply/nested/module/frame.rs";
    let view = awkward();
    let mut saw_elided = false;
    let mut saw_whole = false;
    for width in WIDTHS {
        let rows = rows_at(width, 6, &view, &chrome());
        // Row zero is the header; the file heading is the first body row. The
        // kind letter leads it and the churn is right-aligned after it, so the
        // path is neither the start of the row nor its end.
        let path_row = &rows[1];
        if path_row.contains(PATH) {
            saw_whole = true;
            assert!(
                !path_row.contains(ELIDED) && !path_row.contains(CONTINUES),
                "the path fits at {width} columns and was marked anyway: {path_row:?}"
            );
            continue;
        }
        let Some(at) = path_row.find(ELIDED) else {
            continue;
        };
        saw_elided = true;
        assert!(
            !path_row.contains(CONTINUES),
            "the path row carries both marks at {width} columns: {path_row:?}"
        );
        // Whatever survived has to be a suffix of the real path. A prefix would
        // mean the elision kept the useless half, which is the whole reason this
        // one label marks the other end.
        // Split on a single space rather than on whitespace: at the narrowest
        // widths nothing survives the mark at all, and `split_whitespace` would
        // skip the gap and hand back the churn column instead of the empty
        // string that is the honest answer.
        let kept = path_row[at + ELIDED.len_utf8()..]
            .split(' ')
            .next()
            .unwrap_or("");
        assert!(
            PATH.ends_with(kept),
            "the path kept {kept:?}, which is not its tail, at {width} columns"
        );
    }
    assert!(
        saw_elided && saw_whole,
        "the sweep saw elided={saw_elided} whole={saw_whole}, so it covered one \
         direction only"
    );
}

#[test]
fn a_clipped_content_line_says_it_continues() {
    // Content cannot break the way the hint bar does and has no identifying half
    // the way a path does, so the mark is all it can honestly offer. `SPEC.md`
    // §11.1 rules this is not what I6 means by a truncated label.
    let text = "        for change in self.changes() { let x = compute(change); }";
    let view = View {
        list: Vec::new(),
        list_top: 0,
        current_span: 0,
        total_rows: 0,
        rows_above: 0,
        rows: vec![line(LineKind::Removed, 260, text)],
        files: 1,
        top: Position::default(),
        read: 1,
        peak: 0,
    };

    let mut saw_fit = false;
    let mut saw_cut = false;
    for width in WIDTHS {
        let rows = rows_at(width, 4, &view, &chrome());
        let row = &rows[1];
        if row.is_empty() {
            continue;
        }
        if row.ends_with('}') && row.contains("compute") {
            saw_fit = true;
            assert!(
                !row.contains(CONTINUES),
                "a content line that fits at {width} columns was marked: {row:?}"
            );
        } else {
            saw_cut = true;
            assert!(
                row.ends_with(CONTINUES),
                "a content line was cut at {width} columns without saying so: {row:?}"
            );
        }
    }
    assert!(
        saw_fit && saw_cut,
        "the sweep did not cover both directions"
    );
}

/// `SPEC.md` §11.1's rule, applied to the newest thing on the row.
///
/// > A thing made of items breaks, a thing made of characters marks its edge,
/// > and content is neither.
///
/// A sparkline is made of items, so it drops **whole buckets** and never draws a
/// partial one. Observable because [`glancing`]'s buckets are all non-zero: an
/// empty bucket is a space, so a fixture with gaps would make the strip's width
/// unreadable from the row and this gate would be asserting about nothing.
///
/// The rungs are read off the screen rather than imported. A test comparing the
/// renderer's ladder against the renderer's own constant would agree with itself
/// at every width, which is the failure the hint-bar gate already documents.
#[test]
fn the_sparkline_drops_whole_buckets_and_never_half_of_one() {
    // Counted by **colour** as well as by glyph. Until the heat strip landed a
    // sparkline was the only thing on a heading drawn from blocks, and this gate
    // counted glyphs; a heat slice is the same full block, so the count became
    // eighteen and the gate started failing for a reason that was not a
    // regression. `cells_coloured` says why the pair is needed.
    let spark = theme().spark.fg.expect("the sparkline has a colour");
    let mut seen = std::collections::BTreeSet::new();

    for (name, view, chrome) in cases() {
        for width in WIDTHS {
            for y in 0..6u16 {
                let backend = drawn(width, 6, &view, &chrome);
                let buckets = cells_coloured(&backend, y, &[spark], &RAMP).len();
                let row = rows_at(width, 6, &view, &chrome)[usize::from(y)].clone();
                assert!(
                    buckets <= 8,
                    "{name}: {buckets} buckets at {width} columns, over the eight \
                     the window holds: {row:?}"
                );
                if buckets > 0 {
                    seen.insert(buckets);
                }
            }
        }
    }

    // Whole rungs only. Any count between them is a strip that was squeezed
    // rather than shortened, which is the shape the rule forbids.
    assert_eq!(
        seen,
        [4usize, 8].into_iter().collect(),
        "the sparkline was drawn at bucket counts {seen:?}; only whole rungs are \
         legal, and both of them have to be reachable or the ladder has a rung \
         no width can produce"
    );
}

/// The pulse may take room from the path. It may not take the path.
///
/// `MIN_PATH_WIDTH` is the floor, and this is the assertion that makes it load
/// bearing: at every width from one to a hundred and twenty, a heading either
/// names its file or says which end it lost. A glance element that reduced a row
/// to `M …` would have spent the content to decorate it.
#[test]
fn the_pulse_label_never_pushes_a_path_off_its_own_row() {
    let view = glancing();
    let mut narrowest_named = u16::MAX;

    for width in WIDTHS {
        let rows = rows_at(width, 6, &view, &following());
        let heading = &rows[1];
        // The pulse is whole or absent: it is a ladder, so it never appears cut.
        let dotted = heading.contains('●');
        assert!(
            !dotted || heading.contains("● just changed") || !heading.contains("just"),
            "at {width} columns the pulse is drawn part-way: {heading:?}"
        );

        // And the row still names its file, by tail or by mark.
        let tail = "watch.rs";
        if heading.contains(tail) {
            narrowest_named = narrowest_named.min(width);
        } else {
            assert!(
                width < 20,
                "at {width} columns the heading no longer contains {tail:?}: \
                 {heading:?}"
            );
        }
    }

    assert!(
        narrowest_named <= 24,
        "the heading only named its file from {narrowest_named} columns up, so a \
         glance element is eating the path well before the pane gets small"
    );
}

/// The third case of `SPEC.md` §11.1's layout rule, and the one that is a
/// correctness claim rather than a tidiness one.
///
/// > A thing made of items breaks, a thing made of characters marks its edge,
/// > and content is neither.
///
/// A heat strip is made of items and is **not** a list. Dropping its tail would
/// draw the first half of a file as though it were the whole file, and a reader
/// would conclude the end of the file is untouched. So a narrower rung sums
/// adjacent slices and classifies the sums: less resolution, still the whole
/// file.
///
/// **Read from cell styles, not symbols.** Every slice draws the same block and
/// only the colour differs, so a symbol-based check cannot tell a cool slice
/// from a hot one and would pass against a strip of pure track.
#[test]
fn the_heat_strip_reprojects_rather_than_dropping_buckets() {
    let theme = theme();
    let view = glancing();
    let mut widths_seen = std::collections::BTreeSet::new();

    for width in WIDTHS {
        let backend = drawn(width, 6, &view, &following());

        // Row 1 is the first file heading; the header is row 0.
        let strip = cells_coloured(&backend, 1, &heat_colours(&theme), &[HEAT_BLOCK]);

        if strip.is_empty() {
            continue;
        }
        widths_seen.insert(strip.len());

        // Whole rungs only. Anything between them is a strip that was squeezed
        // rather than re-projected.
        assert!(
            strip.len() == HEAT_BUCKETS || strip.len() == HEAT_BUCKETS / 2,
            "at {width} columns the strip is {} slices wide, which is neither \
             whole rung",
            strip.len()
        );

        // The claim truncation fails. The fixture changes both ends of the file,
        // so at every rung the first and last slice have to be hot. A strip
        // showing a prefix would leave the last one on the track colour.
        assert_ne!(
            strip.last().map(|style| style.fg),
            Some(theme.heat_track.fg),
            "at {width} columns the strip's last slice is track, but the \
             fixture changes the end of the file, so the tail was dropped \
             rather than merged"
        );
        assert_ne!(
            strip.first().map(|style| style.fg),
            Some(theme.heat_track.fg),
            "at {width} columns the strip's first slice is track"
        );
        // And the middle is still visibly untouched, or the merge smeared the
        // ends across the whole file and the strip stopped locating anything.
        assert!(
            strip.iter().any(|style| style.fg == theme.heat_track.fg),
            "at {width} columns every slice is hot, so a file changed only at \
             its ends is being drawn as one changed throughout"
        );
    }

    assert_eq!(
        widths_seen,
        [HEAT_BUCKETS / 2, HEAT_BUCKETS].into_iter().collect(),
        "the strip was drawn at slice counts {widths_seen:?}; both rungs have to \
         be reachable or the ladder has a rung no width can produce"
    );
}

#[test]
fn a_bonus_hint_rung_never_buys_itself_a_footer_row() {
    // The regression that adding `JK files` caused, and the reason
    // `HINT_BASELINE` exists. The widest rung became forty columns, which is
    // exactly the width I6 is named for, so the footer started taking a second
    // line there and every reader lost a body row to advice, including the ones
    // who never press `J`.
    //
    // The rule: the footer's height is decided by the baseline bar a reader is
    // owed at forty columns. Rungs above it are drawn where there is room and are
    // never worth a row.
    let view = every_row_kind();
    let tall = 24u16;
    let rows = |width: u16, chrome: &Chrome| {
        let split = body_layout(Rect::new(0, 0, width, tall), chrome, view.files);
        usize::from(tall) - 1 - (split.list + usize::from(split.rule) + split.diff)
    };

    // The case that broke. Idle at forty columns, where the state is a bare
    // position and the baseline bar fits beside it.
    assert_eq!(
        rows(40, &chrome()),
        1,
        "forty columns idle took a second footer line, so a bonus hint bought a \
         body row at the width I6 is named for"
    );

    // Non-vacuity, and it is the half that would otherwise let this pass against
    // a ladder that simply never draws the extra hint: somewhere wide enough, the
    // bar really is wider than the baseline.
    let baseline = rows_at(40, tall, &view, &chrome())
        .last()
        .expect("a footer")
        .trim_end()
        .to_owned();
    let wide = rows_at(120, tall, &view, &chrome())
        .last()
        .expect("a footer")
        .to_owned();
    let hints = wide.split("  ").next().unwrap_or_default().trim_end();
    assert!(
        hints.len() > baseline.len(),
        "the widest pane drew {hints:?}, no more than the forty-column bar \
         {baseline:?}, so there is no bonus rung to protect"
    );

    // And the height never grows as a pane gets wider, which is the general
    // shape the rule above is one instance of.
    //
    // **From eight columns**, because below that there is no state to move up to
    // a second line and the footer therefore cannot grow at all: it is one row at
    // width two and two rows at width three, which is not the ladder relaxing but
    // the ladder never having engaged. Sweeping into that would be asserting
    // monotonicity across a boundary the rule does not cross.
    for chrome in [chrome(), following()] {
        let mut previous = usize::MAX;
        for width in 8..=*WIDTHS.end() {
            let now = rows(width, &chrome);
            assert!(
                now <= previous,
                "the footer grew from {previous} to {now} rows as the pane widened \
                 to {width}"
            );
            previous = now;
        }
    }
}

#[test]
fn a_scrollbar_costs_its_region_its_own_columns_and_no_more() {
    // I6's floor, held against the newest thing that can take a column. A bar is
    // a glance element and `MIN_PATH_WIDTH` outranks every one of them, so the
    // question is not "is the path at least twelve columns" — the counters are
    // placed before that floor is computed and can already reach past it, which
    // predates this region entirely. The question the bar owes an answer to is
    // whether it costs anything **beyond** the column it occupies.
    //
    // Asked by comparison rather than by arithmetic: the same list row drawn at
    // `width` with a bar must read exactly as it does at `width - BAR_COLUMNS`
    // without one. Anything else means the bar is squeezing the row rather than
    // sitting beside it, and a gate that recomputed the expected path would be
    // restating `Painter::file_row`'s own ladder.
    // The bar itself plus the gap before it, restated rather than imported for
    // the reason `RAMP` and `HEAT_BLOCK` are: a test sharing the renderer's own
    // constant would agree with it by construction.
    const BAR_COLUMNS: u16 = 2;

    let entries = vec![
        entry("crates/vigia-core/src/frame.rs"),
        entry("src/engine/watch.rs"),
        entry("Cargo.toml"),
    ];
    // Ten files with three rows shown, so a bar is drawn; and three with three
    // shown, so one is not.
    let with_bar = View {
        list: entries.clone(),
        list_top: 0,
        top: Position { file: 1, row: 0 },
        files: 10,
        ..every_row_kind()
    };
    let without_bar = View {
        list: entries,
        list_top: 0,
        top: Position { file: 1, row: 0 },
        files: 3,
        ..every_row_kind()
    };

    let mut compared = 0;
    for width in 10..=*WIDTHS.end() {
        let barred = rows_at(width, 24, &with_bar, &chrome());
        let bare = rows_at(width - BAR_COLUMNS, 24, &without_bar, &chrome());

        // Row one is the first list row. Trailing blanks are already trimmed by
        // `rows_at`, and the bar's own column is the only thing that can follow
        // the row's content, so it is stripped before comparing.
        let barred_row = barred[1].trim_end_matches(['▕', '█']).trim_end().to_owned();
        let bare_row = bare[1].trim_end().to_owned();

        // **Skip the widths where the caret's own ladder differs between the two
        // panes being compared.** The caret is decided against the pane width, so
        // the wide side can have one where the narrow side does not, and a row
        // indented by two columns is not the same row. That is the caret's
        // ladder, gated by `the_caret_degrades_once_and_never_flickers` and
        // `the_caret_does_not_vanish_because_another_file_changed`; this gate is
        // about the bar and must not be measuring both at once.
        let caret = |row: &str| row.trim_start().len() != row.len();
        if caret(&barred[1]) != caret(&bare[1]) {
            continue;
        }

        // Only where a bar is actually drawn. Below the floor the two screens are
        // one column apart and genuinely draw different rows.
        if barred[1].ends_with('▕') || barred[1].ends_with('█') {
            assert_eq!(
                barred_row, bare_row,
                "at {width} columns a list row with a bar reads {barred_row:?} \
                 where the same row one column narrower without one reads \
                 {bare_row:?}, so the bar costs more than its column"
            );
            compared += 1;
        }
    }

    assert!(
        compared > 10,
        "only {compared} widths drew a bar, so this compared almost nothing"
    );
}

#[test]
fn a_list_shorter_than_its_region_gives_the_rows_to_the_diff() {
    // `Body::clamped_to`'s whole reason for existing, and nothing reached it
    // until mutation said so: stopping it giving the rows back left every gate
    // green.
    //
    // The case is a **stale view** — one redrawn after a failed collect, holding
    // fewer entries than the pane would afford. The region has to shrink to what
    // the view actually has, or it draws blank rows under a rule announcing files
    // that are not there; and the rows it gives up have to reach the diff, or
    // they are left unpainted between the rule and the content, which is the
    // half-empty pane #59 was filed for wearing a different hat.
    let tall = 24u16;
    let width = 80u16;
    let chrome = chrome();

    // Ten changed files, so the layout affords a full six-row region, but a view
    // carrying only two entries.
    let afforded = body_layout(Rect::new(0, 0, width, tall), &chrome, 10);
    assert!(
        afforded.list > 2,
        "the fixture does not under-fill the region: it affords {} rows",
        afforded.list
    );

    let view = numbered(usize::from(tall) + 8, 10, 2);
    assert_eq!(view.list.len(), 2, "the fixture is not a short list");

    let rows = rows_at(width, tall, &view, &chrome);
    let painted = rows.join("\n");

    // Two list rows, then the rule, then content all the way to the footer. The
    // count is derived from what the layout gave back rather than restated, so it
    // follows a change to the cap.
    let clamped = afforded.clamped_to(view.list.len());
    assert_eq!(clamped.list, 2);
    assert!(clamped.rule, "a two-row list still needs its rule");
    assert_eq!(
        clamped.diff,
        afforded.diff + (afforded.list - 2),
        "the rows the list gave up did not reach the diff"
    );

    let drawn = (0..usize::from(tall) + 8)
        .filter(|i| painted.contains(&format!("R{i:02}")))
        .count();
    assert_eq!(
        drawn, clamped.diff,
        "the pane drew {drawn} content rows where the clamped split says \
         {}, so rows the list gave up went unpainted",
        clamped.diff
    );

    // And no blank row between the rule and the first content row.
    let rule_at = rows
        .iter()
        .position(|row| row.starts_with('─'))
        .expect("a rule");
    assert!(
        rows[rule_at + 1].contains("R00"),
        "row {} under the rule is {:?}, not the first content row",
        rule_at + 1,
        rows[rule_at + 1]
    );
}
