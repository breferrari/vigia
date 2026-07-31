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
use vigia::{
    Chrome, HEAT_BUCKETS, HeatBucket, Mode, Position, Row, Theme, View, body_height, render,
};
use vigia_core::{Class, HISTORY_BUCKETS, LineKind, Recency, Span};

/// The mark the renderer writes where a row runs past its edge.
///
/// Restated here rather than imported: it is one character of published
/// behaviour, and a test that shared the constant would agree with the renderer
/// by construction instead of checking it.
const CONTINUES: &str = "›";

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
        rows: vec![
            file('M', "src/a.rs", 1, 0),
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
    let buffer = backend.buffer();
    (0..buffer.area.width)
        .find(|x| buffer[(*x, y)].symbol() == needle)
        .unwrap_or_else(|| panic!("no {needle:?} anywhere on row {y}"))
}

fn file(kind: char, path: &str, added: u32, removed: u32) -> Row {
    Row::File {
        path: path.to_owned(),
        from: None,
        kind,
        churn: Some((added, removed)),
        spark: [0; HISTORY_BUCKETS],
        recency: Recency::Cold,
        heat: [HeatBucket::default(); HEAT_BUCKETS],
    }
}

/// A view with the shape a real frame produces: a file, a hunk, mixed lines.
fn one_file() -> View {
    View {
        rows: vec![
            file('M', "crates/vigia-core/src/frame.rs", 3, 1),
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
    // The mockup headers `watching · 3 files` and only the count ever shipped.
    //
    // Both directions in one test, because a word drawn unconditionally is not a
    // mode: it has to say something different when something different is true.
    let view = one_file();

    let live = row_text(&screen(80, 6, &view, &chrome()), 0);
    assert!(live.contains("watching · 1 file"), "live header: {live:?}");
    assert!(!live.contains("not watching"), "live header: {live:?}");

    let stopped = Chrome {
        mode: Mode::Lost,
        ..chrome()
    };
    let lost = row_text(&screen(80, 6, &view, &stopped), 0);
    assert!(
        lost.contains("not watching · 1 file"),
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
    assert!(
        body.contains("+42 -7"),
        "no per-file counter was drawn, so the header's silence proves nothing: \
         {body:?}"
    );

    // And the header is populated, so its silence is about the total rather than
    // about the row being empty.
    assert!(header.contains("watching · 3 files"), "header: {header:?}");

    for needle in TOTALS {
        assert!(
            !header.contains(needle),
            "the header drew {needle:?}, which is a changed-line total or half of \
             one: {header:?}"
        );
    }
}

#[test]
fn a_lost_watch_is_loud_and_a_live_one_is_quiet() {
    // A state nobody can see at a glance has not been reported. Drawn in the
    // same dim grey as the count, `not watching` is a word a reader has to go
    // looking for, and a monitor whose failure looks exactly like its working
    // state has failed twice.
    //
    // Invisible to the snapshots by construction: `TestBackend`'s `Display`
    // writes symbols and drops styles, so this has to read cells. Both
    // directions, because a header painted alert unconditionally would pass a
    // one-sided check while shouting at a healthy tree forever.
    let view = one_file();
    let theme = Theme::default();

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
        rows: vec![
            Row::File {
                path: "assets/banner.jpg".to_owned(),
                from: None,
                kind: 'M',
                churn: None,
                spark: [0; HISTORY_BUCKETS],
                recency: Recency::Cold,
                heat: [HeatBucket::default(); HEAT_BUCKETS],
            },
            Row::Note("binary"),
            Row::File {
                path: "src/merge.rs".to_owned(),
                from: None,
                kind: 'U',
                churn: None,
                spark: [0; HISTORY_BUCKETS],
                recency: Recency::Cold,
                heat: [HeatBucket::default(); HEAT_BUCKETS],
            },
            Row::Note("unresolved conflict"),
            Row::File {
                path: "crates/vigia/src/shell.rs".to_owned(),
                from: Some("crates/vigia/src/main.rs".to_owned()),
                kind: 'R',
                churn: Some((0, 0)),
                spark: [0; HISTORY_BUCKETS],
                recency: Recency::Cold,
                heat: [HeatBucket::default(); HEAT_BUCKETS],
            },
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
        rows: vec![file(
            'M',
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
        rows: vec![
            file('M', "VERSION", 1, 1),
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
fn the_frame_cell_never_shifts_what_is_beside_it() {
    // **The one property that makes a per-frame readout safe to draw.** The value
    // changes on every frame by construction, so a cell sized to its own text
    // would be eleven columns one frame and ten the next, and `follow ▶` would
    // slide sideways under a reader who is trying to read it. Nothing else on
    // this screen changes width without the diff changing.
    //
    // Observed by rendering rather than by measuring the formatter's output,
    // which would be the same arithmetic checking itself. What is asserted is
    // where the *neighbouring* element lands, because that is the thing a reader
    // would see move.
    let view = one_file();
    let columns: Vec<u16> = FRAME_TIMES
        .iter()
        .map(|cost| {
            let chrome = Chrome {
                frame: Some(*cost),
                ..diagnostics_chrome()
            };
            column_of(&screen(80, 6, &view, &chrome), 5, "▶")
        })
        .collect();

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
    let view = one_file();
    let columns: Vec<u16> = MEMORY_SIZES
        .iter()
        .map(|bytes| {
            let chrome = Chrome {
                memory: Some(*bytes),
                ..diagnostics_chrome()
            };
            column_of(&screen(80, 6, &view, &chrome), 5, "▶")
        })
        .collect();

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
        rows: vec![
            file('M', "Makefile", 1, 0),
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
        rows: vec![
            file('M', "docs/読み方.md", 2, 0),
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
    // cramped, and `body_height` is what the caller uses to ask for rows, so it
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
            body_height(area, &chrome(), view.files) < usize::from(height).max(1),
            "body_height asked for more rows than {width}x{height} has"
        );
        // Non-vacuity: the loop must actually have produced a buffer of the size
        // asked for, or it proved only that nothing was drawn.
        assert_eq!(backend.buffer().area.width, width);
        assert_eq!(backend.buffer().area.height, height);
    }
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
        rows: vec![
            Row::File {
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
            },
            Row::File {
                path: "src/render/frame.rs".to_owned(),
                from: None,
                kind: 'M',
                churn: Some((11, 3)),
                spark: [0, 0, 0, 2, 1, 0, 0, 0],
                recency: Recency::Live,
                heat: heat(&[(3, 2, 1)]),
            },
            Row::File {
                path: "Cargo.toml".to_owned(),
                from: None,
                kind: 'M',
                churn: Some((2, 0)),
                spark: [0; HISTORY_BUCKETS],
                recency: Recency::Cold,
                heat: [HeatBucket::default(); HEAT_BUCKETS],
            },
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
    // A file nothing has written since startup has no strip, rather than an
    // empty one taking columns from its own path.
    assert!(
        blocks_of(&backend, 3, spark).is_empty(),
        "a file with no churn drew a sparkline"
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
        theme.heat_added_heavy,
        theme.heat_removed,
        theme.heat_removed_heavy,
        theme.heat_mixed,
        theme.heat_mixed_heavy,
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
    assert_eq!(
        strip[0],
        want(theme.heat_added_heavy),
        "slice 0: 9 additions"
    );
    assert_eq!(
        strip[1],
        want(theme.heat_added),
        "slice 1: 2 additions, light"
    );
    assert_eq!(strip[5], want(theme.heat_mixed_heavy), "slice 5: 3 and 4");
    assert_eq!(
        strip[11],
        want(theme.heat_removed_heavy),
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
