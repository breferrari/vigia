//! `SPEC.md` §11.1: the note mark on a file row, and the heat slice that says
//! where in the file the note sits.
//!
//! Read off drawn cells rather than off the view, and matched on glyph **and**
//! colour together, because `✎` also draws in the diff's gutter, `↳` opens the
//! agent's reply on a content row, and the strip's `■` shares a foreground with
//! nothing but itself only until something else lands on the row.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::style::Color;
use vigia::{
    Chrome, Depth, FileEntry, FileNotes, Glyphs, HEAT_BUCKETS, HeatBucket, Mode, NoteMark,
    Position, Row, Scale, Theme, View, render,
};
use vigia_core::{Origin, Recency};

/// A pane wide enough for the widest heat rung, which needs 139 columns once the
/// mark has taken its own.
const WIDE: u16 = 140;

/// The row the first file is drawn on: the header owns row 0.
const FIRST: u16 = 1;

/// One drawn slice of a heat strip.
const SLICE: &str = "■";

/// Every glyph the mark may draw, and the only three. An absolute list rather
/// than a comparison against an unmarked row: a differential assertion is
/// satisfied by a mark drawn in every state, which is the shape that hides a
/// glyph nobody wanted.
const GLYPHS: [&str; 3] = ["✎", "↳", "✓"];

fn chrome() -> Chrome {
    Chrome {
        pressed: None,
        gripped: None,
        hovered: None,
        selected: None,
        scrolling: None,
        overview: false,
        worktree: "vigia".to_owned(),
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
        rail: false,
        sheet: None,
        frame: None,
        memory: None,
        notes: Default::default(),
    }
}

/// A heat map from `(slice, added, removed)` triples, everything else track.
fn heat(slices: &[(usize, u16, u16)]) -> [HeatBucket; HEAT_BUCKETS] {
    let mut map = [HeatBucket::default(); HEAT_BUCKETS];
    for &(at, added, removed) in slices {
        map[at] = HeatBucket { added, removed };
    }
    map
}

/// One file row carrying every glance element, so the mark is drawn beside a
/// full cluster rather than into an empty row.
fn one_file(notes: FileNotes, newest: bool) -> View {
    View {
        hidden: 0,
        whole: Vec::new(),
        landed: false,
        recorded: 0,
        list_span: 1,
        grouped: false,
        list: Vec::new(),
        list_top: 0,
        current_span: 0,
        churn: None,
        total_rows: 0,
        rows_above: 0,
        rows: vec![Row::file(FileEntry {
            origin: Origin::Unstaged,
            path: "src/engine/watch.rs".to_owned(),
            from: None,
            kind: 'M',
            churn: Some((42, 7)),
            spark: [
                0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 5, 5, 8, 8, 5, 5, 9, 9, 12, 12,
            ],
            recency: Recency::Pulse,
            newest,
            notes,
            heat: heat(&[(0, 9, 0), (1, 2, 0), (5, 3, 4), (11, 0, 6)]),
        })],
        files: 1,
        top: Position::default(),
        read: 1,
        scale: Scale::spread(12),
        gutter: None,
        notes: vigia::Noted::default(),
    }
}

fn drawn(width: u16, view: &View, theme: &Theme) -> TestBackend {
    let mut terminal = Terminal::new(TestBackend::new(width, 5)).expect("terminal");
    terminal
        .draw(|f| {
            let area = f.area();
            render(
                f.buffer_mut(),
                area,
                view,
                theme,
                Glyphs::default(),
                &chrome(),
            );
        })
        .expect("draw");
    terminal.backend().clone()
}

/// `(column, glyph, ink)` of the one cell on `y` holding a mark glyph.
fn mark_at(backend: &TestBackend, y: u16) -> Option<(u16, String, Option<Color>)> {
    let buffer = backend.buffer();
    let found: Vec<_> = (0..buffer.area.width)
        .map(|x| (x, &buffer[(x, y)]))
        .filter(|(_, cell)| GLYPHS.contains(&cell.symbol()))
        .map(|(x, cell)| (x, cell.symbol().to_owned(), cell.style().fg))
        .collect();
    assert!(
        found.len() <= 1,
        "row {y} drew {} mark glyphs; the slot is one column",
        found.len()
    );
    found.into_iter().next()
}

/// The heat strip's inks on `y`, in column order.
fn strip(backend: &TestBackend, y: u16) -> Vec<Option<Color>> {
    let buffer = backend.buffer();
    (0..buffer.area.width)
        .map(|x| &buffer[(x, y)])
        .filter(|cell| cell.symbol() == SLICE)
        .map(|cell| cell.style().fg)
        .collect()
}

