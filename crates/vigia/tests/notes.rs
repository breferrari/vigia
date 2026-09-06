//! `SPEC.md` §11.2 B21: what the pane draws for a note, the box a press opens
//! and what Enter and Esc do with it, and where each state of the world puts
//! the rows.

#[path = "../../vigia-core/tests/support/mod.rs"]
mod support;

use std::fs;
use std::time::{Duration, Instant};

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Cell;
use ratatui::crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier};
use vigia::{
    ARRIVING_FRAME, Action, Alerts, App, BOX_ARRIVING, BOX_FRAME, BOX_ROWS, BoxPart, BoxRoute,
    Change, Committed, Glyphs, Hovered, LEAVING, Ledger, NoteCount, NoteEffects, Pointing,
    RESOLVE_ARRIVING, RESOLVE_BEAT, RESOLVED_DEPARTURE, Region, Regions, Row, Theme, Timed, View,
    Viewport, body_layout, box_cells, box_entrance, box_exit, box_route, commit, count_cell,
    has_room, hover_after, note_cells, opening, press_at, regions, render, repainted,
    selection_after,
};
use vigia_core::{ChangeKind, Frame, Highlighter, History, Side, Status, Store, key};

use support::{Scratch, TempDir, files_in, note, numbered_lines};

const PANE: Rect = Rect::new(0, 0, 80, 24);
const NARROW: Rect = Rect::new(0, 0, 40, 24);
const PATH: &str = "src/watch.rs";

/// The mockup's own line, edited into the fixture as its fifth.
const EDITED: &str = "    margin.checked_mul(2).unwrap_or(margin)";

/// The mockup's own note, long enough to wrap at eighty columns.
const BODY: &str =
    "checked_mul on a Duration cannot overflow here; use saturating_mul and drop the unwrap_or.";

/// One committed file whose fifth line changed, which is the mockup's shape.
fn fixture(name: &str) -> Scratch {
    let scratch = Scratch::new(name);
    scratch.write(PATH, numbered_lines(12));
    scratch.commit_all("baseline");
    scratch.edit_line(PATH, 4, EDITED);
    scratch
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
    /// What the shell keeps of the store between wakes, and the effects over it.
    ledger: Ledger,
    effects: NoteEffects,
    /// The effect over the box while it arrives or leaves.
    box_effect: Option<Timed>,
    /// A clock moved by hand, so a departure's end is a fact and not a sleep.
    clock: Instant,
    /// How far the clock moved since the last paint, which is what the effects
    /// are told.
    elapsed: Duration,
    /// The worktree the store is keyed by, so a second handle can be opened.
    workdir: std::path::PathBuf,
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
            ledger: Ledger::default(),
            effects: NoteEffects::default(),
            box_effect: None,
            clock: Instant::now(),
            elapsed: Duration::ZERO,
            workdir: scratch.root().to_path_buf(),
            _root: root,
        }
    }

    /// The agent's hand on the same store: a second handle, as `vigia mcp` has.
    fn agent(&self) -> Store {
        Store::open(self._root.path(), &self.workdir).expect("open a second handle")
    }

    /// What the shell does on a wake from the store and after its own write:
    /// read it back, arm an effect for what moved, and hand the next collect
    /// what the ledger says is drawn.
    fn reload(&mut self) {
        let listing = self.store.list().expect("list the store");
        assert!(listing.skipped.is_empty(), "{:?}", listing.skipped);
        let changes = self.ledger.reload(listing.notes, self.clock);
        self.effects.arm(changes, &self.theme, self.clock);
        self.app.set_notes(self.ledger.drawn());
    }

    /// Move the clock by `by` and take the notes through the frame the shell
    /// would: departures that ended are dropped and spent effects retired.
    fn advance(&mut self, by: Duration) {
        self.clock += by;
        self.elapsed += by;
        if self.ledger.settle(self.clock) {
            self.app.set_notes(self.ledger.drawn());
        }
        self.effects.settle(self.clock);
        self.app.settle_box(self.clock);
        if self
            .box_effect
            .as_ref()
            .is_some_and(|armed| armed.spent(self.clock))
        {
            self.box_effect = None;
        }
    }

    /// The shell's frame: chrome, layout, collect, paint, the effects over the
    /// notes' cells, and the regions the pointer is told about.
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
        let laid = regions(pane, &chrome, &view);
        let mut terminal =
            Terminal::new(TestBackend::new(pane.width, pane.height)).expect("terminal");
        let theme = &self.theme;
        let effects = &mut self.effects;
        let box_effect = &mut self.box_effect;
        let since = std::mem::take(&mut self.elapsed);
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
                if effects.is_running() {
                    effects.draw(since, f.buffer_mut(), &note_cells(&laid, &view));
                }
                if let Some(armed) = box_effect.as_mut()
                    && let Some(over) = box_cells(&laid, &view)
                {
                    armed.draw(since, f.buffer_mut(), over);
                }
            })
            .expect("draw");
        Painted {
            backend: terminal.backend().clone(),
            view,
            laid,
        }
    }

    /// The loop's own routing of a press on the gutter: it opens the box, with
    /// the text of the open note already there, and arms the entrance. `false`
    /// anywhere a press is not a note press. The clock then moves past the
    /// entrance, whose frames `arriving.rs` gates on the effect itself: what the
    /// gates here look at is the box once it has arrived.
    fn press_opens(&mut self, painted: &Painted, column: u16, row: u16) -> bool {
        assert!(
            !self.app.box_open(),
            "the loop routes a press to the open box, so this never reaches the \
             gutter with one up"
        );
        let Some(offset) = press_at(&painted.view, painted.laid, &press(column, row)) else {
            return false;
        };
        // The loop's own refusals, in its own order.
        if !has_room(painted.laid) {
            return false;
        }
        let (anchor, existing) = opening(&painted.view, offset, self.app.notes())
            .expect("a note press resolved to no anchor");
        let existing = existing.or_else(|| self.app.box_over(&anchor).cloned());
        self.app.open_box(anchor, existing.as_ref());
        self.box_effect = Some(Timed::new(
            box_entrance(&self.theme),
            self.clock + BOX_ARRIVING,
        ));
        self.advance(BOX_ARRIVING + ARRIVING_FRAME);
        // The entrance was spent by the clock and never drawn, so the next paint
        // tells whatever it arms nothing of that spell, as the shell's
        // `effect_interval` would.
        self.elapsed = Duration::ZERO;
        true
    }

    /// One event into the open box, routed the way the loop routes it: with the
    /// cells the box drew on the frame `painted`, since a press is judged
    /// against them. `None` where no frame is in hand, which is a key's case.
    fn key_over(&mut self, event: &Event, painted: Option<&Painted>) -> BoxRoute {
        let over = painted.and_then(|painted| box_cells(&painted.laid, &painted.view));
        let route = box_route(event, over);
        match &route {
            BoxRoute::Edit(input) => {
                self.app.box_edit(input.clone());
            }
            BoxRoute::Paste(text) => {
                self.app.box_paste(text);
            }
            _ => {}
        }
        route
    }

    /// One key into the open box, which is judged without geometry.
    fn key(&mut self, event: &Event) -> BoxRoute {
        self.key_over(event, None)
    }

    /// Type `text` into the open box one key at a time, as a terminal delivers it.
    fn type_text(&mut self, text: &str) {
        for c in text.chars() {
            let route = self.key(&Event::Key(KeyEvent::new(
                KeyCode::Char(c),
                KeyModifiers::NONE,
            )));
            assert!(
                matches!(route, BoxRoute::Edit(_)),
                "{c:?} did not reach the box: {route:?}"
            );
        }
    }

    /// Erase everything in the open box, one Backspace at a time.
    fn erase(&mut self) {
        while self
            .app
            .note_box()
            .is_some_and(|open| !open.body().is_empty())
        {
            self.key(&Event::Key(KeyEvent::new(
                KeyCode::Backspace,
                KeyModifiers::NONE,
            )));
        }
    }

    /// Enter, as the loop takes it: write what the box holds, close it at
    /// once, read the store back, and arm the note rows' arrival.
    fn enter(&mut self) -> Committed {
        let open = self.app.note_box().expect("no box is open").clone();
        let done = commit(&self.store, &open).expect("the store refused the write");
        self.app.take_box();
        self.box_effect = None;
        self.reload();
        if let Committed::Written(id) | Committed::Rewritten(id) = &done {
            self.effects
                .arm(vec![Change::Written(id.clone())], &self.theme, self.clock);
        }
        done
    }

    /// Esc, as the loop takes it: the keys are the pane's again now, and the
    /// rows stay drawn while the entrance plays backwards.
    fn esc(&mut self) {
        self.app.close_box(self.clock + BOX_ARRIVING);
        self.box_effect = Some(Timed::new(box_exit(&self.theme), self.clock + BOX_ARRIVING));
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

    /// The consecutive box rows drawn under row `y`, as their whole text.
    fn box_under(&self, y: u16) -> Vec<String> {
        let mut out = Vec::new();
        let mut row = y + 1;
        while row < self.laid.diff.top + self.laid.diff.rows
            && matches!(
                self.view.rows.get(usize::from(row - self.laid.diff.top)),
                Some(Row::Box { .. })
            )
        {
            out.push(self.text(row));
            row += 1;
        }
        out
    }

    /// Whether any row draws the agent's arrow at the content `origin`.
    fn drew_reply(&self, origin: u16) -> bool {
        (0..self.backend.buffer().area.height)
            .any(|row| self.text(row).chars().nth(usize::from(origin)) == Some('↳'))
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
fn a_press_on_the_gutter_opens_the_box_and_writes_nothing() {
    let scratch = fixture("notes-press");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    let plain = rig.paint(&mut frame, PANE, Pointing::default());
    let y = plain.row_of(EDITED);
    let (left, _, origin) = plain.gutter();
    assert!(plain.text(y + 1).contains("line 6"));
    assert!(
        !rig.store.dir().exists(),
        "the store had a directory before any gesture"
    );

    // The press, routed the way the loop routes it: it opens the box and
    // nothing reaches the store.
    let on_gutter = press(left + 1, y);
    let offset = press_at(&plain.view, plain.laid, &on_gutter)
        .expect("a press on a content row's gutter is not a note press");
    assert_eq!(offset, usize::from(y - plain.laid.diff.top));
    assert!(rig.press_opens(&plain, left + 1, y));
    assert!(rig.app.box_open(), "the press opened no box");
    assert!(
        !rig.store.dir().exists(),
        "the press wrote to the store; the gesture that writes is Enter"
    );

    // Under the line, pushing the diff down: the top edge with the anchor, one
    // empty row holding the caret, the bottom edge with the two keys.
    let opened = rig.paint(&mut frame, PANE, Pointing::default());
    let rows = opened.box_under(y);
    assert_eq!(rows.len(), 3, "{rows:?}");
    let o = usize::from(origin);
    assert!(
        rows[0][o..].starts_with("┌ note · src/watch.rs:5 ─"),
        "{:?}",
        rows[0]
    );
    assert!(rows[0].trim_end().ends_with('┐'), "{:?}", rows[0]);
    assert!(rows[1][o..].starts_with("│ "), "{:?}", rows[1]);
    assert!(rows[1].trim_end().ends_with('│'), "{:?}", rows[1]);
    assert!(
        rows[2][o..].starts_with("└ Enter sends · Esc cancels ─"),
        "{:?}",
        rows[2]
    );
    assert!(rows[2].trim_end().ends_with('┘'), "{:?}", rows[2]);
    assert!(
        opened.text(y + 4).contains("line 6"),
        "the diff below did not move down under the box:\n{}",
        opened.rows().join("\n")
    );
    // The caret: the cell after the side, reversed, which is the editor's own.
    assert!(
        opened
            .cell(origin + 2, y + 2)
            .style()
            .add_modifier
            .contains(Modifier::REVERSED),
        "no caret in the empty box"
    );
    // The frame and its labels in the chrome's dim, so nothing reads as code.
    let dim = Theme::default().chrome_dim.fg;
    assert_eq!(opened.fg(origin, y + 1), dim);
    assert_eq!(opened.fg(origin + 3, y + 3), dim);
    // The line keeps the note's ink while the box is open, so the box can be
    // traced to it from across the pane.
    let five = (left..origin)
        .find(|x| opened.cell(*x, y).symbol() == "5")
        .expect("the anchored line's number");
    assert_eq!(opened.fg(five, y), Theme::default().bar_hover.fg);
    assert!(
        opened
            .cell(five, y)
            .style()
            .add_modifier
            .contains(Modifier::BOLD)
    );
    // The cells a press is judged against are the box's three rows across the
    // content width.
    let over = box_cells(&opened.laid, &opened.view).expect("the box's cells");
    assert_eq!((over.x, over.y, over.height), (origin, y + 1, 3));
    assert_eq!(over.width, opened.laid.diff.text - (origin - left));

    // The same row's content is no note press and begins a selection, as B20 rules.
    let on_content = press(origin + 3, y);
    assert_eq!(press_at(&plain.view, plain.laid, &on_content), None);
    assert!(
        selection_after(&on_content, plain.laid, None).0.is_some(),
        "a press on content stopped beginning a selection"
    );
    // And a release on the gutter opens nothing.
    assert_eq!(
        press_at(&plain.view, plain.laid, &release(left + 1, y)),
        None
    );
}

#[test]
fn enter_writes_one_file_and_the_rows_arrive_under_the_line() {
    let scratch = fixture("notes-enter");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    let dim = rig.theme.chrome_dim.fg.expect("the chrome's dim ink");
    let plain = rig.paint(&mut frame, PANE, Pointing::default());
    let y = plain.row_of(EDITED);
    let (left, _, origin) = plain.gutter();

    assert!(rig.press_opens(&plain, left + 1, y));
    rig.type_text(BODY);
    let typed = rig.paint(&mut frame, PANE, Pointing::default());
    assert!(
        typed.box_under(y).len() > 3,
        "the body did not wrap inside the box: {:?}",
        typed.box_under(y)
    );
    assert_eq!(
        rig.key(&Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE
        ))),
        BoxRoute::Send
    );
    let written = match rig.enter() {
        Committed::Written(id) => id,
        other => panic!("Enter did not write a note: {other:?}"),
    };
    assert!(!rig.app.box_open(), "Enter left the box open");

    // One file, holding the anchor and the words.
    assert_eq!(files_in(rig.store.dir()), vec![format!("{written}.note")]);
    let note = rig.store.get(&written).expect("get").expect("the note");
    assert_eq!(
        (
            note.path.as_str(),
            note.side,
            note.line,
            note.text.as_str(),
            note.body.as_str(),
            note.status
        ),
        (PATH, Side::New, 5, EDITED, BODY, Status::Open)
    );

    // The rows stand under the line where the box was, arriving: the note's ink
    // first, the chrome's dim once the arrival has run.
    let arriving = rig.paint(&mut frame, PANE, Pointing::default());
    assert!(arriving.box_under(y).is_empty(), "the box is still drawn");
    let under = arriving.notes_under(y);
    assert_eq!(
        rejoined(&under, "open"),
        BODY,
        "the rows do not carry the body"
    );
    assert!(under.last().expect("a row").trim_end().ends_with("open"));
    assert!(rig.effects.is_running(), "the rows landed without arriving");
    assert_ne!(arriving.fg(origin + 2, y + 1), Some(dim));
    rig.advance(RESOLVE_ARRIVING + ARRIVING_FRAME);
    let settled = rig.paint(&mut frame, PANE, Pointing::default());
    assert_eq!(settled.fg(origin + 2, y + 1), Some(dim));
    assert!(!rig.effects.is_running());
}

