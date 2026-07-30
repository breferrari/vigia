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

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::text::Span;
use vigia::{Chrome, HINT_SEPARATOR, Position, Row, Theme, View, body_height, render};
use vigia_core::LineKind;

/// The mark meaning "this continues past the right edge".
const CONTINUES: char = '›';
/// The mark meaning "the beginning of this is gone".
const ELIDED: char = '…';
/// The follow indicator's own glyph, which no hint contains.
///
/// Matched on rather than the word `follow`, because `f follow` is a hint and
/// would make every state assertion pass against a footer showing only advice.
const FOLLOW_MARK: char = '▶';

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

fn line(kind: LineKind, number: u32, text: &str) -> Row {
    Row::Line {
        kind,
        number,
        text: text.to_owned(),
    }
}

fn chrome() -> Chrome {
    Chrome {
        worktree: "vigia".to_owned(),
        notice: None,
        following: false,
    }
}

fn following() -> Chrome {
    Chrome {
        following: true,
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
        rows: vec![
            Row::File {
                path: "crates/vigia-core/src/frame.rs".to_owned(),
                from: None,
                kind: 'M',
                churn: Some((3, 1)),
            },
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
            Row::File {
                path: "assets/banner.jpg".to_owned(),
                from: None,
                kind: 'M',
                churn: None,
            },
            Row::Note("binary"),
            Row::File {
                path: "crates/vigia/src/shell.rs".to_owned(),
                from: Some("crates/vigia/src/main.rs".to_owned()),
                kind: 'R',
                churn: Some((0, 0)),
            },
        ],
        files: 3,
        top: Position::default(),
        read: 3,
    }
}

/// A view with content nobody wrote for a display: double-width, and a path
/// longer than any pane.
fn awkward() -> View {
    View {
        rows: vec![
            Row::File {
                path: "crates/vigia-core/src/very/deeply/nested/module/frame.rs".to_owned(),
                from: None,
                kind: 'M',
                churn: Some((12, 3)),
            },
            line(LineKind::Added, 1, "見出し a 見出し b 見出し c"),
            line(LineKind::Added, 2, "🙂🙂🙂 tail"),
        ],
        files: 1,
        top: Position::default(),
        read: 1,
    }
}

fn empty() -> View {
    View {
        rows: Vec::new(),
        files: 0,
        top: Position::default(),
        read: 0,
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
fn numbered(n: usize, files: usize) -> View {
    View {
        rows: (0..n)
            .map(|i| line(LineKind::Added, 1, &format!("R{i:02}")))
            .collect(),
        files,
        top: Position::default(),
        read: 1,
    }
}

/// Whether `row` treats `label` honestly: drawn whole, dropped entirely, or cut
/// with the mark. Silently cut is the one illegal outcome, and it is the shape
/// I6 calls truncated-to-useless.
///
/// Dropped entirely is legal because the right-hand text is placed first: a
/// header at six columns spends them on `1 file` and shows no name at all,
/// which `Painter::put_right` documents as deliberate.
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
    vec![
        ("every row kind, idle", every_row_kind(), chrome()),
        ("every row kind, following", every_row_kind(), following()),
        ("every row kind, notice", every_row_kind(), with_notice()),
        ("awkward content, following", awkward(), following()),
        ("clean worktree, following", empty(), following()),
        ("clean worktree, idle", empty(), chrome()),
    ]
}

#[test]
fn no_row_ever_occupies_more_columns_than_the_screen() {
    // The overflow half of I6, and the half that would corrupt the screen rather
    // than merely read badly: a row wider than the pane wraps in the terminal,
    // which pushes every row below it down and makes the shape meaningless.
    for (name, view, chrome) in cases() {
        for width in WIDTHS {
            for height in [3u16, 6, 24] {
                for y in 0..height {
                    let columns = occupied(width, height, &view, &chrome, y);
                    assert!(
                        columns <= usize::from(width),
                        "{name}: row {y} at {width}x{height} occupies {columns} columns"
                    );
                }
            }
        }
    }
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
fn the_body_gets_exactly_the_rows_the_caller_was_promised() {
    // `body_height` is what a caller asks `View::collect` for, and the renderer
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
                    let promised = body_height(area, &chrome, files);
                    let view = numbered(promised + 3, files);
                    let rows = rows_at(width, height, &view, &chrome);
                    let painted = rows.join("\n");

                    let drawn = (0..promised + 3)
                        .filter(|i| painted.contains(&format!("R{i:02}")))
                        .count();
                    assert_eq!(
                        drawn, promised,
                        "at {width}x{height} over {files} files the caller was \
                         promised {promised} body rows and the renderer drew {drawn}"
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

    let wide = body_height(Rect::new(0, 0, 80, tall), &following(), view.files);
    assert_eq!(
        wide,
        usize::from(tall) - 2,
        "eighty columns hold the hints and the state on one line"
    );
    let widest = body_height(Rect::new(0, 0, 120, tall), &following(), view.files);
    assert_eq!(widest, usize::from(tall) - 2, "so do a hundred and twenty");

    let narrow_idle = body_height(Rect::new(0, 0, 40, tall), &chrome(), view.files);
    assert_eq!(
        narrow_idle,
        usize::from(tall) - 2,
        "forty columns hold them too once the follow marker is gone"
    );

    let narrow = body_height(Rect::new(0, 0, 40, tall), &following(), view.files);
    assert_eq!(
        narrow,
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
    for width in WIDTHS {
        for height in [5u16, 24] {
            let area = Rect::new(0, 0, width, height);
            assert_eq!(
                body_height(area, &following(), view.files),
                body_height(area, &with_notice(), view.files),
                "a notice changed the body height at {width}x{height}"
            );
        }
    }
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
            body_height(area, &following(), view.files),
            usize::from(height) - 2,
            "the footer grew at 40x{height} and spent a body row it could not spare"
        );
    }
    // Non-vacuity: one row taller and it does grow, so the guard is what stops
    // it rather than the collision never happening at these widths.
    let area = Rect::new(0, 0, 40, 5);
    assert_eq!(
        body_height(area, &following(), view.files),
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
    };
    let long_name = Chrome {
        worktree: "a-worktree-with-a-very-long-name-indeed".to_owned(),
        ..chrome()
    };

    let mut fitted = 0usize;
    let mut cut = 0usize;
    for width in WIDTHS {
        let rows = rows_at(width, 8, &view, &long_name);
        // The header shares its line with the file count, so how much room the
        // name actually gets is the renderer's business and `label_is_honest` is
        // the whole assertion for it. The two body rows own their full width, so
        // for those the width alone decides and both directions can be checked.
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
        rows: vec![line(LineKind::Removed, 260, text)],
        files: 1,
        top: Position::default(),
        read: 1,
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