/// Which slices of the strip carry the note's ink.
fn noted_slices(inks: &[Option<Color>], theme: &Theme) -> Vec<usize> {
    inks.iter()
        .enumerate()
        .filter(|(_, ink)| **ink == theme.heat_note.fg)
        .map(|(at, _)| at)
        .collect()
}

/// The full row as text.
fn row(backend: &TestBackend, y: u16) -> String {
    let buffer = backend.buffer();
    (0..buffer.area.width)
        .map(|x| buffer[(x, y)].symbol().chars().next().unwrap_or(' '))
        .collect()
}

const STATES: [(NoteMark, &str); 3] = [
    (NoteMark::Waiting, "✎"),
    (NoteMark::Replied, "↳"),
    (NoteMark::Resolved, "✓"),
];

fn marked(mark: NoteMark) -> FileNotes {
    FileNotes {
        mark: Some(mark),
        at: [false; HEAT_BUCKETS],
    }
}

#[test]
fn each_state_of_a_note_draws_its_own_glyph_in_its_own_ink() {
    let theme = Theme::dark().resolve(Depth::Truecolor);
    let mut seen: Vec<(String, Option<Color>)> = Vec::new();

    for (mark, glyph) in STATES {
        let backend = drawn(WIDE, &one_file(marked(mark), false), &theme);
        let (_, drawn_glyph, ink) =
            mark_at(&backend, FIRST).unwrap_or_else(|| panic!("{mark:?} drew no mark"));
        assert_eq!(drawn_glyph, glyph, "{mark:?} drew the wrong glyph");
        assert_eq!(
            ink,
            theme.note_mark(mark).fg,
            "{mark:?} did not take its own key"
        );
        seen.push((drawn_glyph, ink));
    }

    // Both halves have to be distinct or the gate above passes on a renderer that
    // tells no two states apart: three equal glyphs would each match themselves,
    // and three equal inks would each match a key that happens to be one colour.
    let glyphs: std::collections::BTreeSet<_> = seen.iter().map(|(g, _)| g.clone()).collect();
    assert_eq!(glyphs.len(), STATES.len(), "two states share a glyph");
    let inks: std::collections::BTreeSet<_> = seen.iter().map(|(_, i)| format!("{i:?}")).collect();
    assert_eq!(inks.len(), STATES.len(), "two states share an ink");
}

#[test]
fn the_mark_tells_its_states_apart_where_there_is_no_colour_at_all() {
    // `Depth::None` serves `NO_COLOR`, and it is why the state rides the glyph
    // rather than the ink: an ink-only mark is three identical cells here.
    let theme = Theme::dark().resolve(Depth::None);
    let glyphs: Vec<String> = STATES
        .iter()
        .map(|(mark, _)| {
            let backend = drawn(WIDE, &one_file(marked(*mark), false), &theme);
            mark_at(&backend, FIRST)
                .unwrap_or_else(|| panic!("{mark:?} drew no mark without colour"))
                .1
        })
        .collect();
    let distinct: std::collections::BTreeSet<_> = glyphs.iter().collect();
    assert_eq!(
        distinct.len(),
        STATES.len(),
        "the three states collapsed to {glyphs:?} once the colour was gone"
    );
}

#[test]
fn a_row_with_no_note_keeps_the_marks_column_rather_than_closing_it() {
    // Swept, and the narrow widths are the ones that can fail: a slot that closes
    // when it is empty hands its two columns back to the path, and a pane wide
    // enough to spell the path whole has nothing to spend them on. At 140 columns
    // a closing slot is invisible; at 40 it lengthens the path by two characters.
    let theme = Theme::dark().resolve(Depth::Truecolor);
    let mut elided = 0;
    for width in [WIDE, 60, 47, 40, 32, 30] {
        let with = drawn(width, &one_file(marked(NoteMark::Waiting), false), &theme);
        let without = drawn(width, &one_file(FileNotes::default(), false), &theme);
        let (column, _, _) =
            mark_at(&with, FIRST).unwrap_or_else(|| panic!("{width} columns drew no mark"));

        // Absolute over the cell: the slot is blank, not merely different.
        assert_eq!(
            without.buffer()[(column, FIRST)].symbol(),
            " ",
            "at {width} columns the unmarked row put something else in the mark's column"
        );

        // And nothing else on the row moved, which is what a reserved slot buys
        // and what right-packing would have taken away.
        let (a, b) = (row(&with, FIRST), row(&without, FIRST));
        let moved: Vec<usize> = a
            .chars()
            .zip(b.chars())
            .enumerate()
            .filter(|(_, (x, y))| x != y)
            .map(|(at, _)| at)
            .collect();
        assert_eq!(
            moved,
            vec![usize::from(column)],
            "at {width} columns a note changed the row outside its own column:\n{a}\n{b}"
        );
        if a.contains('\u{2026}') {
            elided += 1;
        }
    }
    // Non-vacuity, and it is the whole gate: on a pane whose path is spelled
    // whole, two columns handed back change nothing a comparison can see.
    assert!(
        elided >= 3,
        "only {elided} of the swept widths elided the path, so the sweep never \
         reaches a width where a closing slot would show"
    );
}
#[test]
fn the_mark_arrives_before_the_pulse_and_outlives_it() {
    // The drop order §11.1 records, read off the drawn rows rather than off the
    // layout table, so a table edited without the renderer following fails here.
    let theme = Theme::dark().resolve(Depth::Truecolor);
    let view = one_file(marked(NoteMark::Waiting), true);
    let pulse = theme.pulse.fg;

    let first = |wanted: &dyn Fn(&TestBackend) -> bool| {
        (20u16..=60)
            .find(|width| wanted(&drawn(*width, &view, &theme)))
            .expect("the sweep never drew it")
    };
    let mark_from = first(&|backend| mark_at(backend, FIRST).is_some());
    let pulse_from = first(&|backend| {
        let buffer = backend.buffer();
        (0..buffer.area.width)
            .map(|x| &buffer[(x, FIRST)])
            .any(|cell| cell.symbol() == "●" && cell.style().fg == pulse)
    });

    assert!(
        mark_from < pulse_from,
        "the mark arrived at {mark_from} columns and the pulse at {pulse_from}, \
         so the pulse is not the element the mark outlives"
    );
    // Both ends pinned, or the claim holds for a mark that never leaves at all.
    assert_eq!(
        (mark_from, pulse_from),
        (30, 32),
        "the ladder's two narrowest mark rungs moved"
    );
}