#[test]
fn esc_closes_the_box_and_writes_nothing() {
    let scratch = fixture("notes-esc");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    let plain = rig.paint(&mut frame, PANE, Pointing::default());
    let y = plain.row_of(EDITED);
    let (left, _, _) = plain.gutter();

    assert!(rig.press_opens(&plain, left + 1, y));
    rig.type_text("never sent");
    assert_eq!(
        rig.key(&Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))),
        BoxRoute::Cancel
    );
    rig.esc();
    // The keys are the pane's again at once.
    assert!(!rig.app.box_open(), "Esc left the keys with the box");
    assert!(!rig.store.dir().exists(), "Esc wrote to the store");

    // The rows stay while the entrance plays backwards, and go on the frame
    // after it ends.
    let leaving = rig.paint(&mut frame, PANE, Pointing::default());
    let whole = leaving.box_under(y);
    assert_eq!(whole.len(), 3, "the rows left with the keys");
    assert!(whole[0].contains("note · src/watch.rs:5"), "{whole:?}");
    rig.advance(BOX_ARRIVING / 2);
    let halfway = rig.paint(&mut frame, PANE, Pointing::default());
    assert_eq!(halfway.box_under(y).len(), 3);
    assert_ne!(
        halfway.box_under(y),
        whole,
        "halfway out the box is drawn whole"
    );
    rig.advance(BOX_ARRIVING / 2);
    let gone = rig.paint(&mut frame, PANE, Pointing::default());
    assert_eq!(gone.rows(), plain.rows(), "the box left something drawn");
    assert!(rig.box_effect.is_none(), "the exit outlived the rows");
}

#[test]
fn an_emptied_box_on_enter_writes_nothing() {
    let scratch = fixture("notes-empty");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    let plain = rig.paint(&mut frame, PANE, Pointing::default());
    let y = plain.row_of(EDITED);
    let (left, _, _) = plain.gutter();

    assert!(rig.press_opens(&plain, left + 1, y));
    // Blank is empty: the body is trimmed at both ends.
    rig.type_text("   ");
    assert_eq!(rig.enter(), Committed::Nothing);
    assert!(!rig.app.box_open());
    assert!(!rig.store.dir().exists(), "an empty box wrote a file");
    let after = rig.paint(&mut frame, PANE, Pointing::default());
    assert_eq!(after.rows(), plain.rows());
}

#[test]
fn a_press_on_a_noted_line_reopens_the_box_with_its_text_and_an_emptied_box_withdraws_it() {
    let scratch = fixture("notes-reopen");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    rig.store.put(&note("n1", 5, EDITED, BODY)).expect("put");
    rig.reload();
    let noted = rig.paint(&mut frame, PANE, Pointing::default());
    let y = noted.row_of(EDITED);
    let (left, _, _) = noted.gutter();
    assert_eq!(noted.notes_under(y).len(), 2);

    // The press reopens the note in the box rather than writing a second one,
    // and the note's own rows stand aside while the box holds its text.
    assert!(rig.press_opens(&noted, left + 1, y));
    let open = rig.app.note_box().expect("the box");
    assert_eq!(open.over(), Some("n1"));
    assert_eq!(open.lines().join(" "), BODY);
    assert_eq!(open.cursor(), (0, BODY.chars().count()));
    let reopened = rig.paint(&mut frame, PANE, Pointing::default());
    let rows = reopened.box_under(y);
    assert!(rows.len() > 3, "{rows:?}");
    assert!(
        rows[1].contains("checked_mul on a Duration"),
        "{:?}",
        rows[1]
    );
    assert!(
        !reopened
            .view
            .rows
            .iter()
            .any(|row| matches!(row, Row::Note { .. })),
        "the note's rows are drawn under the box that holds its text"
    );
    // The line keeps the note's ink from the box rather than from the note the
    // box holds, so nothing on screen says that note twice.
    let offset = usize::from(y - reopened.laid.diff.top);
    assert_eq!(reopened.view.notes.boxed, Some(offset));
    assert!(reopened.view.marked_at(offset).is_empty());
    let five = (left..reopened.gutter().2)
        .find(|x| reopened.cell(*x, y).symbol() == "5")
        .expect("the anchored line's number");
    assert_eq!(reopened.fg(five, y), Theme::default().bar_hover.fg);

    // Emptied and sent, the note is withdrawn: one open note per line, and
    // this is how the reader takes it back.
    rig.erase();
    assert_eq!(rig.enter(), Committed::Withdrawn("n1".to_owned()));
    assert!(
        files_in(rig.store.dir()).is_empty(),
        "the file was not removed"
    );
    // The rows leave over `LEAVING` and are dropped on the frame after.
    rig.advance(LEAVING);
    let clear = rig.paint(&mut frame, PANE, Pointing::default());
    let plain = {
        let mut bare = Rig::open(&scratch);
        bare.paint(&mut frame, PANE, Pointing::default())
    };
    assert_eq!(
        clear.rows(),
        plain.rows(),
        "the withdrawn note left something drawn"
    );
}

#[test]
fn a_rewritten_note_is_open_again_and_keeps_its_id_and_reply() {
    let scratch = fixture("notes-rewrite");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    rig.store
        .put(&left_as("n1", "short", Status::Seen, Some("which margin?")))
        .expect("put");
    rig.reload();
    let noted = rig.paint(&mut frame, PANE, Pointing::default());
    let y = noted.row_of(EDITED);
    let (left, _, _) = noted.gutter();

    assert!(rig.press_opens(&noted, left + 1, y));
    rig.type_text(", the settle one");
    assert_eq!(rig.enter(), Committed::Rewritten("n1".to_owned()));
    let note = rig.store.get("n1").expect("get").expect("the note");
    assert_eq!(note.body, "short, the settle one");
    assert_eq!(
        note.status,
        Status::Open,
        "the agent has not read the new text"
    );
    assert_eq!(note.reply.as_deref(), Some("which margin?"));
    assert_eq!(files_in(rig.store.dir()).len(), 1);

    let drawn = rig.paint(&mut frame, PANE, Pointing::default());
    let under = drawn.notes_under(y);
    assert!(under[0].starts_with("short, the settle one"), "{under:?}");
    assert!(under[0].trim_end().ends_with("open"), "{under:?}");
    assert!(under[1].starts_with("which margin?"), "{under:?}");
}

#[test]
fn a_note_resolved_under_an_open_box_is_written_as_a_new_note() {
    let scratch = fixture("notes-resolved-under-box");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    rig.store.put(&note("n1", 5, EDITED, "short")).expect("put");
    rig.reload();
    let noted = rig.paint(&mut frame, PANE, Pointing::default());
    let y = noted.row_of(EDITED);
    let (left, _, _) = noted.gutter();
    assert!(rig.press_opens(&noted, left + 1, y));

    // The agent resolves it while the reader is typing: the resolve answers the
    // old text and keeps its file, and the new text goes down beside it.
    rig.agent()
        .rewrite(&left_as("n1", "short", Status::Resolved, Some(REPLY)))
        .expect("rewrite");
    rig.type_text(" and more");
    let fresh = match rig.enter() {
        Committed::Written(id) => id,
        other => panic!("the new text was not written as a new note: {other:?}"),
    };
    assert_ne!(fresh, "n1");
    assert_eq!(files_in(rig.store.dir()).len(), 2);
    let resolved = rig.store.get("n1").expect("get").expect("the resolve");
    assert_eq!(resolved.status, Status::Resolved);
    assert_eq!(resolved.reply.as_deref(), Some(REPLY));
    let written = rig.store.get(&fresh).expect("get").expect("the new note");
    assert_eq!(written.body, "short and more");
    assert_eq!(written.status, Status::Open);
}

