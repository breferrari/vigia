//! `SPEC.md` §11.2 B21, the mark-only half: what the pane draws for a note, the
//! press that writes and withdraws one, and where each state of the world puts
//! the rows.

#[path = "../../vigia-core/tests/support/mod.rs"]
mod support;

use std::fs;
use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Cell;
use ratatui::crossterm::event::{Event, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier};
use vigia::{
    Action, App, Glyphs, Hovered, Pointing, Region, Regions, Row, Theme, Toggled, View, Viewport,
    body_layout, count_cell, hover_after, press_at, regions, render, repainted, selection_after,
    toggle,
};
use vigia_core::{ChangeKind, Frame, Highlighter, History, Note, Side, Status, Store, key};

use support::{Scratch, TempDir};

const PANE: Rect = Rect::new(0, 0, 80, 24);
const NARROW: Rect = Rect::new(0, 0, 40, 24);
const PATH: &str = "src/watch.rs";

/// The mockup's own line, edited into the fixture as its fifth.
const EDITED: &str = "    margin.checked_mul(2).unwrap_or(margin)";

/// The mockup's own note, long enough to wrap at eighty columns.
const BODY: &str =
    "checked_mul on a Duration cannot overflow here; use saturating_mul and drop the unwrap_or.";

fn numbered(lines: usize) -> String {
    (1..=lines).map(|n| format!("line {n}\n")).collect()
}

/// One committed file whose fifth line changed, which is the mockup's shape.
fn fixture(name: &str) -> Scratch {
    let scratch = Scratch::new(name);
    scratch.write(PATH, numbered(12));
    scratch.commit_all("baseline");
    scratch.edit_line(PATH, 4, EDITED);
    scratch
}

/// A note as the store holds it, on `line` of [`PATH`]'s working-tree side.
fn note(id: &str, line: u32, text: &str, body: &str) -> Note {
    Note {
        id: id.to_owned(),
        path: PATH.to_owned(),
        side: Side::New,
        line,
        text: text.to_owned(),
        body: body.to_owned(),
        status: Status::Open,
        reply: None,
        written: UNIX_EPOCH + Duration::from_secs(1_800_000_000),
    }
}

