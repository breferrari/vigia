//! The renderer, as text.
//!
//! `SPEC.md` §7 makes this the instrument the UI is asserted with: render into an
//! in-memory buffer and snapshot it, because that turns a screen into something a
//! diff can argue about. The suite goes through `TestBackend` and `Terminal::draw`
//! rather than calling into a bare `Buffer`, so the backend and the frame
//! plumbing are inside what is being tested and not beside it.
//!
//! Two things it deliberately does not cover.
//!
//! Colour. `TestBackend`'s `Display` writes symbols and drops styles, so a
//! snapshot cannot see a theme at all. The palette is checked by reading cells
//! instead, at the bottom of this file.
//!
//! I6, mostly. The snapshots at 40, 80 and 120 columns that `SPEC.md` §3 names
//! are here, and they are worth having: a picture is the only artifact that
//! shows a layout is *good* rather than merely legal. But a snapshot records one
//! width and asserts no rule, so the invariant itself lives in
//! `tests/legibility.rs`, which sweeps every width from 1 to 120.
//!
//! Views are built by hand, not from a repository. That is forced, and it is
//! worth knowing: `vigia_core::FileChange` keeps a private field, so no test
//! outside that crate can construct one. Whether the shell turns a real frame
//! into these rows is `reads.rs` and `scroll.rs`; whether these rows become the
//! right cells is here.

use std::time::Duration;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use vigia::{
    Chrome, FileEntry, HEAT_BUCKETS, HeatBucket, Mode, Position, Row, Theme, View, diff_height,
    render,
};
use vigia_core::{Class, HISTORY_BUCKETS, LineKind, Recency, Span};

/// The mark the renderer writes where a row runs past its edge.
///
/// Restated here rather than imported: it is one character of published
/// behaviour, and a test that shared the constant would agree with the renderer
/// by construction instead of checking it.
const CONTINUES: &str = "›";

/// What joins two facts about one subject on a line of chrome.
///
/// Restated for [`CONTINUES`]' reason: the renderer keeps this apart from its
/// hint separator on purpose, so that a change to how hints are joined cannot
/// silently reshape the header, and a test importing the exported one would undo
/// that separation. Named rather than inlined because the header now spells it
/// in several assertions, and a separator change should be one edit.
const FACT_JOIN: &str = " · ";