#[test]
fn a_press_outside_the_box_closes_it_and_a_press_inside_leaves_it_open() {
    let scratch = fixture("notes-press-elsewhere");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    let plain = rig.paint(&mut frame, PANE, Pointing::default());
    let y = plain.row_of(EDITED);
    let (left, _, origin) = plain.gutter();
    assert!(rig.press_opens(&plain, left + 1, y));
    let opened = rig.paint(&mut frame, PANE, Pointing::default());
    let over = box_cells(&opened.laid, &opened.view).expect("the box's cells");

    // Inside: nothing. Outside, anywhere: close, and nothing else.
    assert_eq!(
        box_route(&press(origin + 4, y + 2), Some(over)),
        BoxRoute::Inert
    );
    assert_eq!(box_route(&press(left + 1, y), Some(over)), BoxRoute::Cancel);
    assert_eq!(
        box_route(&press(2, opened.laid.list.top), Some(over)),
        BoxRoute::Cancel
    );
    // The wheel, a resize, focus and the pointer resting pass through to the pane.
    for through in [
        at(MouseEventKind::ScrollDown, origin + 4, y + 2),
        at(MouseEventKind::ScrollUp, 2, opened.laid.list.top),
        moved(origin + 4, y + 2),
        release(origin + 4, y + 2),
        Event::Resize(120, 40),
        Event::FocusLost,
        Event::FocusGained,
    ] {
        assert_eq!(
            box_route(&through, Some(over)),
            BoxRoute::Through,
            "{through:?}"
        );
    }
    // And a paste goes into the box.
    assert_eq!(
        rig.key(&Event::Paste("pasted".to_owned())),
        BoxRoute::Paste("pasted".to_owned())
    );
    assert_eq!(rig.app.note_box().expect("the box").body(), "pasted");
}

#[test]
fn alt_enter_breaks_the_body_and_the_note_rows_break_with_it() {
    let scratch = fixture("notes-newline");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    let plain = rig.paint(&mut frame, PANE, Pointing::default());
    let y = plain.row_of(EDITED);
    let (left, _, origin) = plain.gutter();
    assert!(rig.press_opens(&plain, left + 1, y));

    rig.type_text("first");
    let newline = Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT));
    assert!(matches!(rig.key(&newline), BoxRoute::Edit(_)));
    rig.type_text("second");
    let open = rig.app.note_box().expect("the box");
    assert_eq!(open.lines(), ["first", "second"]);
    assert_eq!(open.cursor(), (1, 6));
    let typed = rig.paint(&mut frame, PANE, Pointing::default());
    let rows = typed.box_under(y);
    assert_eq!(rows.len(), 4, "{rows:?}");
    assert!(rows[1].contains("first") && rows[2].contains("second"));
    // The caret on the second row, after its word.
    assert!(
        typed
            .cell(origin + 2 + 6, y + 3)
            .style()
            .add_modifier
            .contains(Modifier::REVERSED)
    );

    assert!(matches!(rig.enter(), Committed::Written(_)));
    let listing = rig.store.list().expect("list");
    assert_eq!(listing.notes[0].body, "first\nsecond");
    let drawn = rig.paint(&mut frame, PANE, Pointing::default());
    let under = drawn.notes_under(y);
    assert_eq!(under.len(), 2, "{under:?}");
    assert!(under[0].starts_with("first") && under[1].starts_with("second"));
}

#[test]
fn the_box_wraps_at_the_inner_width_and_scrolls_so_the_caret_is_drawn() {
    let scratch = fixture("notes-box-scrolls");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    let plain = rig.paint(&mut frame, PANE, Pointing::default());
    let y = plain.row_of(EDITED);
    let (left, _, origin) = plain.gutter();
    assert!(rig.press_opens(&plain, left + 1, y));

    let long = format!("{BODY} {BODY} {BODY} at the end");
    rig.type_text(&long);
    let typed = rig.paint(&mut frame, PANE, Pointing::default());
    let rows = typed.box_under(y);
    assert_eq!(
        rows.len(),
        BOX_ROWS + 2,
        "the box grew past its cap: {rows:?}"
    );
    // Every body row fits between the sides, and the sides stand at the
    // content's edges.
    let inner = usize::from(typed.laid.diff.text - (origin - left)) - BOX_FRAME;
    let past_side = |row: &str| -> String { row.chars().skip(usize::from(origin) + 2).collect() };
    for row in &rows[1..=BOX_ROWS] {
        let body = past_side(row);
        let text = body[..body.rfind('│').expect("the right side")].trim_end();
        assert!(
            text.chars().count() <= inner,
            "{text:?} is wider than {inner}"
        );
    }
    // The first words scrolled out and the last are drawn: what the box shows
    // is the tail of the text, with the caret after it on the last row.
    let visible = rejoined(
        &rows[1..=BOX_ROWS]
            .iter()
            .map(|row| {
                let body = past_side(row);
                body[..body.rfind('│').expect("the right side")].to_owned()
            })
            .collect::<Vec<_>>(),
        "",
    );
    assert!(
        long.ends_with(&visible) && visible.len() < long.len(),
        "the box shows {visible:?}, which is not the tail of the text"
    );
    let last = past_side(&rows[BOX_ROWS]);
    let tail = last
        .find("at the end")
        .expect("the last words are not drawn");
    let caret = usize::from(origin) + 2 + last[..tail].chars().count() + "at the end".len();
    assert!(
        typed
            .cell(caret as u16, y + 1 + BOX_ROWS as u16)
            .style()
            .add_modifier
            .contains(Modifier::REVERSED),
        "the caret is not after the last word"
    );
}

#[test]
fn at_forty_columns_the_box_takes_the_content_width_and_the_label_drops_its_head() {
    let scratch = fixture("notes-box-narrow");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    let plain = rig.paint(&mut frame, NARROW, Pointing::default());
    let y = plain.row_of("margin.checked");
    let (left, _, origin) = plain.gutter();
    assert!(rig.press_opens(&plain, left + 1, y));
    rig.type_text(BODY);
    let narrow = rig.paint(&mut frame, NARROW, Pointing::default());
    let rows = narrow.box_under(y);
    assert!(rows.len() > 3, "{rows:?}");
    // Across the content width, the sides at its edges, and the whole anchor
    // still fits here.
    let end = usize::from(origin) + usize::from(narrow.laid.diff.text - (origin - left));
    for row in &rows {
        assert_eq!(row.trim_end().chars().count(), end, "{row:?}");
        assert_eq!(row.chars().nth(end).unwrap_or(' '), ' ', "{row:?}");
    }
    assert!(rows[0].contains("note · src/watch.rs:5"), "{:?}", rows[0]);

    // Narrower still, the anchor gives up its word first and keeps its path;
    // `legibility.rs` sweeps the rung below, where the path loses its head the
    // way a heading's does.
    let tight = rig.paint(&mut frame, Rect::new(0, 0, 20, 24), Pointing::default());
    let y = tight.row_of("margin");
    let rows = tight.box_under(y);
    assert!(rows.len() > 3, "{rows:?}");
    let top = rows[0].trim_end();
    assert!(!top.contains("note ·"), "{top:?}");
    assert!(
        top.contains("src/watch.rs:5") && top.ends_with('┐'),
        "{top:?}"
    );
}

#[test]
fn a_pane_with_no_room_for_the_box_opens_none() {
    // Below the box's own floor nothing of it can be drawn, and a mode the
    // reader cannot see is one they cannot leave on purpose: every key would go
    // into it and Enter would write a note nothing on screen ever showed.
    let scratch = fixture("notes-box-no-room");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);

    // Wide enough to draw content rows a press can land on, too narrow for the
    // box between its two sides.
    let cramped = rig.paint(&mut frame, Rect::new(0, 0, 6, 24), Pointing::default());
    assert!(
        !has_room(cramped.laid),
        "the six-column pane has room for the box, so this gate is not narrow"
    );
    let (left, _, _) = cramped.gutter();
    let y = cramped
        .view
        .rows
        .iter()
        .position(|row| matches!(row, Row::Line { .. }))
        .map(|at| cramped.laid.diff.top + at as u16)
        .expect("a content row on the cramped pane");
    // The press still lands on a gutter, so the room is what refuses it rather
    // than the absence of a target.
    assert!(
        press_at(&cramped.view, cramped.laid, &press(left, y)).is_some(),
        "the cramped pane has no gutter to press, so this gate proves nothing"
    );
    // And the press itself opens nothing: no box, no keys taken, no store.
    assert!(!rig.press_opens(&cramped, left, y));
    assert!(
        !rig.app.box_open(),
        "a pane too narrow to draw the box still gave it the keys"
    );
    assert!(rig.app.note_box().is_none());
    assert!(!rig.store.dir().exists());
    let after = rig.paint(&mut frame, Rect::new(0, 0, 6, 24), Pointing::default());
    assert_eq!(
        after.rows(),
        cramped.rows(),
        "the refused press drew something"
    );

    // And where the box does draw, the same rule says so and the press opens one,
    // so the refusal above is the width and not something else about the press.
    let roomy = rig.paint(&mut frame, PANE, Pointing::default());
    assert!(has_room(roomy.laid));
    let y = roomy.row_of(EDITED);
    let (left, _, _) = roomy.gutter();
    assert!(rig.press_opens(&roomy, left + 1, y));
    assert!(rig.app.box_open());
}

#[test]
fn follow_holds_the_viewport_under_an_open_box() {
    // The mockup's file and a second one after it, both changed.
    let scratch = Scratch::new("notes-box-follow");
    scratch.write(PATH, numbered_lines(12));
    scratch.write("zzz/other.rs", numbered_lines(40));
    scratch.commit_all("baseline");
    scratch.edit_line(PATH, 4, EDITED);
    scratch.edit_line("zzz/other.rs", 30, "    changed under the reader");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    let plain = rig.paint(&mut frame, PANE, Pointing::default());
    let y = plain.row_of(EDITED);
    let (left, _, _) = plain.gutter();
    assert!(rig.app.following());
    assert!(rig.press_opens(&plain, left + 1, y));
    let before = rig.app.position();

    // The agent writes the other file: follow would jump, and holds instead.
    assert!(!rig.app.follow("zzz/other.rs", &frame));
    assert_eq!(rig.app.position(), before);
    assert!(rig.app.following(), "holding still disengaged follow");
    let held = rig.paint(&mut frame, PANE, Pointing::default());
    assert_eq!(held.box_under(y).len(), 3);

    // Closed, the next write moves the viewport again.
    rig.esc();
    assert!(rig.app.follow("zzz/other.rs", &frame));
    assert_ne!(rig.app.position(), before);
}