#[test]
fn a_slice_holding_a_note_takes_the_notes_ink_and_keeps_its_glyph() {
    let theme = Theme::dark().resolve(Depth::Truecolor);
    let mut notes = FileNotes::default();
    notes.at[5] = true;
    let backend = drawn(WIDE, &one_file(notes, false), &theme);
    let inks = strip(&backend, FIRST);

    // The tinted cell is still a slice: a strip that lost a column to the note
    // would read as a narrower rung, and the ladder gate cannot see that.
    assert_eq!(
        inks.len(),
        HEAT_BUCKETS,
        "the strip drew {} slices with a note in it",
        inks.len()
    );
    let noted = noted_slices(&inks, &theme);
    assert_eq!(
        noted,
        vec![5],
        "the note's ink landed on {noted:?} rather than on its own slice"
    );
    // And that slice would have been mixed, so the ink replaced a value rather
    // than filling a blank one.
    assert_ne!(
        theme.heat_note.fg, theme.heat_mixed_hot.fg,
        "the note's ink is the ink it is meant to be distinguishable from"
    );
}

#[test]
fn two_notes_ink_their_own_slices_and_nothing_between_them() {
    // A single-note fixture cannot tell per-slice from per-file: with one note
    // the two rules draw the same row.
    let theme = Theme::dark().resolve(Depth::Truecolor);
    let mut notes = FileNotes::default();
    notes.at[1] = true;
    notes.at[11] = true;
    let inks = strip(&drawn(WIDE, &one_file(notes, false), &theme), FIRST);
    let noted = noted_slices(&inks, &theme);
    assert_eq!(noted, vec![1, 11]);
}

#[test]
fn a_notes_slice_folds_into_the_rung_the_strip_degrades_to() {
    // Twenty-four source buckets projected onto twelve: a note in bucket 3 is in
    // slice 1, and the fold is `any` rather than the counts' `sum`.
    let theme = Theme::dark().resolve(Depth::Truecolor);
    let mut notes = FileNotes::default();
    notes.at[3] = true;
    let inks = strip(&drawn(109, &one_file(notes, false), &theme), FIRST);
    assert_eq!(
        inks.len(),
        HEAT_BUCKETS / 2,
        "109 columns is meant to be the twelve-slice rung"
    );
    let noted = noted_slices(&inks, &theme);
    assert_eq!(noted, vec![1]);
}

#[test]
fn a_file_with_a_note_and_no_line_diff_draws_the_mark_and_no_strip() {
    // The case that is the argument for having both surfaces: `has_heat` gates
    // the strip off entirely, and the row is the only thing left to say so.
    let theme = Theme::dark().resolve(Depth::Truecolor);
    let mut view = one_file(marked(NoteMark::Waiting), false);
    if let Row::File(entry) = &mut view.rows[0] {
        entry.heat = [HeatBucket::default(); HEAT_BUCKETS];
        entry.notes.at[5] = true;
    }
    let backend = drawn(WIDE, &view, &theme);
    assert!(strip(&backend, FIRST).is_empty(), "a strip was drawn");
    assert!(
        mark_at(&backend, FIRST).is_some(),
        "the row lost its mark with its strip"
    );
}