/// The sparkline's ramp, tallest last.
///
/// Restated for [`CONTINUES`]' reason, and declared **once** for a different
/// one: three copies appeared in this file and a fourth spelling as a string to
/// `contains`, and a second copy does not check the renderer, it checks the
/// first copy.
const RAMP: [&str; 8] = ["▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];

/// What a sparkline bucket nothing was written in draws.
///
/// Restated for [`CONTINUES`]' reason: a test sharing the renderer's own
/// constant would agree with it by construction rather than check it.
///
/// **`SPARK_` rather than plain `TRACK`, which four gates below already declare
/// with a different value.** The scrollbar's track is `▕`, function-local in
/// each of them, and a file-level `TRACK` beside those would compile by
/// shadowing and mean one thing here and another there. That is the same
/// symbol-collision hazard this file's other helpers exist to name, arriving as
/// two constants rather than two elements.
const SPARK_TRACK: &str = "_";

/// Every foreground a heat slice can take.
///
/// One list, because adding a band to the theme should be one edit here rather
/// than three, and a missed one makes a gate quietly stop seeing a colour rather
/// than fail. `tests/legibility.rs` already solved it this way.
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

/// Draw a view at `width` by `height` and hand back the backend to snapshot.
fn screen(width: u16, height: u16, view: &View, chrome: &Chrome) -> TestBackend {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
    let theme = Theme::default();
    terminal
        .draw(|f| {
            let area = f.area();
            render(f.buffer_mut(), area, view, &theme, chrome);
        })
        .expect("draw");
    terminal.backend().clone()
}

/// The neutral chrome, which is deliberately **not** the state a shell starts
/// in.
///
/// Follow mode is on by default (I5), so most of these snapshots show a footer
/// no reader will see on their first frame. That is on purpose: this file is
/// about what the *body* draws, and `follow ▶` in every picture would be a
/// constant nothing here tests. It also keeps the forty-column body pictures on
/// a one-line footer, so they show the widest body rather than I6's two-line
/// one. The follow state gets its own snapshots instead, below.
fn chrome() -> Chrome {
    Chrome {
        worktree: "vigia".to_owned(),
        // `None` because these views have a diff in them, and only the empty
        // state names a branch. A populated frame never asks, which is I4 and
        // which `lib.rs`'s `branch_for` gates.
        branch: None,
        mode: Mode::Watching,
        notice: None,
        following: false,
        // The first paint's chrome: no frame has completed, so there is no p99
        // to draw. Every snapshot below inherits it, which keeps them comparing
        // the same screen they compared before the readouts existed, and
        // [`diagnostics_chrome`] is what covers the other shape.
        frame: None,
        memory: None,
    }
}

/// The chrome of every frame after the first, on a platform that reads memory.
fn diagnostics_chrome() -> Chrome {
    Chrome {
        frame: Some(Duration::from_micros(800)),
        memory: Some(19 * 1024 * 1024),
        ..following_chrome()
    }
}

/// The chrome of a worktree with nothing in it, which is what B3 specifies.
fn empty_chrome() -> Chrome {
    Chrome {
        branch: Some("main".to_owned()),
        ..chrome()
    }
}

/// The chrome a shell actually starts with.
fn following_chrome() -> Chrome {
    Chrome {
        following: true,
        ..chrome()
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

/// A one-line view whose single content row carries `spans`.
///
/// Built by hand because that is the only way to hand the renderer a *chosen*
/// classification: `rows.rs` covers the other half, where the spans come from a
/// real diff through a real highlighter.
fn highlighted(kind: LineKind, text: &str, spans: Vec<Span>) -> View {
    // Guard the fixture. `Span` promises the runs of a line sum to its bytes,
    // and a fixture that broke that would have the renderer drawing an
    // unclassified tail while the assertions below read columns nobody
    // classified.
    //
    // No spans at all is the exception and is legal: that is what a file type
    // nothing recognises produces, and drawing it is its own test below.
    let covered: usize = spans.iter().map(|span| span.len).sum();
    assert!(
        spans.is_empty() || covered == text.len(),
        "the fixture's spans cover {covered} bytes of {}",
        text.len()
    );

    View {
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
            },
        ],
        files: 1,
        top: Position::default(),
        read: 1,
        peak: 0,
    }
}

/// The first column of row `y` holding `needle`.
///
/// Found rather than computed, because where the text starts depends on the
/// gutter's width, and a test that recomputed that would be a second
/// implementation of it agreeing with itself.
fn column_of(backend: &TestBackend, y: u16, needle: &str) -> u16 {
    column_where(backend, y, |symbol, _| symbol == needle)
        .unwrap_or_else(|| panic!("no {needle:?} anywhere on row {y}"))
}

/// One changed file, as the pinned list carries it.
fn entry(path: &str, added: u32, removed: u32) -> FileEntry {
    FileEntry {
        path: path.to_owned(),
        from: None,
        kind: 'M',
        churn: Some((added, removed)),
        spark: [0; HISTORY_BUCKETS],
        recency: Recency::Cold,
        heat: [HeatBucket::default(); HEAT_BUCKETS],
    }
}

/// The same file, as a heading in the diff stream.
///
/// One constructor behind both, because `SPEC.md` §11.1 draws the two regions
/// from one `FileEntry` and a fixture that built them separately could drift in
/// exactly the way the shared type exists to prevent. Two of the call sites
/// below build the same file for both regions of one screen.
fn file(path: &str, added: u32, removed: u32) -> Row {
    Row::File(entry(path, added, removed))
}

/// A view with the shape a real frame produces: a file, a hunk, mixed lines.
fn one_file() -> View {
    View {
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
        peak: 0,
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

/// A worktree with nothing in it, which is the screen the tool sits on most.
fn nothing_changed() -> View {
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

#[test]
fn a_clean_worktree_says_so_rather_than_showing_nothing() {
    // A monitor is read by glancing at it, so "nothing has changed" and "I am
    // broken" must not look identical. An empty pane says both, which is B3, and
    // this is the picture of the answer: the header carries which tree and that
    // it is watching, the body carries which branch and what it did not find.
    //
    // Forty columns first, because it is the width I6 exists for and the one
    // where the header has least room to keep the mode word.
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
    // The mockup headered `watching · 3 files` and only the count ever shipped,
    // until #67 split the two across the row and `assets/preview.svg` with them.
    //
    // Both directions in one test, because a word drawn unconditionally is not a
    // mode: it has to say something different when something different is true.
    //
    // **Distinctness is the whole of this test's job**, and it is not a subset of
    // the sweep below. `"not watching"` ends with `" watching"`, so a renderer
    // drawing the wrong word on a live watch satisfies every `ends_with` in
    // `the_header_never_lets_the_mode_word_take_the_count_as_its_object`; only
    // the negative assertion here catches it. What that sweep *does* cover, at
    // this exact fixture and width, is where the count sits, so it is not
    // asserted twice here.
    let view = one_file();

    let live = row_text(&screen(80, 6, &view, &chrome()), 0);
    assert!(
        live.trim_end().ends_with("watching"),
        "live header: {live:?}"
    );
    assert!(!live.contains("not watching"), "live header: {live:?}");

    let stopped = Chrome {
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
    // #49, and the ruling `SPEC.md` §10 closed with. The header carries which
    // tree, the mode word and the file count, and there is no fourth fact. A
    // repository-wide `+`/`-` needs every changed file diffed, so it would make
    // first paint follow the size of the diff, which is the one thing I4 exists
    // to forbid. The only variant that dodges that is to compute it behind the
    // frame and reveal it when it arrives, which is a wake no filesystem event
    // caused and a number that is stale between the tick and the reveal: the
    // frozen-clock failure §11.1 already rejected for the pulse.
    //
    // **Content rather than cost, which is the half `reads.rs` cannot see.**
    // `one_screenful_costs_the_same_however_much_else_changed` already fails if a
    // total is computed through the shell's `Frame`, because it compares the
    // diffs one screenful computes across two fixtures differing only in
    // changed-file count. A total computed behind the frame on a handle of its
    // own would never touch those stats. So the assertion here is over the drawn
    // row, which is the level the ruling was made at.
    //
    // `glancing()` is the fixture rather than a plainer one on purpose: its rows
    // carry the pulse label, the heat strips and the sparklines, so the header is
    // asserted silent against the busiest row set the shell can draw rather than
    // against the emptiest. Its counters are the mockup's own.
    let backend = screen(80, 5, &glancing(), &chrome());
    let header = row_text(&backend, 0);

    // What a header total would have to draw, in **either** form: the counters'
    // own sigils, or the bare sum if it dropped them. `+42 −7`, `+11 −3` and
    // `+2 −0` sum to `+55 −10`, so all four are things only an aggregate could
    // put on the top row.
    const TOTALS: [&str; 4] = ["+", "-", "55", "10"];

    // Guard the fixture, the way [`highlighted`] guards its spans. The assertion
    // below reads the **whole** row rather than recomputing where the right-hand
    // side begins, and that is only sound while the left-hand side contains none
    // of these itself. A worktree named `my-repo` would make it lie, silently and
    // in the passing direction.
    let worktree = chrome().worktree;
    for needle in TOTALS {
        assert!(
            !worktree.contains(needle),
            "the fixture's worktree name {worktree:?} contains {needle:?}, so the \
             assertion below would read its own left-hand side as a total"
        );
    }

    // Non-vacuity, and it is what makes the rest worth asserting. The counters
    // have to really be on this screen, or a header with no numbers in it would
    // pass against a fixture that had none to draw and the test would be
    // checking that nothing is nothing.
    let height = backend.buffer().area.height;
    let body: String = (1..height).map(|y| row_text(&backend, y)).collect();
    // The two halves separately rather than as one joined string: since #77 each
    // is right-anchored in its own fixed-width column, so how many spaces sit
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
    // #67, and the whole of it. The header's two facts are about two different
    // subjects: the mode word says whether the **watch thread** is live, and the
    // count says how many files **differ from the index**. Drawn as
    // `watching · 3 files` they fuse, because English reads a participle
    // followed by a number as a verb with an object, and the set that names does
    // not exist: `vigia` watches the whole worktree minus gitignore, and the
    // count is what changed inside it.
    //
    // So the count sits with the worktree, which is the other **tree**-fact on
    // the line, and the mode word takes the right alone where it can fuse with
    // nothing.
    //
    // Three assertions, because the defect has three spellings and only the
    // first is the one that shipped. Fusing again on the right is what this
    // reverts; fusing on the *left* is what a naive fix would produce by putting
    // the mode word beside the count on the other side; and a count that drifted
    // away from the worktree would leave it modifying nothing at all.
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
            // Every width §3 names. Forty is where the ladder is under most
            // pressure and a hundred and twenty where nothing degrades, so a
            // renderer that only fused when it was short and a renderer that
            // only fused when it was roomy are both caught.
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
                // Non-vacuity and placement in one: a header that had dropped
                // the count entirely would pass the two assertions above while
                // saying nothing.
                assert!(
                    header.contains(&format!("{worktree}{FACT_JOIN}{files} changed")),
                    "at {width} columns the count is not beside the worktree: \
                     {header:?}"
                );

                // And the mode word ends the row, with blank before it, which is
                // what "the right-hand side carries it alone" means where a test
                // can see it. The leading space is half the assertion rather
                // than tidiness: `1 changedwatching` would satisfy every check
                // above and is the one arrangement that fuses harder than the
                // defect being fixed.
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
///
/// File-level rather than a closure, because more than one gate now needs to
/// build a window out of these.
fn listed(path: &str, added: u32, removed: u32) -> FileEntry {
    FileEntry {
        path: path.to_owned(),
        from: None,
        kind: 'M',
        churn: Some((added, removed)),
        spark: [0, 0, 1, 3, 8, 5, 9, 12],
        recency: Recency::Cold,
        heat: heat(&[(0, 9, 0), (5, 3, 4), (11, 0, 6)]),
    }
}

/// Three list rows whose counts cells are deliberately three different widths.
///
/// That is the whole fixture design and the reason no existing test could fail
/// on #77: every list fixture in this file gave every row the same churn, so
/// right-packing and columns drew identically and the defect was invisible.
fn ragged_counts() -> View {
    let row = |path: &str, added: u32, removed: u32| Row::File(listed(path, added, removed));
    View {
        list: vec![
            listed("src/engine/watch.rs", 139, 131),
            listed("src/render/frame.rs", 42, 7),
            listed("Cargo.toml", 2, 0),
        ],
        list_top: 0,
        current_span: 400,
        total_rows: 400,
        rows_above: 0,
        rows: vec![row("src/engine/watch.rs", 139, 131)],
        files: 3,
        top: Position::default(),
        read: 1,
        peak: 12,
    }
}

/// The leftmost column of row `y` whose cell satisfies `is`.
///
/// **Glyph and colour together**, because neither alone identifies a glance
/// element. The heat strip and a full sparkline bucket draw the same block, so a
/// scan for a character finds whichever comes first ([`blocks_of`] gives that
/// reason one screen down); and `Theme::spark` shares its foreground with
/// `Theme::chrome` on every palette here, so a scan for the colour finds the
/// caret. The callers below pass both.
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
///
/// The **end** rather than the start, because each half of the counts cell is
/// right-anchored in its own sub-column the way `assets/preview.svg` draws it:
/// `+139` and `+42` in a four-column field start one apart and finish together,
/// and finishing together is the property that lets an eye run down three files'
/// additions and compare them.
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

#[test]
fn the_glance_columns_agree_down_the_list() {
    // #77. `assets/preview.svg` puts every glance element at the same x on every
    // file row, which is what makes three sparklines read as one small-multiples
    // chart and three heat strips as a comparison. The shell right-packed
    // instead, so each element's x was a function of the widths of the elements
    // outside it, and the counts cell is the widest variable thing there.
    //
    // Asserted over the pinned list, because that is the region the
    // small-multiples reading belongs to: three summary rows one above another.
    let view = ragged_counts();

    // Guard the fixture first. If every row's counts were the same width, right
    // packing and columns would draw identically and this test would pass
    // against the defect it exists to catch.
    let widths: Vec<usize> = view
        .list
        .iter()
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

    // Each element by its own colour, and the counts cell by the `+` it opens
    // with, which nothing else on a list row draws. Read off the cells rather
    // than computed, because recomputing where the renderer put them would be
    // its own arithmetic agreeing with itself.
    let spark = theme.spark.fg;
    let heats_fg = heat_colours(&theme);

    let sparks: Vec<(u16, u16)> = (1..4u16)
        .filter_map(|y| {
            column_where(&backend, y, |sym, fg| fg == spark && RAMP.contains(&sym)).map(|x| (y, x))
        })
        .collect();
    let heats: Vec<(u16, u16)> = (1..4u16)
        .filter_map(|y| {
            column_where(&backend, y, |sym, fg| sym == "█" && heats_fg.contains(&fg))
                .map(|x| (y, x))
        })
        .collect();
    // Both halves, by where each ends. The fixture's paths carry neither sigil,
    // so a run found here came from the counts cell.
    for path in view.list.iter().map(|e| &e.path) {
        assert!(
            !path.contains('+') && !path.contains('-'),
            "the fixture path {path:?} carries a counts sigil, so the scan below \
             would read the path instead"
        );
    }
    let added: Vec<(u16, u16)> = (1..4u16)
        .filter_map(|y| run_end(&backend, y, "+").map(|x| (y, x)))
        .collect();
    let removed: Vec<(u16, u16)> = (1..4u16)
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
///
/// **Positions rather than contents.** A row drawing different digits, a shorter
/// path or a pulse label must compare equal; a row whose elements sit in
/// different columns must not. So everything that is not a sparkline bucket, a
/// heat slice or a digit collapses to `_`, which is what lets this be compared
/// across screens that legitimately say different things.
///
/// Colour and glyph together, for the reason [`blocks_of`] gives: the heat strip
/// and a full sparkline bucket draw the same block, and `Theme::pulse` shares a
/// foreground with `Theme::spark`, so neither alone identifies anything.
/// Takes a drawn backend rather than a view, the way every other reader helper
/// in this file does, so a caller that also needs the cells does not render the
/// same screen twice.
fn glance_columns(backend: &TestBackend) -> Vec<String> {
    let theme = Theme::default();
    let heats = heat_colours(&theme);
    let buffer = backend.buffer();

    (1..4u16)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| {
                    let cell = &buffer[(x, y)];
                    let (sym, fg) = (cell.symbol(), cell.style().fg);
                    if sym == "█" && heats.contains(&fg) {
                        'h'
                    } else if RAMP.contains(&sym) && fg == theme.spark.fg {
                        's'
                    } else if sym == SPARK_TRACK && fg == theme.spark_track.fg {
                        // Its own class rather than folded into `s`. A track and
                        // a bar occupy the same slot and must therefore compare
                        // equal *positionally*, which they do because both are
                        // read; but a row that lost its buckets and a row that
                        // kept them are different screens, and collapsing the
                        // two would make the non-vacuity check below unable to
                        // tell them apart.
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
    // The launch case, and the reason #77 is universal rather than occasional.
    // `spark_of` used to yield nothing until a file had been written once, which
    // is every file on the first frame, and `heat_at` yields nothing for a file
    // with no line diff. Under right-packing each absence let that row's
    // remaining elements slide right into the space, so one quiet file pulled
    // its neighbours out of line.
    //
    // **[#78](https://github.com/breferrari/vigia/issues/78) closed the
    // sparkline half**, so what this now drives is a row that draws *track*
    // where its neighbours draw buckets. The property is unchanged and the
    // fixture still produces it: a row may draw something different, and it may
    // not move anything else.
    //
    // Asserted as "the *other* rows are unchanged", which is the property that
    // matters: a row may draw less, and it may not move anything else.
    //
    // Guard the fixture, the way `the_glance_columns_agree_down_the_list` does
    // for its sigils: `glance_columns` reads any digit as a counts cell, so a
    // path carrying one would be classified as content and the comparison would
    // be over the wrong columns.
    for path in ragged_counts().list.iter().map(|entry| entry.path.clone()) {
        assert!(
            !path.contains(|c: char| c.is_ascii_digit()),
            "the fixture path {path:?} carries a digit, which `glance_columns` \
             reads as a counts cell"
        );
    }

    let full = ragged_counts();
    let mut gapped = ragged_counts();
    gapped.list[1].spark = [0; HISTORY_BUCKETS];
    gapped.list[2].heat = [HeatBucket::default(); HEAT_BUCKETS];

    let before = glance_columns(&screen(80, 10, &full, &chrome()));
    let after = glance_columns(&screen(80, 10, &gapped, &chrome()));

    assert_eq!(
        before[0], after[0],
        "the first row moved when a *different* row lost its sparkline"
    );

    // **The load-bearing half, and it has to be read across rows of the *same*
    // screen.** The comparison above is between two screens, and row 0's own
    // data is identical in both, so right-packing draws it identically too: it
    // is a real property but not one that can fail against the defect. What
    // separates a column from a cluster is that a row which drew *less* still
    // has its remaining elements where its neighbours have theirs. Under
    // right-packing the freed columns are reclaimed and everything outside them
    // slides.
    let columns_of = |row: &str, class: char| -> Vec<usize> {
        row.char_indices()
            .filter(|(_, c)| *c == class)
            .map(|(i, _)| i)
            .collect()
    };
    // The strip and the sparkline only. The digits are *not* comparable across
    // rows here and that is not a gap: `ragged_counts` gives each row a
    // different count, and each half is right-aligned inside its fixed field, so
    // `+2` and `+139` legitimately occupy different columns of the same slot.
    // What pins the field itself is
    // `a_counts_cell_never_rounds_a_change_to_nothing`, which holds the removed
    // half still while the added half grows.
    // **Only the sparkline's slot is readable here, and that is a property of
    // the drawing order rather than an omission.** The elements are placed right
    // to left, so a closed slot pulls what is *left* of it rightwards and leaves
    // everything right of it alone. The sparkline sits between the heat strip
    // and the counts, so closing its slot moves the strip and this can see it.
    // The strip's own slot is the leftmost of the three, and the only things
    // left of *it* are the pulse and the path, both of which `glance_columns`
    // maps to `_`. Pairing the heat-losing row with the sparkline would assert
    // something that cannot fail, and an earlier form of this test did.
    //
    // What holds the strip's slot instead is
    // `the_glance_columns_collapse_in_one_order`, whose pinned walk reads the
    // strip's own width at every column from 1 to 120, and the snapshots. Said
    // here because a gate that looks symmetrical and is not is worse than one
    // that states its own reach.
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
    //
    // "Lost its sparkline" means lost its **buckets** since #78; the slot is
    // still drawn, as the track. That is asserted here as well as in
    // `a_worktree_already_dirty_at_launch_draws_a_track_on_every_row`, because
    // this gate's own fixture is the one that turns a row's history off and it
    // would otherwise be the place a silently blank column hid.
    assert!(
        before[1].contains('s') && !after[1].contains('s'),
        "row 1 was supposed to lose its sparkline buckets: {:?} then {:?}",
        before[1],
        after[1]
    );
    // **Sorted, or this asserts an ordering rather than a set.** `ragged_counts`
    // happens to put its empty buckets to the left of its written ones, so a
    // plain chain of "was track" then "was bar" comes out ascending today; a
    // fixture with an interleaved gap would make the same correct renderer fail
    // on the order. The sibling gate
    // `an_empty_bucket_draws_the_track_and_a_written_one_draws_a_bar` sorts for
    // exactly this reason, against a fixture that *is* interleaved.
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
    // **Reported from use, and the reason the columns are a property of the pane
    // rather than of the rows.** The first attempt sized the counts field to the
    // widest count among the rows the region was about to draw. That holds
    // *within* a window and moves *between* windows: scroll a list until a file
    // with `+1500 -1500` enters it and the field widens by six columns, sliding
    // every heat strip and sparkline on every row. Intermittent, and therefore
    // worse than a layout that was simply wrong.
    //
    // Two windows over the same list, one containing a file whose counts are
    // four columns wider than anything in the other.
    let mut view = ragged_counts();
    view.list = vec![
        listed("src/small.rs", 1, 1),
        listed("src/also-small.rs", 2, 0),
        listed("src/huge.rs", 1500, 1500),
    ];
    view.files = 3;

    let narrow = View {
        list: view.list[..2].to_vec(),
        ..view.clone()
    };
    let wide = View {
        list: view.list[1..].to_vec(),
        list_top: 1,
        ..view.clone()
    };

    // Non-vacuity: the two windows really do disagree about how wide a raw count
    // is, or this compares two identical screens.
    assert_ne!(
        narrow.list.iter().filter_map(|e| e.churn).max(),
        wide.list.iter().filter_map(|e| e.churn).max(),
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
    // **The scrollbar is the other way the contents reached the layout**, and it
    // survived the fix above because it does not look like contents. A region's
    // bar is drawn when `scrollable` finds the region cannot show everything it
    // holds, so a seventh changed file makes one appear, which narrows the region
    // by two columns, which re-plans every row in it. At some widths that merely
    // slid every element sideways; at others it crossed a rung boundary and took
    // the whole counts cell off every row of the list, for no reason a reader
    // could see and on the exact frame they were looking at.
    //
    // The repo had already ruled this hazard once: `CARET_FLOOR` counts
    // `BAR_WIDTH` on a screen with no bar so that the caret's presence cannot
    // depend on the file count. The columns pay it for the same reason now.
    //
    // Swept, because a boundary is the only place a rung can move, and asserted
    // over both regions: the list's bar answers to the changed-file count and
    // the stream's to the diff's total height, so each needs its own fixture
    // pair. An earlier form of this test claimed both and compared only the
    // list, which is the shape it exists to catch one level down.
    let entries: Vec<_> = (0..8)
        .map(|n| listed(&format!("src/file{n}.rs"), 42, 7))
        .collect();
    let view_of = |files: usize| View {
        list: entries[..files.min(entries.len())].to_vec(),
        rows: entries[..files.min(entries.len())]
            .iter()
            .cloned()
            .map(Row::File)
            .collect(),
        files,
        total_rows: files,
        ..ragged_counts()
    };

    // Few enough to need no bar, and enough to force one, at a height that shows
    // the list region.
    let (few, many) = (view_of(2), view_of(8));
    // Non-vacuity, and it has to be counted rather than flagged: a `differed`
    // bool set beside the assertion below can never be read, because the
    // assertion panics first. What can go wrong silently is the sweep drawing
    // *nothing* at every width, so what is counted is the widths that drew a
    // glance element at all.
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

    // **The stream, whose bar answers to the diff's height rather than the file
    // count.** A separate fixture pair, because nothing about the list can make
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
    // Compared with the bar's own column stripped, since that column is what
    // differs by construction; what must not differ is everything left of it.
    let strip = |row: String| row.trim_end_matches(['▕', '█']).trim_end().to_owned();
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
fn a_pulse_does_not_move_the_columns() {
    // **The pulse has a reserved slot, and that is the mechanism being
    // asserted.** It is reserved on every row of the region whether or not any
    // row is pulsing, so a file starting or stopping to pulse changes what is
    // *drawn* in that slot and never where any column sits.
    //
    // This comment has now been wrong in both directions, which is worth leaving
    // on the record: the slot was removed mid-branch when reserving the label
    // looked unaffordable at forty columns, and restored once that turned out to
    // be a bug in the choosing rather than a fact about the pulse. The assertion
    // never changed. Its stated reason was inverted twice, and a reader learning
    // the design from a test comment would have learned the opposite each time.
    let quiet = ragged_counts();
    let mut pulsing = ragged_counts();
    pulsing.list[0].recency = Recency::Pulse;

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
    let row = row_text(&drawn, 1);
    assert!(
        row.contains('●'),
        "no pulse reached the row, so nothing was asserted: {row:?}"
    );
}

#[test]
fn the_headers_two_tree_facts_are_drawn_in_one_weight() {
    // `SPEC.md` §11.1's other half of #67, and it had no gate. The count moved
    // next to the worktree name because the two are one clause about one
    // subject; drawing them in two weights would say in colour that they are
    // separate claims, which is the seam the move exists to remove.
    //
    // **This is untestable by omission and that is why it is here.** The
    // renderer takes one `style` for the whole left rung, so the ruling looks
    // enforced by the signature — but a signature is not evidence, and the
    // *which* half is unasserted by everything else on this screen: the
    // lost-watch gate reads the first `w`, which is the mode word, and every
    // legibility sweep reads symbols rather than cells. Drawing the left in
    // `chrome_dim` restores the seam and reddens nothing without this.
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

    // **Both modes and three widths**, because reading one screen is how a
    // style gate passes while the case a reader actually hits is unasserted.
    // A lost watch paints the right-hand side in `alert`, which is a third
    // style on this row, so the clause has to hold its own weight against that
    // too; and the continuation mark on a cut name inherits whatever style the
    // run that reached the edge carried, which is only exercised where the
    // clause does not fit.
    let mut saw_the_clause = 0usize;
    let mut saw_a_mark = 0usize;
    for (label, mode) in [("live", Mode::Watching), ("lost", Mode::Lost)] {
        // Seventeen is the clause's own width, so it is where the left is cut
        // and the continuation mark inherits a style. Without it the sweep only
        // ever saw a clause that fitted, and the comment above claimed a case
        // the widths could not reach.
        for width in [13u16, 17, 40, 80, 120] {
            let chrome = Chrome { mode, ..chrome() };
            let backend = screen(width, 8, &view, &chrome);
            let header = row_text(&backend, 0);

            // However much of the clause reached the screen. Read off the row
            // rather than computed from the width, because where the left ends
            // is what the ladder decided and recomputing it here would be the
            // renderer's own arithmetic agreeing with itself.
            // Trailing blanks are trimmed off the match, because the clause and
            // the background agree on a space: at seventeen columns the row is
            // `vigia   watching`, and the space after the name belongs to the
            // gap rather than to the clause the ladder drew.
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

            // All ASCII, so one char is one column. The mark, where the clause
            // was cut, is included deliberately: it inherits the style of the
            // run that reached the edge, so it is part of the clause's weight
            // rather than a separate decision.
            let cut = header[drawn.len()..].starts_with(CONTINUES);
            if cut {
                saw_a_mark += 1;
            }
            let cells = drawn.chars().count() + usize::from(cut);
            for x in 0..cells {
                let cell = &backend.buffer()[(x as u16, 0)];
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

    // And the test can tell the two apart: the blank after the clause is the
    // background style, which is the one a dimmed count would have taken.
    // Without this the loop above would pass against a renderer that painted the
    // entire row in `chrome`.
    let backend = screen(80, 8, &view, &chrome());
    let gap = &backend.buffer()[(clause.chars().count() as u16 + 1, 0)];
    assert_eq!(
        gap.style().fg,
        theme.chrome_dim.fg,
        "the column after the clause is not the chrome background, so this test \
         cannot distinguish the two weights it is asserting between"
    );
}

#[test]
fn a_nameless_worktree_draws_no_separator_with_nothing_on_its_left() {
    // The inversion of #67, and this diff is what made it reachable. ` · `
    // promises two facts about one subject; with the count on the right, a name
    // that came back empty drew an empty left-hand side and nothing was joined
    // to anything. With the count beside it, the same name builds
    // `" · 3 changed"`: a separator modifying nothing, which is the same false
    // promise the issue was filed about with the halves swapped.
    //
    // `short_name` cannot return an *empty* name on the shipped path, so the
    // first case here is reachable only through the public `render` with a
    // hand-built `Chrome`, which is what every test in this file does and what
    // `Chrome::default()` produces. The other two classes are not so narrow, and
    // that is the correction rather than a footnote: every name below is a legal
    // directory name on Linux and macOS, so `short_name` returns them verbatim
    // and they arrive on the shipped path like any other. **Linux and macOS
    // rather than all three targets**, which is a narrowing of an earlier claim
    // here: Windows strips a trailing space, so a name of one space resolves to
    // the parent, and it refuses a tab outright. The rest are legal everywhere.
    let view = View {
        files: 3,
        ..one_file()
    };
    // **Empty is the easy half and not the reachable one.** Four classes,
    // because the guard was wrong three times and each spelling failed a class
    // the last one passed. Empty is caught by `is_empty`; the zero-width names
    // are not, because they are non-empty `String`s that draw nothing; the
    // whitespace names are caught by neither of those, because they *have* width
    // and still show a reader nothing; and the control characters are caught by
    // none of the three, because they measure a column each and `trim` keeps
    // them while `ratatui` drops them before they reach a cell.
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
            worktree: name.to_owned(),
            ..chrome()
        };

        // **Every width, and the separator looked for anywhere on the row.**
        // Both halves of that were wrong first and both let a mutation through:
        // asserting only that the row does not *open* with the separator passes
        // against `3 changed · `, which is the same false promise with the
        // halves swapped again, and sampling three widths hid that the mutant's
        // wider clause also dropped the count entirely at 18 to 20 columns.
        //
        // Nothing else on this row can supply a `·`: the worktree name draws
        // nothing by construction here, the count is digits and a word, and the
        // mode words have no punctuation. So its absence is the whole rule.
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
    // A state nobody can see at a glance has not been reported. Drawn in the
    // header's dim grey, `not watching` is a word a reader has to go
    // looking for, and a monitor whose failure looks exactly like its working
    // state has failed twice.
    //
    // Invisible to the snapshots by construction: `TestBackend`'s `Display`
    // writes symbols and drops styles, so this has to read cells. Both
    // directions, because a header painted alert unconditionally would pass a
    // one-sided check while shouting at a healthy tree forever.
    let view = one_file();
    let theme = Theme::default();

    // Guard the fixture, the way `the_header_carries_no_changed_line_total`
    // does. The mode word is found by looking for the first `w` on the row, and
    // since #67 the left of that row carries the worktree name **and** the
    // count. A name or a count containing a `w` would silently point this at the
    // wrong cell, and it would fail in the passing direction: the left is drawn
    // in `chrome`, so a live watch would still look quiet.
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
    //
    // Before the split the durable half rode the notice alone and survived only
    // because the tick that clears a notice can never arrive again once the
    // watch is gone. Correct by coincidence is a bug waiting for the
    // coincidence to change.
    let view = one_file();
    let stopped = Chrome {
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
fn the_empty_state_names_the_branch_and_what_it_did_not_find() {
    // B3's four facts, two of which are the header's, so the body spends one row
    // rather than four.
    //
    // Not `working tree clean`, which is what this said before and which was
    // wrong rather than merely plain: that is git's phrase and git compares the
    // index against HEAD as well, so a fully staged worktree draws nothing here
    // and was being told it was clean while `git status` said the opposite.
    let backend = screen(80, 6, &nothing_changed(), &empty_chrome());
    assert_eq!(
        row_text(&backend, 1).trim_end(),
        "no unstaged changes · main"
    );
}

#[test]
fn a_detached_head_leaves_the_empty_state_naming_no_branch() {
    // Ordinary rather than exceptional: a rebase or a bisect leaves an agent
    // here routinely. The line drops the branch instead of inventing one,
    // because `HEAD@abc123` would put a commit id in a monitor that shows no
    // commits.
    let backend = screen(80, 6, &nothing_changed(), &chrome());
    assert_eq!(row_text(&backend, 1).trim_end(), "no unstaged changes");
}

#[test]
fn a_file_with_no_line_diff_says_why() {
    let view = View {
        list: Vec::new(),
        list_top: 0,
        current_span: 0,
        total_rows: 0,
        rows_above: 0,
        rows: vec![
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
                path: "src/merge.rs".to_owned(),
                from: None,
                kind: 'U',
                churn: None,
                spark: [0; HISTORY_BUCKETS],
                recency: Recency::Cold,
                heat: [HeatBucket::default(); HEAT_BUCKETS],
            }),
            Row::Note("unresolved conflict"),
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
    };
    insta::assert_snapshot!(screen(60, 8, &view, &chrome()));
}

#[test]
fn a_path_too_long_to_fit_keeps_the_end_that_names_the_file() {
    // Losing the tail would leave a column of `crates/vigia-core/…`, which names
    // nothing. This is the truncated-to-useless shape I6 forbids, and it is the
    // one part of I6 the renderer decides on its own rather than by layout.
    let view = View {
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
        peak: 0,
    };
    insta::assert_snapshot!(screen(40, 4, &view, &chrome()));
}

#[test]
fn a_hunk_covering_one_line_is_written_git_s_way() {
    // Git omits the count when a side covers exactly one line, and a reader
    // calibrated on `git diff` reads its absence as "one". Reproducing that is
    // cheaper than teaching them a second dialect, and a one-line file is the
    // only way to reach it: with three lines of context either side, no larger
    // file produces a single-line hunk. Found by mutation, which is also why it
    // has a test of its own rather than a comment.
    let view = View {
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
        peak: 0,
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
        notice: Some("the index entry for src/lib.rs points at a missing blob".to_owned()),
        ..chrome()
    };
    insta::assert_snapshot!(screen(80, 6, &view, &chrome));
}

#[test]
fn the_footer_shows_that_follow_is_engaged() {
    // I5 is otherwise invisible. A view that has not moved because nothing
    // changed and one that has not moved because following was switched off
    // look identical, and the reader's next action differs completely between
    // them.
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
///
/// The interesting ones are `9_949` and `9_950`. Formatting to one decimal
/// carries at 9.95, so `{:.1}` of the second would be `10.0ms` and six columns
/// wide: the branch has to end below where rounding carries rather than at a
/// round number, and this pair is what says so. `999_499` and `999_500` are the
/// same story one branch over.
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
///
/// `999` and `1000` mebibytes are the pair that matters: below it the cell draws
/// a four-digit number that would not fit, above it a sigil that does. The two
/// on the end are the degenerate cases a `u64` allows even though this process
/// cannot reach them.
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
    // `tests/legibility.rs` proves the ladder is legal at every width from 1 to
    // 120; a snapshot is the only artifact that shows the result is *good*, and
    // forty columns is where the footer has least to spend.
    //
    // The answer is better than the drop order alone would predict, and it is
    // worth a picture for exactly that reason. At forty columns the footer has
    // **already** taken a second line, because I6 makes it break rather than
    // shorten a hint, and the state it moves up there occupies thirteen of the
    // forty columns. So the readouts cost nothing at the width where columns are
    // scarcest: they fill a row that was bought for something else and was
    // mostly blank.
    //
    // Where they *do* go is narrower still, and `tests/legibility.rs` sweeps for
    // it rather than guessing at a number here.
    let view = one_file();
    insta::assert_snapshot!(screen(40, 6, &view, &diagnostics_chrome()));
}

/// Where `follow ▶` starts on the footer, for each of `chromes`.
///
/// **The neighbouring element, not the cell itself**, because that is the thing
/// a reader would see move. Observed by rendering rather than by measuring the
/// formatter's output, which would be the same arithmetic checking itself.
fn follow_marker_columns(chromes: impl IntoIterator<Item = Chrome>) -> Vec<u16> {
    let view = one_file();
    chromes
        .into_iter()
        .map(|chrome| column_of(&screen(80, 6, &view, &chrome), 5, "▶"))
        .collect()
}

#[test]
fn the_frame_cell_never_shifts_what_is_beside_it() {
    // **The one property that makes a per-frame readout safe to draw.** The value
    // changes on every frame by construction, so a cell sized to its own text
    // would be eleven columns one frame and ten the next, and `follow ▶` would
    // slide sideways under a reader who is trying to read it. Nothing else on
    // this screen changes width without the diff changing.
    let columns = follow_marker_columns(FRAME_TIMES.map(|cost| Chrome {
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
    // [`the_frame_cell_never_shifts_what_is_beside_it`]'s property, one cell
    // over. This one is the less obvious of the two: RSS looks like a number
    // that barely moves, and it is, right up to the frame where it crosses from
    // `999MiB` to `1024MiB` and takes a column with it.
    let columns = follow_marker_columns(MEMORY_SIZES.map(|bytes| Chrome {
        memory: Some(bytes),
        ..diagnostics_chrome()
    }));

    assert!(
        columns.windows(2).all(|pair| pair[0] == pair[1]),
        "the follow marker moved as memory changed: {columns:?} for {MEMORY_SIZES:?}"
    );
}

#[test]
fn the_memory_readout_is_drawn_wherever_the_read_is_a_syscall() {
    // **A content gate, and it is the kind `reads.rs` structurally cannot be.**
    // `reads.rs` counts bytes read from the worktree, so a readout that shelled
    // out to `tasklist` on every frame, or read `/proc` on every frame, would
    // leave every one of its eleven gates green. The gap between a cost gate and
    // a content gate is where a rejected design hides, so what is asserted here
    // is what reaches the screen.
    //
    // Both directions, and neither is vacuous. On a tier-1 target the cell is
    // present, which is what `SPEC.md` §5.1 rules. On a platform `crate::memory`
    // has no cheap answer for, `Chrome::memory` is `None` and nothing is drawn,
    // which is the branch that must not silently start drawing `0MiB`.
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
        memory: None,
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
    // The state a reader actually starts in, and it is not reachable by
    // narrowing: no frame has completed, so there is no p99 of anything, and
    // `App::chrome` reports `None`. Worth its own gate because the obvious
    // implementation draws `0.0ms frame` there, which is a measurement of
    // nothing presented as a measurement.
    //
    // Memory is deliberately `Some` here. It is readable on the first paint, and
    // this asserts that the pair still draws nothing: a lone memory cell on an
    // otherwise bare status bar would read as the important one, and it is the
    // cell the ladder drops *first* everywhere else.
    let view = one_file();
    let first = Chrome {
        frame: None,
        memory: Some(19 * 1024 * 1024),
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
    // The hardest screen the footer has to lay out, and the **default** one: at
    // forty columns with follow engaged, the hints and the state cannot share a
    // line. The footer takes a second rather than shortening anything, with the
    // state above and the hints keeping the bottom row they hold at eighty.
    //
    // The picture is here as well as in `tests/legibility.rs` because that file
    // can prove the layout is legal and only this one shows it is good.
    let view = one_file();
    insta::assert_snapshot!(screen(40, 6, &view, &following_chrome()));
}

#[test]
fn tabs_become_columns_and_control_characters_become_visible() {
    // Not cosmetic. A raw tab occupies one cell and advances nothing, so every
    // column after it is wrong; an escape character written through to the
    // terminal can move the cursor or open a sequence, which corrupts the whole
    // screen rather than one row. Both arrive from ordinary files that nobody
    // wrote for a display.
    let view = View {
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
        peak: 0,
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
    // Diffs carry whatever is in the files, and a CJK ideograph or an emoji
    // occupies two columns. Writing one into the last column of a row would
    // either overflow the buffer or print half a glyph, and either way every
    // column after it on that row is wrong. Swept across widths because the
    // failure only happens when a character straddles the exact clip boundary,
    // so a single width tests one alignment out of two.
    let view = View {
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
        peak: 0,
    };

    for width in 6..48u16 {
        let backend = screen(width, 5, &view, &chrome());
        let buffer = backend.buffer();
        for y in 0..5 {
            // A two-column symbol lives in one cell and the cell after it is left
            // as a blank placeholder. Counting that placeholder as a column is
            // wrong, and getting it wrong the first time made a correct row
            // measure two columns over: the row has to be reconstructed the way a
            // terminal walks it, skipping what the previous symbol already
            // covered.
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
        list: Vec::new(),
        list_top: 0,
        current_span: 0,
        total_rows: 0,
        rows_above: 0,
        rows: vec![line(LineKind::Added, 1234, "let value = compute(input);")],
        files: 1,
        top: Position::default(),
        read: 1,
        peak: 0,
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
    // A pane being dragged narrow steps through every one of these sizes. A
    // monitor that panics on the way is worse than one that draws something
    // cramped, and `diff_height` is what the caller uses to ask for rows, so it
    // must never ask for more rows than the screen has after its chrome.
    //
    // That it asks for exactly the right number is a stronger claim and it is
    // `tests/legibility.rs` that makes it, by counting the rows that come back
    // rather than by restating the renderer's own arithmetic.
    let view = one_file();
    for (width, height) in [(0, 0), (1, 1), (1, 2), (80, 1), (80, 2), (80, 3), (2, 30)] {
        let backend = screen(width, height, &view, &chrome());
        let area = ratatui::layout::Rect::new(0, 0, width, height);
        assert!(
            diff_height(area, &chrome(), view.files) < usize::from(height).max(1),
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
    //
    // **This one exists because a saturated heat strip panicked at 33x6**, as
    // the layout table stood when the fault was found.
    // `heat_at` folded a group of `u16` bucket counts with `sum()`, and the
    // six-slice rung groups two buckets, so a file busy enough to fill them
    // overflowed and took the pane down. #77's layout table is what made that
    // rung the one an ordinary forty-column pane picks, so the fault went from
    // theoretical to reachable. A monitor that dies on a file is the failure
    // mode this repo rules out everywhere else.
    //
    // Values at the type's limit rather than at a plausible one, because the
    // point is the arithmetic and not the plausibility.
    let saturated = FileEntry {
        path: "src/generated.rs".to_owned(),
        from: None,
        kind: 'M',
        churn: Some((u32::MAX, u32::MAX)),
        spark: [u16::MAX; HISTORY_BUCKETS],
        recency: Recency::Pulse,
        heat: [HeatBucket {
            added: u16::MAX,
            removed: u16::MAX,
        }; HEAT_BUCKETS],
    };
    let view = View {
        list: vec![saturated.clone(), listed("a.rs", 0, 0)],
        list_top: 0,
        current_span: 400,
        total_rows: 400,
        rows_above: 0,
        rows: vec![Row::File(saturated)],
        files: 2,
        top: Position::default(),
        read: 1,
        peak: u16::MAX,
    };

    // Every heat and sparkline rung is reached inside this range, which is what
    // makes the grouping arithmetic exercised at all: the six-slice rung, where
    // the fault was, is drawn between 37 and 47 columns in a region with no
    // caret, and 39 to 49 in the pinned list, which has one. Read off
    // `ROW_LAYOUTS` rather than swept, because a full sparkline bucket draws the
    // same block as a heat slice and a sweep that counted glyphs would count
    // both.
    for width in 0..=60u16 {
        for height in 0..=8u16 {
            let backend = screen(width, height, &view, &chrome());
            assert_eq!(backend.buffer().area.width, width);
        }
    }
}

#[test]
fn a_rename_never_names_only_the_file_it_came_from() {
    // `elide_head` cuts the head because a path's tail identifies the file. That
    // premise is false of `new ← old`: cutting the head of the pair leaves
    // `…src/main.rs`, which names the file the rename came *from* and never
    // mentions the one the row is about.
    //
    // Latent before this branch and ordinary after it, because fixed slots left
    // the path less room: the pair stopped fitting at 107 columns where it used
    // to stop at 60.
    let renamed = FileEntry {
        path: "crates/vigia/src/shell.rs".to_owned(),
        from: Some("crates/vigia/src/main.rs".to_owned()),
        kind: 'R',
        churn: Some((0, 0)),
        spark: [0; HISTORY_BUCKETS],
        recency: Recency::Cold,
        heat: [HeatBucket::default(); HEAT_BUCKETS],
    };
    let view = View {
        list: vec![renamed.clone()],
        rows: vec![Row::File(renamed)],
        files: 1,
        ..ragged_counts()
    };

    let mut saw_pair = 0usize;
    let mut saw_alone = 0usize;
    for width in 1..=120u16 {
        let row = row_text(&screen(width, 8, &view, &chrome()), 1);
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
    // The counts abbreviation had no gate, and it shipped a wrong number: a
    // narrower cell left two characters, a 250-line change has no truthful form
    // in two, and the search fell through to the thousands unit and drew `+0k`.
    // At exactly forty columns, which is the width I6 is named for.
    //
    // Asserted over the drawn row, and at the boundaries of every unit rather
    // than at comfortable values, because an abbreviation is only ever wrong
    // where one unit gives way to the next.
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

    // **Whole tokens, not prefixes**, because `contains` is the wrong instrument
    // for a value: `contains("+1")` is satisfied by `+1k`, `+1M` and `+139`, and
    // `contains("-0")` is satisfied by `-0k`, which is precisely the wrong number
    // this test is named after. A token counts only when what follows it cannot
    // be part of the same number.
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
            list: vec![listed("src/f.rs", lines, 0)],
            files: 1,
            ..ragged_counts()
        };
        let backend = screen(80, 8, &view, &chrome());
        let row = row_text(&backend, 1);
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
        removed_at.push(column_of(&backend, 1, "-"));
        // Where the *added* half starts, which is the half a content-sized cell
        // actually moves. The removed half is right-anchored at the row's edge
        // and cannot move however the cell is sized, so recording only that
        // would state a property the layout gets for free.
        added_at.push(column_of(&backend, 1, "+"));
    }

    // **The field is fixed, which is the whole reason `COUNT_CELL` is a constant
    // rather than a rung.** The added half's text grows from `+0` to `+9999`
    // across these boundaries. Right-aligned inside a fixed field its start
    // column moves by exactly the text's length; sized to its contents, the
    // *removed* half would move too. Both are recorded, because each catches a
    // different way of getting this wrong and neither catches the other's.
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
    // Snapshots cannot see this: `TestBackend`'s `Display` writes symbols and
    // drops styles, so every colour in the theme is invisible to the rest of this
    // file. Without this test the palette could be one colour throughout and the
    // whole suite would stay green.
    let view = one_file();
    let backend = screen(80, 14, &view, &chrome());
    let buffer = backend.buffer();
    let theme = Theme::default();

    let row_of = |needle: char, y: u16| {
        let cell = &buffer[(0, y)];
        assert_eq!(
            cell.symbol(),
            needle.to_string(),
            "row {y} does not start with {needle:?}, so this test is reading the \
             wrong line"
        );
    };

    // Body rows start at y = 1: the header is y = 0. The gutter occupies the
    // first columns, so the sigil and its colour are found past it.
    let sigil_x = 4;
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
        buffer[(0, 5)].style().fg,
        theme.gutter.fg,
        "the line number is not drawn in the gutter colour"
    );
    row_of('v', 0);
}

/// The row of content this file's syntax tests read.
const CONTENT_ROW: u16 = 3;

#[test]
fn a_syntax_class_reaches_the_cells_while_the_sigil_keeps_the_diff() {
    // `SPEC.md` §11.1's ruling, as cells, and it is two claims at once. The
    // mockup colours added, removed and context lines identically and leaves the
    // diff signal to the sigil, so the text must take its class colour *and* the
    // `+` must keep the green the text no longer has.
    //
    // Invisible to every snapshot in this file, for the reason the palette test
    // above gives: `TestBackend`'s `Display` writes symbols and drops styles.
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
    let at = |offset: u16| buffer[(sigil + 1 + offset, CONTENT_ROW)].style().fg;

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
    //
    // Both widths below are that rule. At a workable width the mark belongs to
    // whichever run ran out of room, so a clipped comment is marked in the
    // comment's colour. At one column there is no room for any run at all, and
    // the mark still has to mean something: it takes the first run's style,
    // which is the sigil's, so a clipped added line is marked in green rather
    // than in whatever the theme's default happened to be.
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

    let wide = screen(30, 6, &view(), &chrome());
    let mark = column_of(&wide, CONTENT_ROW, CONTINUES);
    assert_eq!(
        wide.buffer()[(mark, CONTENT_ROW)].style().fg,
        theme.comment.fg,
        "the mark is not drawn in the colour of the comment it cut"
    );

    let narrow = screen(1, 6, &view(), &chrome());
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
    // The subtle half of drawing a line as runs. Tab stops are measured from the
    // start of the line's own content, so the column counter has to be carried
    // **across** span boundaries. Reset per span, a tab in the second run would
    // advance to a stop measured from that run's start, which is invisible until
    // a file indents with tabs and then wrong on every row of it.
    //
    // `a` sits at column 0, so the tab after it advances three columns to the
    // stop at four. Counted from the span instead, it would advance four.
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
///
/// Deliberately the mockup's own three files and counts: `assets/preview.svg`
/// draws `watch.rs +42 −7` bright, `frame.rs +11 −3` below it and `Cargo.toml
/// +2 −0` visibly fainter than either. `SPEC.md` §5.1 reads that dimmed row as a
/// recency gradient, so a fixture that invented its own files would be checking
/// the renderer against nothing in particular.
///
/// The buckets are chosen so the two live files disagree about their own peak
/// and agree about the screen's, which is what makes the scale assertion below
/// able to fail.
fn glancing() -> View {
    View {
        list: Vec::new(),
        list_top: 0,
        current_span: 0,
        total_rows: 0,
        rows_above: 0,
        rows: vec![
            Row::File(FileEntry {
                path: "src/engine/watch.rs".to_owned(),
                from: None,
                kind: 'M',
                churn: Some((42, 7)),
                spark: [0, 0, 1, 3, 8, 5, 9, 12],
                recency: Recency::Pulse,
                // Additions at the head, a mixed slice in the middle, removals
                // at the tail. One row carrying all three kinds plus the track,
                // which is what the colour gate below reads.
                heat: heat(&[(0, 9, 0), (1, 2, 0), (5, 3, 4), (11, 0, 6)]),
            }),
            Row::File(FileEntry {
                path: "src/render/frame.rs".to_owned(),
                from: None,
                kind: 'M',
                churn: Some((11, 3)),
                spark: [0, 0, 0, 2, 1, 0, 0, 0],
                recency: Recency::Live,
                heat: heat(&[(3, 2, 1)]),
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

/// Block glyphs on row `y` drawn in `colour`, in column order.
///
/// **The colour is the discriminator, not decoration.** A sparkline's top rung
/// and every heat slice are both `█`, so a symbol-only scan of a heading counts
/// one strip as part of the other. Two tests here were written before the heat
/// strip existed and started reading thirteen blocks the moment it landed.
fn blocks_of(backend: &TestBackend, y: u16, colour: ratatui::style::Color) -> Vec<char> {
    let buffer = backend.buffer();
    (0..buffer.area.width)
        .map(|x| &buffer[(x, y)])
        .filter(|cell| cell.style().fg == Some(colour))
        .filter_map(|cell| cell.symbol().chars().next())
        .filter(|glyph| "▁▂▃▄▅▆▇█".contains(*glyph))
        .collect()
}

/// Which columns of row `y` carry a sparkline track.
///
/// [`blocks_of`]'s twin, and matched on symbol **and** style for the reason that
/// function gives, sharpened by what the track is drawn from: `_` is an ordinary
/// character, and a `snake_case` path two columns to the left of the strip is
/// full of them. `Theme::spark_track` is what separates the two here, and
/// `tests/palette.rs` holds that it can: under `Theme::default`, which is what
/// every caller below draws with, the track is `DarkGray` where a path is
/// `White`, `White` bold or `Gray`. That gate also records the one palette and
/// depth where the separation does **not** hold, which is why this says "here"
/// rather than "always".
///
/// Columns rather than glyphs, because every track cell carries the same glyph
/// and what the gates below ask is *where* and *how many*.
fn track_at(backend: &TestBackend, y: u16, theme: &Theme) -> Vec<u16> {
    let buffer = backend.buffer();
    (0..buffer.area.width)
        .filter(|&x| {
            let cell = &buffer[(x, y)];
            cell.symbol() == SPARK_TRACK && cell.style().fg == theme.spark_track.fg
        })
        .collect()
}

/// Which columns of row `y` carry a sparkline **bucket**.
///
/// [`track_at`]'s twin, the way `has_heat` and `has_spark` were: the two halves
/// of one slot, asked the same way, so a gate comparing them is comparing like
/// with like. [`blocks_of`] answers a different question on the same cells and
/// cannot stand in for this, because it returns the glyphs and drops the columns.
fn bars_at(backend: &TestBackend, y: u16, theme: &Theme) -> Vec<u16> {
    let buffer = backend.buffer();
    (0..buffer.area.width)
        .filter(|&x| {
            let cell = &buffer[(x, y)];
            RAMP.contains(&cell.symbol()) && cell.style().fg == theme.spark.fg
        })
        .collect()
}

/// [`glancing`], on the frame a reader actually opens the pane to.
///
/// **The launch case, which nothing else here covers**: `SPEC.md` §5.1's history
/// is fed from the watch, so a worktree that was already dirty when `vigia`
/// started has no tick behind any of its files and every row's buckets are zero
/// ([#78](https://github.com/breferrari/vigia/issues/78)). Every fixture in this
/// suite drives ticks first, which is exactly why the state a reader sees first
/// went two phases without a gate.
///
/// Every rung set to `Cold` as well as every bucket to zero, because the two are
/// the same fact: `Recency::Cold` means *nothing is tracked for this path*, so a
/// row that pulsed with an empty history would be a state the store cannot
/// produce, and a fixture that draws one is the same defect as the picture that
/// did.
///
/// `peak` is zero for the same reason, which also puts the `peak == 0` branch of
/// the renderer's scaling under a gate rather than under an argument.
fn launched() -> View {
    let mut view = glancing();
    for row in &mut view.rows {
        if let Row::File(entry) = row {
            entry.spark = [0; HISTORY_BUCKETS];
            entry.recency = Recency::Cold;
        }
    }
    view.peak = 0;
    view
}

#[test]
fn a_worktree_already_dirty_at_launch_draws_a_track_on_every_row() {
    // **The state a reader sees first, and it had no fixture.** The pane is
    // opened *because* work is in progress, so the ordinary first frame is one
    // where no file has been written since the watch opened. Before #78 the
    // sparkline column was blank on every row of it, and the element `SPEC.md`
    // §5 names as one of four differentiators was absent exactly when it
    // mattered most.
    let theme = Theme::default();
    let spark = theme.spark.fg.expect("the sparkline has a colour");
    let backend = screen(80, 5, &launched(), &chrome());

    let mut starts = Vec::new();
    for y in 1..=3u16 {
        let track = track_at(&backend, y, &theme);
        assert_eq!(
            track.len(),
            HISTORY_BUCKETS,
            "row {y} drew {} track cells where the slot is {HISTORY_BUCKETS} \
             wide, so a file with no history is still leaving part of its \
             column blank: {track:?}",
            track.len()
        );
        // Contiguous, or the cells counted above are not one strip. A path
        // carrying an underscore cannot reach this, because `track_at` matches
        // the style too, but a renderer that wrote the track into the wrong
        // columns could.
        assert!(
            track.windows(2).all(|pair| pair[1] == pair[0] + 1),
            "row {y}'s track is not one run of cells: {track:?}"
        );
        // **Nothing invented.** The track says "no churn in the window", and a
        // single bar would be a number the store never recorded.
        assert!(
            blocks_of(&backend, y, spark).is_empty(),
            "row {y} drew a sparkline bar for a file with no history, which is \
             churn the store cannot have"
        );
        starts.push(track[0]);
    }

    // The columnar reading #77 landed, on the screen it is least able to give
    // today: three tracks at one `x` are what makes three sparklines read as one
    // small-multiples chart once the files start moving.
    assert!(
        starts.windows(2).all(|pair| pair[0] == pair[1]),
        "the tracks start at different columns on different rows: {starts:?}"
    );
}

#[test]
fn the_first_tick_after_launch_moves_no_column() {
    // **The transition, which is the moment both #77 and #78 are actually for.**
    // A reader opens the pane on a dirty worktree (every row track), the agent
    // writes one file, and that row grows a bucket. `SPEC.md` §11.1 reserves the
    // slot from the pane precisely so this frame does not reflow, and #78 is
    // what makes the before-picture a drawn column rather than a blank one, so
    // the two frames are comparable at all.
    //
    // Its own gate because `peak` was only ever fixtured at 0, 12 and `u16::MAX`
    // in this suite: one is the launch, one is a settled screen and one is the
    // saturation guard. **A peak of 1 is the first frame after launch**, and it
    // is the only value at which the ramp's numerator and denominator are equal,
    // so a single write draws the *top* of the ramp rather than its floor.
    let theme = Theme::default();
    let launch = screen(80, 5, &launched(), &chrome());

    let mut view = launched();
    if let Row::File(entry) = &mut view.rows[0] {
        entry.spark[HISTORY_BUCKETS - 1] = 1;
        entry.recency = Recency::Pulse;
    }
    view.peak = 1;
    let after = screen(80, 5, &view, &chrome());

    // The written file: seven track cells and one bucket, and the bucket is the
    // top of the ramp because it is the busiest thing on screen.
    assert_eq!(
        track_at(&after, 1, &theme).len(),
        HISTORY_BUCKETS - 1,
        "the file that was just written did not keep the rest of its window as \
         track"
    );
    assert_eq!(
        blocks_of(&after, 1, theme.spark.fg.expect("a colour")),
        vec!['█'],
        "one write against a screen peak of one is not the top of the ramp"
    );
    // Every other file is still cold, and still says so.
    for y in [2, 3] {
        assert_eq!(
            track_at(&after, y, &theme).len(),
            HISTORY_BUCKETS,
            "row {y} stopped drawing its track when a *different* file was \
             written"
        );
    }

    // **Nothing moved.** The whole slot occupies the same columns before and
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
    for y in 1..=3u16 {
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
    // **A guard that no test could fail is a wish, and this one was.** The
    // renderer scales a bucket against `View::peak`, and a peak of zero beside a
    // bucket that holds something is a view the store cannot produce: the peak
    // is the maximum over every tracked path, so a non-zero bucket lifts it.
    // Constructing one by hand is legal, though, and `SPEC.md` §11.1 rules that
    // a monitor which dies on a file is the failure to avoid, so the division
    // has to survive an inconsistent caller rather than assume a consistent one.
    //
    // Found by mutation: deleting the guard killed nothing, because every fixture
    // that sets `peak` to zero also has every bucket empty and the loop skips the
    // division before reaching it.
    let mut view = launched();
    if let Row::File(entry) = &mut view.rows[0] {
        entry.spark = [3; HISTORY_BUCKETS];
    }
    assert_eq!(
        view.peak, 0,
        "the fixture stopped being the inconsistent one"
    );

    let theme = Theme::default();
    let backend = screen(80, 5, &view, &chrome());
    assert_eq!(
        track_at(&backend, 1, &theme).len(),
        HISTORY_BUCKETS,
        "a bucket with no scale to measure it against drew something other than \
         the track"
    );
}

#[test]
fn a_bucket_busier_than_the_screens_peak_draws_the_top_and_not_a_panic() {
    // The **other** inconsistent caller, and the one the clamp's upper bound is
    // for. `peak == 0` above is the store's own empty state; this is a peak that
    // exists and is too small, where `count * 8 / peak` runs off the end of
    // `SPARK_RAMP` and indexing it aborts the pane. `SPEC.md` §11.1 rules that a
    // monitor which dies on a file is the failure to avoid, and `heat_at`'s
    // saturating fold two functions away is the same ruling applied to the same
    // shape of arithmetic.
    //
    // Its own gate because the guard is a `clamp` whose two bounds are not alike:
    // the lower one is unreachable (a count of at least one already puts the
    // numerator at or above the ramp's length) and the upper one is live.
    let mut view = glancing();
    if let Row::File(entry) = &mut view.rows[0] {
        entry.spark = [u16::MAX; HISTORY_BUCKETS];
    }
    view.peak = 1;

    let theme = Theme::default();
    let backend = screen(80, 5, &view, &chrome());
    assert_eq!(
        blocks_of(&backend, 1, theme.spark.fg.expect("a colour")),
        vec!['█'; HISTORY_BUCKETS],
        "a bucket far busier than the screen's peak did not simply top out"
    );
}

#[test]
fn an_empty_bucket_draws_the_track_and_a_written_one_draws_a_bar() {
    // One rule rather than a special case for the cold file: the launch screen
    // above is just the all-empty end of *this*. Row 2 of the mockup's own
    // fixture holds `[0, 0, 0, 2, 1, 0, 0, 0]`, so it draws both kinds and the
    // order has to survive.
    let theme = Theme::default();
    let backend = screen(80, 5, &glancing(), &chrome());

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
        drawn, "tttssttt",
        "`[0, 0, 0, 2, 1, 0, 0, 0]` drew {drawn:?}, so a bucket's emptiness is \
         not where the store says it is"
    );
}

#[test]
fn the_track_is_never_the_shape_of_a_written_bucket() {
    // **The gate on the height channel, and the reason the track is not `▁`.**
    // The heat strip beside it may reuse `█` between a live slice and its track
    // because colour is its only channel; a sparkline's channel is height, so a
    // track drawn at the ramp's floor would make "one write" and "no writes" the
    // same shape and leave colour alone carrying a distinction `SPEC.md` §11.1
    // spends the lowest block to protect.
    //
    // **The columns are chosen by position and the glyphs are read off them**,
    // which is the third spelling of this gate and the first that can fail.
    //
    // It ended on `assert_ne!(TRACK, RAMP[0])` to begin with, comparing two
    // constants this file declares. That was replaced with a comparison of two
    // cells, which looked like a fix and was the same tautology one indirection
    // deeper: the cells were *found* by matching those constants, so their
    // symbols were what the search asked for and the assertion still could not
    // fail. Worse, the violation it is named for (`SPARK_TRACK` at the ramp's
    // floor) made the search find nothing and died on an `expect` whose message
    // blamed the fixture.
    //
    // So the slot is located **off a different row**, and that is what finally
    // makes the assertion load-bearing. Row 1 is given a history with no empty
    // bucket, so all eight of its cells are bars and `bars_at` returns the
    // slot's columns without the track glyph entering into it at all. The
    // layout is a property of the region rather than of a row (#77), so those
    // columns are row 2's slot too.
    //
    // Then bucket 0 of row 2's `[0, 0, 0, 2, 1, 0, 0, 0]` is empty according to
    // the *store*, and what the renderer put in that column is read out of the
    // buffer with nothing having said what to expect. Changing `SPARK_TRACK` to
    // the ramp's floor now fails on the claim rather than on a lookup.
    let theme = Theme::default();
    let mut view = glancing();
    if let Row::File(entry) = &mut view.rows[0] {
        entry.spark = [1, 2, 3, 4, 5, 6, 7, 8];
    }
    let backend = screen(80, 5, &view, &chrome());
    let buffer = backend.buffer();

    let slot = bars_at(&backend, 1, &theme);
    assert_eq!(
        slot.len(),
        HISTORY_BUCKETS,
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
        blocks_of(&backend, 2, theme.spark.fg.expect("a colour")).contains(&'▁'),
        "the fixture no longer draws the ramp's floor, so the comparison above \
         is not against the glyph the ruling is about"
    );
}

#[test]
fn a_narrowed_sparkline_keeps_the_newest_buckets_and_drops_the_oldest() {
    // **The one property of the narrow rung nothing could see.** At the widths
    // where the sparkline halves, `Painter::file_row` draws `strip[8 - n..]`, the
    // *tail* of the window, because dropping buckets means dropping the oldest
    // and the oldest are on the left. Drawing `strip[..n]` instead is the same
    // number of cells in the same columns, so every gate that counts a rung
    // agrees with both, and every fixture in this file is drawn at a width where
    // the slot is 8 or 0 and the two slices are identical.
    //
    // Verified by mutation before it was written: swapping the slice for its
    // head killed **nothing** across the whole workspace. Widths 42 to 51 are the
    // band where the rung is four, measured rather than assumed.
    //
    // Row 2 holds `[0, 0, 0, 2, 1, 0, 0, 0]`. Its newest four are `[1, 0, 0, 0]`,
    // which draws bar-then-track; its oldest four are `[0, 0, 0, 2]`, which draws
    // track-then-bar. The two are each other's reverse, so this cannot pass
    // against the wrong end.
    let theme = Theme::default();
    let backend = screen(44, 5, &glancing(), &chrome());

    let mut slot: Vec<(u16, char)> = track_at(&backend, 2, &theme)
        .into_iter()
        .map(|x| (x, 't'))
        .chain(bars_at(&backend, 2, &theme).into_iter().map(|x| (x, 's')))
        .collect();
    slot.sort_unstable();

    assert_eq!(
        slot.len(),
        HISTORY_BUCKETS / 2,
        "44 columns is meant to be the four-bucket rung, so this fixture is no \
         longer exercising the narrow slice at all: {slot:?}"
    );
    let drawn: String = slot.iter().map(|&(_, class)| class).collect();
    assert_eq!(
        drawn, "sttt",
        "the narrowed strip drew {drawn:?}; \"sttt\" is the newest four buckets \
         and \"ttts\" is the oldest four, so this is the wrong end of the window"
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
fn a_file_that_just_changed_says_so_and_the_rest_dim() {
    // Two claims, and the second is invisible to every snapshot in this file
    // because `TestBackend`'s `Display` drops styles: the pulse label belongs to
    // exactly one row, and the three rungs have to be three *different*
    // intensities or the gradient `SPEC.md` §5.1 asks for is not being drawn.
    let theme = Theme::default();
    let backend = screen(80, 5, &glancing(), &chrome());

    // Body rows start at y = 1; the header is y = 0.
    let pulsing = row_text(&backend, 1);
    assert!(
        pulsing.contains("just changed"),
        "the file named by the newest tick carries no pulse: {pulsing:?}"
    );
    for y in [2, 3] {
        let row = row_text(&backend, y);
        assert!(
            !row.contains("just changed"),
            "row {y} carries the pulse too, so it marks more than the newest \
             tick: {row:?}"
        );
    }

    // The path's own cell, past the kind letter and its space. Compared on
    // foreground and modifiers rather than on the whole `Style`, because a cell
    // carries the buffer's own defaults for everything the theme left alone.
    let path_x = 2;
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
    // The whole reason `View::peak` exists. Scaled per file, both rows below
    // would top out at the full block and the eye would read two files of very
    // different activity as equally busy. Scaled against the screen, only the
    // busiest one reaches the top.
    //
    // Read by colour rather than by glyph. The heat strip beside it draws the
    // same full block, so a scan of the row's text counts a heat slice as a
    // sparkline bucket; this test passed on symbols alone until that strip
    // landed. See [`blocks_of`].
    let spark = Theme::default()
        .spark
        .fg
        .expect("the sparkline has a colour");
    let backend = screen(80, 5, &glancing(), &chrome());
    let busiest = blocks_of(&backend, 1, spark);
    let quieter = blocks_of(&backend, 2, spark);

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
    // A file nothing has written since startup draws no *bar*. It draws the
    // track instead, which is #78's ruling and which
    // `a_worktree_already_dirty_at_launch_draws_a_track_on_every_row` is the
    // gate for. Stated as two halves rather than one, because the first alone
    // was the whole assertion until then and it stays green against a renderer
    // that draws nothing at all: `blocks_of` cannot see a track.
    assert!(
        blocks_of(&backend, 3, spark).is_empty(),
        "a file with no churn drew a sparkline bar, which is churn the store \
         cannot have"
    );
    assert_eq!(
        track_at(&backend, 3, &Theme::default()).len(),
        HISTORY_BUCKETS,
        "a file with no churn drew no track either, so its column is blank"
    );
}

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
    // Invisible to every snapshot in this file, and more so than the palette
    // test above: a heat strip draws the *same glyph* for all four kinds, so a
    // picture of one is twelve identical blocks. The colour is the entire
    // signal, which makes this the only place the strip is really tested.
    let theme = Theme::default();
    let backend = screen(120, 5, &glancing(), &chrome());
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
    let strip: Vec<_> = (0..120)
        .map(|x| &buffer[(x, 1)])
        .filter(|cell| cell.symbol() == "█")
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

// The plan for #39 asked for heat-strip snapshots at 80 and 40 columns. They
// exist, and they are `the_glance_elements_at_*` above: the shared fixture now
// carries heat, so those two pictures *are* the heat-strip pictures. A third one
// was written and came out byte-identical to the 80-column glance snapshot,
// which is a second copy of one assertion rather than a second assertion, so it
// was deleted rather than stored. What a symbol snapshot cannot see at all is
// the colour, and that is
// `the_four_heat_kinds_reach_the_cells_and_are_distinct` above.

/// The two-region screen `SPEC.md` §11.1 rules: a pinned list over a diff.
///
/// `current` is an index into the list as drawn, so the caret's row is chosen by
/// the fixture rather than derived from it. `row` is how far into that file the
/// viewport has scrolled, which is what the diff's scrollbar reads.
fn two_regions_at(current: usize, row: usize) -> View {
    View {
        list: vec![
            entry("src/engine/change.rs", 8, 2),
            entry("src/engine/watch.rs", 42, 7),
            entry("src/render/frame.rs", 11, 3),
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
        peak: 0,
    }
}

/// The same screen at the top of its current file.
fn two_regions(current: usize) -> View {
    two_regions_at(current, 0)
}

#[test]
fn the_caret_marks_the_file_the_diff_is_inside() {
    // The one thing on screen that says which of the listed files the diff below
    // belongs to. Asserted by row rather than by presence: a caret drawn on every
    // row, or on a fixed row, would satisfy "there is a caret somewhere" and say
    // nothing.
    //
    // Both directions, per this file's own rule: the marked row has it and the
    // others do not.
    const CARET: &str = "▸";

    for current in 0..3usize {
        let view = two_regions(current);
        let backend = screen(64, 18, &view, &chrome());
        let buffer = backend.buffer();

        for row in 0..3u16 {
            // The list starts on row 1, immediately under the single header line.
            let marked = buffer[(0, row + 1)].symbol() == CARET;
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

        // Three list rows under the header, so the rule is row four.
        let y = 4u16;
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
    // I6's ladder applied to the newest glance element. The caret costs the list
    // a column, so it is dropped on a pane too narrow to spare one, and the only
    // thing that makes such a drop legible is that it happens **once**: a marker
    // that came back at a narrower width would read as the current file changing
    // while a reader dragged a pane edge.
    //
    // Monotonicity rather than a threshold, deliberately. The threshold is the
    // renderer's own constant, and a test that restated it would agree with the
    // code by construction instead of checking it. This asserts the shape of the
    // ladder, which no constant can satisfy by accident.
    const CARET: &str = "▸";

    let drawn: Vec<bool> = (1..=60u16)
        .map(|width| {
            let view = two_regions(1);
            let backend = screen(width, 18, &view, &chrome());
            let buffer = backend.buffer();
            (0..width)
                .map(|x| buffer[(x, 2)].symbol())
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

#[test]
fn the_list_scrollbar_spans_the_visible_window() {
    // The list's bar is exact, because both of its numbers are free: the window
    // it shows and the changed-file count are known without reading anything.
    //
    // Ten files with three on screen, so the thumb is a proper fraction rather
    // than the whole bar, and it has somewhere to move to.
    const TRACK: &str = "▕";
    let width = 64u16;

    let mut seen = Vec::new();
    for list_top in [0usize, 3, 7] {
        let view = View {
            list_top,
            files: 10,
            ..two_regions(list_top)
        };
        let backend = screen(width, 18, &view, &chrome());
        let marks = thumb_rows(&backend, width - 1, 1..4);
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
        for y in 1..4u16 {
            let symbol = buffer[(width - 1, y)].symbol();
            assert!(
                symbol == TRACK || marks.contains(&y),
                "row {y} of the list bar is {symbol:?}, neither thumb nor track"
            );
        }
        seen.push(marks[0]);
    }

    // Monotone, and moving overall. Not strictly increasing at every step, and
    // that is resolution rather than a defect: three rows of bar over ten files
    // cannot separate every window, so windows 0 and 3 legitimately round to the
    // same row. What must hold is that it never goes backwards and that it does
    // move, which a bar ignoring its input would fail.
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
    // **What the bar means since I4 was narrowed**: the thumb is the screen's
    // rows over the diff's rows, and it sits at the rows above the screen. That
    // is what every other scrollbar means, and it is only sayable because
    // counting a diff's height turned out to cost 8.76ms where building it cost
    // 442.71ms.
    //
    // Replaces a gate named for the approximation this shipped with first, which
    // interpolated the whole from the current file's height. That bar vanished on
    // a short file, ballooned on a long one and never reached the bottom, and no
    // assertion about "moving within a file" can catch any of those.
    let width = 64u16;
    let height = 24u16;
    let region = 5u16..height - 1;
    let rows = usize::from(region.end - region.start);

    // A thumb that halves when the diff doubles, which is the proportionality no
    // file-counting scheme can express.
    let mut lengths = Vec::new();
    for total in [rows * 2, rows * 4, rows * 8] {
        let view = View {
            total_rows: total,
            rows_above: 0,
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

    // And it travels the whole track, ending exactly at the bottom.
    let total = rows * 6;
    let mut firsts = Vec::new();
    for above in [0, total / 4, total / 2, total - rows] {
        let view = View {
            total_rows: total,
            rows_above: above,
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
            assert_eq!(marks[0], region.start, "the top of the diff is not the top");
        }
        if above == total - rows {
            assert_eq!(
                *marks.last().expect("a thumb"),
                region.end - 1,
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
    const TRACK: &str = "▕";
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

    for y in 1..4u16 {
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
    const TRACK: &str = "▕";
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
}

/// A pinned list of `shown` rows over `files` changed files, scrolled to `top`.
fn a_list_of(files: usize, shown: usize, top: usize) -> View {
    View {
        list: (0..shown)
            .map(|i| entry(&format!("src/f{i}.rs"), 1, 0))
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
        peak: 0,
    }
}

#[test]
fn a_scrollbar_reaches_the_bottom_at_its_last_window() {
    // **The invariant `Painter::scrollbar`'s own doc claims and neither region's
    // gate stated**: the thumb's travel maps onto the track's travel, so the last
    // position fills the bottom row exactly as the first fills the top.
    //
    // Swept over the file count rather than checked at one, because the defect
    // this exists for hides at particular denominators: the previous gate used
    // ten files in a three-row region, where the floor division truncates to zero
    // and `.max(1)` makes the wrong formula accidentally right. That is the
    // "measured at its cheapest position" shape §7 already records, one axis over.
    let width = 64u16;
    let shown = 6usize;
    let region = 1u16..1 + shown as u16;

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
            region.end - 1,
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
            Some(region.start),
            "{files} files: the first window's thumb does not start at the top"
        );
        assert_ne!(
            marks, top_marks,
            "{files} files: the bar draws the same column at both ends, so it \
             says nothing about where the window is"
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
        // The diff region starts under three list rows and the rule.
        let region = 5u16..height - 1;
        let rows = usize::from(region.end - region.start);
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
            region.end - 1,
            "span {span}: the last screenful's thumb ends at {:?}, not the \
             bottom of the track",
            marks.last()
        );
    }
}

#[test]
fn the_caret_does_not_vanish_because_another_file_changed() {
    // Two ladders that collide. `Painter::list` decides the caret against the
    // width it is handed, and `render` has already taken the bar's columns off
    // that width — so whether the caret survives depends on whether the list is
    // *scrollable*, which depends on the changed-file count. Both floors are
    // sixteen, so at sixteen and seventeen columns a seventh changed file made
    // the marker saying which file the diff is inside disappear with nothing
    // about the pane having moved.
    //
    // That is precisely the reading `the_caret_degrades_once_and_never_flickers`
    // exists to prevent, and it was blind to this because its fixture has
    // `files == list.len()` and is never scrollable. The file count is the second
    // axis.
    const CARET: &str = "▸";

    for width in 1..=60u16 {
        let mut drawn = Vec::new();
        for files in [3usize, 30] {
            let view = a_list_of(files, 3, 0);
            let backend = screen(width, 24, &view, &chrome());
            let buffer = backend.buffer();
            drawn.push(
                (0..width)
                    .map(|x| buffer[(x, 1)].symbol())
                    .collect::<String>()
                    .contains(CARET),
            );
        }
        assert_eq!(
            drawn[0],
            drawn[1],
            "at {width} columns the caret is {} with three changed files and {} \
             with thirty, so a file appearing elsewhere moved the marker",
            if drawn[0] { "drawn" } else { "absent" },
            if drawn[1] { "drawn" } else { "absent" }
        );
    }
}

#[test]
fn a_row_keeps_its_floor_after_both_the_bar_and_the_caret() {
    // What `CARET_FLOOR`'s `BAR_WIDTH` term buys, which is not the same property
    // as `the_caret_does_not_vanish_because_another_file_changed`. That one says
    // the caret's presence depends on the pane alone; this one says the caret is
    // never drawn on a row too narrow to still name its file afterwards, on a
    // screen where the bar has already taken its columns.
    //
    // Found by mutation: dropping the `BAR_WIDTH` term left the consistency gate
    // green, because both file counts then lost the caret at the same widths and
    // agreed with each other while the row underneath was two columns short.
    //
    // Constants restated for the reason this file always restates them: sharing
    // the renderer's own would make the assertion agree with the code by
    // construction.
    const ROW_FLOOR: usize = 2 + 12; // the kind letter and its gap, plus MIN_PATH_WIDTH
    const BAR_COLUMNS: usize = 2;
    const CARET_COLUMNS: usize = 2;
    const CARET: &str = "▸";
    const TRACK: &str = "▕";
    const THUMB: &str = "█";

    let mut saw_both = false;
    for width in 1..=60u16 {
        // Thirty files over three rows, so the list is scrollable and the bar is
        // drawn wherever the pane can afford one.
        let view = a_list_of(30, 3, 0);
        let backend = screen(width, 24, &view, &chrome());
        let buffer = backend.buffer();

        let row: String = (0..width).map(|x| buffer[(x, 1)].symbol()).collect();
        let caret = row.contains(CARET);
        let bar = row.ends_with(TRACK) || row.ends_with(THUMB);

        if !caret {
            continue;
        }
        if bar {
            saw_both = true;
        }
        let left = usize::from(width) - if bar { BAR_COLUMNS } else { 0 } - CARET_COLUMNS;
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
    // `any_area_renders_including_the_ones_that_fit_nothing` sweeps pane sizes
    // but only over `one_file()`, whose list is empty and whose `current_span` is
    // zero — so the three fields this branch added are never degenerate in it.
    // Writing past a `Buffer`'s area panics inside ratatui, which is how the
    // region overdrawing the footer was found at 1x3, and that was caught by a
    // different sweep by luck rather than by this one.
    //
    // The origin is non-zero as well, because every `Rect` the renderer builds
    // inherits `..area` and an off-by-one in `x` or `y` is invisible at (0, 0).
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
            list: vec![entry("src/日本語/テスト.rs", 3, 1)],
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
                    render(&mut buf, area, view, &Theme::default(), &chrome());
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

/// A removed line's wash stops before the scrollbar's column.
///
/// [#81](https://github.com/breferrari/vigia/issues/81) was filed **undiagnosed
/// on purpose**, from a real pane where the wash appeared to reach the far right
/// and meet the bar. Two explanations fit that report and only one is ours: the
/// row may be washing the columns `with_bar` took, or the host terminal may be
/// drawing its own scrollbar over a correct full-bleed band.
///
/// This is the gate that tells them apart, and it is the thing the issue says
/// does not exist. It reads the **background** of the bar's column on a row that
/// is definitely washed, which is the property the snapshots structurally cannot
/// see: `TestBackend`'s `Display` writes symbols and drops styles.
#[test]
fn a_wash_stops_before_the_scrollbar_column() {
    /// The same draw as [`screen`], on the palette that actually tints a row.
    ///
    /// `Theme::default()` is the sixteen named colours, which draw **no row tint
    /// at any depth** by the ruling in `theme.rs`. Rendering this gate through it
    /// would assert that a wash which was never painted did not reach a column,
    /// which is the shape §7 keeps finding: a gate that cannot fail.
    fn washed_screen(width: u16, height: u16, view: &View, chrome: &Chrome) -> TestBackend {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
        let theme = vigia::Theme::dark();
        terminal
            .draw(|f| {
                let area = f.area();
                vigia::render(f.buffer_mut(), area, view, &theme, chrome);
            })
            .expect("draw");
        terminal.backend().clone()
    }

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

    let wash = buffer[(1, washed)].bg;
    let bar = buffer[(width - 1, washed)].bg;
    assert_ne!(
        wash,
        ratatui::style::Color::Reset,
        "the removed line was not washed at all, so this gate proves nothing"
    );
    assert_ne!(
        bar,
        wash,
        "the wash reached the scrollbar's own column at x={}",
        width - 1
    );

    // And the gap beside it, which is what `BAR_WIDTH` reserves so the thumb does
    // not sit flush against a count.
    assert_ne!(
        buffer[(width - 2, washed)].bg,
        wash,
        "the wash reached the column reserved beside the bar"
    );
}

#[test]
fn the_follow_marker_is_green_where_the_word_beside_it_is_dim() {
    // `assets/preview.svg` draws `follow ` in `.dim` and `▶` in `.grn`, and
    // §5.1's rule is that a published artifact answering a question is the
    // answer. The shell drew the whole state in one dim grey, so the one glyph a
    // reader checks at a glance rather than reads looked like the word beside
    // it.
    //
    // Invisible to every snapshot by construction: `TestBackend`'s `Display`
    // writes symbols and drops styles, so this has to read cells.
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
    // `assets/preview.svg` draws `0.8ms` and `24MiB` in `.cyn` and the word
    // `frame` beside them in `.dim`. The shipped footer drew all of it in one
    // grey, so the two numbers a reader checks at a glance looked like the words
    // around them.
    //
    // Both directions in one test: a footer painted `chrome` throughout would
    // satisfy the first assertion and fail the second.
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
    // **Found to the right of the frame number, not by the leftmost `f`.** The
    // hint bar shares this row and opens `q quit · f follow`, so a scan from
    // column 0 lands on the hints' own `f` at column 9 and reads a cell that is
    // dim for reasons of its own. That made this assertion vacuous: adding
    // `is_ascii_alphabetic` to the tint's opening test paints the whole word
    // `frame` cyan and the old form still passed.
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
    // because `concat!` takes no `char`. Two spellings of one glyph can drift,
    // and the drift is silent: the recolouring pass would simply find nothing
    // and the marker would quietly go back to grey.
    //
    // Asserted through the drawn row rather than by importing either constant,
    // which is this file's rule for anything the renderer also spells.
    //
    // **The claim is about `FOLLOWING`'s own token, not about the row.** An
    // earlier form allowed `state.ends_with('▶')` *or* any word containing the
    // marker, and the second disjunct subsumes the first: the position is drawn
    // after the state at this width, so the row never ends in the marker and
    // only the weak half ever fired. What it proved was "the marker is on the
    // footer somewhere", which is not what it is named for and not what
    // `FOLLOW_MARK`'s docblock cites it for.
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
    // **This one gates a hang, not a colour**, and it is the more valuable half.
    // The recolouring pass walked a run by asking two questions of a cell: does
    // it *open* a measurement, and does it *carry* one. `>` answers yes and no,
    // so on `>1s` the inner walk broke without consuming the column the outer
    // walk had just accepted, and the two spun against each other forever with
    // the pane frozen mid-frame. A monitor that stops redrawing is the one
    // failure this product class cannot absorb.
    //
    // It was reachable from the ordinary path rather than from hostile input:
    // `>1s` is what a frame over a second draws and `>1GiB` what memory over a
    // gigabyte draws, both of which `SPEC.md` §11.1 specifies.
    //
    // Two existing gates already drew `>1s` and so *hung* rather than failed,
    // which is why this is stated as its own property: a suite that times out
    // names no defect.
    let theme = Theme::default();
    for (what, chrome, sigil) in [
        (
            "a frame over a second",
            Chrome {
                frame: Some(Duration::from_secs(2)),
                ..diagnostics_chrome()
            },
            ">1s",
        ),
        (
            "memory over a gigabyte",
            Chrome {
                memory: Some(2 * 1024 * 1024 * 1024),
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

        // The sigil opens the run, so it and every column of the abbreviation
        // after it carry the measurement's colour. A `>` left grey would mean
        // the run started one column late, which is the shape the hang came
        // from.
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
    // A notice is an error string, an error string carries a path, and `▶` is a
    // legal character in a path on every platform this ships to. The recolouring
    // pass scanned the whole footer row for the marker and stopped at the first
    // one it found, so a file called `▶.rs` in an error message took the green.
    //
    // Both directions, and both are wrong screens rather than cosmetic ones.
    // With follow off it *fabricates* the one glyph on the footer that says the
    // view is live. With follow on it lights the notice and leaves the real
    // marker grey, so the reader is told the opposite in the wrong place.
    let theme = Theme::default();
    for following in [false, true] {
        let chrome = Chrome {
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
                // **Exactly one, not at most one.** `<= 1` alone is satisfied by
                // a pass that stopped tinting the real marker altogether, which
                // is the other half of the same defect: the notice's glyph won
                // the scan and the marker it should have lit stayed grey.
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
    // §11.1 rules that `N/M` stays dim: the picture gives it no colour, and it
    // is a *place* rather than a measurement, which is the whole distinction the
    // footer's three colours draw. It is a number sitting a few columns from two
    // other numbers that are cyan, so the rule is one bound away from being
    // wrong and nothing was reading it.
    //
    // Ungated until now, and the gap was reachable by a one-word edit: widening
    // the tint's `end` from the diagnostics' own columns to the row's edge turns
    // `1/1` cyan and leaves every other gate green, because the `frame` label
    // survives on its own (a letter opens no run).
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
    // **[#77](https://github.com/breferrari/vigia/issues/77)'s ruling one
    // element over, and on the rows a reader actually reads.** The gutter was
    // measured against the region's width, and the stream's region loses two
    // columns to a scrollbar, and that bar appears when the diff outgrows the
    // pane. So crossing the pane height took the whole gutter off every content
    // row: watching an agent write, the line numbers vanish the moment the diff
    // gets long, for no reason on screen.
    //
    // Asserted on the gutter, not on the whole row. A drawn scrollbar takes two
    // columns and the text reflows into what is left, which is what a scrollbar
    // is for and is true on `main` too. What was the defect is the gutter
    // *vanishing*: all-or-nothing rather than a reflow.
    //
    // Swept, because the gutter gives way at a width band that moves with the
    // digit count rather than at a single boundary; it was reachable at 29 and
    // 30 columns for three-digit numbers.
    let body = |total_rows: usize| View {
        rows: vec![
            Row::File(listed("src/engine/watch.rs", 42, 7)),
            line(LineKind::Added, 258, "    pub fn advance(&mut self) {"),
        ],
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
            "at {width} columns a diff taller than the pane lost its line              numbers entirely, so the gutter is sized from the diff's height:              {flat:?} became {deep:?}"
        );
    }
    assert!(
        compared > 40,
        "only {compared} widths drew the line number at all, so this swept over          rows with no gutter to lose"
    );
}

#[test]
fn render_clips_to_the_buffer_rather_than_the_area() {
    // `render`'s own contract is that any area is legal, and most writers here
    // reach the cells through `Buffer::set_stringn` or `set_style`, which clip.
    // Three reached them by index and asserted: the heat strip, the rule and the
    // scrollbar. Each traded a string call for a direct cell write to stop an
    // allocation per cell, which was the right trade, and each traded the
    // clipping away with it unremarked.
    //
    // **The fixture matrix is the gate here, not the sweep.** An earlier form
    // carried neither a heat strip nor a pinned list, so it reached only one
    // writer and passed against a renderer that still panicked in two other
    // places. A view carrying each is what makes the claim true.
    //
    // **Width only, and the height case is deliberately not here.** An area
    // *taller* than its buffer panics too, in the row-drawing paths, and does so
    // identically on `origin/main`: that is
    // [#91](https://github.com/breferrari/vigia/issues/91), a pre-existing gap
    // in the same contract, filed rather than widened into here.
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
                list: Vec::new(),
                rows: Vec::new(),
                ..one_file()
            },
            // A heat strip, a pinned list to put a rule on screen, and enough
            // files to make the list scrollable so the bar is drawn too.
            View {
                list: vec![
                    listed("src/engine/watch.rs", 42, 7),
                    listed("src/render/frame.rs", 11, 3),
                ],
                rows: vec![Row::File(listed("src/engine/watch.rs", 42, 7))],
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
                &chrome(),
            );
        }
    }
}