#[test]
fn a_box_opened_on_the_last_drawn_row_is_still_whole() {
    // The bottom of the pane is an ordinary place to click, and a top edge with
    // nothing under it is a mode the reader is in and cannot see: the window
    // comes forward instead, the way it does for the diff's own last row.
    let scratch = fixture("notes-box-bottom");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    // Short enough that the box cannot fit under the last line it draws.
    let short = Rect::new(0, 0, 80, 10);
    let plain = rig.paint(&mut frame, short, Pointing::default());
    let last = plain
        .view
        .rows
        .iter()
        .enumerate()
        .filter(|(_, row)| matches!(row, Row::Line { .. }))
        .map(|(at, _)| at)
        .next_back()
        .expect("a content row on the short pane");
    assert_eq!(
        last + 1,
        plain.view.rows.len(),
        "the fixture's last drawn row is not a line, so this gate is not \
         pressing the row it is named for"
    );
    let y = plain.laid.diff.top + last as u16;
    let anchored = match &plain.view.rows[last] {
        Row::Line { text, .. } => text.clone(),
        other => panic!("the last row is not a line: {other:?}"),
    };
    let (left, _, _) = plain.gutter();

    assert!(rig.press_opens(&plain, left + 1, y));
    rig.type_text("at the bottom");
    let opened = rig.paint(&mut frame, short, Pointing::default());

    // Every part of it: both edges and the row the caret stands on.
    let parts: Vec<&BoxPart> = opened
        .view
        .rows
        .iter()
        .filter_map(|row| match row {
            Row::Box { part } => Some(part),
            _ => None,
        })
        .collect();
    assert!(
        matches!(parts.first(), Some(BoxPart::Top { .. }))
            && matches!(parts.last(), Some(BoxPart::Bottom)),
        "the box drew {} rows and not a closed box:\n{}",
        parts.len(),
        opened.rows().join("\n")
    );
    let caret = parts
        .iter()
        .any(|part| matches!(part, BoxPart::Body { caret: Some(_), .. }));
    assert!(caret, "the caret's row is not among the box's drawn rows");
    let text = opened.rows().join("\n");
    assert!(
        text.contains("at the bottom"),
        "the reader's own words are not on screen:\n{text}"
    );
    // And the line it is anchored to came with it, above the box rather than
    // scrolled off to make room for it.
    assert!(
        text.contains(&anchored),
        "the anchored line {anchored:?} left the screen:\n{text}"
    );
}

#[test]
fn a_box_opened_at_the_top_of_a_bottom_anchored_screen_stays_on_it() {
    // The diff resting on its last row already drops every row the box would,
    // so the box's own floor never binds there. What binds is the ceiling: the
    // box's rows grow the diff, the bottom clamp answers by dropping that many
    // more off the front, and without a bound at the anchored line the box the
    // reader just opened is carried off the top of the screen.
    // A file long enough that its diff outruns the pane, or there is no bottom
    // for the clamp to hold the window against.
    let scratch = Scratch::new("notes-box-both-clamps");
    scratch.write(PATH, numbered_lines(60));
    scratch.commit_all("baseline");
    for line in (4..60).step_by(6) {
        scratch.edit_line(PATH, line, &format!("changed {}", line + 1));
    }
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    let short = Rect::new(0, 0, 80, 10);

    // Scrolled to the diff's own end, which is what arms the bottom clamp: the
    // walk lands there and pulls the window back so the last row rests on the
    // bottom of the pane.
    let height = body_layout(
        short,
        &rig.app.chrome("fixture", None, Pointing::default(), 0, ""),
        1,
        1,
    )
    .diff;
    rig.app
        .apply(Action::Scroll(500), &mut frame, height)
        .expect("scroll past the end");
    let bottom = rig.paint(&mut frame, short, Pointing::default());
    assert!(
        bottom.view.rows_above > 0,
        "the fixture's diff fits the pane, so nothing here is bottom anchored"
    );
    let last_line = bottom
        .view
        .rows
        .iter()
        .enumerate()
        .filter(|(_, row)| matches!(row, Row::Line { .. }))
        .map(|(at, _)| at)
        .next_back()
        .expect("a content row at the bottom");
    let tail = match &bottom.view.rows[last_line] {
        Row::Line { text, .. } => text.clone(),
        other => panic!("not a line: {other:?}"),
    };
    let (left, _, _) = bottom.gutter();

    // The first line on that screen, which is the row the bottom clamp carries
    // off the top as soon as the box's rows grow the diff under it.
    let on = bottom
        .view
        .rows
        .iter()
        .position(|row| matches!(row, Row::Line { .. }))
        .expect("a content row at the top of the bottom-anchored screen");
    assert!(
        on < last_line,
        "the fixture drew one content row, not a screen"
    );
    assert!(rig.press_opens(&bottom, left + 1, bottom.laid.diff.top + on as u16));
    rig.type_text("both clamps");
    let opened = rig.paint(&mut frame, short, Pointing::default());

    let parts: Vec<&BoxPart> = opened
        .view
        .rows
        .iter()
        .filter_map(|row| match row {
            Row::Box { part } => Some(part),
            _ => None,
        })
        .collect();
    assert!(
        matches!(parts.first(), Some(BoxPart::Top { .. }))
            && matches!(parts.last(), Some(BoxPart::Bottom)),
        "the box the reader just opened was carried off the screen by the clamp \
         for the diff's own end:\n{}",
        opened.rows().join("\n")
    );
    assert!(
        opened.rows().join("\n").contains("both clamps"),
        "the reader's own words left the screen:\n{}",
        opened.rows().join("\n")
    );
    // The diff's end gave up exactly the rows the box took and no more, so the
    // bottom clamp is still doing its own job around it.
    let shown = opened.rows().join("\n");
    assert!(
        !shown.contains(&tail),
        "the window did not move at all, so this screen never had the bottom \
         clamp on it:\n{shown}"
    );
}

