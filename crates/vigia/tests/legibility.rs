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
    Body, Chrome, FileEntry, HEAT_BUCKETS, HINT_SEPARATOR, HeatBucket, Mode, Position, Row, Theme,
    View, body_layout, diff_height, render,
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

/// #119's margin ladder, widest pane first: blank columns the pane keeps between
/// its own edge and any glyph, **both sides counted together**.
///
/// Restated rather than imported, for [`CONTINUES`]' and `FACT_JOIN`'s reason and
/// for the one this file states over the glance rungs: *a test that read the
/// renderer's own table would agree with it by construction instead of checking
/// it.* Every width in this file is derived against these numbers, so importing
/// them would make the whole sweep unfalsifiable in exactly the direction it
/// exists to watch.
///
/// A total rather than a per-side figure because a per-side ladder steps both
/// sides on one column and hands a widening pane a narrower row, which
/// `a_bonus_hint_rung_never_buys_itself_a_footer_row` below is what catches. The
/// odd rungs at 43 and 79 are the step between the two even ones #119 names.
const MARGIN_RUNGS: [(u16, u16); 4] = [(80, 4), (79, 3), (44, 2), (43, 1)];

/// What marks the row for the file the diff is inside.
///
/// Restated rather than imported, on this file's own rule two docblocks up.
const CARET: &str = "▸";

/// Columns that mark occupies.
///
/// **One since [#173](https://github.com/breferrari/vigia/issues/173)**, where it
/// was two: a glyph and a trailing space, which the list was indented by and
/// which put its status sigil two columns right of the same sigil on a diff
/// heading. The gap after it is [`MARGIN_RUNGS`]'s now, so it is a column wide at
/// every width and the pane's own ladder decides what follows it.
const CARET_WIDTH: usize = 1;

/// Columns a pane this wide spends on margins, both sides together. What a row's
/// text loses off the pane's own width.
fn margin_at(width: u16) -> usize {
    MARGIN_RUNGS
        .iter()
        .find(|(from, _)| width >= *from)
        .map_or(0, |(_, cells)| usize::from(*cells))
}

/// The column a pane this wide begins drawing text at: the margin above, split
/// evenly with the odd column going left.
///
/// Both exist because a fit predicate wants the whole margin while a cell read
/// wants the left half, and at the odd rungs those are different numbers: at 43
/// the margin is 1 and the inset is 1, at 79 the margin is 3 and the inset is 2.
/// Reaching for one where the other belongs is off by a column at exactly the
/// two widths nothing else singles out.
fn inset_at(width: u16) -> usize {
    margin_at(width).div_ceil(2)
}

/// A row with its caret taken off, and the columns the caret took.
///
/// **Written once because three sites had it by hand.** [`content`], the inset
/// sweep and [`sigil_column`] each matched `strip_prefix(CARET)` and each
/// adjusted an offset by [`CARET_WIDTH`], which is three places that have to
/// agree about what a marked row's head looks like with no compiler help if the
/// marker ever changes width or regains a trailing space. `content`'s own
/// docblock argues exactly this about the callers *it* consolidated, and the
/// diff that added it then hand-wrote the same shape twice more.
///
/// An unmarked row comes back untouched with a cost of zero, so a caller can use
/// the pair unconditionally.
fn past_caret(row: &str) -> (&str, usize) {
    match row.strip_prefix(CARET) {
        Some(rest) => (rest, CARET_WIDTH),
        None => (row, 0),
    }
}

/// A drawn row with the pane's inset taken off its head, having first checked
/// that the inset is exactly what is there.
///
/// **The check and the strip are one operation on purpose, and that is the whole
/// reason this exists rather than a `trim_start`.** Trimming would pass whatever
/// the pane did: two columns, five columns, or a row drawn at column zero when
/// the ladder says otherwise. Every assertion in this file that reads a row from
/// its head goes through here, so the inset is asserted once per read instead of
/// once in a gate that the other reads then quietly stop covering.
///
/// A blank row has nothing to say about the inset and is handed back untouched,
/// since [`rows_at`] has already trimmed it to nothing.
fn content(row: &str, width: u16) -> &str {
    if row.is_empty() {
        return row;
    }
    // **The caret stands in the pane's margin by licence**
    // ([#173](https://github.com/breferrari/vigia/issues/173)), so it is taken
    // off here along with the blank the margin still has left. Done in the shared
    // reader rather than at each caller, because a rule about what a row's head
    // looks like belongs where every read of a head goes through, and the
    // alternative is thirty call sites that each remember it or quietly stop
    // covering the marked row.
    //
    // The remaining inset is still **asserted** below, not trimmed: a marked row
    // that also lost its margin fails here exactly as an unmarked one would.
    let (row, taken) = past_caret(row);
    let inset = inset_at(width).saturating_sub(taken);
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

/// The narrowest text column the renderer will keep a gutter for, restated
/// rather than imported for [`CONTINUES`]' reason.
///
/// A test that read `MIN_TEXT_WIDTH` would agree with the renderer by
/// construction, and agreeing with it is exactly what let this number go
/// unenforced: it was a threshold two expressions held by hand, and when
/// [#164](https://github.com/breferrari/vigia/issues/164) gave the sigil a clear
/// column one of them was not recharged, so the gutter survived on 23 columns
/// of text while the constant read 24 and every gate stayed green.
const MIN_TEXT_WIDTH: usize = 24;

/// The eighth-blocks a sparkline is drawn from, restated rather than imported.
///
/// A test sharing the renderer's own table would agree with it by construction.
const RAMP: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// The block one heat slice is drawn as, restated for the same reason as
/// [`RAMP`].
const HEAT_SLICE: char = '■';

/// What a sparkline bucket nothing was written in draws, restated for [`RAMP`]'s
/// reason.
const TRACK: char = '_';

/// Cells on row `y` whose foreground is one of `colours`, in column order.
///
/// **Symbol and colour together, because neither alone separates the two strips
/// any more.** The sparkline's top rung is `█` and so is every heat slice, so a
/// symbol-only match counts one as the other. Both gates below were symbol-only
/// and one of them silently started counting eighteen buckets the moment the
/// heat strip landed.
///
/// The colour half has changed hands twice and is worth keeping straight. It
/// read "the heat track is the same dim grey as the `+42 -7` counters", which
/// was true when `chrome_dim` was colour 8 and stopped being true when
/// [#60](https://github.com/breferrari/vigia/issues/60) lifted it off. What
/// makes the colour term load-bearing **now** is the other direction: since
/// [#157](https://github.com/breferrari/vigia/issues/157) a counts cell is drawn
/// in `Theme::added` and `Theme::removed`, and those collide with the heat
/// strip's own ramp rather than with its track. On `ansi` they are `Green` and
/// `Red`, the names `heat_added` and `heat_removed` also take; on `dark`
/// `added` is `heat_added`'s `#3fb950` and `removed` is `heat_removed_warm`'s
/// `#f85149`, and [`heat_colours`] collects every rung. So a colour-only match
/// still counts the counters as heat slices, which is what this doc always
/// claimed and now has the right reason for. The symbol term is what separates
/// them: a slice is `█` and a counter is digits.
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

/// How many columns of row `y` the sparkline slot occupies, bars and track
/// together.
///
/// **The slot rather than the data, which is what the ladder is about.** Since
/// [#78](https://github.com/breferrari/vigia/issues/78) an empty bucket draws
/// the track, so counting bars alone would read a rung as narrower than it is
/// wherever a file's history has a gap in it, and as *zero* on a file with no
/// history. Both gates below read a rung off this.
///
/// Two calls rather than one with both symbol sets and both colours, because
/// [`cells_coloured`] would then accept the cross products: a `_` in the bucket
/// colour, or a block in the track's. Neither is ever drawn, and a helper that
/// would count them is the loose selector this file's own doc warns about.
fn spark_slot(backend: &TestBackend, y: u16, theme: &Theme) -> usize {
    // **Every stop of the ramp**, which is three since #196 and was one. A
    // helper reading only the quietest would count a busy row as mostly empty
    // and the slot would look narrower than it is.
    let bars: Vec<_> = [theme.spark, theme.spark_warm, theme.spark_hot]
        .into_iter()
        .filter_map(|style| style.fg)
        .collect();
    let track = theme.spark_track.fg.expect("the track has a colour");
    cells_coloured(backend, y, &bars, &RAMP).len()
        + cells_coloured(backend, y, &[track], &[TRACK]).len()
}

/// Every row the body holds, across every region it has.
///
/// **Named because two gates recover the footer's height by subtracting this**,
/// and both got it wrong the same way when a region was added: they each summed
/// `list + rule + diff` inline, so #158's masthead made them read the footer as
/// four rows taller than it is. A region added to `Body` is one edit here now.
fn body_rows(split: &vigia::Body) -> usize {
    split.rows()
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
        pressed: None,
        gripped: None,
        hovered: None,
        scrolling: None,
        worktree: "vigia".to_owned(),
        // Only the empty state names a branch, so every populated fixture leaves
        // this `None`. That is not tidiness: it is what a real frame carries,
        // because a detached head names no branch anywhere, which is the one
        // case that draws none since #158.
        branch: None,
        mode: Mode::Watching,
        notice: None,
        following: false,
        masthead: true,
        sheet: false,
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
        pressed: None,
        gripped: None,
        scrolling: None,
        frame: Some(Duration::from_millis(999)),
        memory: Some(999 * 1024 * 1024),
        ..following()
    }
}

fn following() -> Chrome {
    Chrome {
        pressed: None,
        gripped: None,
        scrolling: None,
        following: true,
        masthead: true,
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
        pressed: None,
        gripped: None,
        scrolling: None,
        mode: Mode::Lost,
        ..chrome()
    }
}

/// A worktree with nothing in it, on a branch.
fn on_a_branch() -> Chrome {
    Chrome {
        pressed: None,
        gripped: None,
        scrolling: None,
        branch: Some("main".to_owned()),
        ..chrome()
    }
}