fn files_in(dir: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .map(|entry| {
            entry
                .expect("a directory entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

fn at(kind: MouseEventKind, column: u16, row: u16) -> Event {
    Event::Mouse(MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    })
}

fn press(column: u16, row: u16) -> Event {
    at(MouseEventKind::Down(MouseButton::Left), column, row)
}

fn release(column: u16, row: u16) -> Event {
    at(MouseEventKind::Up(MouseButton::Left), column, row)
}

fn moved(column: u16, row: u16) -> Event {
    at(MouseEventKind::Moved, column, row)
}

/// The pane's state over a scratch worktree, with a store of its own under a
/// fresh state root. The frame stays outside, since it borrows the worktree.
struct Rig {
    app: App,
    highlighter: Highlighter,
    history: History,
    theme: Theme,
    store: Store,
    _root: TempDir,
}

/// One painted frame, and what the pointer would be told about it.
struct Painted {
    backend: TestBackend,
    view: View,
    laid: Regions,
}

impl Rig {
    fn open(scratch: &Scratch) -> Self {
        let root = TempDir::new("notes-state");
        let store = Store::open(root.path(), scratch.root()).expect("open the store");
        Self {
            app: App::past_first_paint(),
            highlighter: Highlighter::eager(),
            history: History::new(),
            theme: Theme::default(),
            store,
            _root: root,
        }
    }

    /// What the shell does after its own write: read the store back.
    fn reload(&mut self) {
        let listing = self.store.list().expect("list the store");
        assert!(listing.skipped.is_empty(), "{:?}", listing.skipped);
        self.app.set_notes(listing.notes);
    }

    /// The shell's frame: chrome, layout, collect, paint, and the regions the
    /// pointer is told about.
    fn paint(&mut self, frame: &mut Frame, pane: Rect, pointing: Pointing) -> Painted {
        let files = frame.files().len();
        let chrome = self.app.chrome("fixture", None, pointing, 0, "");
        let body = body_layout(pane, &chrome, files, files);
        let view = self
            .app
            .view(frame, &mut self.highlighter, &self.history, body)
            .expect("collect a view");
        // Rebuilt after the collect, as the shell rebuilds it, so the count this
        // frame placed reaches this frame's footer.
        let chrome = self.app.chrome("fixture", None, pointing, 0, "");
        let mut terminal =
            Terminal::new(TestBackend::new(pane.width, pane.height)).expect("terminal");
        let theme = &self.theme;
        terminal
            .draw(|f| {
                let area = f.area();
                render(
                    f.buffer_mut(),
                    area,
                    &view,
                    theme,
                    Glyphs::default(),
                    &chrome,
                );
            })
            .expect("draw");
        let laid = regions(pane, &chrome, &view);
        Painted {
            backend: terminal.backend().clone(),
            view,
            laid,
        }
    }

    /// The loop's own routing of a press: a note press goes to the store, and
    /// anything else is left to the selection. Answers what the store did.
    fn click(&mut self, painted: &Painted, column: u16, row: u16) -> Option<Toggled> {
        let offset = press_at(&painted.view, painted.laid, &press(column, row))?;
        let done = toggle(&self.store, &painted.view, offset)
            .expect("a note press resolved to no anchor")
            .expect("the store refused the write");
        self.reload();
        Some(done)
    }
}

impl Painted {
    fn cell(&self, x: u16, y: u16) -> &Cell {
        &self.backend.buffer()[(x, y)]
    }

    fn fg(&self, x: u16, y: u16) -> Option<Color> {
        self.cell(x, y).style().fg
    }

    /// Row `y` as text, one char per cell.
    fn text(&self, y: u16) -> String {
        let width = self.backend.buffer().area.width;
        (0..width)
            .map(|x| self.cell(x, y).symbol().chars().next().unwrap_or(' '))
            .collect()
    }

    fn rows(&self) -> Vec<String> {
        (0..self.backend.buffer().area.height)
            .map(|y| self.text(y))
            .collect()
    }

    /// The first row of the diff region whose text holds `needle`.
    fn row_of(&self, needle: &str) -> u16 {
        let diff = self.laid.diff;
        (diff.top..diff.top + diff.rows)
            .find(|y| self.text(*y).contains(needle))
            .unwrap_or_else(|| panic!("no diff row holds {needle:?}:\n{}", self.rows().join("\n")))
    }

    /// The gutter's first column and its width, and the content origin after it.
    fn gutter(&self) -> (u16, u16, u16) {
        let (left, columns) = self.laid.diff.gutter;
        assert!(columns > 0, "the diff region published no gutter");
        (left, columns, left + columns)
    }

    /// The consecutive note rows drawn under row `y`, as their text past the lead.
    fn notes_under(&self, y: u16) -> Vec<String> {
        let (_, _, origin) = self.gutter();
        let mut out = Vec::new();
        let mut row = y + 1;
        while row < self.laid.diff.top + self.laid.diff.rows {
            let text = self.text(row);
            let lead = text.chars().nth(usize::from(origin));
            if !matches!(lead, Some('▎' | '↳'))
                && !matches!(
                    self.view.rows.get(usize::from(row - self.laid.diff.top)),
                    Some(Row::Note { .. })
                )
            {
                break;
            }
            out.push(
                text.chars()
                    .skip(usize::from(origin) + 2)
                    .collect::<String>(),
            );
            row += 1;
        }
        out
    }

    /// The footer's bottom row.
    fn footer(&self) -> String {
        self.text(self.backend.buffer().area.height - 1)
    }
}

/// The words of `rows` joined the way the body was written, for a wrap gate.
fn rejoined(rows: &[String], word: &str) -> String {
    rows.iter()
        .map(|row| row.trim_end().trim_end_matches(word).trim_end().to_owned())
        .filter(|row| !row.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn the_gutter_of_a_content_row_answers_a_hover_and_content_does_not() {
    let scratch = fixture("notes-hover-target");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    let painted = rig.paint(&mut frame, PANE, Pointing::default());
    let y = painted.row_of(EDITED);
    let (left, columns, origin) = painted.gutter();

    // Every cell of the gutter answers, and the first cell of content does not.
    for x in left..origin {
        assert_eq!(
            painted.laid.hover_at(x, y),
            Some(Hovered::Gutter(y)),
            "column {x} of the gutter did not answer"
        );
    }
    assert_eq!(
        painted.laid.hover_at(origin, y),
        None,
        "the first content column answered as the gutter"
    );
    // The gutter the pointer is told about is where the number is drawn.
    let digit = painted
        .text(y)
        .chars()
        .position(|c| c == '5')
        .expect("the line number") as u16;
    assert!(
        digit >= left && digit < origin,
        "the number sits at column {digit}, outside the gutter {left}..{origin}"
    );
    assert_eq!(
        usize::from(columns),
        1 + 1 + 2,
        "the one digit this screen's numbers need, a blank, the sigil and its gap"
    );

    // The list's rows keep answering as the file they draw.
    if painted.laid.list.rows > 0 {
        let row = painted.laid.list.top;
        assert_eq!(
            painted.laid.hover_at(origin + 10, row),
            Some(Hovered::Row(row))
        );
    }
}

#[test]
fn the_hover_icon_takes_the_number_cell_and_no_content_cell() {
    let scratch = fixture("notes-hover-icon");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    let plain = rig.paint(&mut frame, PANE, Pointing::default());
    let y = plain.row_of(EDITED);
    let hovering = rig.paint(
        &mut frame,
        PANE,
        Pointing {
            hovered: Some(Hovered::Gutter(y)),
            ..Pointing::default()
        },
    );
    let (left, _, origin) = plain.gutter();

    let icon = (left..origin)
        .find(|x| hovering.cell(*x, y).symbol() == "✎")
        .unwrap_or_else(|| {
            panic!(
                "no icon in the gutter under the pointer:\n{}",
                hovering.text(y)
            )
        });
    assert_eq!(
        plain.cell(icon, y).symbol(),
        "5",
        "the icon did not take the number's own cell"
    );
    assert_eq!(
        hovering.fg(icon, y),
        Theme::default().bar_hover.fg,
        "the icon is not in the pointer's ink"
    );
    // Every content cell of the row, and every other row, is the plain frame.
    for x in origin..PANE.width {
        assert_eq!(
            hovering.cell(x, y),
            plain.cell(x, y),
            "content cell {x} moved under the mark"
        );
    }
    for other in (0..PANE.height).filter(|row| *row != y) {
        assert_eq!(
            hovering.text(other),
            plain.text(other),
            "row {other} changed under a mark on row {y}"
        );
    }
}

#[test]
fn the_hover_icon_clears_by_b10s_ladder() {
    let scratch = fixture("notes-hover-clears");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    let painted = rig.paint(&mut frame, PANE, Pointing::default());
    let y = painted.row_of(EDITED);
    let (left, _, origin) = painted.gutter();
    let mark = Some(Hovered::Gutter(y));

    // Motion inside the pane re-resolves: the content beside it is no target.
    assert_eq!(hover_after(&moved(origin + 4, y), painted.laid, mark), None);
    // And motion onto the gutter of the row below marks that row.
    assert_eq!(
        hover_after(&moved(left, y + 1), painted.laid, mark),
        Some(Hovered::Gutter(y + 1))
    );
    // Leaving the window retires it.
    assert_eq!(hover_after(&Event::FocusLost, painted.laid, mark), None);
    // A repaint that moved the regions retires it, and one that did not keeps it.
    let shifted = Regions {
        diff: Region {
            top: painted.laid.diff.top + 1,
            ..painted.laid.diff
        },
        ..painted.laid
    };
    assert_eq!(repainted(mark, painted.laid, shifted), None);
    assert_eq!(repainted(mark, painted.laid, painted.laid), mark);
}

#[test]
fn a_hunk_header_and_a_heading_draw_no_icon_under_the_pointer() {
    let scratch = fixture("notes-no-target");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    let plain = rig.paint(&mut frame, PANE, Pointing::default());
    let heading = plain.row_of("watch.rs");
    let header = plain.row_of("@@");
    let (left, _, _) = plain.gutter();

    for target in [heading, header] {
        // The geometry answers, because it cannot tell one row from another.
        assert_eq!(
            plain.laid.hover_at(left, target),
            Some(Hovered::Gutter(target))
        );
        // And the painter draws nothing for it.
        let hovering = rig.paint(
            &mut frame,
            PANE,
            Pointing {
                hovered: Some(Hovered::Gutter(target)),
                ..Pointing::default()
            },
        );
        assert_eq!(
            hovering.backend.buffer(),
            plain.backend.buffer(),
            "row {target} is no target and drew a mark:\n{}",
            hovering.text(target)
        );
        // Nor is it a note press.
        assert_eq!(
            press_at(&plain.view, plain.laid, &press(left, target)),
            None
        );
    }
}

#[test]
fn a_press_on_the_gutter_writes_one_file_and_a_press_on_content_still_selects() {
    let scratch = fixture("notes-press");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    let painted = rig.paint(&mut frame, PANE, Pointing::default());
    let y = painted.row_of(EDITED);
    let (left, _, origin) = painted.gutter();
    assert!(
        !rig.store.dir().exists(),
        "the store had a directory before any gesture"
    );

    // The press, routed the way the loop routes it.
    let on_gutter = press(left + 1, y);
    let offset = press_at(&painted.view, painted.laid, &on_gutter)
        .expect("a press on a content row's gutter is not a note press");
    assert_eq!(offset, usize::from(y - painted.laid.diff.top));
    let written = match toggle(&rig.store, &painted.view, offset) {
        Some(Ok(Toggled::Written(id))) => id,
        other => panic!("the press did not write a note: {other:?}"),
    };

    // One file, holding the anchor alone.
    let files = files_in(rig.store.dir());
    assert_eq!(files, vec![format!("{written}.note")]);
    let listing = rig.store.list().expect("list");
    let note = &listing.notes[0];
    assert_eq!(
        (
            note.path.as_str(),
            note.side,
            note.line,
            note.text.as_str(),
            note.body.as_str(),
            note.status
        ),
        (PATH, Side::New, 5, EDITED, "", Status::Open)
    );

    // The same row's content is no note press and begins a selection, as B20 rules.
    let on_content = press(origin + 3, y);
    assert_eq!(press_at(&painted.view, painted.laid, &on_content), None);
    assert!(
        selection_after(&on_content, painted.laid, None).0.is_some(),
        "a press on content stopped beginning a selection"
    );
    // And neither a release on the gutter nor a press elsewhere writes anything.
    assert_eq!(
        press_at(&painted.view, painted.laid, &release(left + 1, y)),
        None
    );
    assert_eq!(files_in(rig.store.dir()).len(), 1);
}

#[test]
fn a_second_press_on_a_noted_line_withdraws_it() {
    let scratch = fixture("notes-withdraw");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    let plain = rig.paint(&mut frame, PANE, Pointing::default());
    let y = plain.row_of(EDITED);
    let (left, _, origin) = plain.gutter();

    assert!(matches!(
        rig.click(&plain, left + 1, y),
        Some(Toggled::Written(_))
    ));
    let marked = rig.paint(&mut frame, PANE, Pointing::default());
    // The anchor alone draws the icon durably, in the pointer's ink, and one row
    // under the line carrying the word the agent will climb from.
    let icon = (left..origin)
        .find(|x| marked.cell(*x, y).symbol() == "✎")
        .unwrap_or_else(|| {
            panic!(
                "a line carrying a bare note draws no icon:\n{}",
                marked.text(y)
            )
        });
    assert_eq!(marked.fg(icon, y), Theme::default().bar_hover.fg);
    let under = marked.notes_under(y);
    assert_eq!(under.len(), 1, "a bare note drew {} rows", under.len());
    assert!(under[0].trim_end().ends_with("open"), "{:?}", under[0]);
    assert_eq!(
        marked
            .view
            .marked_at(usize::from(y - marked.laid.diff.top))
            .len(),
        1
    );

    // The second press withdraws it rather than adding a second: one open note per line.
    assert_eq!(rig.click(&marked, left + 1, y), Some(Toggled::Withdrawn(1)));
    assert!(
        files_in(rig.store.dir()).is_empty(),
        "the file was not removed"
    );
    let clear = rig.paint(&mut frame, PANE, Pointing::default());
    assert_eq!(
        clear.rows(),
        plain.rows(),
        "the withdrawn note left something drawn"
    );
}

#[test]
fn a_note_draws_under_its_line_with_a_bar_the_body_and_the_word() {
    let scratch = fixture("notes-rows");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    rig.store.put(&note("n1", 5, EDITED, BODY)).expect("put");
    rig.reload();
    let painted = rig.paint(&mut frame, PANE, Pointing::default());
    let y = painted.row_of(EDITED);
    let (left, _, origin) = painted.gutter();
    let theme = Theme::default();

    // The number stays a number and keeps the icon's ink, so the anchored line
    // can be found from across the pane.
    let five = (left..origin)
        .find(|x| painted.cell(*x, y).symbol() == "5")
        .unwrap_or_else(|| panic!("the number was replaced:\n{}", painted.text(y)));
    assert_eq!(painted.fg(five, y), theme.bar_hover.fg);

    // Two rows under it: the bar at the content origin with a blank gutter behind
    // it, the body in the chrome's dim ink, and the word on the last row.
    let under = painted.notes_under(y);
    assert_eq!(under.len(), 2, "{under:?}");
    for row in y + 1..=y + 2 {
        let text = painted.text(row);
        assert!(
            text.chars().take(usize::from(origin)).all(|c| c == ' '),
            "the gutter under a note row is not blank: {text:?}"
        );
        assert_eq!(text.chars().nth(usize::from(origin)), Some('▎'));
        assert_eq!(
            painted.fg(origin, row),
            theme.bar_hover.fg,
            "the bar is not in the note's ink"
        );
        assert_eq!(
            painted.fg(origin + 2, row),
            theme.chrome_dim.fg,
            "the body is not dim"
        );
    }
    assert!(under[1].trim_end().ends_with("open"), "{:?}", under[1]);
    // Prose wraps at a blank, so the first row ends on a whole word, and nothing
    // of it is lost.
    let first = under[0].trim_end();
    assert!(
        !first.is_empty() && BODY.starts_with(first) && BODY.as_bytes()[first.len()] == b' ',
        "the first row broke inside a word: {first:?}"
    );
    assert_eq!(rejoined(&under, "open"), BODY);
    // The word stands apart from the body at the row's right edge: further right
    // than any content on the noted line, with a gap before it.
    let last_row = painted.text(y + 2);
    let word_end = last_row.trim_end().chars().count();
    assert!(
        word_end > painted.text(y).trim_end().chars().count(),
        "the word is not against the right edge: {last_row:?}"
    );
    assert!(
        last_row.trim_end().trim_end_matches("open").ends_with("  "),
        "the word runs into the body: {last_row:?}"
    );
    assert!(
        painted.text(y + 3).contains("line 6"),
        "{}",
        painted.text(y + 3)
    );
    assert_eq!(painted.view.notes.placed, 1);
    assert!(painted.footer().contains("1 note"), "{}", painted.footer());
}

#[test]
fn a_note_wraps_at_the_content_width_at_forty_columns() {
    let scratch = fixture("notes-forty");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    rig.store.put(&note("n1", 5, EDITED, BODY)).expect("put");
    rig.reload();
    let wide = rig.paint(&mut frame, PANE, Pointing::default());
    let wide_rows = wide.notes_under(wide.row_of(EDITED));
    let narrow = rig.paint(&mut frame, NARROW, Pointing::default());
    let y = narrow.row_of("checked_mul");
    let under = narrow.notes_under(y);

    assert!(
        under.len() > wide_rows.len(),
        "forty columns wrapped the body into {} rows, no more than eighty's {}",
        under.len(),
        wide_rows.len()
    );
    for row in y + 1..=y + under.len() as u16 {
        assert!(
            narrow.text(row).trim_end().chars().count() <= usize::from(NARROW.width),
            "a note row over-occupies the pane: {:?}",
            narrow.text(row)
        );
    }
    assert_eq!(rejoined(&under, "open"), BODY);
    assert!(under.last().expect("rows").trim_end().ends_with("open"));
}

#[test]
fn a_moved_line_keeps_its_note_and_the_store_is_not_rewritten() {
    let scratch = fixture("notes-moved");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    rig.store.put(&note("n1", 5, EDITED, "short")).expect("put");
    rig.reload();
    let file = rig.store.dir().join("n1.note");
    let bytes = fs::read(&file).expect("the note file");

    // A line inserted above pushes the noted line down to 6.
    let mut lines: Vec<String> = numbered(12).lines().map(str::to_owned).collect();
    lines[4] = EDITED.to_owned();
    lines.insert(1, "inserted".to_owned());
    scratch.write(PATH, lines.join("\n") + "\n");
    frame.advance().expect("advance after the insert");
    let painted = rig.paint(&mut frame, PANE, Pointing::default());
    let y = painted.row_of(EDITED);

    assert!(
        painted.text(y).trim_start().starts_with('6'),
        "the noted line is not numbered 6 after the insert: {:?}",
        painted.text(y)
    );
    let under = painted.notes_under(y);
    assert_eq!(under.len(), 1, "{under:?}");
    assert!(
        under[0].starts_with("short") && under[0].trim_end().ends_with("open"),
        "{:?}",
        under[0]
    );
    assert_eq!(painted.view.notes.marked.len(), 1);
    assert_eq!(
        fs::read(&file).expect("the note file"),
        bytes,
        "the pane rewrote the store on a move, and the pane writes only on a gesture"
    );
}

#[test]
fn an_edited_line_draws_its_note_dimmer_with_the_word_changed() {
    let scratch = fixture("notes-changed");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    rig.store.put(&note("n1", 5, EDITED, "short")).expect("put");
    rig.reload();

    // The agent did what the note asked.
    scratch.edit_line(PATH, 4, "    margin.saturating_mul(2)");
    frame.advance().expect("advance after the edit");
    let painted = rig.paint(&mut frame, PANE, Pointing::default());
    let y = painted.row_of("saturating_mul(2)");
    let (_, _, origin) = painted.gutter();

    let under = painted.notes_under(y);
    assert_eq!(under.len(), 1, "{under:?}");
    assert!(under[0].trim_end().ends_with("changed"), "{:?}", under[0]);
    let row = y + 1;
    for x in [
        origin,
        origin + 2,
        painted.text(row).trim_end().chars().count() as u16 - 1,
    ] {
        assert!(
            painted
                .cell(x, row)
                .style()
                .add_modifier
                .contains(Modifier::DIM),
            "cell {x} of a changed note's row is not dim"
        );
    }
    // The mark stays on the line, so a click there still withdraws it.
    assert_eq!(
        painted
            .view
            .marked_at(usize::from(y - painted.laid.diff.top)),
        vec!["n1"]
    );
}

#[test]
fn a_line_gone_from_the_diff_draws_its_note_under_the_heading() {
    let scratch = fixture("notes-gone");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    rig.store.put(&note("n1", 5, EDITED, "short")).expect("put");
    rig.reload();

    // Line five is put back and line ten changes instead, so the file stays in
    // the diff and the noted line is in no hunk of it.
    scratch.edit_line(PATH, 4, "line 5");
    scratch.edit_line(PATH, 9, "changed ten");
    frame.advance().expect("advance after the edits");
    let painted = rig.paint(&mut frame, PANE, Pointing::default());
    let heading = painted.row_of("watch.rs");

    let under = painted.notes_under(heading);
    assert_eq!(under.len(), 1, "{under:?}");
    assert!(under[0].trim_end().ends_with("gone"), "{:?}", under[0]);
    assert!(
        painted.text(heading + 2).contains("@@"),
        "the hunk header did not follow the note"
    );
    assert!(
        painted.view.notes.marked.is_empty(),
        "a gone note marked a line"
    );
    assert_eq!(
        (painted.view.notes.placed, painted.view.notes.adrift),
        (1, 0)
    );
}

#[test]
fn a_file_out_of_the_diff_leaves_its_note_adrift_and_the_footer_counts_it() {
    let scratch = Scratch::new("notes-adrift");
    scratch.write(PATH, numbered(12));
    scratch.write("src/other.rs", numbered(12));
    scratch.commit_all("baseline");
    scratch.edit_line(PATH, 4, EDITED);
    scratch.edit_line("src/other.rs", 2, "other changed");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    rig.store.put(&note("n1", 5, EDITED, "one")).expect("put");
    let mut other = note("n2", 3, "other changed", "two");
    other.path = "src/other.rs".to_owned();
    rig.store.put(&other).expect("put");
    rig.reload();

    let both = rig.paint(&mut frame, PANE, Pointing::default());
    assert_eq!((both.view.notes.placed, both.view.notes.adrift), (2, 0));
    assert!(both.footer().contains("2 notes"), "{}", both.footer());
    assert!(!both.footer().contains("adrift"), "{}", both.footer());

    // The other file leaves the diff: reverted, which is one of the four ways.
    scratch.git(&["checkout", "--", "src/other.rs"]);
    frame.advance().expect("advance after the revert");
    let adrift = rig.paint(&mut frame, PANE, Pointing::default());
    assert_eq!((adrift.view.notes.placed, adrift.view.notes.adrift), (1, 1));
    assert!(
        adrift.footer().contains("2 notes · 1 adrift"),
        "{}",
        adrift.footer()
    );
    assert!(
        !adrift.rows().iter().any(|row| row.contains("two")),
        "an adrift note was drawn somewhere"
    );

    // And back under its line the moment the file re-enters the diff.
    scratch.edit_line("src/other.rs", 2, "other changed");
    frame.advance().expect("advance after the edit");
    let back = rig.paint(&mut frame, PANE, Pointing::default());
    assert_eq!((back.view.notes.placed, back.view.notes.adrift), (2, 0));
    let y = back.row_of("other changed");
    assert!(back.notes_under(y)[0].starts_with("two"));

    assert_eq!(count_cell(0, 0), "");
    assert_eq!(count_cell(1, 0), "1 note");
    assert_eq!(count_cell(2, 1), "2 notes · 1 adrift");
}

#[test]
fn a_renamed_file_carries_its_note_to_the_new_path() {
    let scratch = Scratch::new("notes-renamed");
    scratch.write("old/name.rs", numbered(30));
    scratch.commit_all("baseline");
    scratch.git(&["mv", "old/name.rs", "new-name.rs"]);
    // `git mv` stages the move; the pane watches the worktree, so it is unstaged
    // and edited there, which keeps the rename detectable and gives it a hunk.
    scratch.git(&["reset", "-q"]);
    scratch.edit_line("new-name.rs", 4, EDITED);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    assert!(
        matches!(frame.files()[0].kind, ChangeKind::Renamed { .. }),
        "the fixture is not a rename: {:?}",
        frame.files()[0].kind
    );
    let mut rig = Rig::open(&scratch);
    let mut pinned = note("n1", 5, EDITED, "follow me");
    pinned.path = "old/name.rs".to_owned();
    rig.store.put(&pinned).expect("put");
    rig.reload();

    let painted = rig.paint(&mut frame, PANE, Pointing::default());
    assert_eq!(
        (painted.view.notes.placed, painted.view.notes.adrift),
        (1, 0)
    );
    let y = painted.row_of(EDITED);
    let under = painted.notes_under(y);
    assert_eq!(under.len(), 1, "{under:?}");
    assert!(under[0].starts_with("follow me"), "{:?}", under[0]);
    // A press there writes under the path the diff lists the file by now.
    let (left, _, _) = painted.gutter();
    let anchor = painted
        .view
        .anchor_at(usize::from(y - painted.laid.diff.top))
        .expect("an anchor");
    assert_eq!(anchor.path, "new-name.rs");
    assert_eq!(
        press_at(&painted.view, painted.laid, &press(left, y)),
        Some(usize::from(y - painted.laid.diff.top))
    );
}

#[test]
fn a_deleted_file_draws_its_note_under_the_heading() {
    let scratch = fixture("notes-deleted");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    rig.store.put(&note("n1", 5, EDITED, "short")).expect("put");
    rig.reload();

    scratch.remove(PATH);
    frame.advance().expect("advance after the delete");
    let painted = rig.paint(&mut frame, PANE, Pointing::default());
    let heading = painted.row_of("watch.rs");
    assert!(
        painted.text(heading).contains('D'),
        "{}",
        painted.text(heading)
    );
    let under = painted.notes_under(heading);
    assert_eq!(under.len(), 1, "{under:?}");
    assert!(under[0].trim_end().ends_with("gone"), "{:?}", under[0]);
}

#[test]
fn c_hides_the_rows_and_keeps_the_mark() {
    let scratch = fixture("notes-hidden");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    rig.store.put(&note("n1", 5, EDITED, BODY)).expect("put");
    rig.reload();
    let shown = rig.paint(&mut frame, PANE, Pointing::default());
    let y = shown.row_of(EDITED);
    assert_eq!(shown.notes_under(y).len(), 2);
    let (left, _, origin) = shown.gutter();

    rig.app
        .apply(Action::ToggleNotes, &mut frame, 0)
        .expect("toggle the rows");
    let hidden = rig.paint(&mut frame, PANE, Pointing::default());
    assert!(
        hidden.notes_under(y).is_empty(),
        "{:?}",
        hidden.notes_under(y)
    );
    assert!(
        !hidden
            .view
            .rows
            .iter()
            .any(|row| matches!(row, Row::Note { .. })),
        "a note row survived the toggle"
    );
    // The mark stays: the number in the icon's ink, and the line still marked
    // for a click to withdraw.
    let five = (left..origin)
        .find(|x| hidden.cell(*x, y).symbol() == "5")
        .expect("the number");
    assert_eq!(hidden.fg(five, y), Theme::default().bar_hover.fg);
    assert_eq!(
        hidden.view.marked_at(usize::from(y - hidden.laid.diff.top)),
        vec!["n1"]
    );
    assert!(
        hidden.text(y + 1).contains("line 6"),
        "{}",
        hidden.text(y + 1)
    );
    // And the footer still counts it.
    assert!(hidden.footer().contains("1 note"), "{}", hidden.footer());

    rig.app
        .apply(Action::ToggleNotes, &mut frame, 0)
        .expect("toggle the rows back");
    let again = rig.paint(&mut frame, PANE, Pointing::default());
    assert_eq!(again.rows(), shown.rows());
}

#[test]
fn note_rows_are_display_rows_the_bar_does_not_count() {
    // Many hunks, so the diff is taller than the pane and the bar draws.
    let scratch = Scratch::new("notes-thumb");
    scratch.write(PATH, numbered(200));
    scratch.commit_all("baseline");
    for line in (4..200).step_by(10) {
        scratch.edit_line(PATH, line, &format!("changed {}", line + 1));
    }
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    let bare = rig.paint(&mut frame, PANE, Pointing::default());
    assert!(
        bare.laid.diff.bar.is_some(),
        "the fixture draws no bar to measure"
    );
    rig.store
        .put(&note("n1", 5, "changed 5", BODY))
        .expect("put");
    rig.reload();
    let noted = rig.paint(&mut frame, PANE, Pointing::default());

    let note_rows = noted
        .view
        .rows
        .iter()
        .filter(|row| matches!(row, Row::Note { .. }))
        .count();
    assert!(note_rows >= 2, "the note's rows are not on screen");
    // The bar counts the diff's rows and not the terminal's: the total and the
    // rows above are the same, and the screenful is the logical rows on screen,
    // which the note rows are not among.
    assert_eq!(noted.view.total_rows, bare.view.total_rows);
    assert_eq!(noted.view.rows_above, bare.view.rows_above);
    assert_eq!(noted.view.shown(), noted.view.rows.len() - note_rows);
    assert_eq!(noted.view.shown() + note_rows, bare.view.shown());
    assert_eq!(
        noted.laid.diff, bare.laid.diff,
        "the region the pointer is told about moved"
    );
}

#[test]
fn a_reply_draws_under_the_note_with_the_arrow() {
    let scratch = fixture("notes-reply");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    let mut seen = note("n1", 5, EDITED, "short");
    seen.status = Status::Seen;
    seen.reply = Some("swapped for saturating_mul; the unwrap_or went with it".to_owned());
    rig.store.put(&seen).expect("put");
    rig.reload();
    let painted = rig.paint(&mut frame, PANE, Pointing::default());
    let y = painted.row_of(EDITED);
    let (_, _, origin) = painted.gutter();

    let under = painted.notes_under(y);
    assert_eq!(under.len(), 2, "{under:?}");
    assert!(under[0].trim_end().ends_with("seen"), "{:?}", under[0]);
    assert_eq!(
        painted.text(y + 2).chars().nth(usize::from(origin)),
        Some('↳')
    );
    assert!(
        under[1].starts_with("swapped for saturating_mul"),
        "{:?}",
        under[1]
    );
    assert!(painted.text(y + 3).contains("line 6"));

    // Resolved, the reply alone stays, which is the last frame of the departure
    // the store watch will animate.
    let mut resolved = seen.clone();
    resolved.status = Status::Resolved;
    rig.store.put(&resolved).expect("put");
    rig.reload();
    let departing = rig.paint(&mut frame, PANE, Pointing::default());
    let under = departing.notes_under(departing.row_of(EDITED));
    assert_eq!(under.len(), 1, "{under:?}");
    assert!(under[0].starts_with("swapped for"), "{:?}", under[0]);
}

#[test]
fn an_unwritable_store_refuses_the_note_and_the_pane_paints_on() {
    let scratch = fixture("notes-unwritable");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let root = TempDir::new("notes-unwritable-state");
    // A file where the store's directory would go, so nothing can be made there.
    fs::write(
        root.path().join(key(scratch.root()).expect("key")),
        b"in the way",
    )
    .expect("block");
    let store = Store::open(root.path(), scratch.root()).expect("open");
    let mut rig = Rig::open(&scratch);
    let painted = rig.paint(&mut frame, PANE, Pointing::default());
    let y = painted.row_of(EDITED);
    let (left, _, _) = painted.gutter();

    let offset = press_at(&painted.view, painted.laid, &press(left, y)).expect("a note press");
    let refused = toggle(&store, &painted.view, offset).expect("a content row");
    let why = refused.expect_err("the store wrote into a file");
    assert!(!why.to_string().is_empty());
    // Nothing is lost and nothing stops: the next frame is the frame before.
    let after = rig.paint(&mut frame, PANE, Pointing::default());
    assert_eq!(after.rows(), painted.rows());
}

#[test]
fn a_torn_note_file_is_skipped_and_the_rest_are_drawn() {
    let scratch = fixture("notes-torn");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    rig.store.put(&note("n1", 5, EDITED, "kept")).expect("put");
    rig.store
        .put(&note("n2", 6, "line 6", "also kept"))
        .expect("put");
    fs::write(
        rig.store.dir().join("torn-1.note"),
        "vigia note 1\nid: torn-1\nside: new\n",
    )
    .expect("tear");

    let listing = rig.store.list().expect("list");
    assert_eq!(listing.skipped.len(), 1, "{:?}", listing.skipped);
    assert_eq!(listing.notes.len(), 2);
    rig.app.set_notes(listing.notes);
    let painted = rig.paint(&mut frame, PANE, Pointing::default());
    assert!(painted.notes_under(painted.row_of(EDITED))[0].starts_with("kept"));
    assert!(painted.notes_under(painted.row_of("line 6"))[0].starts_with("also kept"));
    assert!(painted.footer().contains("2 notes"), "{}", painted.footer());
}

#[test]
fn a_pane_with_no_notes_draws_todays_frame() {
    let scratch = fixture("notes-none");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    let painted = rig.paint(&mut frame, PANE, Pointing::default());
    let files = frame.files().len();
    let chrome = rig.app.chrome("fixture", None, Pointing::default(), 0, "");
    let body = body_layout(PANE, &chrome, files, files);

    // The collect the pane ran, spelled without any notes at all.
    let direct = View::collect(
        &mut frame,
        &mut rig.highlighter,
        &rig.history,
        Viewport {
            diff_rows: body.diff,
            width: body.diff_width,
            list_rows: body.list,
            list_follows: true,
            measured: body.diff > 1,
            ..Viewport::default()
        },
    )
    .expect("collect");
    assert_eq!(painted.view, direct);
    assert!(painted.view.notes.marked.is_empty());
    assert!(
        !painted
            .rows()
            .iter()
            .any(|row| row.contains('▎') || row.contains('✎'))
    );
    assert!(!painted.footer().contains("note"), "{}", painted.footer());
}

#[test]
fn a_continuation_row_anchors_to_its_head_line() {
    let scratch = Scratch::new("notes-continuation");
    scratch.write(PATH, numbered(12));
    scratch.commit_all("baseline");
    let long = format!("    {}", "a long line that wraps ".repeat(8));
    scratch.edit_line(PATH, 4, long.trim_end());
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    rig.app
        .apply(Action::ToggleWrap, &mut frame, 0)
        .expect("wrap");
    let painted = rig.paint(&mut frame, PANE, Pointing::default());
    let y = painted.row_of("a long line that wraps");
    let (left, _, origin) = painted.gutter();
    assert!(
        matches!(
            painted.view.rows[usize::from(y + 1 - painted.laid.diff.top)],
            Row::Wrap { .. }
        ),
        "the line did not wrap:\n{}",
        painted.text(y + 1)
    );

    // The pointer on the continuation's gutter marks the head's number.
    let hovering = rig.paint(
        &mut frame,
        PANE,
        Pointing {
            hovered: Some(Hovered::Gutter(y + 1)),
            ..Pointing::default()
        },
    );
    assert!(
        (left..origin).any(|x| hovering.cell(x, y).symbol() == "✎"),
        "{}",
        hovering.text(y)
    );
    assert_eq!(
        hovering.text(y + 1),
        painted.text(y + 1),
        "the continuation row itself changed"
    );

    // And a press there pins the whole line at the head's number.
    let offset = press_at(&painted.view, painted.laid, &press(left, y + 1)).expect("a note press");
    assert!(matches!(
        toggle(&rig.store, &painted.view, offset),
        Some(Ok(Toggled::Written(_)))
    ));
    let written = &rig.store.list().expect("list").notes[0];
    assert_eq!(written.line, 5);
    assert_eq!(written.text, long.trim_end());
}

#[test]
fn the_bottom_clamp_counts_note_rows() {
    let scratch = Scratch::new("notes-bottom");
    scratch.write(PATH, numbered(60));
    scratch.commit_all("baseline");
    for line in (4..60).step_by(10) {
        scratch.edit_line(PATH, line, &format!("changed {}", line + 1));
    }
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    rig.store
        .put(&note("n1", 55, "changed 55", BODY))
        .expect("put");
    rig.reload();
    let files = frame.files().len();
    let chrome = rig.app.chrome("fixture", None, Pointing::default(), 0, "");
    let height = body_layout(PANE, &chrome, files, files).diff;
    // Past the end, so the walk's bottom clamp answers with the last screenful.
    rig.app
        .apply(Action::Scroll(1_000), &mut frame, height)
        .expect("scroll past the end");
    let painted = rig.paint(&mut frame, PANE, Pointing::default());

    let note_rows = painted
        .view
        .rows
        .iter()
        .filter(|row| matches!(row, Row::Note { .. }))
        .count();
    assert_eq!(note_rows, 2, "the note's rows are not on the last screen");
    assert_eq!(
        painted.view.rows.len(),
        height,
        "the last screenful is not full once the note rows are counted"
    );
    let last = painted.laid.diff.top + painted.laid.diff.rows - 1;
    assert!(
        painted.text(last).contains("line 58"),
        "the diff's last line is not on the last screenful: {:?}",
        painted.text(last)
    );
}