#[test]
fn box_rows_are_display_rows_the_bar_does_not_count() {
    let scratch = fixture("notes-box-display");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    let plain = rig.paint(&mut frame, PANE, Pointing::default());
    let y = plain.row_of(EDITED);
    let (left, _, _) = plain.gutter();
    assert!(rig.press_opens(&plain, left + 1, y));
    rig.type_text(BODY);
    let opened = rig.paint(&mut frame, PANE, Pointing::default());
    assert!(opened.box_under(y).len() > 3);
    assert_eq!(opened.view.shown(), plain.view.shown());
    assert_eq!(opened.view.total_rows, plain.view.total_rows);
    assert_eq!(opened.view.rows_above, plain.view.rows_above);
    // And a box row is no target: no anchor, no note press.
    let offset = usize::from(y + 2 - opened.laid.diff.top);
    assert_eq!(opened.view.anchor_at(offset), None);
    assert_eq!(
        press_at(&opened.view, opened.laid, &press(left + 1, y + 2)),
        None
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
    let mut lines: Vec<String> = numbered_lines(12).lines().map(str::to_owned).collect();
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
    assert_eq!(painted.view.notes.adrift, 0);
}

#[test]
fn a_file_out_of_the_diff_leaves_its_note_adrift_and_the_footer_counts_it() {
    let scratch = Scratch::new("notes-adrift");
    scratch.write(PATH, numbered_lines(12));
    scratch.write("src/other.rs", numbered_lines(12));
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
    assert_eq!(both.view.notes.adrift, 0);
    assert!(both.footer().contains("2 notes"), "{}", both.footer());
    assert!(!both.footer().contains("adrift"), "{}", both.footer());

    // The other file leaves the diff: reverted, which is one of the four ways.
    scratch.git(&["checkout", "--", "src/other.rs"]);
    frame.advance().expect("advance after the revert");
    let adrift = rig.paint(&mut frame, PANE, Pointing::default());
    assert_eq!(adrift.view.notes.adrift, 1);
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
    assert_eq!(back.view.notes.adrift, 0);
    let y = back.row_of("other changed");
    assert!(back.notes_under(y)[0].starts_with("two"));

    assert_eq!(count_cell(0, 0), "");
    assert_eq!(count_cell(1, 0), "1 note");
    assert_eq!(count_cell(2, 1), "2 notes · 1 adrift");
}

#[test]
fn a_renamed_file_carries_its_note_to_the_new_path() {
    let scratch = Scratch::new("notes-renamed");
    scratch.write("old/name.rs", numbered_lines(30));
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
    assert_eq!(painted.view.notes.adrift, 0);
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
    scratch.write(PATH, numbered_lines(200));
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
fn a_resolved_note_without_a_reply_draws_its_body_and_the_word() {
    // The store's reply block is optional, so a note can be resolved with no
    // line to draw alone; its body stays under the mark with the word resolved,
    // because a marked line with nothing under it reads as hidden rows.
    let scratch = fixture("notes-resolved-bare");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    let mut resolved = note("n1", 5, EDITED, "short");
    resolved.status = Status::Resolved;
    rig.store.put(&resolved).expect("put");
    rig.reload();
    let painted = rig.paint(&mut frame, PANE, Pointing::default());
    let y = painted.row_of(EDITED);
    let under = painted.notes_under(y);
    assert_eq!(under.len(), 1, "{under:?}");
    assert!(under[0].starts_with("short"), "{:?}", under[0]);
    assert!(under[0].trim_end().ends_with("resolved"), "{:?}", under[0]);
    assert!(painted.text(y + 2).contains("line 6"));
}

#[test]
fn a_store_that_refuses_the_write_leaves_the_box_and_its_text() {
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

    assert!(rig.press_opens(&painted, left, y));
    rig.type_text("kept");
    let typed = rig.paint(&mut frame, PANE, Pointing::default());
    let refused = commit(&store, rig.app.note_box().expect("the box"));
    let why = refused.expect_err("the store wrote into a file");
    assert!(!why.to_string().is_empty());
    // Nothing is lost and nothing stops: the box stays with its text, and the
    // next frame is the frame before.
    assert!(rig.app.box_open(), "a refused write closed the box");
    assert_eq!(rig.app.note_box().expect("the box").body(), "kept");
    let after = rig.paint(&mut frame, PANE, Pointing::default());
    assert_eq!(after.rows(), typed.rows());
}

#[test]
fn a_line_that_moves_under_an_open_box_keeps_it() {
    let scratch = fixture("notes-box-moved");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    let plain = rig.paint(&mut frame, PANE, Pointing::default());
    let y = plain.row_of(EDITED);
    let (left, _, _) = plain.gutter();
    assert!(rig.press_opens(&plain, left + 1, y));
    rig.type_text("still here");

    // A line inserted above pushes the anchored line down to 6, and the box
    // goes with it: the anchor is re-resolved every frame, as a note's is.
    let mut lines: Vec<String> = numbered_lines(12).lines().map(str::to_owned).collect();
    lines[4] = EDITED.to_owned();
    lines.insert(1, "inserted".to_owned());
    scratch.write(PATH, lines.join("\n") + "\n");
    frame.advance().expect("advance after the insert");
    let moved = rig.paint(&mut frame, PANE, Pointing::default());
    let y = moved.row_of(EDITED);
    assert!(
        moved.text(y).trim_start().starts_with('6'),
        "the anchored line is not numbered 6 after the insert: {:?}",
        moved.text(y)
    );
    let rows = moved.box_under(y);
    assert_eq!(rows.len(), 3, "{rows:?}");
    assert!(rows[0].contains("src/watch.rs:5"), "{:?}", rows[0]);
    assert!(rows[1].contains("still here"), "{:?}", rows[1]);
    assert!(rig.app.box_open());
    // And Enter pins the note to the number it was opened on, which is what a
    // note stores: the walk finds it by its text from there.
    assert!(matches!(rig.enter(), Committed::Written(_)));
    let listing = rig.store.list().expect("list");
    assert_eq!(
        (listing.notes[0].line, listing.notes[0].text.as_str()),
        (5, EDITED)
    );
    let written = rig.paint(&mut frame, PANE, Pointing::default());
    assert_eq!(written.notes_under(y).len(), 1);
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
    scratch.write(PATH, numbered_lines(12));
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

    // And a press there opens the box on the whole line at the head's number.
    assert!(rig.press_opens(&painted, left, y + 1));
    rig.type_text("on the head");
    assert!(matches!(rig.enter(), Committed::Written(_)));
    let written = &rig.store.list().expect("list").notes[0];
    assert_eq!(written.line, 5);
    assert_eq!(written.text, long.trim_end());
}

#[test]
fn the_bottom_clamp_counts_note_rows() {
    let scratch = Scratch::new("notes-bottom");
    scratch.write(PATH, numbered_lines(60));
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

#[test]
fn the_notes_count_never_buys_the_footer_a_second_line() {
    // The count moves on a collect, after the layout was taken, so a count that
    // grew the footer would leave the body a row shorter than the rows collected
    // for it. Every width, against a count wide enough to matter.
    let app = App::new();
    let without = app.chrome("fixture", None, Pointing::default(), 0, "");
    let mut with = without.clone();
    with.notes = NoteCount {
        total: 12,
        adrift: 3,
    };
    let mut counted_somewhere = false;
    for width in 20..=120u16 {
        let pane = Rect::new(0, 0, width, 24);
        assert_eq!(
            body_layout(pane, &with, 3, 3),
            body_layout(pane, &without, 3, 3),
            "the notes count changed the layout at {width} columns"
        );
        let mut buf = ratatui::buffer::Buffer::empty(pane);
        render(
            &mut buf,
            pane,
            &View::default(),
            &Theme::default(),
            Glyphs::default(),
            &with,
        );
        let footer: String = (0..width)
            .map(|x| buf[(x, 23)].symbol())
            .collect::<String>();
        counted_somewhere |= footer.contains("12 notes · 3 adrift");
    }
    assert!(
        counted_somewhere,
        "no width drew the count, so the layouts above agree about nothing"
    );
}

#[test]
fn below_the_gutters_floor_the_sigil_takes_the_mark() {
    // Twenty-seven columns: one digit and the sigil leave twenty-three for text,
    // under the floor that keeps line numbers, so the sigil and its gap are the
    // whole target and the sigil's cell is the one that draws.
    let scratch = fixture("notes-floor");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    let floor = Rect::new(0, 0, 27, 24);
    let plain = rig.paint(&mut frame, floor, Pointing::default());
    let y = plain.row_of("checked_mul");
    let (left, columns, _) = plain.gutter();
    assert!(
        plain.view.gutter == Some(0),
        "the fixture kept its line numbers at {} columns, so this is not the floor",
        floor.width
    );
    assert_eq!(
        columns, 2,
        "the target below the floor is the sigil and its gap"
    );
    assert_eq!(plain.cell(left, y).symbol(), "+");
    for x in left..left + columns {
        assert_eq!(plain.laid.hover_at(x, y), Some(Hovered::Gutter(y)));
    }
    assert_eq!(plain.laid.hover_at(left + columns, y), None);

    let hovering = rig.paint(
        &mut frame,
        floor,
        Pointing {
            hovered: Some(Hovered::Gutter(y)),
            ..Pointing::default()
        },
    );
    assert_eq!(hovering.cell(left, y).symbol(), "✎", "{}", hovering.text(y));
    let past_the_sigil = |painted: &Painted| painted.text(y).chars().skip(1).collect::<String>();
    assert_eq!(
        past_the_sigil(&hovering),
        past_the_sigil(&plain),
        "the mark reached past the sigil's cell"
    );

    // A note with a body keeps the sigil and marks it; the anchor alone takes the icon.
    rig.store.put(&note("n1", 5, EDITED, "short")).expect("put");
    rig.reload();
    let noted = rig.paint(&mut frame, floor, Pointing::default());
    assert_eq!(noted.cell(left, y).symbol(), "+");
    assert!(
        noted
            .cell(left, y)
            .style()
            .add_modifier
            .contains(Modifier::BOLD)
    );
    rig.store.remove("n1").expect("remove");
    // Gone from the store without a press, so it leaves over `LEAVING` first.
    rig.reload();
    rig.advance(LEAVING);
    rig.store.put(&note("n2", 5, EDITED, "")).expect("put");
    rig.reload();
    let bare = rig.paint(&mut frame, floor, Pointing::default());
    assert_eq!(bare.cell(left, y).symbol(), "✎");
    assert!(
        bare.cell(left, y)
            .style()
            .add_modifier
            .contains(Modifier::BOLD)
    );
}

#[test]
fn a_persisted_mark_is_bold_where_the_pointers_is_not() {
    // On a palette whose pointer colour carries no modifier, the mark that stays
    // has to hold under `NO_COLOR` and read brighter than a pointer resting on the
    // line, and bold is both.
    let scratch = fixture("notes-bold");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    rig.theme = Theme::dark();
    assert!(
        !rig.theme.bar_hover.add_modifier.contains(Modifier::BOLD),
        "the dark palette's pointer is bold, so this compares nothing"
    );
    let plain = rig.paint(&mut frame, PANE, Pointing::default());
    let y = plain.row_of(EDITED);
    let (left, _, origin) = plain.gutter();
    let hovering = rig.paint(
        &mut frame,
        PANE,
        Pointing {
            hovered: Some(Hovered::Gutter(y)),
            ..Pointing::default()
        },
    );
    let icon = (left..origin)
        .find(|x| hovering.cell(*x, y).symbol() == "✎")
        .expect("the icon");
    assert!(
        !hovering
            .cell(icon, y)
            .style()
            .add_modifier
            .contains(Modifier::BOLD),
        "the pointer's mark is bold, so it is as loud as a note"
    );

    rig.store.put(&note("n1", 5, EDITED, "short")).expect("put");
    rig.reload();
    let noted = rig.paint(&mut frame, PANE, Pointing::default());
    assert_eq!(noted.cell(icon, y).symbol(), "5");
    assert!(
        noted
            .cell(icon, y)
            .style()
            .add_modifier
            .contains(Modifier::BOLD)
    );
    assert_eq!(noted.fg(icon, y), rig.theme.bar_hover.fg);
    // And the noted number differs from a plain one by more than colour, which is
    // what a palette with no colour is left with.
    assert_ne!(
        noted.cell(icon, y).style().add_modifier,
        plain.cell(icon, y).style().add_modifier
    );
}

#[test]
fn a_note_whose_line_is_off_screen_draws_nothing_and_stays_counted() {
    let scratch = Scratch::new("notes-off-screen");
    scratch.write(PATH, numbered_lines(200));
    scratch.commit_all("baseline");
    for line in (4..200).step_by(10) {
        scratch.edit_line(PATH, line, &format!("changed {}", line + 1));
    }
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    rig.store
        .put(&note("n1", 195, "changed 195", "far below"))
        .expect("put");
    rig.reload();
    let painted = rig.paint(&mut frame, PANE, Pointing::default());

    assert!(
        !painted.rows().iter().any(|row| row.contains("far below")),
        "a note whose line is below the fold was drawn"
    );
    assert!(painted.view.notes.marked.is_empty());
    assert_eq!(
        painted.view.notes.adrift, 0,
        "an off-screen line is not adrift"
    );
    assert!(painted.footer().contains("1 note"), "{}", painted.footer());
}

#[test]
fn two_notes_on_one_line_draw_both_and_the_box_reopens_the_first() {
    // Two panes on one worktree can each write the same line before either sees
    // the other's note. The line then carries both, the press reopens the first
    // in the box, and the second keeps its rows under it.
    let scratch = fixture("notes-two-on-one");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    rig.store.put(&note("n1", 5, EDITED, "")).expect("put");
    rig.store
        .put(&note("n2", 5, EDITED, "second"))
        .expect("put");
    rig.reload();
    let painted = rig.paint(&mut frame, PANE, Pointing::default());
    let y = painted.row_of(EDITED);
    let (left, _, origin) = painted.gutter();

    let under = painted.notes_under(y);
    assert_eq!(under.len(), 2, "{under:?}");
    assert!(under[0].trim_end().ends_with("open") && under[1].starts_with("second"));
    // One of the two has a body, so the line keeps its number rather than the icon.
    assert!((left..origin).any(|x| painted.cell(x, y).symbol() == "5"));
    assert_eq!(
        painted
            .view
            .marked_at(usize::from(y - painted.laid.diff.top))
            .len(),
        2
    );

    assert!(rig.press_opens(&painted, left, y));
    assert_eq!(rig.app.note_box().expect("the box").over(), Some("n1"));
    let opened = rig.paint(&mut frame, PANE, Pointing::default());
    let rows = opened.box_under(y);
    assert_eq!(rows.len(), 3, "{rows:?}");
    let second = opened.notes_under(y + 3);
    assert_eq!(second.len(), 1, "{second:?}");
    assert!(second[0].starts_with("second"), "{second:?}");
    // The box's own cells, with a note's rows drawn right under them: the
    // effect runs over the box and never over the note that outlived it.
    let over = box_cells(&opened.laid, &opened.view).expect("the box's cells");
    assert_eq!(
        (over.y, over.height),
        (y + 1, 3),
        "the box's cells reach past its own rows onto the note below"
    );
    rig.esc();
    assert_eq!(files_in(rig.store.dir()).len(), 2);
}

#[test]
fn a_press_while_the_box_is_leaving_reopens_the_note_it_held() {
    // Esc takes the keys back at once and the rows stay a beat while they go.
    // A press landing in that beat opens the box again, and the line it lands
    // on is marked by the box rather than by the note under it, so without
    // asking the box the press would find nothing and Enter would write a
    // second note on a line that already has one.
    let scratch = fixture("notes-press-while-leaving");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    rig.store.put(&note("n1", 5, EDITED, BODY)).expect("put");
    rig.reload();
    let noted = rig.paint(&mut frame, PANE, Pointing::default());
    let y = noted.row_of(EDITED);
    let (left, _, _) = noted.gutter();

    assert!(rig.press_opens(&noted, left + 1, y));
    assert_eq!(rig.app.note_box().expect("the box").over(), Some("n1"));
    rig.esc();
    // Halfway out: the keys are the pane's, the rows are still drawn.
    rig.advance(BOX_ARRIVING / 2);
    let leaving = rig.paint(&mut frame, PANE, Pointing::default());
    assert!(!rig.app.box_open(), "Esc left the keys with the box");
    assert!(
        leaving.box_under(y).len() > 2,
        "the rows left with the keys: {:?}",
        leaving.box_under(y)
    );

    assert!(rig.press_opens(&leaving, left + 1, y));
    let open = rig.app.note_box().expect("the box");
    assert_eq!(
        open.over(),
        Some("n1"),
        "the press opened a box over nothing while the first was still leaving"
    );
    assert_eq!(open.lines().join(" "), BODY);

    // And Enter rewrites the one note rather than writing a second beside it.
    rig.type_text(" again");
    assert_eq!(rig.enter(), Committed::Rewritten("n1".to_owned()));
    assert_eq!(
        files_in(rig.store.dir()).len(),
        1,
        "the line carries two notes where the reader wrote one"
    );
}

#[test]
fn a_caret_above_a_grown_box_stays_on_screen_on_a_pane_that_cannot_hold_it() {
    // The box scrolls its body to the caret, and the pane can be shorter than
    // the box: then the window shows the caret's row rather than the box's
    // last, since a reader who cannot see the caret cannot see what they type.
    let scratch = fixture("notes-caret-short-pane");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    // Six rows of pane leaves the diff about three, against a box of six.
    let tiny = Rect::new(0, 0, 80, 6);
    let plain = rig.paint(&mut frame, tiny, Pointing::default());
    let y = plain
        .view
        .rows
        .iter()
        .position(|row| matches!(row, Row::Line { .. }))
        .map(|at| plain.laid.diff.top + at as u16)
        .expect("a content row on the tiny pane");
    let (left, _, _) = plain.gutter();
    assert!(rig.press_opens(&plain, left + 1, y));

    // Five lines, then the caret moved off the last of them.
    for line in ["one", "two", "three", "four", "five"] {
        rig.type_text(line);
        let newline = Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT));
        assert!(matches!(rig.key(&newline), BoxRoute::Edit(_)));
    }
    rig.type_text("six");
    for _ in 0..3 {
        let up = Event::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert!(matches!(rig.key(&up), BoxRoute::Edit(_)));
    }
    let (line, _) = rig.app.note_box().expect("the box").cursor();
    assert_eq!(line, 2, "the caret did not move off the last line");

    let painted = rig.paint(&mut frame, tiny, Pointing::default());
    let carets = painted
        .view
        .rows
        .iter()
        .filter(|row| {
            matches!(
                row,
                Row::Box {
                    part: BoxPart::Body { caret: Some(_), .. }
                }
            )
        })
        .count();
    assert_eq!(
        carets,
        1,
        "the caret's row is not on a pane too short for the whole box:\n{}",
        painted.rows().join("\n")
    );
}

#[test]
fn a_press_on_a_line_whose_note_is_departing_opens_an_empty_box() {
    // The pane still draws a resolved note while it leaves, so its line is
    // still a target. The box does not reopen it: the agent has answered it,
    // and its text is on its way off the screen rather than back into an editor.
    let scratch = fixture("notes-press-departing");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    rig.store.put(&note("n1", 5, EDITED, BODY)).expect("put");
    rig.reload();
    let noted = rig.paint(&mut frame, PANE, Pointing::default());
    let y = noted.row_of(EDITED);
    let (left, _, _) = noted.gutter();

    rig.agent()
        .rewrite(&left_as("n1", BODY, Status::Resolved, Some(REPLY)))
        .expect("rewrite");
    rig.reload();
    let departing = rig.paint(&mut frame, PANE, Pointing::default());
    assert!(
        !departing.notes_under(y).is_empty(),
        "the resolved note left before its departure ran"
    );

    assert!(rig.press_opens(&departing, left + 1, y));
    let open = rig.app.note_box().expect("the box");
    assert_eq!(
        open.over(),
        None,
        "the box reopened a note the agent resolved"
    );
    assert_eq!(
        open.body(),
        "",
        "the resolved note's words went into the box"
    );
    // And Enter writes a note of its own beside the resolve rather than over it.
    rig.type_text("a second look");
    let written = match rig.enter() {
        Committed::Written(id) => id,
        other => panic!("Enter did not write a new note: {other:?}"),
    };
    assert_ne!(written, "n1");
    assert_eq!(
        rig.store
            .get("n1")
            .expect("get")
            .expect("the resolve")
            .reply
            .as_deref(),
        Some(REPLY)
    );
}

#[test]
fn a_note_rows_lead_never_overwrites_its_word() {
    // The word is drawn first at the right edge and the lead is bounded by what
    // it leaves, so a pane too narrow for both drops the lead and never the
    // reader's status.
    let scratch = fixture("notes-narrow-word");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    rig.store.put(&note("n1", 5, EDITED, "")).expect("put");
    rig.reload();
    let mut drawn = 0;
    for width in 6..=20u16 {
        let painted = rig.paint(&mut frame, Rect::new(0, 0, width, 24), Pointing::default());
        // An empty body is one row at every width: the word alone, never a blank
        // row bought for a gap the body does not have.
        let note_rows = painted
            .view
            .rows
            .iter()
            .filter(|row| matches!(row, Row::Note { .. }))
            .count();
        assert!(
            note_rows <= 1,
            "at {width} columns an empty body took {note_rows} rows"
        );
        for (offset, row) in painted.view.rows.iter().enumerate() {
            let Row::Note {
                word: Some(word), ..
            } = row
            else {
                continue;
            };
            let text = painted.text(painted.laid.diff.top + offset as u16);
            let trimmed = text.trim_end();
            if trimmed.ends_with(word) {
                drawn += 1;
            } else {
                assert!(
                    !trimmed.contains(&word[1..]),
                    "at {width} columns the lead overwrote the word: {text:?}"
                );
            }
        }
    }
    assert!(
        drawn > 0,
        "no width drew the word, so nothing here was checked"
    );
}

#[test]
fn the_word_takes_a_row_of_its_own_when_the_body_leaves_it_none() {
    // A body that fills its last row to the column would push the word off the
    // edge, or the word would cut the body; neither happens, the word moves down.
    let scratch = fixture("notes-word-row");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    let plain = rig.paint(&mut frame, PANE, Pointing::default());
    let y = plain.row_of(EDITED);
    let (_, _, origin) = plain.gutter();
    // The room a note's text has: the row's text columns less the lead. Measured
    // off a painted note rather than derived, so the fixture follows the layout.
    rig.store.put(&note("probe", 5, EDITED, "x")).expect("put");
    rig.reload();
    let probe = rig.paint(&mut frame, PANE, Pointing::default());
    let right = probe.text(y + 1).trim_end().chars().count();
    let room = right - usize::from(origin) - 2;
    rig.store.remove("probe").expect("remove");
    // Gone from the store without a press, so it leaves over `LEAVING` first.
    rig.reload();
    rig.advance(LEAVING);

    // Exactly the room, so the word cannot share the row.
    let full = "y".repeat(room);
    rig.store.put(&note("n1", 5, EDITED, &full)).expect("put");
    rig.reload();
    let painted = rig.paint(&mut frame, PANE, Pointing::default());
    let under = painted.notes_under(y);
    assert_eq!(under.len(), 2, "{under:?}");
    assert_eq!(
        under[0].trim_end(),
        full,
        "the body was cut or wrapped early"
    );
    assert_eq!(
        under[1].trim(),
        "open",
        "the word did not take a row of its own"
    );

    // One column short, and the word shares the row again, a blank between them.
    let short = "y".repeat(room - 5);
    rig.store.put(&note("n1", 5, EDITED, &short)).expect("put");
    rig.reload();
    let painted = rig.paint(&mut frame, PANE, Pointing::default());
    let under = painted.notes_under(y);
    assert_eq!(under.len(), 1, "{under:?}");
    assert!(
        under[0].starts_with(&short) && under[0].trim_end().ends_with(" open"),
        "{:?}",
        under[0]
    );
}

/// The mockup's own reply.
const REPLY: &str = "swapped for saturating_mul; the unwrap_or went with it";

/// A note as the agent leaves it: `status`, and the line when it wrote one.
fn left_as(id: &str, body: &str, status: Status, reply: Option<&str>) -> vigia_core::Note {
    let mut note = note(id, 5, EDITED, body);
    note.status = status;
    note.reply = reply.map(str::to_owned);
    note
}

#[test]
fn a_seen_landing_from_another_handle_crossfades_the_word() {
    let scratch = fixture("notes-seen");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    let dim = rig.theme.chrome_dim.fg.expect("the chrome's dim ink");
    assert!(
        vigia::note_arrival(&rig.theme).is_some(),
        "the default palette has nothing to fade between, so this gate would \
         pass on a word that simply changed"
    );
    rig.store.put(&note("n1", 5, EDITED, BODY)).expect("put");
    rig.reload();
    let open = rig.paint(&mut frame, PANE, Pointing::default());
    let y = open.row_of(EDITED);
    assert!(open.notes_under(y)[1].trim_end().ends_with("open"));

    // The agent lists the store: the file is rewritten under the pane's hand and
    // the wake reads it back.
    rig.agent()
        .rewrite(&left_as("n1", BODY, Status::Seen, None))
        .expect("rewrite");
    rig.reload();
    rig.advance(ARRIVING_FRAME);
    let arriving = rig.paint(&mut frame, PANE, Pointing::default());
    let under = arriving.notes_under(y);
    assert!(under[1].trim_end().ends_with("seen"), "{under:?}");
    let cells = note_cells(&arriving.laid, &arriving.view);
    let word = cells[0].word.expect("the word's cells");
    assert!(
        (word.x..word.right()).all(|x| arriving.fg(x, word.y) != Some(dim)),
        "one frame into the crossfade the word is drawn in the chrome's dim, so \
         the agent's reading arrived without arriving"
    );
    // Nothing else on the row moved: the body keeps the chrome's dim.
    let (_, _, origin) = arriving.gutter();
    assert_eq!(arriving.fg(origin + 2, word.y), Some(dim));

    rig.advance(RESOLVE_ARRIVING);
    let settled = rig.paint(&mut frame, PANE, Pointing::default());
    assert!(
        (word.x..word.right()).all(|x| settled.fg(x, word.y) == Some(dim)),
        "the crossfade ran its length and the word did not settle on the chrome's dim"
    );
    assert!(
        !rig.effects.is_running(),
        "the crossfade is still holding the frame clock past its own length"
    );
}

#[test]
fn a_resolve_runs_the_departure_once_and_the_rows_are_gone_after_it() {
    let scratch = fixture("notes-resolve");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    let dim = rig.theme.chrome_dim.fg.expect("the chrome's dim ink");
    let plain = rig.paint(&mut frame, PANE, Pointing::default());
    let y = plain.row_of(EDITED);
    assert!(plain.text(y + 1).contains("line 6"));

    rig.store
        .put(&left_as("n1", "short", Status::Seen, None))
        .expect("put");
    rig.reload();
    let noted = rig.paint(&mut frame, PANE, Pointing::default());
    assert_eq!(noted.notes_under(y).len(), 1);
    assert!(noted.text(y + 2).contains("line 6"));

    // The agent resolves it: the rows become the agent's line, arriving.
    rig.agent()
        .rewrite(&left_as("n1", "short", Status::Resolved, Some(REPLY)))
        .expect("rewrite");
    rig.reload();
    let arriving = rig.paint(&mut frame, PANE, Pointing::default());
    let (_, _, origin) = arriving.gutter();
    let under = arriving.notes_under(y);
    assert_eq!(under.len(), 1, "{under:?}");
    assert!(under[0].starts_with("swapped for"), "{:?}", under[0]);
    assert_eq!(
        arriving.text(y + 1).chars().nth(usize::from(origin)),
        Some('↳')
    );
    assert_ne!(
        arriving.fg(origin + 2, y + 1),
        Some(dim),
        "the agent's line landed in the chrome's dim rather than arriving"
    );
    assert!(arriving.text(y + 2).contains("line 6"));

    // The beat: the line holds, readable, in the chrome's dim.
    rig.advance(RESOLVE_ARRIVING + ARRIVING_FRAME);
    let holding = rig.paint(&mut frame, PANE, Pointing::default());
    assert_eq!(holding.fg(origin + 2, y + 1), Some(dim));
    assert_eq!(holding.notes_under(y), under);

    // The dissolve: halfway through, the line is going and the diff has not
    // closed up yet.
    rig.advance(RESOLVE_BEAT + LEAVING / 2);
    let dissolving = rig.paint(&mut frame, PANE, Pointing::default());
    assert_ne!(
        dissolving.text(y + 1),
        arriving.text(y + 1),
        "halfway through the dissolve the agent's line is drawn whole"
    );
    assert!(
        dissolving.text(y + 2).contains("line 6"),
        "the diff closed up before the departure ended"
    );

    // The frame after: the rows are dropped and the diff is back where it was.
    rig.advance(LEAVING / 2);
    let gone = rig.paint(&mut frame, PANE, Pointing::default());
    assert_eq!(
        gone.rows(),
        plain.rows(),
        "the departed note left something drawn"
    );
    assert!(!rig.effects.is_running());

    // Once: the file is still in the store until the server prunes it, and a
    // listing that holds it does not run the departure again.
    assert_eq!(files_in(rig.store.dir()).len(), 1);
    rig.reload();
    let again = rig.paint(&mut frame, PANE, Pointing::default());
    assert_eq!(again.rows(), plain.rows(), "a resolved note departed twice");
    assert!(!rig.effects.is_running());
}

#[test]
fn a_withdrawal_departs_without_the_agents_line_and_leaves_no_file() {
    let scratch = fixture("notes-withdraw-departs");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    let plain = rig.paint(&mut frame, PANE, Pointing::default());
    let y = plain.row_of(EDITED);
    let (left, _, origin) = plain.gutter();
    rig.store.put(&note("n1", 5, EDITED, BODY)).expect("put");
    rig.reload();
    let noted = rig.paint(&mut frame, PANE, Pointing::default());
    let rows = noted.notes_under(y);
    assert_eq!(rows.len(), 2, "{rows:?}");

    // The emptied box on Enter deletes the file on the spot, and the rows leave
    // over `LEAVING`.
    assert!(rig.press_opens(&noted, left + 1, y));
    rig.erase();
    assert_eq!(rig.enter(), Committed::Withdrawn("n1".to_owned()));
    assert!(
        files_in(rig.store.dir()).is_empty(),
        "the file outlived the withdrawal"
    );
    let leaving = rig.paint(&mut frame, PANE, Pointing::default());
    assert_eq!(
        leaving.notes_under(y),
        rows,
        "the rows snapped away on the click"
    );
    assert!(
        !leaving.drew_reply(origin),
        "a withdrawal drew a line from the agent"
    );

    rig.advance(LEAVING / 2);
    let dissolving = rig.paint(&mut frame, PANE, Pointing::default());
    assert_ne!(
        dissolving.text(y + 1),
        leaving.text(y + 1),
        "halfway through the dissolve the reader's words are drawn whole"
    );
    assert!(dissolving.text(y + 3).contains("line 6"));

    rig.advance(LEAVING / 2);
    let gone = rig.paint(&mut frame, PANE, Pointing::default());
    assert_eq!(
        gone.rows(),
        plain.rows(),
        "the withdrawn note left something drawn"
    );
    assert!(!rig.effects.is_running());
}

#[test]
fn a_note_that_vanished_from_the_store_departs_the_way_a_withdrawal_does() {
    let scratch = fixture("notes-vanished");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    let plain = rig.paint(&mut frame, PANE, Pointing::default());
    let y = plain.row_of(EDITED);
    let (_, _, origin) = plain.gutter();
    rig.store
        .put(&left_as("n1", BODY, Status::Seen, None))
        .expect("put");
    rig.reload();
    let noted = rig.paint(&mut frame, PANE, Pointing::default());
    let rows = noted.notes_under(y);
    assert_eq!(rows.len(), 2, "{rows:?}");

    // Pruned by the server, or withdrawn from another pane: the pane never saw
    // it resolved and cannot know the agent's line, so it leaves as a withdrawal.
    rig.agent().remove("n1").expect("remove");
    rig.reload();
    let leaving = rig.paint(&mut frame, PANE, Pointing::default());
    assert_eq!(
        leaving.notes_under(y),
        rows,
        "the rows snapped away on the wake"
    );
    assert!(
        !leaving.drew_reply(origin),
        "a vanished note drew a line from the agent"
    );
    rig.advance(LEAVING);
    let gone = rig.paint(&mut frame, PANE, Pointing::default());
    assert_eq!(
        gone.rows(),
        plain.rows(),
        "the vanished note left something drawn"
    );
}

#[test]
fn a_resolve_off_screen_departs_unseen_and_the_count_follows() {
    let scratch = fixture("notes-off-screen");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    // Adrift: its file is not in the diff, so it is drawn nowhere and counted.
    let mut adrift = note("n1", 5, EDITED, "short");
    adrift.path = "src/other.rs".to_owned();
    rig.store.put(&adrift).expect("put");
    rig.reload();
    let counted = rig.paint(&mut frame, PANE, Pointing::default());
    assert!(
        counted.footer().contains("1 note"),
        "{:?}",
        counted.footer()
    );
    assert!(
        counted.footer().contains("1 adrift"),
        "{:?}",
        counted.footer()
    );
    let before = counted.rows();

    adrift.status = Status::Resolved;
    adrift.reply = Some(REPLY.to_owned());
    rig.agent().rewrite(&adrift).expect("rewrite");
    rig.reload();
    rig.advance(ARRIVING_FRAME);
    let unseen = rig.paint(&mut frame, PANE, Pointing::default());
    assert_eq!(
        unseen.rows(),
        before,
        "a resolve on a note drawn nowhere changed the screen"
    );
    assert!(
        !unseen.rows().iter().any(|row| row.contains(REPLY)),
        "the agent's line was drawn for a note with no row to draw it under"
    );

    // The departure runs its length unseen; then the note is dropped and the
    // count follows, and the effect nobody could see holds nothing.
    rig.advance(RESOLVED_DEPARTURE);
    let after = rig.paint(&mut frame, PANE, Pointing::default());
    assert!(!after.footer().contains("note"), "{:?}", after.footer());
    assert!(
        !rig.effects.is_running(),
        "an effect over a note drawn nowhere is still holding the frame clock"
    );
}

#[test]
fn a_resolved_note_met_at_startup_runs_the_departure() {
    let scratch = fixture("notes-startup");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    let plain = rig.paint(&mut frame, PANE, Pointing::default());
    let y = plain.row_of(EDITED);
    let (_, _, origin) = plain.gutter();

    // Resolved overnight, before this pane ever listed it.
    rig.agent()
        .put(&left_as("n1", "short", Status::Resolved, Some(REPLY)))
        .expect("put");
    rig.reload();
    let arriving = rig.paint(&mut frame, PANE, Pointing::default());
    assert_eq!(
        arriving.text(y + 1).chars().nth(usize::from(origin)),
        Some('↳')
    );
    assert!(arriving.notes_under(y)[0].starts_with("swapped for"));
    rig.advance(RESOLVED_DEPARTURE);
    let gone = rig.paint(&mut frame, PANE, Pointing::default());
    assert_eq!(gone.rows(), plain.rows());
}

#[test]
fn a_listing_cannot_bring_back_a_note_already_departing() {
    let scratch = fixture("notes-race");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    let plain = rig.paint(&mut frame, PANE, Pointing::default());
    let y = plain.row_of(EDITED);
    let (left, _, origin) = plain.gutter();
    rig.store.put(&note("n1", 5, EDITED, BODY)).expect("put");
    rig.reload();
    let noted = rig.paint(&mut frame, PANE, Pointing::default());
    let rows = noted.notes_under(y);
    assert!(rig.press_opens(&noted, left + 1, y));
    rig.erase();
    assert_eq!(rig.enter(), Committed::Withdrawn("n1".to_owned()));

    // The agent's resolve lands after the withdrawal began: the store holds the
    // file again, and the pane keeps drawing the departure it started.
    rig.agent()
        .put(&left_as("n1", BODY, Status::Resolved, Some(REPLY)))
        .expect("put");
    rig.reload();
    rig.advance(ARRIVING_FRAME);
    let still = rig.paint(&mut frame, PANE, Pointing::default());
    assert!(
        !still.drew_reply(origin),
        "a listing brought the agent's line onto rows already leaving"
    );
    assert_eq!(still.notes_under(y).len(), rows.len());

    // Once the departure has ended, the store is the truth again.
    rig.advance(LEAVING);
    let gone = rig.paint(&mut frame, PANE, Pointing::default());
    assert_eq!(gone.rows(), plain.rows());
    rig.reload();
    let truth = rig.paint(&mut frame, PANE, Pointing::default());
    assert_eq!(
        truth.text(y + 1).chars().nth(usize::from(origin)),
        Some('↳')
    );
}

#[test]
fn the_departed_set_follows_the_store() {
    let scratch = fixture("notes-departed");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    let plain = rig.paint(&mut frame, PANE, Pointing::default());
    let y = plain.row_of(EDITED);
    rig.agent()
        .put(&left_as("n1", "short", Status::Resolved, Some(REPLY)))
        .expect("put");
    rig.reload();
    rig.advance(RESOLVED_DEPARTURE);
    assert_eq!(
        rig.paint(&mut frame, PANE, Pointing::default()).rows(),
        plain.rows()
    );
    // Listed again, still resolved: hidden, since it has departed.
    rig.reload();
    assert_eq!(
        rig.paint(&mut frame, PANE, Pointing::default()).rows(),
        plain.rows()
    );

    // Pruned, then the id is reused for a new note: it draws, because the set
    // of departed ids holds only what the store still does.
    rig.agent().remove("n1").expect("remove");
    rig.reload();
    rig.store.put(&note("n1", 5, EDITED, "again")).expect("put");
    rig.reload();
    let reused = rig.paint(&mut frame, PANE, Pointing::default());
    let under = reused.notes_under(y);
    assert_eq!(under.len(), 1, "{under:?}");
    assert!(under[0].starts_with("again"), "{:?}", under[0]);
}

#[test]
fn note_cells_cover_the_rows_and_the_word_and_never_the_bar() {
    let scratch = fixture("notes-cells");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    rig.store
        .put(&left_as("n1", BODY, Status::Seen, Some(REPLY)))
        .expect("put");
    rig.reload();
    // The third pane is short enough that the bar is drawn and narrow enough to
    // have no trailing margin, so only the bar's own narrowing keeps the rows
    // off its column and the clause below is exercised rather than skipped.
    let short = Rect::new(0, 0, 40, 20);
    for pane in [PANE, NARROW, short] {
        let painted = rig.paint(&mut frame, pane, Pointing::default());
        if pane == short {
            assert!(
                painted.laid.diff.bar.is_some(),
                "the short pane drew no bar, so nothing here checks the rows stop short of one"
            );
        }
        // A needle the forty-column pane does not cut.
        let y = painted.row_of("margin.checked_mul");
        let (_, _, origin) = painted.gutter();
        let under = painted.notes_under(y);
        let cells = note_cells(&painted.laid, &painted.view);
        assert_eq!(cells.len(), 1, "{cells:?}");
        let cells = &cells[0];
        assert_eq!(cells.id, "n1");
        assert_eq!(
            cells.rows.x, origin,
            "the rows do not start at the content origin"
        );
        assert_eq!(cells.rows.y, y + 1);
        assert_eq!(
            usize::from(cells.rows.height),
            under.len(),
            "the rows do not cover every row the note drew"
        );
        // The word, spelled by the cells the rect names and nothing beside them.
        let word = cells.word.expect("the word's cells");
        let spelled: String = (word.x..word.right())
            .map(|x| painted.cell(x, word.y).symbol().to_owned())
            .collect();
        assert_eq!(spelled, "seen", "at {} columns", pane.width);
        assert_eq!(painted.cell(word.x - 1, word.y).symbol(), " ");
        // The agent's line: the rows under the word, from the arrow on.
        let reply = cells.reply.expect("the reply's cells");
        assert_eq!(reply.y, word.y + 1);
        assert_eq!(reply.bottom(), cells.rows.bottom());
        assert_eq!(painted.cell(reply.x, reply.y).symbol(), "↳");
        // And the row's right edge stops short of the bar and the margin: the
        // cell past it is never a glyph of the note.
        assert!(
            cells.rows.right() <= pane.width,
            "the rows run past the pane at {} columns",
            pane.width
        );
        if let Some(bar) = painted.laid.diff.bar {
            assert!(
                cells.rows.right() <= bar,
                "the rows reach the bar's column at {} columns",
                pane.width
            );
        }
        for row in cells.rows.y..cells.rows.bottom() {
            if cells.rows.right() < pane.width {
                let past = painted.cell(cells.rows.right(), row).symbol();
                assert!(
                    past == " " || past == "│" || past == "█",
                    "the cell past the rows' right edge holds {past:?} on row {row}"
                );
            }
        }
    }
}

#[test]
fn the_listing_alert_is_said_once_per_change() {
    use vigia_core::Listing;
    let torn = (std::path::PathBuf::from("a.note"), "torn".to_owned());
    let newer = (std::path::PathBuf::from("b.note"), "newer".to_owned());
    let skipping = |files: &[(std::path::PathBuf, String)]| {
        Ok(Listing {
            notes: Vec::new(),
            skipped: files.to_vec(),
        })
    };
    let mut alerts = Alerts::default();
    assert_eq!(
        alerts.of(&skipping(std::slice::from_ref(&torn))),
        Some("skipped the note file a.note: torn".to_owned())
    );
    assert_eq!(
        alerts.of(&skipping(std::slice::from_ref(&torn))),
        None,
        "the same torn file was said again on the next wake"
    );
    assert_eq!(
        alerts.of(&skipping(&[torn.clone(), newer.clone()])),
        Some("skipped the note file a.note and 1 more: torn".to_owned())
    );
    assert_eq!(
        alerts.of(&skipping(&[])),
        None,
        "files that read again are not news"
    );
    assert_eq!(
        alerts.of(&skipping(std::slice::from_ref(&newer))),
        Some("skipped the note file b.note: newer".to_owned()),
        "a file torn again after reading whole is news again"
    );
    let differently = (newer.0.clone(), "torn differently".to_owned());
    assert_eq!(
        alerts.of(&skipping(std::slice::from_ref(&differently))),
        Some("skipped the note file b.note: torn differently".to_owned()),
        "the same file skipped for a new reason was not news"
    );
    // The store lists in the directory's order, which a write beside the files
    // can move; the same files in another order are not news.
    assert_eq!(
        alerts.of(&skipping(&[differently.clone(), torn.clone()])),
        Some("skipped the note file a.note and 1 more: torn".to_owned()),
        "the sorted set names its first file first"
    );
    assert_eq!(
        alerts.of(&skipping(&[torn.clone(), differently.clone()])),
        None,
        "the same skipped files in another order were said again"
    );

    // A store that cannot be read at all: a file where its directory should be.
    let scratch = fixture("notes-unreadable-listing");
    let root = TempDir::new("notes-unreadable-listing-state");
    let store = Store::open(root.path(), scratch.root()).expect("open the store");
    fs::write(store.dir(), b"not a directory").expect("block the store's directory");
    let failed = store.list();
    assert!(
        failed.is_err(),
        "a file where the directory should be listed"
    );
    let first = alerts
        .of(&failed)
        .expect("a store that cannot be read is news");
    assert!(first.starts_with("could not read the notes: "), "{first:?}");
    assert_eq!(
        alerts.of(&store.list()),
        None,
        "the same failure was said again on the next wake"
    );
    fs::remove_file(store.dir()).expect("unblock the store's directory");
    assert_eq!(
        alerts.of(&store.list()),
        None,
        "a store that reads whole again is not news"
    );
    fs::write(store.dir(), b"not a directory").expect("block it again");
    assert_eq!(
        alerts.of(&store.list()),
        Some(first),
        "a store that fails again after reading whole is news again"
    );
}

#[test]
fn a_reply_landing_on_an_open_note_crossfades_in() {
    let scratch = fixture("notes-replied");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    let dim = rig.theme.chrome_dim.fg.expect("the chrome's dim ink");
    rig.store.put(&note("n1", 5, EDITED, "short")).expect("put");
    rig.reload();
    let open = rig.paint(&mut frame, PANE, Pointing::default());
    let y = open.row_of(EDITED);
    let (_, _, origin) = open.gutter();
    assert_eq!(open.notes_under(y).len(), 1);

    // The agent answers without closing the note: the line arrives under it.
    rig.agent()
        .rewrite(&left_as("n1", "short", Status::Seen, Some(REPLY)))
        .expect("rewrite");
    rig.reload();
    rig.advance(ARRIVING_FRAME);
    let arriving = rig.paint(&mut frame, PANE, Pointing::default());
    let under = arriving.notes_under(y);
    assert_eq!(under.len(), 2, "{under:?}");
    assert!(under[1].starts_with("swapped for"), "{:?}", under[1]);
    let cells = note_cells(&arriving.laid, &arriving.view);
    let reply = cells[0].reply.expect("the reply's cells");
    assert_ne!(
        arriving.fg(reply.x + 2, reply.y),
        Some(dim),
        "one frame in, the agent's line is drawn in the chrome's dim rather than arriving"
    );
    // The note's own rows do not move: the body keeps the chrome's dim.
    assert_eq!(arriving.fg(origin + 2, y + 1), Some(dim));

    rig.advance(RESOLVE_ARRIVING);
    let settled = rig.paint(&mut frame, PANE, Pointing::default());
    assert_eq!(settled.fg(reply.x + 2, reply.y), Some(dim));
    assert!(!rig.effects.is_running());
}

#[test]
fn a_resolve_between_a_stale_view_and_an_emptied_box_survives() {
    let scratch = fixture("notes-stale-withdraw");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    rig.store.put(&note("n1", 5, EDITED, "short")).expect("put");
    rig.reload();
    let marked = rig.paint(&mut frame, PANE, Pointing::default());
    let y = marked.row_of(EDITED);
    let (left, _, origin) = marked.gutter();

    // The agent resolves it while the screen still shows it open, the reader's
    // press lands on that screen, and the box is emptied and sent.
    rig.agent()
        .rewrite(&left_as("n1", "short", Status::Resolved, Some(REPLY)))
        .expect("rewrite");
    assert!(rig.press_opens(&marked, left + 1, y));
    rig.erase();
    assert_eq!(
        rig.enter(),
        Committed::Nothing,
        "the emptied box withdrew a note the agent had already resolved"
    );
    assert_eq!(
        files_in(rig.store.dir()).len(),
        1,
        "Enter deleted the agent's resolve and its line"
    );
    let departing = rig.paint(&mut frame, PANE, Pointing::default());
    assert_eq!(
        departing.text(y + 1).chars().nth(usize::from(origin)),
        Some('↳'),
        "the resolve that landed first is not what the next frame shows"
    );

    // And an emptied box over a note nobody resolved still withdraws it, so the
    // case above is the resolve being honoured and not an Enter that removes
    // nothing.
    rig.store
        .put(&note("n2", 6, "line 6", "short"))
        .expect("put");
    rig.reload();
    let open = rig.paint(&mut frame, PANE, Pointing::default());
    let six = open.row_of("line 6");
    assert!(rig.press_opens(&open, left + 1, six));
    rig.erase();
    assert_eq!(rig.enter(), Committed::Withdrawn("n2".to_owned()));
    assert_eq!(
        files_in(rig.store.dir()).len(),
        1,
        "the emptied box over an open note left its file behind"
    );
}

#[test]
fn a_resolve_landing_inside_a_crossfade_supersedes_it() {
    use vigia::Change;
    let now = Instant::now();
    let theme = Theme::default();
    let mut effects = NoteEffects::default();
    effects.arm(vec![Change::Seen("n1".to_owned())], &theme, now);
    effects.arm(vec![Change::Replied("n1".to_owned())], &theme, now);
    assert_eq!(
        effects.live(),
        2,
        "a word and a line arrive on their own cells"
    );
    effects.arm(vec![Change::Replied("n1".to_owned())], &theme, now);
    assert_eq!(
        effects.live(),
        2,
        "a line arriving again replaced nothing, so two effects draw one cell"
    );
    effects.arm(vec![Change::Resolved("n1".to_owned())], &theme, now);
    assert_eq!(
        effects.live(),
        1,
        "a resolve arrived over a word or a line still arriving, so two effects draw one cell"
    );
    effects.arm(vec![Change::Seen("n2".to_owned())], &theme, now);
    assert_eq!(effects.live(), 2, "another note's effect was evicted");
    effects.arm(vec![Change::Seen("n1".to_owned())], &theme, now);
    assert_eq!(
        effects.live(),
        2,
        "a word arriving over a whole note already leaving was armed beside it"
    );

    // And the one left on n1 is the whole note's: at its first frame the fade
    // holds every cell of the rows in the announcement's ink, the body's
    // included, where a word's or a line's effect would reach the word alone.
    let from = theme.note.fg.expect("the announcement's ink");
    let rows = Rect::new(0, 0, 20, 2);
    let cells = vec![vigia::NoteCells {
        id: "n1".to_owned(),
        rows,
        word: Some(Rect::new(16, 0, 4, 1)),
        reply: Some(Rect::new(0, 1, 20, 1)),
    }];
    let mut buf = ratatui::buffer::Buffer::empty(rows);
    buf.set_string(
        0,
        0,
        "the reader's own  seen",
        ratatui::style::Style::default(),
    );
    buf.set_string(0, 1, "the agent's line", ratatui::style::Style::default());
    effects.draw(Duration::ZERO, &mut buf, &cells);
    assert_eq!(
        buf[(0, 0)].style().fg,
        Some(from),
        "the effect left on n1 does not cover its body, so the narrower one survived"
    );
}

#[test]
fn a_second_reply_on_a_still_open_note_crossfades_too() {
    use vigia::Change;
    let now = Instant::now();
    let mut ledger = Ledger::default();
    ledger.reload(vec![note("n1", 5, EDITED, "short")], now);
    assert_eq!(
        ledger.reload(
            vec![left_as("n1", "short", Status::Seen, Some("first"))],
            now
        ),
        vec![
            Change::Seen("n1".to_owned()),
            Change::Replied("n1".to_owned())
        ]
    );
    assert_eq!(
        ledger.reload(
            vec![left_as("n1", "short", Status::Seen, Some("second"))],
            now
        ),
        vec![Change::Replied("n1".to_owned())],
        "a line the agent rewrote popped into place rather than arriving"
    );
    assert!(
        ledger
            .reload(
                vec![left_as("n1", "short", Status::Seen, Some("second"))],
                now
            )
            .is_empty(),
        "a line that did not change was drawn arriving again"
    );
}

#[test]
fn a_note_that_replies_and_resolves_in_one_wake_departs_once() {
    // Two things moved in one wake, and only the departure is armed: the
    // agent's line arrives inside it, not once for the reply and once again.
    use vigia::Change;
    let now = Instant::now();
    let mut ledger = Ledger::default();
    assert!(
        ledger
            .reload(vec![note("n1", 5, EDITED, "short")], now)
            .is_empty(),
        "a note first met moved nothing"
    );
    let changes = ledger.reload(
        vec![left_as("n1", "short", Status::Resolved, Some(REPLY))],
        now,
    );
    assert_eq!(changes, vec![Change::Resolved("n1".to_owned())]);
}