fn with_notice() -> Chrome {
    Chrome {
        pressed: None,
        gripped: None,
        scrolling: None,
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
        worktree_churn: Default::default(),
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
        worktree_churn: Default::default(),
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
        worktree_churn: Default::default(),
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
        worktree_churn: Default::default(),
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
/// **Takes the pane width so it can strip #119's inset before matching**, and
/// that is a correctness argument rather than a convenience. This helper reads
/// the row from its head, and its `common.is_empty()` arm returns *honest* — the
/// legal "dropped entirely" case above. Once the pane insets its text, a row that
/// no longer begins with the label matches nothing, `common` is empty, and every
/// caller silently reads `true` for every width: the gate does not fail, it stops
/// existing. [`content`] both removes the inset and asserts it was exactly there,
/// so the empty case means dropped again and cannot mean displaced.
fn label_is_honest(row: &str, label: &str, width: u16) -> bool {
    let row = content(row, width);
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
    // **Built from [`pinned_and_streamed`] rather than beside it.** The two were
    // field-for-field identical apart from `top`, so adding a fourth file here
    // or changing a path's length would have left the gates that read the other
    // copy measuring a shape this file's own sweeps no longer use.
    let pinned = View {
        top: Position { file: 1, row: 0 },
        ..pinned_and_streamed()
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
                pressed: None,
                gripped: None,
                scrolling: None,
                following: true,
                masthead: true,
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
        // **A worktree whose name is two columns per character**, which nothing
        // put through the header's left before. A directory name is whatever the
        // filesystem holds, and since #67 the left is a clause the ladder builds
        // by `format!` and `put_marked` then marks — so the reserved-mark column
        // is exercised here against wide glyphs the way
        // `a_wide_glyph_at_the_edge_does_not_swallow_the_mark` exercises it on a
        // content row. Same reasoning as the row above: added to the shared list
        // so every structural sweep covers it rather than one new gate.
        (
            "a wide worktree name, following",
            every_row_kind(),
            Chrome {
                pressed: None,
                gripped: None,
                scrolling: None,
                worktree: "読み方リポジトリ".to_owned(),
                ..following()
            },
        ),
        (
            "clean worktree, watch lost",
            empty(),
            Chrome {
                pressed: None,
                gripped: None,
                scrolling: None,
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
                pressed: None,
                gripped: None,
                scrolling: None,
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
/// **The two live rows' buckets are non-zero on purpose**, and their counts
/// differ, so the shared scale is exercised down the list. The third row is all
/// zeroes, because a cold file is what the ladder has to keep drawing too.
///
/// This used to say every bucket was non-zero *because* an empty one drew a
/// space, which made a strip's width unreadable off the row and left the ladder
/// gate below unable to tell four buckets from eight. Both halves are now
/// wrong: since [#78](https://github.com/breferrari/vigia/issues/78) an empty
/// bucket draws the track, so the width is readable off any fixture, and the
/// third row was never non-zero anyway. `spark_slot` counts the whole slot for
/// exactly this reason. **This is the sentence the two gates below took their
/// rationale from**, so it is the one that had to move for their corrections to
/// mean anything.
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
        worktree_churn: Default::default(),
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
            // The sigil and its gap cost two columns beyond the text itself
            // ([#164](https://github.com/breferrari/vigia/issues/164)), so a row
            // is clipped once the text reaches `room - 1` rather than `room`.
            //
            // **This guard is where that lands, and the arithmetic is easy to
            // put in the wrong place.** The `+ 1` further down is the
            // continuation mark and is unaffected; it is this comparison that
            // decides which widths the sweep bothers looking at, so leaving it
            // at `room` silently skips the widths where the new column is what
            // pushed the row over. The gate would still have passed, on a
            // narrower set of widths than it claims, which is the shape its own
            // non-vacuity flag exists to catch and would not have caught here.
            //
            // **Against the room the row is given, not against the pane.** #119
            // takes the margin off both sides first, so a line whose width falls
            // between the two is genuinely clipped while a guard written against
            // `width` skips it, which is coverage lost silently rather than a
            // failure.
            //
            // **On today's fixture this changes nothing, and saying so is worth
            // more than the change.** The margin is zero below 43 columns and the
            // widest line here is twenty-six, so every width this sweep reaches has
            // `room == width`. That was measured rather than assumed: instrumented,
            // the non-vacuity flag below trips at widths 1 to 26 and nowhere else,
            // before this edit and after it. It is written against the room so the
            // gate stays correct if `awkward()` ever gains a wider line, not
            // because it reaches a hazard today that it did not reach before.
            let room = usize::from(width).saturating_sub(margin_at(width));
            // Read as "the text plus the sigil and its gap still fit", which
            // names the two columns instead of leaving a reader to hold two
            // different `+ 1`s twenty lines apart. Written as an addition rather
            // than `room - 2` because `room` reaches zero in this sweep.
            if Span::raw(full).width() + 2 <= room {
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
            //
            // The final **content** column, for the guard's reason and with the
            // same caveat. `rows_at` keeps the leading blanks, so a row that fills
            // its room measures the inset plus the room, which is the pane less
            // whatever the right-hand margin took. Below 43 columns that is the
            // pane itself, which is every width this fixture reaches, so the two
            // spellings agree today and only the wider fixture would tell them
            // apart.
            let trailing = margin_at(width) - inset_at(width);
            if Span::raw(row.trim_end_matches(CONTINUES)).width() + 1
                == usize::from(width) - trailing
            {
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
    // and would spend a body row on a maybe. The **left** drops a whole rung
    // instead, and the right drops its one token whole, which between them are
    // what make one row always enough.
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
                    // **Where the body starts, not where row one is.** #158 put
                    // a masthead between the header and the list, so the first
                    // content row is one plus whatever that region takes. Asked
                    // of the layout rather than restated, which keeps this a
                    // gate on the *header* rather than a second copy of the
                    // body split.
                    // Clamped to what the view actually holds, which is what `render`
                    // does: the unclamped split answers what the *pane* affords,
                    // and a view with no entries draws B3's sentence rather than
                    // a masthead over blank rows.
                    let body = body_layout(Rect::new(0, 0, width, height), &chrome, view.files)
                        .clamped_to(view.list.len());
                    let starts = list_top(&body);
                    assert_eq!(
                        first, starts,
                        "at {width}x{height} with {listed} listed, the body \
                         started on row {first} rather than {starts}, so the \
                         header took more than one"
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
    // **Three file counts, because the count's own width moves every width at
    // which the ladder changes rung.** `3 changed` is nine columns, `100
    // changed` eleven and `12345 changed` thirteen, so a sweep at one digit
    // exercises one rung-swap width and reads as though it covered them all,
    // which is the trap the two mode words are already here for.
    for (word, chrome) in [("watching", chrome()), ("not watching", lost())] {
        for files in [3usize, 100, 12_345] {
            let view = View {
                files,
                ..every_row_kind()
            };
            let full = format!("{}{FACT_JOIN}{files} changed", chrome.worktree);
            let (mut saw_both, mut saw_word_only, mut saw_neither) = (false, false, false);

            for width in WIDTHS {
                let drawn = rows_at(width, 8, &view, &chrome)[0].clone();
                // #119's inset comes off the head before the row is read at all,
                // through [`content`], which asserts it was exactly there rather
                // than trimming whatever it finds. The right-hand inset never
                // reaches the `strip_suffix` below, because `rows_at` has already
                // trimmed the row's trailing blanks.
                let header = content(&drawn, width);
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
                "{word} at {files} files: the sweep saw both={saw_both} \
                 word-only={saw_word_only} neither={saw_neither}, so it did not \
                 cover the whole ladder"
            );
        }
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

    // **And a clean worktree draws no number at any width**, which `SPEC.md`
    // §11.1 rules ("the count is nothing at all when it is zero") and which only
    // three empty-state snapshots covered. `0 changed` would spend columns
    // restating what the body says in words, and the sweep above cannot see it:
    // every width it walks has three changed files.
    let clean = empty();
    for chrome in [chrome(), lost()] {
        for width in WIDTHS {
            let header = rows_at(width, 8, &clean, &chrome)[0].clone();
            assert!(
                !header.contains(|c: char| c.is_ascii_digit()),
                "at {width} columns a worktree with nothing in it drew a number \
                 on the header: {header:?}"
            );
        }
    }
}

#[test]
fn the_header_facts_degrade_through_one_recorded_sequence() {
    // The header's version of `the_caret_degrades_once_and_never_flickers` and
    // `the_scrollbars_degrade_once_and_never_flicker`, and it exists because the
    // header is **not** monotone and that was ruled deliberate rather than
    // fixed. The two sides have independent budgets, so widening a live pane
    // from 7 to 8 columns *removes* the worktree name. `SPEC.md` §11.1 records
    // the bands and `the_header_degrades_at_the_widths_the_spec_records` is what
    // holds those numbers to the renderer; this gate owns the shape.
    //
    // **The name is three-valued and reading it as a boolean was wrong.** A
    // marked fragment is a different screen from an absent name — `vig› watching`
    // against ` watching` — and collapsing them let a mutation that made the left
    // degrade to nothing instead of marking pass this gate untouched. Narrowing
    // from 120 on a live watch:
    //
    // | name | count | word | widths | why |
    // |---|---|---|---|---|
    // | whole | yes | yes | 26+ | everything fits |
    // | whole | no | yes | 14-25 | the count is the first rung to go |
    // | marked | no | yes | 10-13 | the name cannot fit what is left |
    // | absent | no | yes | 8-9 | the word is placed first and takes the row |
    // | whole | no | no | 5-7 | below the word's own width there is nothing to place |
    // | marked | no | no | 1-4 | not even the name fits |
    //
    // **Asserted as a subsequence, because the full list is the five-column
    // fixture's.** A one-character name never reaches `marked` (it always fits
    // what it is given), and a name longer than the pane never reaches `whole`,
    // so both legitimately skip states. What must never happen is a state that
    // is not on this list at all, or two of them in the wrong order. The
    // fixture's own sweep is asserted to produce the whole list, so the
    // subsequence rule cannot pass vacuously.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Name {
        Whole,
        Marked,
        Absent,
    }

    const ACCEPTED: [(Name, bool, bool); 6] = [
        (Name::Whole, true, true),
        (Name::Whole, false, true),
        (Name::Marked, false, true),
        (Name::Absent, false, true),
        (Name::Whole, false, false),
        (Name::Marked, false, false),
    ];

    // Hoisted past the shadowing below, and it is the name the full walk is
    // claimed of.
    let fixture_name = chrome().worktree;

    for (word, chrome) in [("watching", chrome()), ("not watching", lost())] {
        // Three name widths, because the sequence a pane walks depends on the
        // name's own width and pinning one fixture's walk pins a coincidence.
        for name in [fixture_name.clone(), "v".to_owned(), "a".repeat(64)] {
            let chrome = Chrome {
                pressed: None,
                gripped: None,
                hovered: None,
                scrolling: None,
                worktree: name.clone(),
                ..chrome.clone()
            };
            let view = every_row_kind();
            let mut seen: Vec<(Name, bool, bool)> = Vec::new();

            for width in WIDTHS.rev() {
                let drawn = rows_at(width, 8, &view, &chrome)[0].clone();
                // See the identical strip in `the_header_ladder_keeps_the_mode_word_last`.
                let header = content(&drawn, width);
                let has_word = header.ends_with(word);
                let left = header.strip_suffix(word).unwrap_or(header).trim_end();
                let has_count = left.contains(FACT_JOIN);

                let drawn: String = left
                    .chars()
                    .zip(name.chars())
                    .take_while(|(row, want)| row == want)
                    .map(|(row, _)| row)
                    .collect();
                let named = if left.starts_with(&name) {
                    Name::Whole
                } else if left[drawn.len()..].starts_with(CONTINUES) {
                    Name::Marked
                } else {
                    Name::Absent
                };

                assert!(
                    !has_count || named == Name::Whole,
                    "at {width} columns the count is drawn without the whole \
                     name it modifies: {header:?}"
                );

                let facts = (named, has_count, has_word);
                if seen.last() != Some(&facts) {
                    seen.push(facts);
                }
            }

            // A subsequence of the accepted walk: every state is on the list and
            // they arrive in the listed order.
            let mut accepted = ACCEPTED.iter();
            for state in &seen {
                assert!(
                    accepted.any(|want| want == state),
                    "{word}, name {:?}: the header enters {state:?}, which is \
                     not the next state `SPEC.md` §11.1 records, so it gains or \
                     loses a fact at a width nothing else would catch: {seen:?}",
                    name.chars().count()
                );
            }

            // And the fixture's own name walks all six, or the rule above holds
            // only because the sweep never reached the interesting states.
            if name == fixture_name {
                assert_eq!(
                    seen.as_slice(),
                    ACCEPTED.as_slice(),
                    "{word}: the five-column fixture no longer walks every \
                     recorded state, so the subsequence rule above is vacuous"
                );
            }
        }
    }
}

#[test]
fn the_glance_columns_collapse_in_one_order() {
    // #77's ladder. The columns are decided once for a region now, so the order
    // they give way in is a property of the region rather than of a row, and it
    // is the one `SPEC.md` §11.1 records: counts, then the pulse, then heat,
    // then sparkline.
    //
    // Swept rather than sampled, because a ladder is only ever wrong at the
    // widths where it changes rung, and those move with the fixture's counts.
    //
    // Read off the drawn row by colour and glyph together, for the reason the
    // renderer's own doc gives: the heat strip and a full sparkline bucket draw
    // the same block, and the pulse shares a foreground with the sparkline.
    // Restated rather than imported, the way `CONTINUES` and `FACT_JOIN` are: a
    // test that read the renderer's own table would agree with it by
    // construction instead of checking it. `RAMP` and `HEAT_SLICE` are *not*
    // restated here, because this file already declares them at the top and a
    // second copy would check the first copy rather than the renderer.
    const HEAT_RUNGS: [usize; 3] = [HEAT_BUCKETS, HEAT_BUCKETS / 2, 0];
    const SPARK_RUNGS: [usize; 3] = [HISTORY_BUCKETS, HISTORY_BUCKETS / 2, 0];
    // The first width each state is drawn at, and `(counts, heat slices,
    // sparkline buckets)`. **The widths are pinned as well as the order**,
    // because a sequence alone is a weak gate: removing the gap the heat strip
    // reserves shifts every boundary below it by a column and leaves the walk
    // itself identical, and that mutation survived this test until the widths
    // went in.
    //
    // **Derived rather than recorded from a run**, because a number copied out
    // of a failure message agrees with the renderer by construction and gates
    // nothing. Each boundary is `ROW_FLOOR` plus `BAR_WIDTH` plus the layout's
    // own width, and a layout's width is each slot plus the one column of gap
    // `reserved` adds:
    //
    // | Layout | counts | pulse | heat | spark | width | from |
    // |---|---|---|---|---|---|---|
    // | 5 | 12 | 0 | 0 | 0 | 12 | 28 |
    // | 4 | 12 | 2 | 0 | 0 | 14 | 30 |
    // | 3 | 12 | 2 | 7 | 0 | 21 | 37 |
    // | 2 | 12 | 2 | 7 | 5 | 26 | 42 |
    // | 1 | 12 | 2 | 13 | 5 | 32 | 49 |
    // | 0 | 12 | 2 | 13 | 9 | 36 | 53 |
    //
    // The `from` column is the narrowest pane whose `ROW_FLOOR + BAR_WIDTH +
    // inset + width` fits: a region is planned against the pane less a scrollbar
    // column **whether or not one is drawn**, because whether one is drawn is a
    // fact about the contents and the layout is not allowed to be. That is the
    // whole of `planning_width`.
    //
    // **The `inset` term is #119's, and solving for the pane rather than adding
    // to it is why the last two rows read 49 and 53.** The pane keeps blank
    // columns between its edge and any glyph, on a ladder of its own: none below
    // 44 columns, one from 44, two from 80. The first four boundaries here sit
    // under 44 and are therefore untouched. The last two sit inside the
    // one-column rung, and the term is not simply `+1` on each: at 48 columns the
    // inset is already being charged, so 48 leaves 45 planning columns where
    // layout 1 needs 46, and the boundary is the first width that clears it.
    //
    // **Both were predicted from this table before the change was run, and both
    // came out exactly.** That is the check #119 owed here, and it is worth more
    // than the numbers: because the layouts are a written-out table rather than a
    // greedy allocation, which boundaries move is decidable by reading, so a
    // derivation that had missed would have meant the hand table and the constant
    // table disagree, which no green run would have said.
    //
    // Layout 4 is absent below because it changes nothing this test can see: it
    // differs from 5 only by the pulse mark, and the pulse is read by neither of
    // the two counts.
    //
    // **The table lost a row on 2026-08-03 and the walk did not move**, which is
    // the check that ruling owed this test. A seventh layout used to sit above
    // layout 0 carrying `● just changed`, which is fourteen columns of text and
    // fifteen of slot once `reserved` adds its gap, so it read `15` in the pulse
    // column of this table. From 65 columns up, and it went with the label along
    // with the only rung that column ever had. Every `width` here is the sum of one
    // layout's own slots, so dropping the widest one cannot shift the five below
    // it, and the boundaries below are unchanged rather than re-derived.
    //
    // The walk has moved twice on this branch and both moves are recorded
    // because both cost something visible:
    //
    // - **Four columns up** when the counts cell stopped degrading (22, 31, 36
    //   became 26, 35, 40), which is both halves going from three columns to
    //   five. It bought a cell that never draws `+0k` for a 250-line change.
    // - **Two more** when the scrollbar column became unconditional (26, 35, 40
    //   became 28, 37, 42). It bought a layout that cannot be moved by a seventh
    //   changed file appearing.
    //
    // The bill for both lands on the sparkline, which now needs 42 columns where
    // it used to need 36. §11.1 carries the argument.
    //
    // - **One more on the top two rungs only** when the pane took #119's inset
    //   (48, 52 became 49, 53). Nothing under 44 columns moved, because the
    //   ladder's own floor is what buys I6's forty-column pane its columns back.
    const ACCEPTED_WALK: &[(u16, (bool, usize, usize))] = &[
        (1, (false, 0, 0)),
        (28, (true, 0, 0)),
        (37, (true, 6, 0)),
        (42, (true, 6, 4)),
        (49, (true, 12, 4)),
        (53, (true, 12, 8)),
    ];
    let theme = theme();
    let heats = heat_colours(&theme);
    let view = glancing();
    let (mut saw_all, mut saw_none) = (false, false);
    let mut seen: Vec<(u16, (bool, usize, usize))> = Vec::new();

    for width in WIDTHS {
        let backend = drawn(width, 8, &view, &chrome());
        // Row 1 is the first file heading: `glancing`'s rows start at the top of
        // the body and the header owns row 0.
        let y = 1u16;
        // Through `cells_coloured`, which this file already uses for exactly
        // these two counts. A fourth hand-rolled walk would be a fourth spelling
        // of "is this cell a heat slice", and the one that drifts is the one
        // nobody is reading.
        let heat = cells_coloured(&backend, y, &heats, &[HEAT_SLICE]).len();
        // The whole slot, bars and track together: the ladder is about how many
        // columns the element is given, and since #78 an empty bucket fills its
        // column rather than leaving it. Counting bars alone happens to agree
        // here, because `glancing`'s first row has no empty bucket, and that
        // agreement is a property of the fixture rather than of the renderer.
        let spark = spark_slot(&backend, y, &theme);
        let counts = rows_at(width, 8, &view, &chrome())[usize::from(y)].contains('+');

        // **Whole rungs.** A count of slices or buckets that is not on the
        // ladder is a strip shaved one item at a time, which §11.1 forbids for a
        // thing made of items: it drops whole ones or none.
        assert!(
            HEAT_RUNGS.contains(&heat),
            "at {width} columns the heat strip drew {heat} slices, which is not \
             one of its rungs"
        );
        assert!(
            SPARK_RUNGS.contains(&spark),
            "at {width} columns the sparkline drew {spark} buckets, which is not \
             one of its rungs"
        );

        let state = (counts, heat, spark);
        if seen.last().map(|(_, s)| s) != Some(&state) {
            seen.push((width, state));
        }
        if counts && heat > 0 && spark > 0 {
            saw_all = true;
        }
        if !counts && heat == 0 && spark == 0 {
            saw_none = true;
        }
    }

    // Both ends of the ladder, or the sweep only ever saw one of them.
    assert!(
        saw_all && saw_none,
        "the sweep saw everything drawn={saw_all} and nothing drawn={saw_none}, \
         so it did not cover the whole ladder"
    );

    // **Monotone, which is the property a reader dragging a pane edge notices.**
    // Widening must never take an element away. It did before the layouts were
    // written out as a table: allocating element by element in priority order
    // lost the sparkline at 37 columns, returned it at 40 and dropped both
    // glance elements at 41, because each element took the widest rung it could
    // afford and starved whatever came after it.
    //
    // Asserted as a rule *and* pinned as a walk. The rule is what matters and
    // the walk is what catches a renderer that stayed monotone while moving a
    // boundary, which a sequence alone cannot see.
    for pair in seen.windows(2) {
        let ((below, (was_counts, was_heat, was_spark)), (above, (counts, heat, spark))) =
            (pair[0], pair[1]);
        assert!(
            (counts || !was_counts) && heat >= was_heat && spark >= was_spark,
            "widening from {below} to {above} columns took something away: \
             {:?} became {:?}",
            pair[0].1,
            pair[1].1
        );
    }

    assert_eq!(
        seen.as_slice(),
        ACCEPTED_WALK,
        "the glance ladder walks a different set of states than §11.1 records"
    );
}

#[test]
fn the_header_degrades_at_the_widths_the_spec_records() {
    // `SPEC.md` §11.1 quotes measured column numbers for where the header's
    // facts appear and vanish, and prose carrying a measurement drifts from the
    // thing it measured. It already had: the spec said both facts appear from
    // 13 and the renderer draws `vig› watching` there, with the whole name only
    // from 14. Nothing failed, because the number lived in two documents and no
    // test.
    //
    // So the bands are asserted here and the spec cites this test. Both mode
    // words, because the word's own width shifts every boundary by four.
    // Every number §11.1 quotes, not a subset of them. Citing the gate for the
    // paragraph while asserting six of its ten numbers is the same false comfort
    // as not citing one at all.
    //
    // **The count's own boundary is here for a reason worth stating.** Every
    // other number on this list is a *degradation* width, and pinning only those
    // left the count's appearance movable on the most ordinary screen there is:
    // two separate one-column mutations made `vigia`, three files, a live watch
    // lose its count at 26 columns with the entire workspace green. The rung
    // order was gated, the marking was gated, the styles were gated, and the
    // width at which the header's own number arrives was gated by nothing.
    const BANDS: [(&str, u16, u16, u16, u16, u16, u16); 2] = [
        // word; first and last width the name has the row alone; first width the
        // word has it alone; first width a marked fragment rejoins it; first
        // width both are drawn whole; first width the count joins them.
        ("watching", 5, 7, 8, 10, 14, 26),
        ("not watching", 5, 11, 12, 14, 18, 30),
    ];

    // Guard the fixture, the way this file's other sweeps do. Every number above
    // is hand-derived from two fixture properties: a five-column name and three
    // changed files. They are literals rather than a computation on purpose, so
    // that a renderer which moved a boundary has to change this table too, but
    // that only holds while the fixture is what they were derived from.
    assert_eq!(
        (chrome().worktree.chars().count(), every_row_kind().files),
        (5, 3),
        "the fixture moved out from under the band table, so its widths are \
         derived from something that no longer exists"
    );

    for (word, first_alone, name_alone, word_alone, rejoins, both, counted) in BANDS {
        let chrome = if word == "watching" { chrome() } else { lost() };
        let view = every_row_kind();
        let name = chrome.worktree.clone();
        let row = |width: u16| rows_at(width, 8, &view, &chrome)[0].clone();

        for width in [first_alone, name_alone] {
            let alone = row(width);
            assert_eq!(
                alone.trim(),
                name,
                "at {width} columns the name should have the row to itself"
            );
        }
        // One below the first is where it stops being the whole name, which is
        // what makes `first_alone` a boundary rather than a width that happens
        // to work.
        let under = row(first_alone - 1);
        assert_ne!(
            under.trim(),
            name,
            "at {} columns the name is already whole, so {first_alone} is not \
             the first width that holds it",
            first_alone - 1
        );

        let fragment = row(rejoins);
        assert!(
            fragment.ends_with(word) && fragment.contains(CONTINUES),
            "at {rejoins} columns a marked fragment of the name should share the \
             row with the mode word: {fragment:?}"
        );
        let before = row(rejoins - 1);
        assert!(
            !before.contains(CONTINUES),
            "at {} columns the name already shares the row, so {rejoins} is not \
             where it rejoins: {before:?}",
            rejoins - 1
        );

        let taken = row(word_alone);
        assert_eq!(
            taken.trim(),
            word,
            "at {word_alone} columns the mode word should have taken the row"
        );

        let together = row(both);
        assert_eq!(
            together.trim(),
            format!("{name} {word}"),
            "at {both} columns both facts should be drawn whole"
        );

        // The count's own arrival, and the column below it. `every_row_kind`
        // carries three changed files, so the clause is `{name} · 3 changed`.
        let clause = format!("{name}{FACT_JOIN}3 changed");
        let with_count = row(counted);
        assert_eq!(
            with_count.trim(),
            format!("{clause} {word}"),
            "at {counted} columns the count should have joined the name"
        );
        let without = row(counted - 1);
        assert!(
            !without.contains(&clause),
            "at {} columns the count is already drawn, so {counted} is not the \
             first width that holds it: {without:?}",
            counted - 1
        );

        // And one column below `both` the name is not yet whole, which is what
        // makes `both` the *first* such width rather than merely one of them.
        //
        // The name alone, **not** the name followed by a space. Looking for the
        // space was the first spelling and it left a mutation alive: reclaiming
        // the column `put_right` reserves for the gap draws `vigiawatching`, the
        // two facts fused with nothing between them, at five widths and with the
        // whole workspace green. That is worse than the defect #67 was filed
        // about, and this assertion is the only thing on the row that can see it.
        let below = row(both - 1);
        assert!(
            !below.contains(name.as_str()),
            "at {} columns the whole name is already drawn, so {both} is not the \
             first width that fits both: {below:?}",
            both - 1
        );
    }
}

#[test]
fn a_worktree_name_too_long_for_its_room_is_marked_rather_than_cut_silently() {
    // The header-side twin of `a_wide_glyph_at_the_edge_does_not_swallow_the_mark`,
    // and the hole it fills is specific: `label_is_honest` counts "dropped
    // entirely" as honest, which is correct for a right-hand token but blind
    // here, because a name that lost characters and said nothing looks exactly
    // like one that was never drawn. So `a_label_cut_at_the_right_edge_says_so`
    // cannot see a missing mark on this row, and `put_marked`'s reserved column
    // (`limit - 1`) survives being mutated to `limit` against the whole suite.
    //
    // **Both an ASCII name and a double-width one**, because they fail
    // differently. ASCII fills its budget exactly, so the mark lands on a column
    // the text would otherwise have used. A two-column glyph that cannot fit the
    // last column leaves it blank, so the row reads `読 ›` — a gap before the
    // mark, which is honest (nothing was silently lost, and half a glyph is not
    // drawable) and which the ASCII-only fixtures never produce.
    for (label, name) in [
        ("ascii", "a-worktree-with-a-very-long-name-indeed"),
        ("wide", "読み方リポジトリテスト"),
    ] {
        let chrome = Chrome {
            pressed: None,
            gripped: None,
            scrolling: None,
            worktree: name.to_owned(),
            ..chrome()
        };
        let view = every_row_kind();
        let (mut saw_marked, mut saw_whole) = (0usize, 0usize);

        for width in WIDTHS {
            let drawn_row = rows_at(width, 8, &view, &chrome)[0].clone();
            // #119's margin off the head before anything reads this row from its
            // start. The `drawn.is_empty()` arm below means *the mode word took
            // the whole line*, which is the ladder working; with the margin still
            // on, it would also mean *the name is drawn and simply does not begin
            // at column zero*, so every width from 43 up would take that
            // `continue` while `saw_marked` kept ticking on the narrow ones and
            // the non-vacuity counter stayed satisfied. `content` strips and
            // asserts in one step, so the empty case cannot mean displaced.
            let header = content(&drawn_row, width);
            if header.contains(name) {
                saw_whole += 1;
                assert!(
                    !header.contains(CONTINUES),
                    "at {width} columns the {label} name fits and was marked \
                     anyway: {header:?}"
                );
                continue;
            }
            // What of the name reached the screen. Empty means the mode word
            // took the row, which is the ladder working and not a silent cut.
            let drawn: String = header
                .chars()
                .zip(name.chars())
                .take_while(|(row, want)| row == want)
                .map(|(row, _)| row)
                .collect();
            if drawn.is_empty() {
                continue;
            }
            saw_marked += 1;
            let rest = header[drawn.len()..].trim_start();
            assert!(
                rest.starts_with(CONTINUES),
                "at {width} columns the {label} name lost characters without \
                 saying so: {header:?}"
            );
        }

        assert!(
            saw_marked > 0 && saw_whole > 0,
            "{label}: the sweep saw {saw_marked} marked and {saw_whole} whole"
        );
    }
}

#[test]
fn a_notice_too_long_for_its_pane_is_marked_rather_than_dropped() {
    // The rung the header's ladder shares with the footer, and it had no gate.
    // `widest_fitting_or_last` hands back the **last** rung when none fits, so
    // an over-long notice reaches `put_marked` and is cut with `›`. Replace that
    // fallback with `widest_fitting`'s and the notice does not shorten, it
    // **vanishes**: the footer draws a blank row where a reader was being told a
    // file could not be read.
    //
    // That is a whole-workspace-survivable mutation without this test. The
    // function's doc already argues the arm is load bearing, and an argument in
    // a doc comment is a wish until something fails when it is removed. Every
    // shipped notice is longer than a forty-column pane, so this is the width
    // I6 is named for rather than a pathological one.
    const NOTICE: &str = "the index entry for src/lib.rs points at a missing blob";
    let view = every_row_kind();
    let chrome = with_notice();
    // **Counted per footer shape, because the footer draws itself from two call
    // sites and one counter cannot tell them apart.** `Painter::footer` calls
    // `status_line` once for a one-row footer and once more for the bottom of a
    // two-row one, and both pass the notice as a single-rung ladder. Giving
    // *either* of them the escape hatch `status_line`'s own doc names, a
    // trailing empty rung, blanks the notice at about thirty widths while the
    // other branch keeps a single `saw_marked` counter positive and the whole
    // workspace green. Two counters is what makes each call site answer for
    // itself.
    let mut marked = [0usize; 2];
    let mut saw_whole = 0usize;

    // The fixture has to be the thing under test: a notice that fits every width
    // swept would make the marked half unreachable and the test vacuous.
    assert_eq!(
        chrome.notice.as_deref(),
        Some(NOTICE),
        "the fixture's notice changed, so the widths below no longer straddle it"
    );

    for width in WIDTHS {
        let rows = rows_at(width, 8, &view, &chrome);
        let drawn_row = rows.last().expect("a footer row").clone();
        // #119's margin off the head, through `content`, for the reason
        // `a_worktree_name_too_long_for_its_room_is_marked_rather_than_cut_silently`
        // carries in full: the `drawn.is_empty()` arm below is a legitimate
        // "the state took the whole line", and leaving the margin on would let it
        // silently absorb "the notice is drawn but not at column zero" at every
        // width from 43 up.
        let footer = content(&drawn_row, width).to_owned();
        // Which of `Painter::footer`'s two `status_line` calls drew this row,
        // read off the screen rather than recomputed. A two-row footer puts the
        // state on the upper line and leaves the notice the bottom one alone; a
        // one-row footer shares the line, so the follow marker is on the row
        // itself. `Footer::plan`'s arithmetic is not consulted, because a test
        // that asked it would be the renderer agreeing with itself about which
        // branch it took.
        let two_row = rows
            .len()
            .checked_sub(2)
            .is_some_and(|above| rows[above].contains(FOLLOW_MARK));
        let shape = usize::from(two_row);
        // Below the width where a footer exists at all there is nothing to
        // assert; `the_footer_never_takes_the_body_below_its_floor` owns that.
        if footer.is_empty() {
            continue;
        }
        if footer.contains(NOTICE) {
            saw_whole += 1;
            continue;
        }

        // How much of the notice reached the screen. The state is drawn to its
        // right, so the mark is *inside* the row rather than at its end, and
        // asserting on how the row ends would read the follow marker instead.
        let drawn: String = footer
            .chars()
            .zip(NOTICE.chars())
            .take_while(|(row, want)| row == want)
            .map(|(row, _)| row)
            .collect();
        if drawn.is_empty() {
            // Legitimate at the widths where the state took the whole line and
            // left the notice no room at all. What must not happen is a *room*
            // the notice could have used going unused, which the counter below
            // is what detects.
            continue;
        }
        marked[shape] += 1;
        assert!(
            footer[drawn.len()..].starts_with(CONTINUES),
            "at {width} columns the notice was cut without saying so: {footer:?}"
        );
    }

    // **Both directions, and the marked ones are the mutation detector.** Swap
    // `widest_fitting_or_last` for `widest_fitting` and every over-long notice
    // stops being drawn at all, so the marked counts fall to zero while every
    // other assertion here still holds vacuously. Both footer shapes are
    // required, because each is a different call site and either can be
    // regressed while the other keeps a shared counter positive.
    assert!(
        marked[0] > 0 && marked[1] > 0 && saw_whole > 0,
        "the sweep saw {} marked on a one-row footer, {} on a two-row one, and \
         {saw_whole} whole",
        marked[0],
        marked[1]
    );
}

#[test]
fn the_mode_word_alone_fills_a_row_it_exactly_fits() {
    // The one arithmetic edge #67's deletion of the right-hand ladder touches.
    // At exactly the mode word's own width `put_right` draws it flush and
    // returns `width + 1`, which saturates the left's room to zero — so the row
    // is the word and nothing else.
    //
    // `no_row_ever_occupies_more_columns_than_the_screen` catches an overflow
    // here and would not catch a stray glyph *inside* the row, which is what a
    // left-hand side computing its room from `area.width` rather than from what
    // `put_right` reported would leave behind.
    //
    // Both words, because they are eight and twelve columns, and one of them
    // exercising the boundary reads as though both did.
    for (word, chrome) in [("watching", chrome()), ("not watching", lost())] {
        let width = u16::try_from(word.chars().count()).expect("a word fits a u16");
        let header = rows_at(width, 8, &every_row_kind(), &chrome)[0].clone();
        assert_eq!(
            header.trim(),
            word,
            "at {width} columns, which is exactly {word:?}, the row carries \
             something besides the mode word"
        );
    }
}

#[test]
fn the_empty_state_line_marks_its_edge() {
    // One token, and `SPEC.md` §11.1 lists it by name among them: it cannot drop
    // an item the way the hint bar can, and it has no identifying half the way a
    // path does, so the honest thing is to fill the room and mark the cut.
    //
    // Both directions, because a rule that only ever fires one way is not a rule.
    // **No branch since #158**, which moved that fact to the header where it is
    // drawn on every frame rather than on this one. The line is a token either
    // way and the rule this gates is unchanged; only its length is.
    const LINE: &str = "no unstaged changes";
    let view = empty();
    let (mut fitted, mut cut) = (0usize, 0usize);

    for width in WIDTHS {
        let rows = rows_at(width, 6, &view, &on_a_branch());
        let body = &rows[1];
        assert!(
            label_is_honest(body, LINE, width),
            "the empty state was cut at {width} columns without saying so: {body:?}"
        );

        // Against the room the line is actually given, which is the pane less
        // #119's inset on both sides. Measured against the pane instead, this
        // predicate would claim the line fits at the two widths per rung where
        // the inset has just taken the columns it needed.
        if Span::raw(LINE).width() <= usize::from(width).saturating_sub(margin_at(width)) {
            fitted += 1;
            assert!(
                content(body, width).starts_with(LINE),
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
    // the file list grows, and since #158 when the masthead appears. Summing the
    // split back up isolates the footer again, which is what this has always
    // been measuring.
    let body = |width: u16, chrome: &Chrome| {
        body_rows(&body_layout(
            Rect::new(0, 0, width, tall),
            chrome,
            view.files,
        ))
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
    // widths are restated for the reason `RAMP` and `HEAT_SLICE` are: a test
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
        worktree_churn: Default::default(),
    };
    let long_name = Chrome {
        pressed: None,
        gripped: None,
        scrolling: None,
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
            label_is_honest(&rows[0], &long_name.worktree, width),
            "the worktree name was cut at {width} columns without saying so: {:?}",
            rows[0]
        );

        for (label, row, full) in [
            ("the hunk header", &rows[1], "@@ -258,7 +258,9 @@"),
            ("the note", &rows[2], "  unresolved conflict"),
        ] {
            assert!(
                label_is_honest(row, full, width),
                "{label} was cut at {width} columns without saying so: {row:?}"
            );
            // The room the row is given, for the reason the empty state's own fit
            // predicate carries: #119's inset comes off both sides first.
            if Span::raw(full).width() <= usize::from(width).saturating_sub(margin_at(width)) {
                fitted += 1;
                assert!(
                    content(row, width).starts_with(full),
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
        worktree_churn: Default::default(),
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
/// partial one. This used to say it was observable only because [`glancing`]'s
/// buckets are all non-zero, an empty one being a space; since
/// [#78](https://github.com/breferrari/vigia/issues/78) an empty bucket draws
/// the track, so the strip's width is readable off any fixture and the gate no
/// longer rests on a property of the data.
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
    let theme = theme();
    let mut seen = std::collections::BTreeSet::new();

    for (name, view, chrome) in cases() {
        for width in WIDTHS {
            // **Both hoisted out of the row loop**, which is where they were:
            // neither depends on `y`, so the screen was being drawn six times per
            // width to read six of its rows. Twelve cases over a hundred and
            // twenty widths made that seventeen thousand renders where under
            // three thousand say the same thing.
            let backend = drawn(width, 6, &view, &chrome);
            let rows = rows_at(width, 6, &view, &chrome);
            for y in 0..6u16 {
                let buckets = spark_slot(&backend, y, &theme);
                let row = &rows[usize::from(y)];
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
///
/// **This used to assert that the pulse is never drawn part-way**, back when its
/// widest rung was the fourteen-column `● just changed`. That rung went on
/// 2026-08-03 and the assertion went with it rather than being kept as
/// decoration: one glyph cannot be cut, so the check would have passed against
/// any renderer and gated nothing. What the ladder still owes is the same rule
/// stated for a strip made of many glyphs, and
/// `the_glance_columns_collapse_in_one_order` is where that lives.
#[test]
fn the_pulse_never_pushes_a_path_off_its_own_row() {
    let view = glancing();
    let mut narrowest_named = u16::MAX;

    for width in WIDTHS {
        let rows = rows_at(width, 6, &view, &following());
        let heading = &rows[1];

        // The row still names its file, by tail or by mark.
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

/// The pulse is a mark, and a mark is one column.
///
/// `SPEC.md` §5.1's 2026-08-03 ruling, which without this is a wish: the
/// `just changed` label is gone from the picture and from the shell, and the dot
/// carries the top rung of the recency ladder alone. The ruling is about what a
/// monitor asks a reader to *read*, so the gate is over text rather than over
/// width: any letter drawn in the pulse colour is a caption on a signal that has
/// already been read, whatever it happens to say.
///
/// Swept rather than sampled, because a label would come back the way it left,
/// as the widest rung of a ladder: reachable only at the widths nothing
/// degrades at, and invisible to the two snapshots that pin 40 and 80.
///
/// Read by colour rather than by glyph, and the caret is why that is safe: it
/// shares `Theme::pulse` and is drawn on the same row, so a glyph test would
/// have to know about it. It is `▸`, which is not a letter, and nothing else on
/// a heading takes this colour.
#[test]
fn the_pulse_draws_a_mark_and_never_a_label() {
    let theme = theme();
    let view = glancing();
    let pulse = theme.pulse.fg.expect("the pulse has a colour");

    for width in WIDTHS {
        let backend = drawn(width, 6, &view, &following());
        let buffer = backend.buffer();
        // Row 1 is the pulsing file's heading: `glancing`'s rows start at the
        // top of the body and the header owns row 0.
        let lettered: String = (0..buffer.area.width)
            .map(|x| &buffer[(x, 1)])
            .filter(|cell| cell.style().fg == Some(pulse))
            .map(|cell| cell.symbol())
            .filter(|symbol| symbol.chars().any(char::is_alphanumeric))
            .collect();

        assert!(
            lettered.is_empty(),
            "at {width} columns the pulse drew {lettered:?} beside its mark, so \
             the row states one fact twice"
        );
    }
}

/// [#149](https://github.com/breferrari/vigia/issues/149), and the same shape as
/// the pulse's label one test up: a ruling that a glance surface refuses a
/// *word*, turned into something that fails.
///
/// > The list does not draw the rank the digits address.
///
/// **The refusal is the whole reason this exists, because the change looked
/// cheap.** The caret gutter was two columns wide on every pane the old
/// `CARET_FLOOR` admitted and blank on every row but the one the diff is inside,
/// so `1`-`6` could be drawn there for no column and no frame time, and a session
/// that read only that far would find it free. **That gutter is gone since
/// [#173](https://github.com/breferrari/vigia/issues/173)** and a rank would now
/// have to buy a column of its own, which cuts *for* the ruling and is exactly
/// why this gate is written against what opens a row rather than against the
/// gutter. §11.1 refuses it on what a rank *says* (the
/// region is a map, and a numbered list is a menu), which is an argument no
/// budget gate can hold.
///
/// **Two snapshots do move when a rank lands** — `rows__a_real_repository_draws`
/// and `follow__a_scripted_edit_sequence_draws_the_file_that_changed_last` are
/// the only pictures in the suite carrying a populated list, and drawing a rank
/// reddens both. That is not the same thing as holding the ruling. A snapshot
/// failure is a **diff to accept**: `cargo insta accept` turns it green and
/// leaves no trace of what was decided. This one fails with the rule in the
/// message, so the change has to be argued for rather than accepted, which is
/// the same job `a_wider_hint_bar_cannot_quietly_push_the_readouts_out` does one
/// region down.
///
/// **The claim is written against what opens a row rather than against any
/// gutter**, so it needed no restated floor and survived the gutter's removal
/// unchanged: whatever the width, a list row opens with that file's own kind
/// letter and carries the mark on the pane's leading column, and a rank makes
/// either one a digit.
///
/// **It sweeps because §11.1 refuses a rank drawn only where there is room for
/// one**, which is the same reason the pulse's label is swept one test up: a
/// width-gated rung comes back invisibly to the two snapshots that pin 40 and
/// 80, so only a sweep sees it.
///
/// **What this does not reach is a rank on the *right* of the row**, after the
/// counts, which §11.1 forbids just as squarely — the ruling is not left-scoped.
/// It is not asserted here because a row already ends in `+1 -0` and the fixture
/// paths end in digits, so no whole-row "there is no bare digit" claim is
/// available; closing it properly needs a digit-free fixture and a slot-by-slot
/// assertion, which is a bigger gate than the ruling has yet earned. The name of
/// this test and §11.1's citation of it are both scoped to the gutter on
/// purpose, so nothing here overclaims.
///
/// **Both directions, and derived from observation rather than from a restated
/// constant.** A widening pane walks a list row up three bands: nothing, then
/// the file's name, then the name with the mark. Each band is asserted non-empty
/// and the walk asserted never to descend, so a fixture that stopped drawing the
/// region, or a caret drawn unconditionally, fails here instead of passing
/// blank. `render.rs`'s `the_caret_degrades_once_and_never_flickers` holds the
/// upper crossing too, over a narrower sweep; what is only here is the lower
/// one, which is what catches a region going blank again on a *wider* pane.
#[test]
fn the_caret_column_draws_a_mark_and_never_a_rank() {
    /// The fixture's files, its list, and what the layout has to afford.
    const FILES: usize = 6;

    /// What a list row's opening glyph says, in the order a widening pane climbs
    /// through them. The derived `Ord` is the ladder.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Band {
        Blank,
        Named,
        Marked,
    }

    let tall = 24u16;
    let chrome = chrome();
    let view = numbered(usize::from(tall) + 8, FILES, FILES);
    assert_eq!(view.list.len(), FILES, "the fixture is not a full list");

    // The band each width produced. Row 0 is the current file, so it is the only
    // row that can carry the mark.
    let mut seen: Vec<(u16, Band)> = Vec::new();

    for width in WIDTHS {
        // Not `clamped_to`, which cannot move a figure `Body::split` has already
        // capped at `LIST_ROWS`: the fixture is six files and the cap is six, so
        // clamping would only turn this into "at least six".
        let listed = body_layout(Rect::new(0, 0, width, tall), &chrome, view.files).list;
        assert_eq!(
            listed, FILES,
            "at {width} columns the layout affords {listed} list rows, so this \
             sweep is not reading a full region"
        );

        // **Skipped past the masthead as well as the header**, which #158 put
        // between them. Taken from the layout rather than counted, for the
        // reason this file gives everywhere else: a test that re-derived where
        // the list starts would be a second copy of the body split agreeing
        // with itself, and the band's own baseline draws `_`, so reading its
        // row as a list row fails with a message about the wrong thing.
        let split = body_layout(Rect::new(0, 0, width, tall), &chrome, view.files)
            .clamped_to(view.list.len());
        let first = list_top(&split);
        let rows = rows_at(width, tall, &view, &chrome);
        for (offset, row) in rows.iter().skip(first).take(listed).enumerate() {
            // **The mark and the content are read from two different places
            // now** ([#173](https://github.com/breferrari/vigia/issues/173)). The
            // caret stands on the pane's own leading column, outside the region,
            // so [`content`] strips it with the margin it sits in and what is
            // left is the row proper. That splits one observation into two and
            // makes the assertion below **stronger** rather than weaker: the
            // region's opening glyph is now always the kind letter, with no
            // "or the mark" escape hatch for a digit to hide behind.
            let marked = row.starts_with(CARET);
            let opening = content(row, width).chars().find(|c| !c.is_whitespace());

            // A `None` opening is a blank row, which `rows_at`'s `trim_end` has
            // already reduced to the empty string, so there is nothing to claim
            // of it here. It is legal only where the row has no room to name its
            // file at all, and the bands below are what pin that to the narrowest
            // widths.
            if let Some(glyph) = opening {
                // The one claim. A rank puts a digit here. The letter is read off
                // the fixture rather than restated, so a change to [`entry`]'s
                // kind fails here loudly instead of leaving a magic `'M'` behind;
                // `list_top` is zero, so list row `offset` draws
                // `view.list[offset]`. It does **not** catch a row drawing another
                // file's letter, because every entry this fixture builds carries
                // the same one.
                let kind = view.list[offset].kind;
                assert!(
                    glyph == kind,
                    "at {width} columns list row {offset} opens with {glyph:?} \
                     rather than its kind letter {kind:?}: {row:?}"
                );
            }

            // **Only the current file may carry the mark.** Free to assert now
            // that the caret is read off the row's own head rather than out of
            // the region's first glyph, and it is what stops a caret drawn on
            // every row from reading as a ladder that merely climbed early.
            assert!(
                !marked || offset == 0,
                "at {width} columns list row {offset} carries the mark, which \
                 belongs to the file the diff is inside: {row:?}"
            );

            if offset == 0 {
                seen.push((
                    width,
                    match (opening, marked) {
                        (None, _) => Band::Blank,
                        (Some(_), true) => Band::Marked,
                        (Some(_), false) => Band::Named,
                    },
                ));
            }
        }
    }

    // Non-vacuity in three directions at once: a region that stopped drawing
    // never reaches the last two bands, and a caret drawn without its floor
    // never reaches the first two.
    for band in [Band::Blank, Band::Named, Band::Marked] {
        assert!(
            seen.iter().any(|(_, drawn)| *drawn == band),
            "no width in {WIDTHS:?} drew a {band:?} list row, so the sweep is \
             asserting over fewer bands than the renderer has"
        );
    }

    // And the ladder only ever climbs. A caret that came and went with the width
    // would read as the current file changing, which is the failure the caret's
    // floor was written against.
    for pair in seen.windows(2) {
        let ((below, was), (above, band)) = (pair[0], pair[1]);
        assert!(
            band >= was,
            "widening from {below} to {above} columns went from {was:?} back to \
             {band:?}, so the row bands are not one crossing each"
        );
    }
}

/// `SPEC.md` §11.2 B8 and B9, and the mechanism is what is gated rather than
/// either affordance.
///
/// > B8 — OSC 8 hyperlinks on drawn paths. Ruled 2026-08-15: no.
/// > B9 — a yank key over OSC 52. Ruled 2026-08-15: no.
///
/// **A hyperlink is not nameable from the drawn output**, which is the whole
/// reason this gate is shaped the way it is. A terminal that renders OSC 8
/// correctly shows a path that looks exactly like a path, so no assertion over
/// what a reader *sees* can tell the two apart. What both declined affordances
/// share is a **mechanism**: an escape sequence carried in-band, inside a cell's
/// symbol, which is the only route from a `ratatui` cell grid to a control
/// sequence. So that is what is forbidden, and a build of either reddens this
/// before it reddens anything else.
///
/// **This is also the measurement B8 rests on, kept executable.** An escape
/// smuggled into a symbol is invisible to the terminal and maximally visible to
/// everything else that reads those cells: `Span::raw` measures a one-column
/// linked `M` at **46** columns, because `unicode-width` gives every character
/// in the sequence a width, the two `ESC`s included at one column each, rather
/// than recognising any of it as a control sequence. That is how [`occupied`],
/// [`rows_at`] and every sweep in this file measure a row. `TestBackend`'s
/// `Display` is every snapshot in this repository, and a linked cell lands in
/// one verbatim, carrying the path with it. Neither cost is visible from the
/// pane, which is why the ruling needed a gate rather than a paragraph.
///
/// **Note where the escape has to come from, because it is not `Painter::put`.**
/// `ratatui`'s `set_stringn` filters graphemes containing a control character,
/// so an escape written through the painter is dropped and only its printable
/// payload reaches the screen. The mutation that proves this gate therefore has
/// to write the symbol onto the cell directly; one that goes through `put`
/// leaves this gate green while reddening two others in this file, for a reason
/// that has nothing to do with hyperlinks. A mutation that cannot produce the
/// artifact is not evidence about the gate that watches for it.
///
/// **And the scan is per cell rather than through [`rows_at`], which would
/// weaken it.** That helper skips cells covered by a preceding double-width
/// symbol, so an escape written onto a covered cell would never be examined.
/// The row text it returns is fine for the non-vacuity check below, and is used
/// for exactly that.
///
/// **What this does not reach, stated because §11.2 cites it.** It holds B8,
/// whose only workable mechanism is the one forbidden here. It does **not** hold
/// B9: a `y` bound to an OSC 52 write draws nothing, touches no cell, and leaves
/// this buffer identical. `crates/vigia/tests/input.rs::the_yank_key_is_refused_rather_than_unbound`
/// is B9's gate, and the two are named in each other so neither is mistaken for
/// covering both. `render.rs::tabs_become_columns_and_control_characters_become_visible`
/// asserts no escape at one width over one fixture with an empty list, as a
/// property of the content sanitiser; that overlap is real and partial, and it
/// is not a substitute for this sweep.
///
/// **Two fixtures, chosen to isolate the two regions a path is drawn in**, so a
/// failure names which one and one region going blank cannot be masked by the
/// other. [`numbered`] populates the pinned list and draws no heading;
/// [`every_row_kind`] draws stream headings over an empty list, including a
/// rename, which is the row that names two paths at once. `cases`'s own `pinned`
/// carries both at once and is the right fixture for anyone who wants one screen
/// rather than two claims.
#[test]
fn a_drawn_path_carries_no_escape_sequence_of_its_own() {
    const ESC: char = '\x1b';
    /// A fragment of every fixture path, and what non-vacuity rests on.
    const EXTENSION: &str = ".rs";

    let tall = 24u16;
    let chrome = chrome();

    for (region, view) in [
        ("the pinned list", numbered(usize::from(tall) + 8, 6, 6)),
        ("the diff stream", every_row_kind()),
    ] {
        // Whether this fixture drew anything recognisable as a path at any width.
        // Tracked per fixture rather than over the sweep as a whole, so one region
        // going blank cannot be covered by the other still drawing.
        let mut naming = false;

        for width in WIDTHS {
            let backend = drawn(width, tall, &view, &chrome);
            let buffer = backend.buffer();

            for y in 0..tall {
                for x in 0..width {
                    let symbol = buffer[(x, y)].symbol();
                    // The one claim.
                    assert!(
                        !symbol.contains(ESC),
                        "at {width} columns {region} drew an escape sequence into \
                         the cell at ({x}, {y}): {symbol:?}"
                    );
                }
            }

            // Short-circuited, because the claim is that this fixture draws paths
            // *somewhere* in the sweep and the second render is only worth paying
            // until it does. In practice that is the first eight widths.
            naming = naming
                || rows_at(width, tall, &view, &chrome)
                    .iter()
                    .any(|row| row.contains(EXTENSION));
        }

        assert!(
            naming,
            "no width in {WIDTHS:?} drew a path in {region}, so this sweep proved \
             nothing about the rows it exists to watch"
        );
    }
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
        let strip = cells_coloured(&backend, 1, &heat_colours(&theme), &[HEAT_SLICE]);

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
        usize::from(tall) - 1 - body_rows(&split)
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
    //
    // **The claim inverted on 2026-08-17, and this is the same fact from the other
    // side.** It used to be *somewhere wide enough the bar really is wider than
    // the baseline*, which is what made the rule above worth gating: a bonus rung
    // existed and had to be kept from buying a row. [#80](https://github.com/breferrari/vigia/issues/80)
    // cut the bar to three items a reader is owed at every width, so `HINT_BASELINE`
    // is rung zero and **no rung above it exists**. The assertion is therefore that
    // the widest pane draws exactly what the forty-column pane draws: nothing is
    // held back for a wide screen, so nothing can be spent on a row.
    //
    // Both sides are extracted the same way, which is a correction B12 forced: the
    // forty-column side used to be the whole trimmed footer *line*, state included,
    // compared against the hint bar alone at 120. That passed only while the bonus
    // bar was wide enough to beat a narrow bar plus its state, and a comparison
    // whose two sides are not the same quantity is a gate that happens to be green.
    let bar_at = |width: u16| {
        let line = rows_at(width, tall, &view, &chrome())
            .last()
            .expect("a footer")
            .to_owned();
        // #119's inset off the head first. Split on two spaces without it and the
        // very first field is the inset itself, so `hints` came back empty and this
        // gate reported that the widest pane draws no hint bar at all.
        content(&line, width)
            .split("  ")
            .next()
            .unwrap_or_default()
            .trim_end()
            .to_owned()
    };
    let baseline = bar_at(40);
    let hints = bar_at(120);
    assert_eq!(
        hints, baseline,
        "the widest pane drew {hints:?} where forty columns drew {baseline:?}, so \
         a rung is being held back for wide screens and the constant that stops it \
         buying a footer row is load bearing again rather than structural"
    );
    assert!(
        !baseline.is_empty(),
        "neither pane drew a hint bar at all, so the comparison above proves nothing"
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

/// The widths at which the status readouts reach each of their rungs, in the
/// state that has both to draw.
///
/// Pinned as numbers, the way [`MARGIN_RUNGS`] is, because what they are pinned
/// *against* is the footer's one unguarded allocation: `Footer::plan` hands the
/// diagnostics whatever is left after the state, the gap and the hints, so every
/// column a wider hint bar takes is a column the readouts lose. `HINT_BASELINE`
/// stops a bonus rung from buying a footer **row** and stops nothing else, and
/// these two numbers are what make the next layer visible.
///
/// Derived rather than chosen: at 55 columns the drawn bar is 26 wide, the state
/// and its gap take 15, and 53 − 15 − 2 − 26 leaves 10, which holds neither cell;
/// one column of pane later it holds the frame's 11. Change the state ladder, the
/// diagnostics ladder or the margins and these move legitimately, so a failure
/// here is a question rather than a verdict. Change the **hints** and they move
/// because a hint was paid for out of a readout, which is the case this exists
/// for.
///
/// **They have moved twice on 2026-08-17, from 69 and 77 to 67 and 75 and then to
/// here, and both moves are hints being paid *back*.** B12 swapped `JK files` for
/// the two-columns-narrower `? keys`, and [#80](https://github.com/breferrari/vigia/issues/80)
/// then cut `jk scroll` as well, taking the widest bar from 38 columns to 26, so
/// each cell arrives about eleven columns of pane earlier than
/// [#147](https://github.com/breferrari/vigia/issues/147) measured it. **That issue
/// is not answered, it has lost its subject**: with every rung one a reader is owed,
/// there is no bonus rung whose precedence over a readout could be argued.
const READOUT_RUNGS: [(u16, usize); 2] = [(56, 1), (64, 2)];

#[test]
fn a_wider_hint_bar_cannot_quietly_push_the_readouts_out() {
    // #121 found this by trying to add a fifth hint. `du half` is ten columns
    // including its separator, and the 80-column status bar is **exactly full**:
    // 40 of hints, 2, 19 of readouts, 2, 13 of state, into the 76 that eighty
    // columns leave after the inset. So the bonus rung fits by taking both cells,
    // and `render__the_status_bar_carries_what_a_frame_cost.snap` — the picture
    // whose stated job is to show the layout is *good* at one width, not merely
    // legal at every width — would have gone from `0.8ms frame   19MiB` to
    // neither. The hint was dropped instead; this is what keeps the next one from
    // arriving without anyone noticing what it spent.
    //
    // **Monotonicity was tried first and could not fail on this**, which is worth
    // recording rather than quietly replacing: "a readout, once drawn, is never
    // lost by widening" is true of the naive version too, because the boundaries
    // slide *outward* together (69 and 77 become 81 and 89) instead of crossing.
    // A gate that cannot fail on the case it was written for is a wish, so the
    // claim is the boundaries themselves.
    let view = every_row_kind();
    let tall = 24u16;
    // **Hands back what it matched on, rather than only the verdict.** A message
    // that re-read the screen to print it would be a second derivation of "the
    // footer", and the two can disagree: the hints take the last row and the
    // state takes the one above whenever the footer has grown to two lines, so a
    // message printing `rows.last()` would show the row that structurally cannot
    // carry a readout, on a gate whose whole subject is what the footer
    // allocates.
    //
    // The match is on the pair rather than on a count, because `(true, _) => 2`
    // fails a ladder that drew `MiB` without `frame`, where summing two booleans
    // would score that 1 and pass it.
    let readouts = |width: u16| {
        let rows = rows_at(width, tall, &view, &diagnostics());
        let footer = rows[rows.len() - 2..].join(" ");
        let rung = match (footer.contains("MiB"), footer.contains("frame")) {
            (true, _) => 2,
            (false, true) => 1,
            (false, false) => 0,
        };
        (rung, footer)
    };

    for (width, rung) in READOUT_RUNGS {
        let (arrived, footer) = readouts(width);
        assert_eq!(
            arrived, rung,
            "at {width} columns the footer is not at readout rung {rung}: {footer:?}"
        );
        // The column before it is the half that makes this a boundary rather
        // than a sample: a ladder drawn one rung too generously everywhere would
        // satisfy the line above and fail here.
        let (before, footer) = readouts(width - 1);
        assert_eq!(
            before,
            rung - 1,
            "at {} columns the footer already had readout rung {rung}, so {width} \
             is not where it arrives: {footer:?}",
            width - 1
        );
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
    // **The answer is now "nothing at all", and this gate says so directly.**
    // Until #77 the bar's column was taken only when a bar was drawn, so the
    // question was whether it cost anything *beyond* that column, and the way to
    // ask it was to compare `width` with a bar against `width - 2` without.
    //
    // A region is planned against the pane less a scrollbar column whether or not
    // one is drawn, because whether one is drawn is a fact about the contents:
    // the old form let a seventh changed file re-plan every row. So the bar now
    // costs a drawn row **nothing**, and the comparison is between two screens of
    // the *same* width that differ only in whether the list overflows. That is
    // strictly the stronger statement, and it is the one §11.1 makes.
    //
    // Asked by comparison rather than by arithmetic throughout, because a gate
    // that recomputed the expected path would be restating `Painter::file_row`'s
    // own ladder.

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
        let bare = rows_at(width, 24, &without_bar, &chrome());

        // **Where each side's list actually starts**, asked of the layout per
        // side because the two differ in file count and #158's masthead is
        // decided from the same split. Hardcoding row one read the band's own
        // baseline as a list row once the masthead existed.
        let first = |files: usize| {
            let split = body_layout(Rect::new(0, 0, width, 24), &chrome(), files).clamped_to(3);
            list_top(&split)
        };
        let (barred_at, bare_at) = (first(10), first(3));

        // Trailing blanks are already trimmed by `rows_at`, and the bar's own
        // column is the only thing that can follow the row's content, so it is
        // stripped before comparing.
        let barred_row = barred[barred_at]
            .trim_end_matches(['│', '█'])
            .trim_end()
            .to_owned();
        let bare_row = bare[bare_at].trim_end().to_owned();

        // **Skip the widths where the caret's own ladder differs between the two
        // panes being compared.** The caret is decided against the pane width, so
        // the wide side can have one where the narrow side does not, and a row
        // carrying a marker is not the same row. That is the caret's ladder,
        // gated by `the_caret_degrades_once_and_never_flickers` and
        // `the_caret_does_not_vanish_because_another_file_changed`; this gate is
        // about the bar and must not be measuring both at once.
        //
        // **Asked of the glyph, not of the indent, and it used to be the indent**
        // ([#173](https://github.com/breferrari/vigia/issues/173)). This read
        // `row.trim_start().len() != row.len()`, which meant *has a caret* only
        // while the caret was a two-column inset on the row. The marker sits on
        // the pane's own leading column now, so a caret row has **no** leading
        // blank and a plain one has the pane's inset: the proxy inverted, and the
        // guard went on compiling, running and skipping exactly the wrong widths.
        // It is not a live failure here, because both sides of the comparison are
        // the same pane width and the skip is a no-op either way, which is
        // precisely why nothing went red.
        let caret = |row: &str| row.starts_with(CARET);
        if caret(&barred[barred_at]) != caret(&bare[bare_at]) {
            continue;
        }

        // Only where a bar is actually drawn, or the two screens are identical
        // and the comparison is vacuous.
        if barred[barred_at].ends_with('│') || barred[barred_at].ends_with('█') {
            assert_eq!(
                barred_row, bare_row,
                "at {width} columns a list row with a bar reads {barred_row:?} \
                 where the same row at the same width without one reads \
                 {bare_row:?}, so a file count crossing the point where a bar \
                 appears re-planned the row"
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

#[test]
fn the_pane_insets_its_text_at_every_rung() {
    // [#119](https://github.com/breferrari/vigia/issues/119), swept rather than
    // sampled. `assets/preview.svg` draws its furniture to the window edge and
    // its glyphs one to three cells inside it, and §5.1's law is that a picture in
    // a public README is a specification, so the shell drawing everything from
    // column 0 was a fourth undocumented departure from it.
    //
    // **The claim is about the first column a glyph appears in, not about how
    // much blank a row has.** A row can be blank, or right-aligned with nothing
    // on its left, and neither says anything about the inset; what is illegal is
    // a glyph inside the margin. So this asserts a floor on every row and an
    // equality on the rows that reach it, which together pin the column.
    //
    // Every case and every height, because the two regions and the two chrome
    // rows reach the margin by four different routes and the caret column
    // reaches it by a fifth.
    //
    // Sized from the ladder rather than written as a literal, so adding a rung
    // widens the coverage requirement instead of leaving the new rung unchecked.
    // The widest leading column is the widest total, halved upwards.
    let widest = MARGIN_RUNGS
        .iter()
        .map(|(_, total)| usize::from(*total).div_ceil(2))
        .max()
        .expect("the ladder has rungs");
    let mut touched = vec![false; widest + 1];
    for (label, view, chrome) in cases() {
        for height in [3u16, 6, 24] {
            for width in WIDTHS {
                let inset = inset_at(width);
                let rows = rows_at(width, height, &view, &chrome);
                for (y, row) in rows.iter().enumerate() {
                    if row.is_empty() {
                        continue;
                    }
                    // The rule is furniture and is the one row that must **not**
                    // stand back: it runs edge to edge, which is the other half
                    // of §5.3's law and what `the_rule_separates_the_regions_and_
                    // spans_the_pane` pins in `tests/render.rs`.
                    if row.starts_with('─') {
                        continue;
                    }
                    // **The caret is the one *glyph* licensed to stand on the
                    // pane's own edge** ([#173](https://github.com/breferrari/vigia/issues/173)),
                    // and `SPEC.md` §11.1 records it as such so it reads as a
                    // decision rather than as drift. The reader asked for the
                    // marker flush against the edge with no margin of its own,
                    // and that is what puts it inside the ladder this gate pins.
                    //
                    // **Exempted by stripping it, never by skipping the row**,
                    // which is the difference between narrowing this gate and
                    // holing it. Everything after the caret is still measured,
                    // from the column the caret left off at rather than from
                    // zero, so a list row that lost its inset *and* happened to
                    // draw a caret still fails. Skipping the row outright would
                    // have let exactly that through, on the one row of the one
                    // region this whole ruling touches.
                    let (measured, taken) = past_caret(row);
                    let floor = inset.saturating_sub(taken);
                    let first = measured.chars().take_while(|c| *c == ' ').count();
                    assert!(
                        first >= floor,
                        "{label} at {width}x{height}: row {y} starts at column \
                         {first}, inside the pane's {inset}-column inset: {row:?}"
                    );
                    // Indexed without a bounds guard on purpose. `touched` is
                    // sized from the ladder itself, so a rung wider than it is a
                    // rung this gate cannot see: a guard here would read as
                    // defensive and would in fact drop that rung's coverage in
                    // silence, which is the failure mode this whole test exists
                    // to refuse. Out of range panics and names the width.
                    //
                    // **Only the unexempted rows count towards coverage.** A
                    // caret row is measured against a shifted floor, so letting
                    // one mark a rung would record the ladder as stood on by a
                    // row that is standing a column short of it by licence. The
                    // plain rows of both regions reach every rung on their own,
                    // and the loop below fails loudly if that ever stops being
                    // true.
                    if taken == 0 && first == inset {
                        touched[inset] = true;
                    }
                }
            }
        }
    }

    // **Every rung is reached by a row that sits exactly on it.** Without this
    // the sweep above passes against a pane that insets everything by five, or by
    // a hundred, since `>=` alone cannot tell a margin from an empty screen.
    for (rung, seen) in touched.iter().enumerate() {
        assert!(
            seen,
            "no row anywhere in the sweep began at column {rung}, so the \
             {rung}-column rung of the inset ladder is never actually drawn and \
             this gate is asserting a floor nothing stands on"
        );
    }
}

#[test]
fn the_inset_never_reaches_the_forty_column_pane() {
    // I6 is named for forty columns and every one of them is already contested:
    // `SPEC.md` §11.1 records the sparkline being bought and sold twice over two
    // columns at exactly this width. #119's ladder has a floor for that reason,
    // and a floor stated only in a docblock is a floor that moves.
    //
    // Asserted by drawing rather than by reading the ladder back, so it fails if
    // the floor is respected in the table and lost in the renderer.
    //
    // **Read off the header**, which is the one row on screen carrying no
    // indentation of its own. A first version swept every row and failed on
    // `Row::Note`, which `Painter::body` draws as `format!("  {note}")`: two
    // spaces that belong to the note and say nothing about the pane. Content that
    // indents itself cannot report where the margin is; the header can.
    let mut checked = 0usize;
    for (label, view, chrome) in cases() {
        let header = rows_at(40, 24, &view, &chrome)[0].clone();
        if header.is_empty() {
            continue;
        }
        checked += 1;
        assert!(
            !header.starts_with(' '),
            "{label}: at forty columns the header stands back from the edge \
             ({header:?}), so the inset has reached the width I6 is named for"
        );
    }
    assert!(
        checked > 0,
        "no case drew a header at forty columns, so this gate asserted nothing"
    );
}

#[test]
fn the_inset_never_outgrows_the_scrollbars_reserve() {
    // What `planning_width` rests on, and the reason it subtracts the pane's
    // inset on the **left alone**. `SPEC.md` §11.1 rules there is no trailing
    // reserve beyond the scrollbar column, and that the two columns a glance row
    // stops short of the pane's edge are the bar's rather than a margin. That
    // holds only while the pane's own right-hand margin is no wider than that
    // reserve. Today it never exceeds it and reaches it at the top rung: the
    // trailing half is 0 below 44 columns, 1 from 44 to 79, and 2 from 80 up.
    //
    // A rung whose right half outgrew the bar's reserve would put a trailing
    // reserve back and needs that ruling re-decided by a person, so this is a
    // gate rather than a `max` in the renderer quietly doing it for nobody.
    //
    // **A claim about the table rather than about a drawn row, deliberately.**
    // The screen-side half of this is
    // `tests/render.rs::a_row_pays_its_margin_once_and_the_bars_reserve_once`, which
    // measures both margins of a real row; what is left here is the standing
    // condition that lets `planning_width` charge the inset on one side, and the
    // only way to break it is to edit the ladder. A first version also asserted
    // that no row overruns the pane and was wrong twice over: `rows_at` keeps the
    // leading blanks, so `occupied` already counts the inset and adding it again
    // double-charged, and the overrun claim is what
    // `no_row_ever_occupies_more_columns_than_the_screen` already sweeps.
    // **The trailing half, which is the one the ruling is about.** A first
    // version asserted the *leading* half, which is the column `planning_width`
    // charges explicitly and which is therefore under nobody's ruling: it can be
    // as wide as the pane can afford. What has to stay inside the bar's reserve
    // is the margin the row does **not** pay for, because the reserve is standing
    // in for it. The two happen to be equal at every rung today, so the wrong one
    // passed, which is why this is stated rather than checked by eye.
    const BAR_COLUMNS: usize = 2;
    for width in WIDTHS {
        let trailing = margin_at(width) - inset_at(width);
        assert!(
            trailing <= BAR_COLUMNS,
            "at {width} columns the pane wants a {trailing}-column right margin, \
             wider than the {BAR_COLUMNS} the scrollbar already reserves, so \
             those columns stop being the bar's and §11.1's no-trailing-reserve \
             ruling has to be re-decided"
        );
    }
}

#[test]
fn the_pane_keeps_its_trailing_margin_with_nothing_to_scroll() {
    // Found by the adversarial pass on
    // [#119](https://github.com/breferrari/vigia/issues/119), which deleted
    // `Painter::region_text`'s trailing term and watched the whole suite stay
    // green while the diff's content rows ran two columns further right.
    //
    // **The gap was a fixture property rather than an oversight.**
    // `a_diff_outgrowing_its_pane_does_not_move_the_content_rows_edge` in
    // `tests/render.rs` is the other half of this rule, and it draws only the
    // screens where a scrollbar exists. There, the region has already lost the
    // bar's two columns and `region_text`'s `min` picks that edge whatever the
    // margin says, so the term it is about is unreachable from that gate. This
    // one draws the opposite screen: a diff that fits, no bar, and the trailing
    // margin as the only thing holding the row back from the pane's edge.
    let long = "        for change in self.changes() { ".repeat(8);
    let view = View {
        // No total reported, so `scrollable` is false and no bar is drawn at any
        // width. That is the whole point of the fixture. Zero means "no total"
        // rather than "a short diff", and the screen is the same either way; a
        // realistic spelling would be `total_rows: 3`.
        total_rows: 0,
        rows_above: 0,
        rows: vec![
            Row::Hunk {
                old_start: 258,
                old_lines: 7,
                new_start: 258,
                new_lines: 9,
            },
            line(LineKind::Removed, 260, &long),
            line(LineKind::Added, 261, &long),
        ],
        ..every_row_kind()
    };

    let mut reached = 0usize;
    for width in WIDTHS {
        let trailing = margin_at(width) - inset_at(width);
        let rows = rows_at(width, 8, &view, &chrome());
        for (y, row) in rows.iter().enumerate() {
            // The rule runs edge to edge by §5.3 and is the one row exempt.
            if row.is_empty() || row.starts_with('─') {
                continue;
            }
            let occupied = Span::raw(row.as_str()).width();
            assert!(
                occupied + trailing <= usize::from(width),
                "at {width} columns row {y} occupies {occupied} of the pane and \
                 leaves {} columns behind it, inside the {trailing}-column \
                 trailing margin: {row:?}",
                usize::from(width) - occupied
            );
            if trailing > 0 && occupied + trailing == usize::from(width) {
                reached += 1;
            }
        }
    }

    // **A row has to actually stand on the margin somewhere**, or this asserts a
    // ceiling nothing reaches and deleting the term under test stays green,
    // which is exactly how the term got shipped ungated in the first place.
    assert!(
        reached > 0,
        "no row anywhere in the sweep ended exactly on the trailing margin, so \
         this gate is a bound nothing touches"
    );
}

#[test]
fn the_hint_bar_never_marks_its_own_edge() {
    // §11.1: the hint bar is a **list**, so it drops whole hints and never part
    // of one. `the_hint_bar_drops_whole_hints_and_never_half_of_one` asserts the
    // rung is whole; this asserts the row it was drawn into was wide enough to
    // hold it, which is a different failure and was reachable.
    //
    // Found by the adversarial pass on #119 by deleting `Footer::plan`'s
    // trailing term: the suite stayed green while the footer drew
    // `q quit · f follow · jk scro›` at 44, 55, 68 and 76 columns. The plan
    // picks a rung against one width and `Painter::status_line` draws it into
    // another, so any drift between the two marks the bar rather than dropping a
    // rung, and nothing was watching the *drawn* row for it.
    let view = every_row_kind();
    let mut saw_hints = 0usize;
    let mut seen_at: Vec<u16> = Vec::new();
    for chrome in [chrome(), following(), diagnostics()] {
        // **The field extraction below rests on there being no notice**, because
        // a notice *replaces* the hints and is a single token that marks its own
        // edge by design. Reading one as a hint bar would redden this gate for
        // the one reason it is not about. Guarded rather than assumed: none of
        // the three fixtures carries a notice today and nothing else stops one
        // being added to them.
        assert!(
            chrome.notice.is_none(),
            "a fixture grew a notice, so the footer's left field is no longer the \
             hint bar and this gate would be asserting the wrong rule against it"
        );
        for width in WIDTHS {
            let rows = rows_at(width, 24, &view, &chrome);
            let footer = rows.last().expect("a footer row");
            if footer.is_empty() {
                continue;
            }
            // **The whole row, not a parse of it.** A first version split on two
            // spaces and took the first field as the hints, and that field is a
            // *superset*: wherever `put_right`'s gap comes out one column wide it
            // carries the state or the frame cell along with the hints, on 8 of
            // 339 screens. It never failed open, being only ever stricter, but the
            // comment claiming it was the hint bar was false, and a gate that
            // asserts on a field it has misidentified is one edit from asserting
            // nothing.
            //
            // With the notice guarded away above, the row's only markable
            // left-hand token *is* the hint bar: the state and the diagnostics are
            // ladders resolved by `widest_fitting`, which drops whole rungs, and
            // `put_right` drops its token whole rather than cutting it. So a mark
            // anywhere on this row is the thing this gate is about.
            let drawn = content(footer, width);
            if drawn.contains(HINT_SEPARATOR) || drawn.contains("f follow") {
                saw_hints += 1;
                seen_at.push(width);
            }
            assert!(
                !drawn.contains(CONTINUES),
                "at {width} columns the footer marked an edge ({drawn:?}). With no \
                 notice on this chrome the only token that can be marked is the \
                 hint bar, so a rung was drawn into a row narrower than the plan \
                 measured it against instead of a whole hint being dropped"
            );
        }
    }
    // **Named widths rather than a count**, because a count is satisfied by the
    // wrong widths. The floor here was `saw_hints > 100` against an actual 339 of
    // 360, which tolerates 239 skips: prefixing the loop's skip with
    // `width < 87 ||` still passed at 102 while dropping every width where the
    // defect this gate was written for actually shows, 44 through 76. A
    // non-vacuity floor with that much slack is a floor that has stopped being
    // one.
    for width in [44u16, 55, 68, 76] {
        assert!(
            seen_at.contains(&width),
            "the sweep never read a hint bar at {width} columns, which is inside \
             the band where dropping `Footer::plan`'s trailing term marks it, so \
             this gate no longer covers what it was written for"
        );
    }
    assert!(
        saw_hints > 300,
        "only {saw_hints} of 360 screens drew a hint bar, so the sweep is reading \
         far fewer rows than it should"
    );
}

#[test]
fn the_pane_holds_its_trailing_margin_off_the_chrome() {
    // The half of the ladder no drawn glance row can report, found by round 5 of
    // #119's audit as a mutation nothing killed: widening the source's trailing
    // half at a single rung survives the whole workspace suite.
    //
    // **Why the glance rows cannot see it.** A file row's right-hand blank is the
    // scrollbar's reserve of two columns, drawn or not, at every width. The
    // trailing margin is narrower than that reserve everywhere, so it never
    // decides where a glance row stops and changing it moves nothing on those
    // rows. `a_row_pays_its_margin_once_and_the_bars_reserve_once` in
    // `tests/render.rs` therefore pins a constant two on the right by design.
    //
    // Chrome is where the trailing half is load bearing, because the header and
    // the footer have no bar to reserve against and stop at the margin itself.
    // The header's mode word is right-aligned and drawn whole or not at all, so
    // the blank behind it is exactly the trailing margin whenever it is drawn.
    let view = every_row_kind();
    let chrome = chrome();
    let word = chrome.mode.word();
    let mut checked: Vec<u16> = Vec::new();

    for width in WIDTHS {
        let rows = rows_at(width, 24, &view, &chrome);
        let header = &rows[0];
        // `rows_at` trims trailing blanks, so the drawn row cannot report them.
        // The mode word's own end is what locates the margin instead: the row is
        // trimmed back to it, so the columns after it are the blank.
        if !header.ends_with(word) {
            continue;
        }
        let occupied = Span::raw(header.as_str()).width();
        let trailing = usize::from(width).saturating_sub(occupied);
        checked.push(width);
        assert_eq!(
            trailing,
            margin_at(width) - inset_at(width),
            "at {width} columns the header's mode word leaves {trailing} columns \
             behind it where the ladder's trailing half is {}, so the pane's right \
             margin and the table have come apart: {header:?}",
            margin_at(width) - inset_at(width)
        );
    }

    // The rungs where the trailing half actually changes, named rather than
    // counted, for the reason every sweep on this branch now names them.
    for rung in [43u16, 44, 79, 80] {
        assert!(
            checked.contains(&rung),
            "the sweep never drew the mode word at {rung} columns, which is a rung \
             boundary of the margin ladder"
        );
    }
}

/// Numbered content rows whose text is far too long to fit any pane in the
/// sweep, over a diff tall enough that the region always draws a scrollbar.
///
/// Both halves are load-bearing. The text has to **clip at every width**, or the
/// sweep measures how long a line happened to be instead of how many columns the
/// renderer gave it. And the bar has to be **drawn**, because that is the case
/// the gutter's affordability rule gets wrong: it measures against a planning
/// width that always charges the bar's column, where the row is drawn into an
/// area that only loses it when a bar exists, so the two coincide exactly when
/// one does. A short diff would leave the sweep looking at the case that works.
fn overlong(rows: usize) -> View {
    View {
        list: vec![entry("src/f.rs")],
        list_top: 0,
        current_span: rows,
        total_rows: rows,
        rows_above: 0,
        rows: (0..rows)
            .map(|i| line(LineKind::Added, 1000 + i as u32, &"x".repeat(200)))
            .collect(),
        files: 1,
        top: Position::default(),
        read: 1,
        peak: 0,
        worktree_churn: Default::default(),
    }
}

#[test]
fn a_drawn_gutter_leaves_the_text_its_floor() {
    // **The gate `MIN_TEXT_WIDTH` never had**, and its absence is the reason a
    // one-column regression shipped through a green suite.
    //
    // The rule is a threshold rather than a drawn thing: *if the line numbers
    // are drawn at all, what is left for text is at least `MIN_TEXT_WIDTH`*.
    // Every other gate in this file reads what the renderer drew, and a
    // threshold is invisible that way, so nothing here could see
    // `gutter_width` deciding affordability against a prefix that had grown by
    // a column ([#164](https://github.com/breferrari/vigia/issues/164)). The
    // two gutter gates that do exist sample away from the boundary
    // (`render.rs::the_gutter_gives_way_before_the_text_does` takes 40 and 24)
    // and the other asserts the gutter does not *vanish* rather than that what
    // survives clears this floor.
    //
    // Measured off the drawn row rather than recomputed. The fixture's line is
    // 200 characters, so a drawn row fills its content region exactly and the
    // occupied cells past the sigil's gap *are* the columns the renderer gave
    // it, mark included.
    let view = overlong(40);
    let chrome = chrome();
    let mut checked = 0usize;
    let mut dropped = 0usize;

    for width in WIDTHS {
        let backend = drawn(width, 8, &view, &chrome);
        let buffer = backend.buffer();
        // The first content row: the list takes one, the rule one, the header
        // one, and the heading and hunk header one each.
        let Some(y) = (0..8u16).find(|&y| (0..width).any(|x| buffer[(x, y)].symbol() == "x"))
        else {
            continue;
        };
        let Some(sigil) = (0..width).find(|&x| buffer[(x, y)].symbol() == "+") else {
            continue;
        };

        // A gutter is drawn when there are digits before the sigil. With none,
        // the row opens on the sigil and this rule has nothing to say: the
        // renderer already gave the text every column it had.
        let numbered = (0..sigil).any(|x| {
            buffer[(x, y)]
                .symbol()
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_digit())
        });
        if !numbered {
            dropped += 1;
            continue;
        }

        // **The content's own glyphs, not every non-blank cell**, and the
        // difference is the whole gate. The scrollbar draws in the last column
        // and is never blank, so counting occupancy adds one and masks exactly
        // the one-column shortfall this exists to catch. The first version of
        // this test did that and passed against the defect: it was mutated, went
        // green, and only then said anything useful.
        //
        // The fixture's line is a run of `x`, so its cells and the continuation
        // mark are the content region's whole contents.
        let text: usize = ((sigil + 2)..width)
            .filter(|&x| {
                let symbol = buffer[(x, y)].symbol();
                symbol == "x" || symbol == CONTINUES.to_string()
            })
            .count();
        assert!(
            text >= MIN_TEXT_WIDTH,
            "at {width} columns the gutter is drawn and leaves {text} columns of \
             text, where the floor it is kept for is {MIN_TEXT_WIDTH}"
        );
        checked += 1;
    }

    // Both ends, or the sweep proved one case and called it a rule. Without the
    // second the gate passes on a renderer that never draws a gutter at all,
    // which is the vacuous shape this file records twice already.
    assert!(
        checked > 0 && dropped > 0,
        "the sweep saw the gutter drawn at {checked} widths and dropped at \
         {dropped}, so it never crossed the boundary it is about"
    );
}

/// A screen with **both** regions drawing file rows through `Painter::file_row`.
///
/// The list carries three files and the stream opens on a heading, so the two
/// sigils the gates below compare are drawn by one function through two areas,
/// which is the whole thing [#173](https://github.com/breferrari/vigia/issues/173)
/// is about. `cases()` builds the same shape for the sweeps above; it is named
/// here because these gates need the two regions specifically rather than a
/// representative screen.
fn pinned_and_streamed() -> View {
    View {
        list: vec![
            entry("crates/vigia-core/src/frame.rs"),
            entry("src/engine/watch.rs"),
            entry("Cargo.toml"),
        ],
        list_top: 0,
        top: Position { file: 0, row: 0 },
        ..every_row_kind()
    }
}

/// The column a file row's status sigil sits in, or `None` on a row that draws
/// none.
///
/// **The caret is skipped and counted rather than searched for**, which is what
/// makes one function usable on both regions: since
/// [#173](https://github.com/breferrari/vigia/issues/173) the marker stands on
/// the pane's own leading column, outside the row it points at, so it is part of
/// the offset and never part of the content. A helper that trimmed it instead
/// would report the marked row a column left of its neighbours, and the gates
/// below would be asserting the caret's absence rather than the sigil's column.
fn sigil_column(row: &str) -> Option<usize> {
    let (rest, from) = past_caret(row);
    let blanks = rest.chars().take_while(|c| *c == ' ').count();
    rest.chars().nth(blanks).map(|_| from + blanks)
}

/// The column a file row's path starts in, or `None` on a row that names no
/// file.
///
/// **Found by walking, never derived as `sigil + KIND`**, and the difference is
/// the whole reason this exists. The first draft of
/// `the_path_starts_in_one_column_in_both_regions` asserted
/// `listed + KIND == streamed + KIND + owed`, which cancels to the assertion the
/// gate above it already makes: it added a constant to both sides and never
/// looked at a path at all, so the failure its own docblock claimed (a region
/// whose paths were narrowed) could not reach it. Caught by review rather than
/// by any run, because a gate that restates its neighbour is green for the same
/// reason its neighbour is.
///
/// Every glyph a row can open with is one column wide, so a `char` index and a
/// pane column are the same number here: the caret, the kind letter, the blank
/// after it and the elision are all single-width.
fn path_column(row: &str) -> Option<usize> {
    let sigil = sigil_column(row)?;
    let after = sigil + 1;
    let blanks = row.chars().skip(after).take_while(|c| *c == ' ').count();
    row.chars().nth(after + blanks).map(|_| after + blanks)
}

/// The row a pinned list starts on, given its layout.
///
/// **One line, and it is the fourth site to have written it out.** [`body_rows`]
/// exists for exactly this reason one region over, and its own doc records that
/// two gates open-coded that sum and both got it wrong the same way when a
/// region was added. The masthead is the region that did it, and the lead blank
/// [#174](https://github.com/breferrari/vigia/issues/174) added is the second,
/// so the sum has now moved twice under call sites that spell it by hand.
fn list_top(split: &Body) -> usize {
    1 + split.above_list()
}

/// Where each region's first file row is drawn, on a pane that draws both.
///
/// Asked of the layout rather than counted, for the reason this file gives
/// everywhere else: a test that re-derived the row offsets would be a second copy
/// of the body split agreeing with itself. What the gates below assert is the
/// **column**, which the layout has no opinion about at all.
fn region_rows(width: u16, height: u16, view: &View, chrome: &Chrome) -> Option<(usize, usize)> {
    let split =
        body_layout(Rect::new(0, 0, width, height), chrome, view.files).clamped_to(view.list.len());
    if split.list == 0 || !split.rule {
        return None;
    }
    let list = list_top(&split);
    Some((list, list + split.list + 1))
}

/// What the caret costs the list's own row at this width.
///
/// **Derived from [`MARGIN_RUNGS`] rather than restated as a width.** The first
/// draft of these gates carried `const CARET_LENDS_AT: u16 = 43`, the ladder's
/// floor written out by hand beside a file that already resolves the same rung
/// the derived way. That is the drift shape `affords_caret` was rewritten to
/// remove one crate over, reintroduced in the tests that watch it: move a rung
/// in [`MARGIN_RUNGS`] and the constant would have gone on reading 43 while
/// every gate around it moved.
///
/// Zero once the pane has a margin to lend, one below that. `row` decides
/// whether anything is owed at all, because under the caret's own floor the
/// marker is dropped and the list charges nothing for it.
fn caret_owes(row: &str, width: u16) -> usize {
    if !row.starts_with(CARET) {
        return 0;
    }
    CARET_WIDTH.saturating_sub(inset_at(width))
}

/// Whether the pane's margin can lend the caret its column at this width.
///
/// The other half of [`caret_owes`], named so a sweep can say which side of the
/// rung it counted without restating the rung.
fn caret_is_lent(width: u16) -> bool {
    inset_at(width) >= CARET_WIDTH
}

/// `SPEC.md` §11.1, and the defect was visible in a committed snapshot for
/// months before anyone read it as one.
///
/// > The status sigil starts at a different column in the pinned list than it
/// > does on the diff's file headings. They should line up.
/// > -- [#173](https://github.com/breferrari/vigia/issues/173)
///
/// The gap was the old two-column `CARET_WIDTH`: the list was indented by the
/// caret and its trailing space, so every glance element downstream of the path
/// inherited the same offset. The caret is one column standing in the pane's own
/// margin now, so wherever [`caret_is_lent`] holds the two regions share an
/// origin.
///
/// **The ruled rung is inside the assertion rather than around it.** Below that
/// width the ladder lends nothing and the caret takes one column of the list's
/// own row, so the regions differ by one. Writing that as `<= 1` would let the
/// two-column defect back in at exactly the widths where the pane can least
/// afford it; asserting an equality per rung is what makes the residual a
/// decision instead of a tolerance.
///
/// **Both sides of the rung, or the sweep proves nothing.** A gate that only saw
/// wide panes would pass against a build that never touched the narrow case, and
/// one that only saw narrow panes would pass against the original defect.
#[test]
fn the_sigil_sits_in_one_column_in_both_regions() {
    let view = pinned_and_streamed();
    let chrome = chrome();
    let (mut above, mut below) = (0usize, 0usize);

    for width in WIDTHS {
        let Some((list_row, diff_row)) = region_rows(width, 24, &view, &chrome) else {
            continue;
        };
        let rows = rows_at(width, 24, &view, &chrome);
        let (Some(listed), Some(streamed)) = (
            rows.get(list_row).and_then(|row| sigil_column(row)),
            rows.get(diff_row).and_then(|row| sigil_column(row)),
        ) else {
            continue;
        };

        // **Owed only while a caret is actually drawn.** Below the caret's own
        // floor the marker is dropped and the list charges nothing for it, so
        // the two regions agree at those widths as well. Read off the drawn row
        // rather than re-derived, which keeps this gate about the *column* and
        // leaves whether the caret exists at all to
        // `the_caret_threshold_is_the_row_floor_it_claims`.
        let owed = caret_owes(&rows[list_row], width);
        assert_eq!(
            listed,
            streamed + owed,
            "at {width} columns the list's sigil is in column {listed} and the \
             heading's in column {streamed}, where the caret is owed {owed} \
             column(s) of the row's own\n  list: {:?}\n  diff: {:?}",
            rows[list_row],
            rows[diff_row]
        );
        if caret_is_lent(width) {
            above += 1;
        } else {
            below += 1;
        }
    }

    assert!(
        above > 0 && below > 0,
        "the sweep compared {above} widths where the pane lends the caret its column \
         and {below} where it does not, so it never crossed the rung it is about"
    );
}

/// The same claim one element over, and what it reaches is stated narrowly
/// because the first draft of it reached nothing at all.
///
/// A sigil that moved without the path following would be a **new** departure
/// rather than the fix. `Painter::file_row` draws the kind letter and then the
/// path from one origin, so the two travel together only for as long as nothing
/// pulls them apart.
///
/// **Two claims, and each has a mutation that kills it and leaves the sigil gate
/// green.** Widening the kind letter's own gap (`format!("{} ")` to `"{}  "` in
/// `Painter::file_row`) moves every path without moving a sigil, and only the
/// per-row `path - sigil` assertion below sees it. Moving one region's origin
/// reddens this and the sigil gate together, which is the overlap being paid for
/// rather than hidden.
///
/// **What it does not reach, said out loud.** Narrowing a region's *width* moves
/// the right-anchored glance elements and truncates the path sooner; it does not
/// move the path's origin, so nothing here fires. That is `MIN_PATH_WIDTH`'s
/// property and `Columns`' gates hold it. An earlier version of this docblock
/// claimed the narrowing case, and the claim was false in a way no run could
/// show: the assertion then read `listed + KIND == streamed + KIND + owed`, which
/// cancels to the gate above it, so the test could not fail alone whatever it
/// claimed. Found by review, not by a run, because a gate that restates its
/// neighbour is green for its neighbour's reasons.
#[test]
fn the_path_starts_in_one_column_in_both_regions() {
    /// Columns the kind letter and its gap take at the head of a file row.
    /// Restated rather than imported, on this file's rule.
    const KIND: usize = 2;

    let view = pinned_and_streamed();
    let chrome = chrome();
    let mut compared = (0usize, 0usize);

    for width in WIDTHS {
        let Some((list_row, diff_row)) = region_rows(width, 24, &view, &chrome) else {
            continue;
        };
        let rows = rows_at(width, 24, &view, &chrome);
        let (Some(listed), Some(streamed)) = (
            rows.get(list_row).and_then(|row| path_column(row)),
            rows.get(diff_row).and_then(|row| path_column(row)),
        ) else {
            continue;
        };

        // **Owed only while a caret is actually drawn.** Below the caret's own
        // floor the marker is dropped and the list charges nothing for it, so
        // the two regions agree at those widths as well. Read off the drawn row
        // rather than re-derived, which keeps this gate about the *column* and
        // leaves whether the caret exists at all to
        // `the_caret_threshold_is_the_row_floor_it_claims`.
        let owed = caret_owes(&rows[list_row], width);

        assert_eq!(
            listed,
            streamed + owed,
            "at {width} columns the list's path starts in column {listed} and the \
             heading's in column {streamed}, where the caret is owed {owed} \
             column(s) of the row's own\n  list: {:?}\n  diff: {:?}",
            rows[list_row],
            rows[diff_row]
        );

        // **And the path really is a path's width from its own sigil**, which is
        // the half that makes this more than the gate above restated. The two
        // could agree with each other while both had lost the blank after the
        // kind letter, and a sweep over the *difference* alone cannot see that:
        // it cancels. `sigil_column` is read again here rather than threaded
        // down, because what is being checked is the distance between two
        // independently found columns on one row.
        for (label, row) in [("list", &rows[list_row]), ("diff", &rows[diff_row])] {
            let sigil = sigil_column(row).expect("a row with a path has a sigil");
            let path = path_column(row).expect("checked above");
            assert_eq!(
                path - sigil,
                KIND,
                "at {width} columns the {label} row's path starts {} columns after \
                 its sigil rather than {KIND}, so the kind letter and its gap \
                 changed width: {row:?}",
                path - sigil
            );
        }

        if caret_is_lent(width) {
            compared.0 += 1;
        } else {
            compared.1 += 1;
        }
    }

    let (above, below) = compared;
    assert!(
        above > 0 && below > 0,
        "the sweep compared {above} widths where the pane lends the caret its column \
         and {below} where it does not, so it never crossed the rung it is about"
    );
}

/// The **threshold**, which nothing that reads the drawn output can see.
///
/// `SPEC.md` §11.1 drops the caret on a pane too narrow to carry it and still
/// name the file, and the comparison that decides is made before anything is
/// drawn. Every other caret gate in this repository asserts the branch that was
/// taken; this one asserts the branch **condition**. That is the shape the
/// recorded lesson about hand-kept thresholds names: the drawn side stays correct
/// and only the width at which the element vanishes drifts, invisibly.
///
/// It matters more since [#173](https://github.com/breferrari/vigia/issues/173),
/// because what the caret costs a row is a function of the width now. A restated
/// flat two would be wrong above the margin ladder's floor and a restated flat
/// one wrong below it, and neither would show up in any rendered row.
///
/// **Both directions.** *Never drawn where the row cannot afford it* is what
/// `a_row_keeps_its_floor_after_both_the_bar_and_the_caret` holds in
/// `tests/render.rs`. What is only here is the converse, *always drawn where it
/// can be*, which is what fails when a floor is left overstated after the pieces
/// it sums get cheaper. The constant this replaced read eighteen columns and
/// would have gone on reading eighteen while the caret had come to cost one.
#[test]
fn the_caret_threshold_is_the_row_floor_it_claims() {
    /// The kind letter and its gap, plus `MIN_PATH_WIDTH`. Restated.
    const ROW_FLOOR: usize = 2 + 12;
    /// The bar's column and the blank in front of it, paid whether or not a bar
    /// is drawn. Restated.
    const BAR: usize = 2;

    let view = pinned_and_streamed();
    let chrome = chrome();
    let (mut drawn, mut dropped) = (0usize, 0usize);

    for width in WIDTHS {
        let Some((list_row, _)) = region_rows(width, 24, &view, &chrome) else {
            continue;
        };
        let rows = rows_at(width, 24, &view, &chrome);
        let Some(row) = rows.get(list_row) else {
            continue;
        };

        // `caret_gutter` written out rather than imported: what the marker takes
        // off the row is only what the pane's own margin cannot lend it.
        let inset = inset_at(width);
        let gutter = CARET_WIDTH.saturating_sub(inset);
        let left = usize::from(width)
            .saturating_sub(BAR)
            .saturating_sub(inset)
            .saturating_sub(gutter);
        let affordable = left >= ROW_FLOOR;
        let marked = row.starts_with(CARET);

        assert_eq!(
            marked,
            affordable,
            "at {width} columns the row {} a caret while its floor says it {}: \
             the pane leaves {left} columns after the bar's {BAR}, the inset's \
             {inset} and the caret's {gutter}, against the {ROW_FLOOR} the row \
             needs to name its file: {row:?}",
            if marked { "draws" } else { "draws no" },
            if affordable { "can" } else { "cannot" },
        );

        if affordable {
            drawn += 1;
        } else {
            dropped += 1;
        }
    }

    assert!(
        drawn > 0 && dropped > 0,
        "the sweep saw the caret affordable at {drawn} widths and unaffordable \
         at {dropped}, so it never crossed the threshold it is about"
    );
}

/// `SPEC.md` §11.1, and the one boundary on this pane that was drawn with
/// nothing at all.
///
/// > The header row sits directly against the first row of the pinned list, with
/// > nothing between them. It should have a row, or half a row, of separation.
/// > -- [#174](https://github.com/breferrari/vigia/issues/174)
///
/// **Coextensive with the list, exactly as the rule under it is.** §11.2 B11
/// rests on `rule: list > 0` making the rule and the list one thing, and the lead
/// blank is given that same shape: a body with no map has nothing to separate the
/// header from. So this asserts the row is blank wherever a list is drawn **and**
/// that the layout keeps no lead where one is not, which is what stops the fix
/// from being a row spent unconditionally on chrome announcing an absent region.
#[test]
fn the_body_opens_with_one_blank_row_under_the_header() {
    let chrome = chrome();
    let (mut separated, mut joined) = (0usize, 0usize);

    // Both fixtures at every height, so the two sides of the claim are swept
    // rather than sampled at one pane each. `empty()` is B3's screen, which draws
    // one line of prose and no map at all.
    for (view, drawable) in [(pinned_and_streamed(), true), (empty(), false)] {
        for height in 2..=40u16 {
            for width in [40u16, 64, 80, 120] {
                let split = body_layout(Rect::new(0, 0, width, height), &chrome, view.files)
                    .clamped_to(view.list.len());
                if split.list > 0 {
                    assert!(
                        drawable,
                        "at {width}x{height} the empty state drew a list, so this \
                         sweep is not reading the screen it thinks it is"
                    );
                    // **Rendered only on the branch that reads a row.** The
                    // `else` below asserts a fact about `Body` alone, and
                    // `empty()` takes it at every pane in the sweep, so drawing
                    // before the branch threw a whole screen away on half of it.
                    // Indexed rather than guarded, too: `rows_at` returns exactly
                    // `height` rows and this loop starts at two, so `rows.get(1)`
                    // could never be `None` and the guard read as a case that
                    // does not exist.
                    let rows = rows_at(width, height, &view, &chrome);
                    let under = &rows[1];
                    // `rows_at` trims trailing blanks, so a wholly blank row is
                    // the empty string. Asserted that way rather than by counting
                    // spaces, which would also pass on a row the renderer had
                    // painted furniture into and then blanked at the end.
                    assert!(
                        under.is_empty(),
                        "at {width}x{height} the row under the header is \
                         {under:?} rather than the blank a drawn list is owed"
                    );
                    separated += 1;
                } else {
                    assert_eq!(
                        split.lead, 0,
                        "at {width}x{height} the layout kept a lead blank with no \
                         list under it, so a row went to chrome announcing an \
                         absent region"
                    );
                    joined += 1;
                }
            }
        }
    }

    assert!(
        separated > 0 && joined > 0,
        "the sweep saw {separated} screens with a list and {joined} without, so \
         it never exercised both sides of the rule"
    );
}

/// What the whole of [#174](https://github.com/breferrari/vigia/issues/174)'s
/// design rests on, and what no snapshot in this repository states.
///
/// The blank the body opens with **is** the masthead's leading air when a band is
/// drawn, rather than a row on top of it. So a masthead screen spends exactly the
/// rows it spent before the separator existed, and #174's cost falls only on the
/// masthead-off screen, which is the default since
/// [#204](https://github.com/breferrari/vigia/issues/204).
///
/// **Asserted as the total, because the total is the claim.** How the four rows
/// divide between `lead`, `graph` and `air` is an arrangement that may reasonably
/// move; that a band pane keeps four rows between its header and its list, so the
/// list starts where it always did, is the promise. Folding the lead on top of
/// the band instead of into it would leave every field looking sensible and push
/// the list down a row on the one screen a reader turned the band on for.
#[test]
fn the_lead_row_is_the_mastheads_air_when_a_band_is_drawn() {
    /// Rows a band pane keeps between the header and the list: two of band with a
    /// blank either side. Restated rather than imported, and it is the number
    /// that shipped before the header had a separator at all.
    const BAND_AND_AIR: usize = 4;

    let view = pinned_and_streamed();
    let with = Chrome {
        masthead: true,
        ..chrome()
    };
    let without = Chrome {
        masthead: false,
        ..chrome()
    };
    let mut banded = 0usize;

    for height in 2..=40u16 {
        for width in [40u16, 64, 80, 120] {
            let area = Rect::new(0, 0, width, height);
            let shown = body_layout(area, &with, view.files).clamped_to(view.list.len());
            if shown.graph == 0 {
                continue;
            }

            assert_eq!(
                shown.above_list(),
                BAND_AND_AIR,
                "at {width}x{height} a band pane keeps {} rows between its header \
                 and its list rather than the {BAND_AND_AIR} it kept before the \
                 header gained a separator, so the band grew a row nobody asked \
                 it for",
                shown.above_list()
            );
            assert_eq!(
                shown.lead, 1,
                "at {width}x{height} the band swallowed the header's own blank, \
                 so hiding the band would put the header back against the list"
            );

            // And the trade is one-directional: hiding the band gives back the
            // band and its own air, never the lead. The row *counts* are what
            // `hiding_the_masthead_gives_its_rows_to_the_diff` holds in
            // `tests/render.rs`; what is only here is that the separator survives
            // the toggle at every pane that can draw a band at all.
            let hidden = body_layout(area, &without, view.files).clamped_to(view.list.len());
            assert_eq!(
                hidden.lead, shown.lead,
                "at {width}x{height} turning the masthead off took the header's \
                 separator with it"
            );

            banded += 1;
        }
    }

    assert!(
        banded > 0,
        "no pane in the sweep drew a band, so this gate asserts nothing"
    );
}
