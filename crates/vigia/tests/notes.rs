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
    Change, Committed, Glyphs, Hovered, LEAVING, Ledger, NoteCount, NoteEffects, NoteLead,
    Pointing, RESOLVE_ARRIVING, RESOLVE_BEAT, RESOLVED_DEPARTURE, Region, Regions, Row, Theme,
    Timed, View, Viewport, WORD_INSET, body_layout, box_cells, box_entrance, box_exit, box_route,
    commit, count_cell, edge_at, effect_interval, has_room, hover_after, note_cells, opening,
    press_at, regions, render, repainted, selection_after, withdraw,
};
use vigia_core::{ChangeKind, Frame, Highlighter, History, Side, Status, Store, key};

use support::{Scratch, TempDir, files_in, note, numbered_lines};

const PANE: Rect = Rect::new(0, 0, 80, 24);
const NARROW: Rect = Rect::new(0, 0, 40, 24);
/// Tall enough to draw both runs of one path at once, which the gates over a
/// path in both runs assert before they read a placement.
const TALL: Rect = Rect::new(0, 0, 80, 40);
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

/// One committed file staged at `staged` and then edited further at line ten,
/// so the path is two entries: the staged run holds the earlier hunk and the
/// unstaged one the later.
fn in_both_runs(name: &str, staged: usize, text: &str) -> Scratch {
    let scratch = Scratch::new(name);
    scratch.write(PATH, numbered_lines(12));
    scratch.commit_all("baseline");
    scratch.edit_line(PATH, staged, text);
    scratch.git(&["add", PATH]);
    scratch.edit_line(PATH, 9, "unstaged ten");
    scratch
}

/// The shade blocks an arriving surface evolves through.
const SHADES: [&str; 4] = ["░", "▒", "▓", "█"];

/// Cells of `over` holding a shade block rather than what the renderer drew.
fn shaded(painted: &Painted, over: Rect) -> usize {
    (over.y..over.bottom())
        .flat_map(|row| (over.x..over.right()).map(move |x| (x, row)))
        .filter(|(x, row)| SHADES.contains(&painted.cell(*x, *row).symbol()))
        .count()
}

/// The same over every row a note drew under `y`, at any width.
fn shading(painted: &Painted, y: u16) -> usize {
    let width = painted.backend.buffer().area.width;
    let under = y + 1;
    shaded(
        painted,
        Rect::new(
            0,
            under,
            width,
            painted.after_notes(y).saturating_sub(under),
        ),
    )
}

/// Whether `row` is one of note `id`'s own.
fn is_note(row: &Row, id: &str) -> bool {
    matches!(row, Row::Note { id: at, .. } if at == id)
}

/// Rows note `id` drew on `painted`, wherever in the frame they landed.
fn note_rows(painted: &Painted, id: &str) -> usize {
    painted
        .view
        .rows
        .iter()
        .filter(|row| is_note(row, id))
        .count()
}

/// The same rows by screen row, for a gate that presses one of them.
fn side_rows(painted: &Painted, id: &str) -> Vec<u16> {
    painted
        .view
        .rows
        .iter()
        .enumerate()
        .filter(|(_, row)| is_note(row, id))
        .map(|(offset, _)| painted.laid.diff.top + offset as u16)
        .collect()
}

/// How many times note `id` is drawn on `painted`.
///
/// Its rows are contiguous, so this counts the places they begin rather than the
/// rows they take: the enclosure's height follows the body's wrap, and what
/// every caller is asking is whether the note was placed once, twice or not at
/// all.
fn note_blocks(painted: &Painted, id: &str) -> usize {
    let drawn = |row: Option<&Row>| row.is_some_and(|row| is_note(row, id));
    painted
        .view
        .rows
        .iter()
        .enumerate()
        .filter(|(n, row)| {
            drawn(Some(row))
                && !drawn(
                    n.checked_sub(1)
                        .and_then(|above| painted.view.rows.get(above)),
                )
        })
        .count()
}

/// File headings on `painted`, which both gates below assert is two before they
/// read a placement: one entry off screen makes a single placement look right
/// for the wrong reason.
fn headings(painted: &Painted) -> usize {
    painted
        .view
        .rows
        .iter()
        .filter(|row| matches!(row, Row::File(_)))
        .count()
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
    /// would: beats that ran out arm their sweep, departures that ended are
    /// dropped and spent effects retired.
    fn advance(&mut self, by: Duration) {
        self.clock += by;
        self.elapsed += by;
        // Read before the settle, because the settle is what arms the sweep and
        // the question is what was drawing before it did. Everything the shell
        // counts as drawing, the box included, or a sweep armed beside a box
        // still leaving is told the whole beat passed and ends in one frame.
        let ran =
            self.effects.is_running() || self.box_effect.as_ref().is_some_and(Timed::is_running);
        let settled = self.ledger.settle(self.clock);
        if settled.changed {
            self.app.set_notes(self.ledger.drawn());
        }
        if !settled.sweeping.is_empty() {
            self.effects.arm(
                settled
                    .sweeping
                    .into_iter()
                    .map(Change::Swept)
                    .collect::<Vec<_>>(),
                &self.theme,
                self.clock,
            );
            // `effect_interval`'s rule, in this rig's terms. The beat is a
            // stretch with nothing drawing, so a sweep armed at its end has
            // lived through none of it; with something else still running the
            // pane was painting at its cadence and the interval is real.
            self.elapsed = effect_interval(ran, self.elapsed);
        }
        // The shell's own prune: a resolved file is the pane's to remove once
        // the departure it drew has run, and a refusal is an alert there rather
        // than an end, so it is not one here either.
        for id in &settled.prune {
            let _ = self.store.remove(id);
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
        self.box_effect = Some(Timed::armed(box_entrance(&self.theme), self.clock));
        self.advance(BOX_ARRIVING + ARRIVING_FRAME);
        // The entrance was spent by the clock and never drawn, so the next paint
        // tells whatever it arms nothing of that spell, as the shell's
        // `effect_interval` would.
        self.elapsed = Duration::ZERO;
        true
    }

    /// A press on a note's left side, routed the way the loop routes it and read
    /// back through the store the way `Shell::withdraw_note` does. `None` where
    /// the press landed on no note's side, `Some(false)` where the store no
    /// longer held one for the reader to take back.
    fn edge_press(&mut self, painted: &Painted, column: u16, row: u16) -> Option<bool> {
        let id = edge_at(&painted.view, painted.laid, &press(column, row))?;
        let went = withdraw(&self.store, &id).expect("the store refused the removal");
        if went {
            self.reload();
        }
        Some(went)
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
    /// rows stay drawn while the box is swept away.
    fn esc(&mut self) {
        self.app.close_box(self.clock + BOX_ARRIVING);
        self.box_effect = Some(Timed::armed(box_exit(), self.clock));
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
    /// The status word, read off whichever surface carries it: the enclosure's
    /// bottom edge, or the last row of the bar rung on a pane too narrow to
    /// hold an enclosure. Drawn cells rather than the row model, so a word that
    /// stopped being painted fails here rather than passing.
    fn note_word(&self, y: u16) -> String {
        let mut last = String::new();
        for row in y + 1..self.after_notes(y) {
            match self.lead_at(row) {
                Some(NoteLead::Bottom) => return self.text(row).trim_end().to_owned(),
                Some(NoteLead::Bar) => last = self.text(row).trim_end().to_owned(),
                _ => {}
            }
        }
        last
    }

    /// The row under `y` carrying the answer's arrow.
    ///
    /// Found rather than counted: the enclosure's height follows the body's
    /// wrap, so an offset from the noted line is an off-by-one waiting for the
    /// next fixture whose body is a word longer.
    fn reply_row(&self, y: u16) -> u16 {
        (y + 1..self.after_notes(y))
            .find(|row| self.lead_at(*row) == Some(NoteLead::Reply))
            .unwrap_or_else(|| {
                panic!(
                    "no answer under row {y}:
{}",
                    self.rows().join(
                        "
"
                    )
                )
            })
    }

    /// The first row under `y` that is not one of its note's, which is where
    /// the diff picks up again.
    fn after_notes(&self, y: u16) -> u16 {
        let floor = self.laid.diff.top + self.laid.diff.rows;
        (y + 1..floor)
            .find(|row| self.lead_at(*row).is_none())
            .unwrap_or(floor)
    }

    /// What `row` of the diff region draws, when a note drew it.
    fn lead_at(&self, row: u16) -> Option<NoteLead> {
        match self.view.rows.get(usize::from(row - self.laid.diff.top)) {
            Some(Row::Note { lead, .. }) => Some(*lead),
            _ => None,
        }
    }

    /// What a note says under `y`: the reader's own words and the agent's, with
    /// the enclosure around the first left out.
    ///
    /// The frame is not content and every caller here is asking what the note
    /// reads. `a_committed_note_is_enclosed_and_the_word_rides_the_bottom_edge`
    /// is what holds the edges themselves, off the painted cells rather than
    /// through this.
    fn notes_under(&self, y: u16) -> Vec<String> {
        let (_, _, origin) = self.gutter();
        let mut out = Vec::new();
        for row in y + 1..self.after_notes(y) {
            let text = self.text(row);
            let Some(lead) = self.lead_at(row) else {
                break;
            };
            let skip = usize::from(origin) + 2;
            match lead {
                // The edges carry no words of anyone's.
                NoteLead::Top | NoteLead::Bottom => {}
                NoteLead::Body => {
                    // Between the two sides, and the trailing one goes with the
                    // padding it stands in.
                    let inner: String = text.chars().skip(skip).collect();
                    out.push(inner.trim_end().trim_end_matches('│').trim_end().to_owned());
                }
                NoteLead::Bar | NoteLead::Reply | NoteLead::Blank => {
                    out.push(text.chars().skip(skip).collect::<String>());
                }
            }
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
        Some(Hovered::NoteEdge(y)),
        "the first content column answered as the gutter"
    );
    // And the column past it answers nothing at all: a drag says where it is
    // going as it goes, so a mark before it would be the second thing saying so.
    assert_eq!(
        painted.laid.hover_at(origin + 1, y),
        None,
        "a content column the pointer only drags from answered"
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
    // The frame and its labels in the note's own ink, so nothing reads as code
    // and a palette can colour the surface without moving the pane's furniture.
    let frame = Theme::default().note_frame.fg;
    assert_eq!(opened.fg(origin, y + 1), frame);
    assert_eq!(opened.fg(origin + 3, y + 3), frame);
    // The line keeps the note's ink while the box is open, so the box can be
    // traced to it from across the pane.
    let five = (left..origin)
        .find(|x| opened.cell(*x, y).symbol() == "5")
        .expect("the anchored line's number");
    assert_eq!(opened.fg(five, y), Theme::default().note_line.fg);
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

    // The rows stand under the line where the box was, evolving in: shade
    // blocks first, the reader's words once the arrival has run.
    let opened = rig.paint(&mut frame, PANE, Pointing::default());
    assert!(opened.box_under(y).is_empty(), "the box is still drawn");
    rig.advance(RESOLVE_ARRIVING / 2);
    let arriving = rig.paint(&mut frame, PANE, Pointing::default());
    assert!(
        shading(&arriving, y) > 0,
        "the rows arrived drawn rather than evolving:
{}",
        arriving.rows().join(
            "
"
        )
    );
    rig.advance(RESOLVE_ARRIVING);
    let settled = rig.paint(&mut frame, PANE, Pointing::default());
    let under = settled.notes_under(y);
    assert_eq!(
        rejoined(&under, "open"),
        BODY,
        "the rows do not carry the body"
    );
    assert!(settled.note_word(y).contains("open"));
    // And the rows settle on what the renderer drew, in the ink the box showed
    // the reader's words in while they were being typed.
    rig.advance(ARRIVING_FRAME);
    let settled = rig.paint(&mut frame, PANE, Pointing::default());
    assert_eq!(
        shading(&settled, y),
        0,
        "the arrival is still shading the rows"
    );
    assert_eq!(settled.fg(origin + 2, y + 2), rig.theme.chrome.fg);
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
    assert_eq!(reopened.fg(five, y), Theme::default().note_line.fg);

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

    rig.advance(RESOLVE_ARRIVING);
    let drawn = rig.paint(&mut frame, PANE, Pointing::default());
    let under = drawn.notes_under(y);
    assert!(under[0].starts_with("short, the settle one"), "{under:?}");
    assert!(drawn.note_word(y).contains("open"), "{under:?}");
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
    rig.advance(RESOLVE_ARRIVING);
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
    // way a heading's does. Twenty-two columns rather than twenty since the box
    // stopped being drawn two columns wider than the rows it is built from: this
    // rung is the same content, read on the pane that now holds it.
    let tight = rig.paint(&mut frame, Rect::new(0, 0, 22, 24), Pointing::default());
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
fn a_notes_body_fills_the_enclosure_it_is_drawn_in() {
    // Reported on #474 from a fifteen-column pane: the body broke after `checked`
    // with two columns standing empty at the end of every row, because the wrap
    // was sized by one expression and the frame by another. Sixty rows so nothing
    // scrolls, which is the screen the two parted on.
    let narrow = Rect::new(0, 0, 15, 60);
    let scratch = fixture("notes-enclosure-fills");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    rig.store.put(&note("n1", 5, EDITED, BODY)).expect("put");
    rig.reload();
    let painted = rig.paint(&mut frame, narrow, Pointing::default());
    assert!(
        painted.laid.diff.bar.is_none(),
        "the fixture scrolls, and a barred screen is where the two expressions \
         already agreed"
    );
    // The line, not the note: the body opens with `checked_mul` too, and a row
    // anchored on that finds the note's first row instead of the line above it.
    let y = painted.row_of("margin");
    let top = painted.text(y + 1);
    // Columns, not bytes. A corner is three bytes of UTF-8 and so is every rule
    // between them, so `str::find` measures this enclosure at three times its width.
    let column = |glyph: char| top.chars().position(|drawn| drawn == glyph);
    let left = column('┌').unwrap_or_else(|| {
        let screen = painted.rows().join("\n");
        panic!("no enclosure under the line:\n{screen}")
    });
    let right = column('┐').expect("the enclosure drew no top-right corner");
    // The columns between the two rules, less the rule and its space each side.
    let inner = right - left + 1 - BOX_FRAME;
    let body = painted.notes_under(y);
    assert!(
        body.len() > 2,
        "the body did not wrap on a fifteen-column pane: {body:?}"
    );

    // Some row reaches the far side. A greedy wrapper that broke one column early
    // would leave every row short of it, which is the reported complaint, and a
    // per-row check cannot say so: this body wraps inside `checked_mul`, where
    // there is no space, so "the next word would have fitted" is never false there
    // whatever the wrap was sized at.
    let widest = body
        .iter()
        .map(|row| row.trim_end().chars().count())
        .max()
        .unwrap_or(0);
    assert_eq!(
        widest, inner,
        "the widest row of the enclosure is {widest} columns inside a box drawn \
         for {inner}, so the body was wrapped for a narrower box than it is in"
    );
}

#[test]
fn a_pane_that_says_it_has_room_for_a_box_draws_one() {
    // `notes::has_room` asks the width the pointer is told about and `box_rows`
    // asks the width the walk wrapped at. A band where the two disagreed let a
    // press take the keys while nothing on screen said where they had gone,
    // which is the one thing `has_room`'s own docblock exists to prevent.
    let scratch = fixture("notes-room-agrees");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);

    let mut roomy = 0usize;
    let mut refused = 0usize;
    for width in 1..=60u16 {
        let pane = Rect::new(0, 0, width, 24);
        let painted = rig.paint(&mut frame, pane, Pointing::default());
        if !has_room(painted.laid) {
            refused += 1;
            continue;
        }
        let Some(offset) = painted
            .view
            .rows
            .iter()
            .position(|row| matches!(row, Row::Line { .. }))
        else {
            continue;
        };
        let y = painted.laid.diff.top + offset as u16;
        let (left, _, _) = painted.gutter();
        if !rig.press_opens(&painted, left, y) {
            continue;
        }
        roomy += 1;
        let opened = rig.paint(&mut frame, pane, Pointing::default());
        assert!(
            opened
                .view
                .rows
                .iter()
                .any(|row| matches!(row, Row::Box { .. })),
            "at {width} columns the pane said it had room for the box and drew \
             none of it, so the press took the keys and nothing on screen says so"
        );
        rig.app.take_box();
        rig.box_effect = None;
    }

    assert!(
        roomy > 20 && refused > 0,
        "the sweep opened {roomy} boxes and was refused {refused} times, so it is \
         not reading both sides of the rung"
    );
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
fn a_note_draws_under_its_line_enclosed_with_the_word_on_the_bottom_edge() {
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
    let at = usize::from(origin);
    let theme = Theme::default();

    // The number stays a number and keeps the icon's ink, so the anchored line
    // can be found from across the pane.
    let five = (left..origin)
        .find(|x| painted.cell(*x, y).symbol() == "5")
        .unwrap_or_else(|| {
            panic!(
                "the number was replaced:
{}",
                painted.text(y)
            )
        });
    assert_eq!(painted.fg(five, y), theme.note_line.fg);

    // Four rows under it: an edge, the reader's words between two sides, and an
    // edge carrying the word. The gutter behind all four is blank.
    let under = painted.notes_under(y);
    assert_eq!(under.len(), 2, "{under:?}");
    let rows: Vec<String> = (y + 1..=y + 4).map(|row| painted.text(row)).collect();
    for text in &rows {
        assert!(
            text.chars().take(at).all(|c| c == ' '),
            "the gutter under a note row is not blank: {text:?}"
        );
    }
    assert_eq!(rows[0].chars().nth(at), Some('┌'), "{:?}", rows[0]);
    assert!(rows[0].trim_end().ends_with('┐'), "{:?}", rows[0]);
    assert_eq!(
        painted.fg(origin, y + 1),
        theme.note_open.fg,
        "the enclosure is not in the open state's ink"
    );
    for row in y + 2..=y + 3 {
        assert_eq!(painted.text(row).chars().nth(at), Some('│'));
        assert_eq!(
            painted.fg(origin, row),
            theme.note_open.fg,
            "the enclosure's side is not in the state's ink"
        );
        // The reader's own words, in the ink the box showed them in while they
        // were being typed: committing them does not demote them.
        assert_eq!(
            painted.fg(origin + 2, row),
            theme.chrome.fg,
            "the body is dimmer committed than it was typed"
        );
    }
    let bottom = &rows[3];
    assert_eq!(bottom.chars().nth(at), Some('└'), "{bottom:?}");
    assert!(bottom.trim_end().ends_with('┘'), "{bottom:?}");
    assert!(
        painted.note_word(y).contains("open"),
        "the word is not on the surface that carries it: {:?}",
        painted.note_word(y)
    );
    // Prose wraps at a blank, so the first row ends on a whole word, and nothing
    // of it is lost.
    let first = under[0].trim_end();
    assert!(
        !first.is_empty() && BODY.starts_with(first) && BODY.as_bytes()[first.len()] == b' ',
        "the first row broke inside a word: {first:?}"
    );
    assert_eq!(rejoined(&under, "open"), BODY);
    // The word rides the bottom edge, set in from the corner, so it reads as a
    // label on the frame rather than the frame ending. Counted in characters:
    // the rule and the corners are three bytes each.
    let edge: Vec<char> = bottom.trim_end().chars().collect();
    let word_at = edge
        .windows(4)
        .position(|four| four.iter().copied().eq("open".chars()))
        .unwrap_or_else(|| panic!("the word is not on the bottom edge: {bottom:?}"));
    assert!(
        word_at > at + 1 && word_at + 4 + WORD_INSET < edge.len(),
        "the word does not ride the bottom edge clear of both corners: {bottom:?}"
    );
    assert!(
        painted.text(y + 5).contains("line 6"),
        "{}",
        painted.text(y + 5)
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
    assert!(
        narrow.note_word(y).contains("open"),
        "the word is not on the surface that carries it: {:?}",
        narrow.note_word(y)
    );
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
    assert!(under[0].starts_with("short"), "{:?}", under[0]);
    assert!(
        painted.note_word(y).contains("open"),
        "{:?}",
        painted.note_word(y)
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
    assert!(
        painted.note_word(y).contains("changed"),
        "the word is not on the surface that carries it: {:?}",
        painted.note_word(y)
    );
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
    assert!(
        painted.note_word(heading).contains("gone"),
        "the word is not on the surface that carries it: {:?}",
        painted.note_word(heading)
    );
    assert!(
        painted.text(painted.after_notes(heading)).contains("@@"),
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
fn a_note_on_a_line_only_the_staged_diff_holds_draws_under_that_run_alone() {
    let scratch = in_both_runs("notes-runs-staged", 2, "staged three");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.show_staged(true);
    frame.advance().expect("advance");
    assert_eq!(
        frame.files().len(),
        2,
        "the fixture is not a path in both runs"
    );
    let mut rig = Rig::open(&scratch);
    rig.store
        .put(&note("n1", 3, "staged three", "only the staged run"))
        .expect("put");
    rig.reload();

    let painted = rig.paint(&mut frame, TALL, Pointing::default());
    assert_eq!(headings(&painted), 2, "{}", painted.rows().join("\n"));

    let y = painted.row_of("staged three");
    let under = painted.notes_under(y);
    assert_eq!(under.len(), 1, "{under:?}");
    assert!(
        under[0].starts_with("only the staged run"),
        "{:?}",
        under[0]
    );
    assert_eq!(
        note_blocks(&painted, "n1"),
        1,
        "{}",
        painted.rows().join("\n")
    );
    assert_eq!(painted.view.notes.marked.len(), 1);
    assert_eq!(
        painted.view.notes.marked[0].row,
        usize::from(y - painted.laid.diff.top)
    );
    assert!(
        !painted.rows().iter().any(|row| row.contains("gone")),
        "the run that does not hold the line said gone about it:\n{}",
        painted.rows().join("\n")
    );
    assert_eq!(painted.view.notes.adrift, 0);
}

#[test]
fn a_line_both_runs_hold_takes_its_note_under_the_unstaged_run() {
    let scratch = in_both_runs("notes-runs-tie", 5, "staged six");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.show_staged(true);
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    rig.store
        .put(&note("n1", 8, "line 8", "context in both"))
        .expect("put");
    rig.reload();

    let painted = rig.paint(&mut frame, TALL, Pointing::default());
    assert_eq!(headings(&painted), 2, "{}", painted.rows().join("\n"));
    let drawn: Vec<u16> = (painted.laid.diff.top..painted.laid.diff.top + painted.laid.diff.rows)
        .filter(|y| painted.text(*y).contains("line 8"))
        .collect();
    assert_eq!(drawn.len(), 2, "line 8 is not context in both hunks");

    let under = painted.notes_under(drawn[0]);
    assert_eq!(under.len(), 1, "{under:?}");
    assert!(under[0].starts_with("context in both"), "{:?}", under[0]);
    assert!(painted.notes_under(drawn[1]).is_empty());
    assert_eq!(
        note_blocks(&painted, "n1"),
        1,
        "{}",
        painted.rows().join("\n")
    );
    assert_eq!(painted.view.notes.marked.len(), 1);
    assert_eq!(
        painted.view.notes.marked[0].row,
        usize::from(drawn[0] - painted.laid.diff.top)
    );
}

#[test]
fn a_line_neither_run_holds_draws_its_note_once_under_the_unstaged_heading() {
    // Line 1 is outside the staged hunk at 3 to 9 and the unstaged one at 7 to 12,
    // so neither run resolves it and the earlier run takes it.
    let scratch = in_both_runs("notes-runs-gone", 5, "staged six");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.show_staged(true);
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    rig.store
        .put(&note("n1", 1, "line 1", "in neither hunk"))
        .expect("put");
    rig.reload();

    let painted = rig.paint(&mut frame, TALL, Pointing::default());
    assert_eq!(headings(&painted), 2, "{}", painted.rows().join("\n"));
    assert_eq!(
        note_blocks(&painted, "n1"),
        1,
        "{}",
        painted.rows().join("\n")
    );
    let heading = painted.row_of("src/watch.rs");
    let under = painted.notes_under(heading);
    assert_eq!(under.len(), 1, "{under:?}");
    assert!(under[0].starts_with("in neither hunk"), "{:?}", under[0]);
    assert!(
        painted.note_word(heading).contains("gone"),
        "{:?}",
        painted.note_word(heading)
    );
    assert!(
        painted.view.notes.marked.is_empty(),
        "a gone note marked a line"
    );
    assert_eq!(painted.view.notes.adrift, 0, "the file is in the diff");
}

#[test]
fn a_note_whose_text_one_run_moved_and_the_other_edited_over_goes_where_the_text_is() {
    // The index holds `line 8` one row down, where a staged insert put it; the
    // working tree then edits that row, so the stored number is drawn with other
    // text there. Moved outranks changed, which is the rung neither gate above
    // reaches.
    let scratch = Scratch::new("notes-runs-moved");
    let lines: Vec<String> = (1..=12).map(|i| format!("line {i}")).collect();
    scratch.write(PATH, format!("{}\n", lines.join("\n")));
    scratch.commit_all("baseline");
    let mut staged = lines.clone();
    staged.insert(6, "inserted".to_owned());
    scratch.write(PATH, format!("{}\n", staged.join("\n")));
    scratch.git(&["add", PATH]);
    let mut edited = staged.clone();
    assert_eq!(edited[8], "line 8", "the fixture moved the wrong row");
    edited[8] = "edited".to_owned();
    scratch.write(PATH, format!("{}\n", edited.join("\n")));
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.show_staged(true);
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    rig.store
        .put(&note("n1", 8, "line 8", "follows its text"))
        .expect("put");
    rig.reload();

    let painted = rig.paint(&mut frame, TALL, Pointing::default());
    assert_eq!(headings(&painted), 2, "{}", painted.rows().join("\n"));
    assert_eq!(
        note_blocks(&painted, "n1"),
        1,
        "{}",
        painted.rows().join("\n")
    );
    let at = painted
        .view
        .rows
        .iter()
        .position(|row| matches!(row, Row::Note { .. }))
        .expect("a note row");
    let staged_from = painted
        .view
        .rows
        .iter()
        .enumerate()
        .filter(|(_, row)| matches!(row, Row::File(_)))
        .nth(1)
        .map(|(index, _)| index)
        .expect("a second heading");
    assert!(
        at > staged_from,
        "the note landed in the run that edited over its line:\n{}",
        painted.rows().join("\n")
    );
    assert!(
        matches!(&painted.view.rows[at - 1], Row::Line { number: 9, text, .. } if text == "line 8"),
        "{:?}",
        painted.view.rows[at - 1]
    );
}

/// Both occurrences of a line both runs hold, in row order. The two are drawn
/// with the same number and the same text, so nothing but the row the press
/// landed on says which run the reader meant.
fn both_occurrences(painted: &Painted, needle: &str) -> Vec<u16> {
    let diff = painted.laid.diff;
    let drawn: Vec<u16> = (diff.top..diff.top + diff.rows)
        .filter(|y| painted.text(*y).contains(needle))
        .collect();
    assert_eq!(drawn.len(), 2, "{needle} is not drawn in both runs");
    drawn
}

#[test]
fn the_box_opens_under_whichever_run_was_pressed() {
    for occurrence in [0, 1] {
        let scratch = in_both_runs(&format!("notes-runs-box-{occurrence}"), 5, "staged six");
        let worktree = scratch.worktree();
        let mut frame = worktree.frame();
        frame.show_staged(true);
        frame.advance().expect("advance");
        let mut rig = Rig::open(&scratch);
        let painted = rig.paint(&mut frame, TALL, Pointing::default());
        assert_eq!(headings(&painted), 2, "{}", painted.rows().join("\n"));
        let y = both_occurrences(&painted, "line 8")[occurrence];
        let (left, _, _) = painted.gutter();

        assert!(rig.press_opens(&painted, left + 1, y));
        let opened = rig.paint(&mut frame, TALL, Pointing::default());
        assert_eq!(
            opened.view.notes.boxed,
            Some(usize::from(y - opened.laid.diff.top)),
            "a press on occurrence {occurrence} opened the box on the other run"
        );
    }
}

#[test]
fn the_box_keeps_the_note_it_holds_when_a_later_edit_moves_the_rank() {
    // Line 8 is context in both hunks, so the note ties and takes the unstaged
    // run, and the reader reopens it there. The agent then edits line 8 in the
    // working tree: the note's stored text stops resolving in the unstaged run
    // and still resolves in the staged one, so its rank moves while the box
    // stays where the reader is typing.
    let scratch = in_both_runs("notes-runs-box-rank", 5, "staged six");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.show_staged(true);
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    rig.store
        .put(&note("n1", 8, "line 8", "reopened here"))
        .expect("put");
    rig.reload();

    let painted = rig.paint(&mut frame, TALL, Pointing::default());
    let y = both_occurrences(&painted, "line 8")[0];
    let (left, _, _) = painted.gutter();
    assert_eq!(
        painted
            .view
            .marked_at(usize::from(y - painted.laid.diff.top)),
        ["n1"]
    );
    assert!(rig.press_opens(&painted, left + 1, y));
    assert_eq!(rig.app.note_box().expect("the box").over(), Some("n1"));

    scratch.edit_line(PATH, 7, "the agent edited eight");
    frame.advance().expect("advance after the edit");
    let after = rig.paint(&mut frame, TALL, Pointing::default());
    assert_eq!(
        note_blocks(&after, "n1"),
        0,
        "the note the box holds drew its rows in the other run:\n{}",
        after.rows().join("\n")
    );
    let tops = after
        .view
        .rows
        .iter()
        .filter(|row| {
            matches!(
                row,
                Row::Box {
                    part: BoxPart::Top { .. }
                }
            )
        })
        .count();
    assert_eq!(tops, 1, "{}", after.rows().join("\n"));
}

#[test]
fn the_box_on_a_line_both_runs_hold_opens_under_the_run_pressed() {
    let scratch = in_both_runs("notes-runs-box", 5, "staged six");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.show_staged(true);
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    let painted = rig.paint(&mut frame, TALL, Pointing::default());
    assert_eq!(headings(&painted), 2, "{}", painted.rows().join("\n"));
    let y = painted.row_of("line 8");
    let (left, _, _) = painted.gutter();

    assert!(rig.press_opens(&painted, left + 1, y));
    let opened = rig.paint(&mut frame, TALL, Pointing::default());
    let tops = opened
        .view
        .rows
        .iter()
        .filter(|row| {
            matches!(
                row,
                Row::Box {
                    part: BoxPart::Top { .. }
                }
            )
        })
        .count();
    assert_eq!(tops, 1, "{}", opened.rows().join("\n"));
    assert_eq!(
        opened.view.notes.boxed,
        Some(usize::from(y - opened.laid.diff.top)),
        "the box left the line the press was on"
    );
    let rows = opened.box_under(y);
    assert!(rows[0].contains("src/watch.rs:8"), "{rows:?}");
}

#[test]
fn a_note_whose_path_a_rename_carried_into_the_other_run_goes_with_its_line() {
    // The staged run renamed the file and edited it; the working tree then put a
    // new file back at the old name. Both runs answer to that name, one through
    // the rename's source, and only the staged run still holds the note's line.
    let scratch = Scratch::new("notes-runs-rename");
    scratch.write("src/a.rs", numbered_lines(30));
    scratch.commit_all("baseline");
    scratch.git(&["mv", "src/a.rs", "src/b.rs"]);
    scratch.edit_line("src/b.rs", 7, "staged eight");
    scratch.git(&["add", "src/b.rs"]);
    scratch.write("src/a.rs", "a brand new file that took the old name\n");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.show_staged(true);
    frame.advance().expect("advance");
    let answering = frame
        .files()
        .iter()
        .filter(|change| change.paths().any(|path| path == "src/a.rs"))
        .count();
    assert_eq!(
        answering, 2,
        "the fixture is not a path answered by both runs"
    );

    let mut rig = Rig::open(&scratch);
    let mut pinned = note("n1", 5, "line 5", "went with the rename");
    pinned.path = "src/a.rs".to_owned();
    rig.store.put(&pinned).expect("put");
    rig.reload();

    let painted = rig.paint(&mut frame, TALL, Pointing::default());
    assert_eq!(painted.view.notes.adrift, 0, "the path is in the diff");
    assert_eq!(
        note_blocks(&painted, "n1"),
        1,
        "{}",
        painted.rows().join("\n")
    );
    let y = painted.row_of("line 5");
    let under = painted.notes_under(y);
    assert_eq!(under.len(), 1, "{under:?}");
    assert!(
        under[0].starts_with("went with the rename"),
        "{:?}",
        under[0]
    );
}

#[test]
fn a_note_the_box_holds_stands_aside_in_the_run_the_box_is_not_drawn_in() {
    // The box is opened on the unstaged run's line 8 and the reader then scrolls
    // into the staged run, which draws the same line. The box has gone off screen
    // with the row it is anchored to, and the note it holds does not take the
    // chance to draw itself in the run that is left.
    let scratch = in_both_runs("notes-runs-box-scrolled", 5, "staged six");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.show_staged(true);
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    rig.store
        .put(&note("n1", 8, "line 8", "held by the box"))
        .expect("put");
    rig.reload();

    let painted = rig.paint(&mut frame, TALL, Pointing::default());
    let y = both_occurrences(&painted, "line 8")[0];
    let (left, _, _) = painted.gutter();
    assert!(rig.press_opens(&painted, left + 1, y));
    assert_eq!(rig.app.note_box().expect("the box").over(), Some("n1"));

    let height = body_layout(
        TALL,
        &rig.app.chrome("fixture", None, Pointing::default(), 0, ""),
        2,
        2,
    )
    .diff;
    rig.app
        .apply(Action::Scroll(500), &mut frame, height)
        .expect("scroll into the staged run");
    let scrolled = rig.paint(&mut frame, TALL, Pointing::default());
    assert!(
        scrolled.rows().iter().any(|row| row.contains("staged six")),
        "the scroll did not reach the staged run:\n{}",
        scrolled.rows().join("\n")
    );
    assert_eq!(
        note_blocks(&scrolled, "n1"),
        0,
        "the note the box holds drew itself in the other run:\n{}",
        scrolled.rows().join("\n")
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
    assert!(
        painted.note_word(heading).contains("gone"),
        "the word is not on the surface that carries it: {:?}",
        painted.note_word(heading)
    );
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
    assert_eq!(hidden.fg(five, y), Theme::default().note_line.fg);
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

/// The reader's note is enclosed, and the answer descends from the enclosure.
///
/// The enclosure is what tells the two speakers apart, so the answer carries no
/// mark down its side: the arrow opens it once in the enclosure's own rule, and
/// the rest is free indented text.
#[test]
fn a_committed_note_is_enclosed_and_the_word_rides_the_bottom_edge() {
    let scratch = fixture("notes-enclosure");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    rig.store.put(&note("n1", 5, EDITED, BODY)).expect("put");
    rig.reload();
    let painted = rig.paint(&mut frame, PANE, Pointing::default());
    let y = painted.row_of(EDITED);
    let (_, _, origin) = painted.gutter();
    let theme = Theme::default();
    let at = |row: u16| painted.text(row).chars().nth(usize::from(origin));

    let under = painted.notes_under(y);
    assert!(
        under.len() >= 2,
        "an enclosed note is at least a top edge, a body row and a bottom edge: {under:?}"
    );

    // The edges, found rather than counted, so the body may wrap to any height.
    let rows: Vec<u16> = (y + 1..=y + 8).collect();
    let top = rows
        .iter()
        .copied()
        .find(|row| matches!(at(*row), Some('╭' | '┌')))
        .unwrap_or_else(|| panic!("no top edge under the line:\n{}", painted.text(y + 1)));
    let bottom = rows
        .iter()
        .copied()
        .find(|row| matches!(at(*row), Some('╰' | '└')))
        .unwrap_or_else(|| panic!("no bottom edge under the line:\n{}", painted.text(y + 2)));
    assert!(bottom > top, "the bottom edge is above the top one");

    // The frame carries the state, which is what the bar carried before it.
    assert_eq!(
        painted.fg(origin, top),
        theme.note_open.fg,
        "the enclosure is not drawn in the state's ink"
    );

    // The word rides the bottom edge, where the box being typed in carries its
    // two keys, rather than taking a row of the body.
    assert!(
        painted.text(bottom).contains("open"),
        "the status word is not on the bottom edge: {:?}",
        painted.text(bottom)
    );
    for row in top + 1..bottom {
        assert!(
            !painted.text(row).contains("open"),
            "the word is still in the body at row {row}: {:?}",
            painted.text(row)
        );
    }
}

/// The agent's answer is not drawn in the reader's own ink.
#[test]
fn the_reply_is_not_the_readers_ink() {
    let scratch = fixture("notes-reply-ink");
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
    let theme = Theme::default();

    let arrow = (y + 1..=y + 8)
        .find(|row| painted.text(*row).contains('↳'))
        .unwrap_or_else(|| panic!("no reply arrow under the note"));
    let column = painted
        .text(arrow)
        .chars()
        .position(|c| c == '↳')
        .map(|c| c as u16)
        .expect("the arrow's column");

    assert_eq!(
        painted.fg(column, arrow),
        theme.note_reply.fg,
        "the arrow is not in the reply's ink"
    );
    assert_ne!(
        painted.fg(column, arrow),
        theme.chrome_dim.fg,
        "the arrow is still the reader's own ink"
    );
    // And the answer's text with it, which is the half the reader reported.
    assert_eq!(
        painted.fg(column + 2, arrow),
        theme.note_reply.fg,
        "the answer's text is not in the reply's ink"
    );
    let _ = origin;
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
    assert!(
        painted.note_word(y).contains("seen"),
        "the word is not on the surface that carries it: {:?}",
        painted.note_word(y)
    );
    let arrow = painted.reply_row(y);
    assert_eq!(
        painted.text(arrow).chars().nth(usize::from(origin)),
        Some('↳'),
        "the answer does not stand in the enclosure's own left rule"
    );
    assert!(
        under[1].starts_with("swapped for saturating_mul"),
        "{:?}",
        under[1]
    );
    assert!(painted.text(painted.after_notes(y)).contains("line 6"));

    // Resolved, the reply alone stays, which is the last frame of the departure
    // the store watch will animate. Read once the answer has evolved in: the
    // first frames of an arrival are shade blocks, which is the arrival.
    let mut resolved = seen.clone();
    resolved.status = Status::Resolved;
    rig.store.put(&resolved).expect("put");
    rig.reload();
    rig.advance(RESOLVE_ARRIVING);
    let departing = rig.paint(&mut frame, PANE, Pointing::default());
    let under = departing.notes_under(departing.row_of(EDITED));
    assert_eq!(under.len(), 1, "{under:?}");
    assert!(under[0].starts_with("swapped for"), "{:?}", under[0]);
}

/// `SPEC.md` §11.1: the enclosure is the same whether or not there is an answer.
///
/// The status word is held at `seen` across both frames, since a note is marked
/// seen when the agent reads it and answered whenever it answers, so the word
/// moving is the state being honest and the enclosure moving is not. Compared
/// character for character, because what was reported is the surface
/// restructuring under an answer rather than any one glyph.
#[test]
fn the_enclosure_does_not_change_when_the_agent_answers() {
    let scratch = fixture("notes-steady");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    let mut seen = note("n1", 5, EDITED, BODY);
    seen.status = Status::Seen;
    rig.store.put(&seen).expect("put");
    rig.reload();
    let painted = rig.paint(&mut frame, PANE, Pointing::default());
    let y = painted.row_of(EDITED);
    let enclosure: Vec<String> = (y + 1..painted.after_notes(y))
        .map(|row| painted.text(row))
        .collect();

    let mut answered = seen.clone();
    answered.reply = Some("swapped for saturating_mul".to_owned());
    rig.store.put(&answered).expect("put");
    rig.reload();
    let painted = rig.paint(&mut frame, PANE, Pointing::default());
    let y = painted.row_of(EDITED);
    let answer = painted.reply_row(y);
    let under: Vec<String> = (y + 1..answer).map(|row| painted.text(row)).collect();

    assert_eq!(
        under, enclosure,
        "the answer restructured the enclosure it arrived under"
    );
}

/// `SPEC.md` §11.1: the answer follows one `↳` in the enclosure's own left rule.
///
/// Three edges are asserted rather than one, because the rule is what the eye
/// takes the arrow to hang from and the rule is drawn by three glyphs down the
/// same column. The answer's text is asserted against the body's for the other
/// half of the report: an answer set in past the words it answers is a third
/// left edge in a stack that reads as one column.
#[test]
fn the_answers_arrow_stands_in_the_enclosures_own_rule() {
    let scratch = fixture("notes-rule");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    let mut seen = note("n1", 5, EDITED, BODY);
    seen.status = Status::Seen;
    seen.reply = Some("swapped for saturating_mul".to_owned());
    rig.store.put(&seen).expect("put");
    rig.reload();
    let painted = rig.paint(&mut frame, PANE, Pointing::default());
    let y = painted.row_of(EDITED);
    let answer = painted.reply_row(y);
    let column = |row: u16, glyph: char| {
        painted
            .text(row)
            .chars()
            .position(|c| c == glyph)
            .unwrap_or_else(|| panic!("row {row} draws no {glyph}: {:?}", painted.text(row)))
    };

    let at = column(answer, '↳');
    assert_eq!(at, column(y + 1, '┌'), "the arrow misses the rule's head");
    assert_eq!(at, column(y + 2, '│'), "the arrow misses the rule itself");
    assert_eq!(
        at,
        column(answer - 1, '└'),
        "the arrow misses the corner it descends from"
    );

    // Past the lead glyph on each row, so what is compared is where the words
    // start rather than what the lead is.
    let words = |row: u16| {
        painted
            .text(row)
            .chars()
            .enumerate()
            .skip(at + 1)
            .find(|(_, c)| !c.is_whitespace())
            .map(|(column, _)| column)
            .unwrap_or_else(|| panic!("row {row} carries no words: {:?}", painted.text(row)))
    };
    assert_eq!(
        words(answer),
        words(y + 2),
        "the answer does not stand in the column the words it answers do"
    );

    // And nothing on the edge above claims to carry the answer instead. A mark
    // there points at a column the arrow does not stand in, which is the defect
    // this replaced, and the enclosure staying the same either way cannot see
    // it: a mark drawn in both states is a mark that never changes.
    let edge = painted.text(answer - 1);
    let bare = edge.trim_end().replacen(" seen ", "", 1);
    let stray = bare
        .chars()
        .skip(at)
        .find(|c| !matches!(c, '└' | '┘' | '╰' | '╯' | '─'));
    assert_eq!(
        stray, None,
        "the bottom edge carries a mark beside its corners, its rule and its word: {edge:?}"
    );
}

/// The answer degrades with the note rather than before it.
///
/// Down the whole ladder the arrow and the bar are drawn together or not at
/// all: an answer that leaves first says the note is unanswered, which is the
/// one thing this surface cannot say wrongly, and it leaves on the narrow panes
/// nobody reads a pane at.
#[test]
fn the_answer_is_drawn_wherever_the_note_itself_is() {
    let scratch = fixture("notes-ladder");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    let mut seen = note("n1", 5, EDITED, "hm");
    seen.status = Status::Seen;
    seen.reply = Some("ok".to_owned());
    rig.store.put(&seen).expect("put");
    rig.reload();

    let mut blank = 0;
    let mut drawn = 0;
    for width in 1..=12u16 {
        let painted = rig.paint(&mut frame, Rect::new(0, 0, width, 40), Pointing::default());
        let mut rows = painted
            .view
            .rows
            .iter()
            .enumerate()
            .filter_map(|(at, row)| {
                let Row::Note { lead, .. } = row else {
                    return None;
                };
                let text = painted.text(painted.laid.diff.top + at as u16);
                Some((*lead, text.trim_end().is_empty()))
            });
        let answer = rows
            .clone()
            .find(|(lead, _)| matches!(lead, NoteLead::Reply))
            .map(|(_, empty)| empty);
        let note = rows
            .find(|(lead, _)| !matches!(lead, NoteLead::Reply | NoteLead::Blank))
            .map(|(_, empty)| empty);
        assert_eq!(
            answer, note,
            "at {width} columns the answer and the note it belongs to are not \
             drawn together"
        );
        match note {
            Some(true) => blank += 1,
            Some(false) => drawn += 1,
            None => panic!("at {width} columns the note drew no row at all"),
        }
    }
    assert!(
        blank > 0 && drawn > 0,
        "the sweep never crossed the floor: {blank} blank against {drawn} drawn"
    );
}

/// The widest word a note carries still fits the edge it rides.
///
/// `resolved` is wider than `changed`, and only a note the agent closed without
/// writing a line carries it while its rows are still drawn, which is why the
/// sweep over a note that stays on screen cannot reach it.
#[test]
fn the_widest_word_still_fits_the_edge_it_rides() {
    let scratch = fixture("notes-widest-edge");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    let mut resolved = note("n1", 5, EDITED, "hm");
    resolved.status = Status::Resolved;
    rig.store.put(&resolved).expect("put");
    rig.reload();
    rig.advance(RESOLVE_ARRIVING);

    let mut enclosed = 0;
    let mut barred = 0;
    for width in 10..=40u16 {
        let painted = rig.paint(&mut frame, Rect::new(0, 0, width, 40), Pointing::default());
        let Some(bottom) = painted.view.rows.iter().position(|row| {
            matches!(
                row,
                Row::Note {
                    lead: NoteLead::Bottom,
                    ..
                }
            )
        }) else {
            barred += 1;
            continue;
        };
        enclosed += 1;
        let edge = painted.text(painted.laid.diff.top + bottom as u16);
        let edge = edge.trim_end();
        assert!(
            edge.contains("resolved") && edge.ends_with(['┘', '╯']),
            "at {width} columns the widest word did not fit its edge: {edge:?}"
        );
    }
    assert!(
        enclosed > 0 && barred > 0,
        "the sweep did not cross the boundary for the widest word: {enclosed} \
         enclosed against {barred} barred"
    );
}

/// The answer's own wrap counts columns, and it wraps where the body does.
///
/// Its width moved with the arrow, so this holds the reply's wrap rather than
/// the body's: a double-width glyph counted as one column would carry a row
/// past the enclosure the answer hangs from.
#[test]
fn a_wide_answer_wraps_in_the_column_the_body_does() {
    let scratch = fixture("notes-wide-answer");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    let mut seen = note("n1", 5, EDITED, BODY);
    seen.status = Status::Seen;
    // Wide enough to wrap at the widest pane in the sweep, so every rung of it
    // exercises a continuation rather than only the narrow ones.
    seen.reply = Some("実装を共有する方法です".repeat(4));
    rig.store.put(&seen).expect("put");
    rig.reload();

    for width in [80u16, 60, 40] {
        let painted = rig.paint(&mut frame, Rect::new(0, 0, width, 40), Pointing::default());
        let at = |want: NoteLead| {
            painted
                .view
                .rows
                .iter()
                .position(|row| matches!(row, Row::Note { lead, .. } if *lead == want))
                .map(|row| painted.text(painted.laid.diff.top + row as u16))
        };
        let edge = at(NoteLead::Bottom)
            .expect("the bottom edge")
            .trim_end()
            .to_owned();
        // The column itself rather than the first thing on the row: a wrapped
        // answer's lead is a blank, so `where the words begin` and `where the
        // ink begins` are the same question only on the rows that have a glyph.
        let (_, _, origin) = painted.gutter();
        let words = usize::from(origin) + 2;
        for lead in [NoteLead::Body, NoteLead::Reply, NoteLead::Blank] {
            let Some(row) = at(lead) else { continue };
            assert!(
                row.chars().nth(words).is_some_and(|c| !c.is_whitespace()),
                "at {width} columns a row's words do not begin in column {words}: {row:?}"
            );
            assert!(
                row.trim_end().chars().count() <= edge.chars().count(),
                "at {width} columns a wide answer runs past the enclosure: {row:?}"
            );
        }
        assert!(
            at(NoteLead::Blank).is_some(),
            "at {width} columns the answer did not wrap, so its continuation is \
             not exercised"
        );
    }
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
    // Once the departure's first movement has run: it evolves the rows in
    // before it holds them, so their first frames are shade blocks.
    rig.advance(RESOLVE_ARRIVING);
    let painted = rig.paint(&mut frame, PANE, Pointing::default());
    let y = painted.row_of(EDITED);
    let under = painted.notes_under(y);
    assert_eq!(under.len(), 1, "{under:?}");
    assert!(under[0].starts_with("short"), "{:?}", under[0]);
    assert!(
        painted.note_word(y).contains("resolved"),
        "the word is not on the surface that carries it: {:?}",
        painted.note_word(y)
    );
    assert!(painted.text(painted.after_notes(y)).contains("line 6"));
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
    assert_eq!(
        note_rows, 4,
        "the note's enclosure and its two body rows are not on the last screen"
    );
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
    // Past the target the content begins, and its first column is the one a
    // note spends on its own left side.
    assert_eq!(
        plain.laid.hover_at(left + columns, y),
        Some(Hovered::NoteEdge(y))
    );
    assert_eq!(plain.laid.hover_at(left + columns + 1, y), None);

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
    assert_eq!(noted.fg(icon, y), rig.theme.note_line.fg);
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
    assert!(
        painted.note_word(y).contains("open") && under[1].starts_with("second"),
        "{under:?}"
    );
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

/// The mockup's own reply.
const REPLY: &str = "swapped for saturating_mul; the unwrap_or went with it";

/// A note as the agent leaves it: `status`, and the line when it wrote one.
fn left_as(id: &str, body: &str, status: Status, reply: Option<&str>) -> vigia_core::Note {
    let mut note = note(id, 5, EDITED, body);
    note.status = status;
    note.reply = reply.map(str::to_owned);
    note
}

/// The enclosure's own side, which every body row closes on.
const SIDE: char = '\u{2502}';

#[test]
fn the_rung_boundary_follows_the_longest_word_and_a_wide_body_stays_inside() {
    // The width the enclosure needs is its frame plus the word riding its
    // bottom edge, so the boundary moves with the word. `resolved` is the
    // longest one a note carries, and a body of double-width characters is what
    // would push a side over if the wrap counted characters rather than columns.
    let scratch = fixture("notes-widest-word");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    // Its stored text is no line of the diff, so it lands on `changed`, which
    // is the longest word a note carries while it stays on screen.
    rig.store
        .put(&note(
            "n1",
            5,
            "a line the file no longer holds",
            "実装を共有する方法",
        ))
        .expect("put");
    rig.reload();
    rig.advance(RESOLVE_ARRIVING);

    let mut enclosed = 0;
    let mut barred = 0;
    for width in 10..=40u16 {
        let painted = rig.paint(&mut frame, Rect::new(0, 0, width, 24), Pointing::default());
        let leads: Vec<NoteLead> = painted
            .view
            .rows
            .iter()
            .filter_map(|row| match row {
                Row::Note { lead, .. } => Some(*lead),
                _ => None,
            })
            .collect();
        if leads.is_empty() {
            continue;
        }
        if !leads.contains(&NoteLead::Top) {
            barred += 1;
            continue;
        }
        enclosed += 1;
        // The widest word still fits its edge, and the edge still closes.
        let bottom = painted
            .view
            .rows
            .iter()
            .position(|row| {
                matches!(
                    row,
                    Row::Note {
                        lead: NoteLead::Bottom,
                        ..
                    }
                )
            })
            .expect("the enclosure's bottom edge");
        let edge = painted.text(painted.laid.diff.top + bottom as u16);
        assert!(
            edge.contains("changed") && edge.trim_end().ends_with(['┘', '╯']),
            "at {width} columns the widest word did not fit its edge: {edge:?}"
        );
        // And every body row closes, which a double-width glyph counted as one
        // column would break.
        for row in painted
            .view
            .rows
            .iter()
            .enumerate()
            .filter(|(_, row)| {
                matches!(
                    row,
                    Row::Note {
                        lead: NoteLead::Body,
                        ..
                    }
                )
            })
            .map(|(at, _)| painted.text(painted.laid.diff.top + at as u16))
        {
            let drawn = row.trim_end();
            assert!(
                drawn.ends_with(SIDE),
                "at {width} columns a body row of wide glyphs does not close: {row:?}"
            );
            // And it is exactly as wide as its own edge. A double-width glyph
            // counted as one column would carry the side out past the corner,
            // or wrap early and leave the row short.
            assert_eq!(
                drawn.chars().count(),
                edge.trim_end().chars().count(),
                "at {width} columns a body row of wide glyphs is not as wide as \
                 its edge: {row:?}"
            );
        }
    }
    assert!(
        enclosed > 0 && barred > 0,
        "the sweep did not cross the boundary for the widest word: {enclosed} \
         enclosed against {barred} barred"
    );
}

#[test]
fn on_the_bar_rung_a_full_row_pushes_the_word_onto_its_own() {
    // The rung under the enclosure keeps the behaviour the enclosure made
    // unnecessary: there the word shares the reader's last row, so a body that
    // fills that row would either be cut or push the word off the edge. Neither
    // happens; the word moves down, and the body keeps every character.
    let scratch = fixture("notes-bar-word-row");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);

    // Narrow enough that the enclosure gives way to the bar.
    let narrow = Rect::new(0, 0, 14, 24);
    // By the rows rather than by the line's text, which this pane cuts.
    let noted = |painted: &Painted| {
        let first = painted
            .view
            .rows
            .iter()
            .position(|row| matches!(row, Row::Note { .. }))
            .expect("a note row");
        painted.laid.diff.top + first as u16 - 1
    };

    // The body grows a character at a time until the word gives up the row.
    // Measured rather than derived, so the fixture follows the layout.
    let rows_at = |rig: &mut Rig, frame: &mut Frame, body: &str| {
        rig.store.put(&note("n1", 5, EDITED, body)).expect("put");
        rig.reload();
        rig.advance(RESOLVE_ARRIVING);
        let painted = rig.paint(frame, narrow, Pointing::default());
        let y = noted(&painted);
        (painted.notes_under(y), painted.note_word(y), painted)
    };
    let mut moved = None;
    for len in 1..40usize {
        let body = "y".repeat(len);
        let (under, _, painted) = rows_at(&mut rig, &mut frame, &body);
        assert_eq!(
            painted.lead_at(noted(&painted) + 1),
            Some(NoteLead::Bar),
            "the pane drew an enclosure, so this gate is not on the rung it is              named for:
{}",
            painted.rows().join("
")
        );
        if under.len() > 1 {
            moved = Some(len);
            break;
        }
        assert!(
            under[0].starts_with(&body),
            "the body was cut to fit the word: {:?}",
            under[0]
        );
    }
    let moved = moved.expect("no body up to forty characters moved the word down");

    // At that length the word is alone on the row under the body, and the body
    // is whole; one character shorter and they share a row.
    let full = "y".repeat(moved);
    let (under, word, _) = rows_at(&mut rig, &mut frame, &full);
    assert_eq!(under.len(), 2, "{under:?}");
    assert_eq!(
        under[0].trim_end(),
        full,
        "the body gave up a character to the word it no longer shares a row with"
    );
    assert_eq!(
        under[1].trim(),
        "open",
        "the word did not take a row of its own"
    );
    assert!(word.ends_with("open"), "{word:?}");

    let short = "y".repeat(moved - 1);
    let (under, word, _) = rows_at(&mut rig, &mut frame, &short);
    assert_eq!(under.len(), 1, "{under:?}");
    assert!(
        under[0].starts_with(&short) && under[0].trim_end().ends_with("open"),
        "one column short of the boundary the word left the row: {:?}",
        under[0]
    );
    assert!(word.ends_with("open"), "{word:?}");
}

#[test]
fn a_body_that_fills_the_enclosure_is_not_cut_by_the_word() {
    // The reader's words are never cut to fit a status: the word has an edge of
    // its own, so a body filling its row to the column keeps every character.
    let scratch = fixture("notes-word-row");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    let plain = rig.paint(&mut frame, PANE, Pointing::default());
    let y = plain.row_of(EDITED);
    let (_, _, origin) = plain.gutter();
    // The room the enclosure leaves its body. Measured off a painted note rather
    // than derived, so the fixture follows the layout.
    rig.store.put(&note("probe", 5, EDITED, "x")).expect("put");
    rig.reload();
    let probe = rig.paint(&mut frame, PANE, Pointing::default());
    let edge = probe.text(y + 1).trim_end().chars().count();
    let inner = edge - usize::from(origin) - BOX_FRAME;
    rig.store.remove("probe").expect("remove");
    // Gone from the store without a press, so it leaves over `LEAVING` first.
    rig.reload();
    rig.advance(LEAVING);

    // Exactly the room, so a column less would cut it.
    let full = "y".repeat(inner);
    rig.store.put(&note("n1", 5, EDITED, &full)).expect("put");
    rig.reload();
    rig.advance(RESOLVE_ARRIVING);
    let painted = rig.paint(&mut frame, PANE, Pointing::default());
    let under = painted.notes_under(y);
    assert_eq!(under.len(), 1, "{under:?}");
    assert_eq!(
        under[0].trim_end(),
        full,
        "the body was cut or wrapped early"
    );
    assert!(
        painted.note_word(y).contains("open"),
        "the word left the edge when the body filled the row: {:?}",
        painted.note_word(y)
    );

    // One column more, and it wraps rather than losing the character.
    let over = "y".repeat(inner + 1);
    rig.store.put(&note("n1", 5, EDITED, &over)).expect("put");
    rig.reload();
    rig.advance(RESOLVE_ARRIVING);
    let painted = rig.paint(&mut frame, PANE, Pointing::default());
    let under = painted.notes_under(y);
    assert_eq!(under.len(), 2, "{under:?}");
    assert_eq!(
        under.iter().map(|row| row.trim_end()).collect::<String>(),
        over,
        "a character was lost at the wrap"
    );
}

#[test]
fn the_bar_is_the_rung_under_the_enclosure_and_keeps_the_word() {
    // A box returns early where it has no room, so a committed note on a narrow
    // enough pane would draw nothing at all. The bar is the rung under it, and
    // there the word is drawn first at the right edge with the lead bounded by
    // what it leaves, so the reader's status is never what gives way.
    let scratch = fixture("notes-narrow-word");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    rig.store.put(&note("n1", 5, EDITED, "")).expect("put");
    rig.reload();
    let mut drawn = 0;
    let mut rungs = (0, 0);
    for width in 6..=30u16 {
        let painted = rig.paint(&mut frame, Rect::new(0, 0, width, 24), Pointing::default());
        let leads: Vec<NoteLead> = painted
            .view
            .rows
            .iter()
            .filter_map(|row| match row {
                Row::Note { lead, .. } => Some(*lead),
                _ => None,
            })
            .collect();
        if leads.is_empty() {
            continue;
        }
        if leads.contains(&NoteLead::Top) {
            rungs.0 += 1;
            // An empty body is still enclosed: an edge, an empty row between the
            // sides, and the edge that carries the word.
            assert_eq!(
                leads,
                vec![NoteLead::Top, NoteLead::Body, NoteLead::Bottom,],
                "at {width} columns the enclosure is not three rows"
            );
            let bottom = painted
                .view
                .rows
                .iter()
                .position(|row| {
                    matches!(
                        row,
                        Row::Note {
                            lead: NoteLead::Bottom,
                            ..
                        }
                    )
                })
                .expect("the enclosure's bottom edge");
            let edge = painted.text(painted.laid.diff.top + bottom as u16);
            assert!(
                edge.contains("open"),
                "at {width} columns the enclosure lost the word: {edge:?}"
            );
            // And the edge closes. This is where the rung's boundary is pinned:
            // one column narrower than the word and its frame need, the corner
            // is what the row runs out of room for.
            assert!(
                edge.trim_end().ends_with(['┘', '╯']),
                "at {width} columns the enclosure's bottom edge does not close: {edge:?}"
            );
            continue;
        }
        rungs.1 += 1;
        // The rung under it: the bar, where an empty body is one row, the word
        // alone, never a blank row bought for a gap the body does not have.
        assert!(
            leads.len() <= 1,
            "at {width} columns an empty body took {} bar rows",
            leads.len()
        );
        for (offset, row) in painted.view.rows.iter().enumerate() {
            let Row::Note {
                state: word,
                last: true,
                ..
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
    assert!(
        rungs.0 > 0 && rungs.1 > 0,
        "the sweep did not cross the width where the enclosure gives way:          {} enclosed against {} barred",
        rungs.0,
        rungs.1
    );
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
        vigia::word_arrival(&rig.theme).is_some(),
        "the default palette has nothing to fade between, so this gate would \
         pass on a word that simply changed"
    );
    rig.store.put(&note("n1", 5, EDITED, BODY)).expect("put");
    rig.reload();
    let open = rig.paint(&mut frame, PANE, Pointing::default());
    let y = open.row_of(EDITED);
    assert!(open.note_word(y).contains("open"));

    // The agent lists the store: the file is rewritten under the pane's hand and
    // the wake reads it back.
    rig.agent()
        .rewrite(&left_as("n1", BODY, Status::Seen, None))
        .expect("rewrite");
    rig.reload();
    rig.advance(ARRIVING_FRAME);
    let arriving = rig.paint(&mut frame, PANE, Pointing::default());
    assert!(
        arriving.note_word(y).contains("seen"),
        "{:?}",
        arriving.note_word(y)
    );
    let cells = note_cells(&arriving.laid, &arriving.view);
    let word = cells[0].word.expect("the word's cells");
    assert!(
        (word.x..word.right()).all(|x| arriving.fg(x, word.y) != Some(dim)),
        "one frame into the crossfade the word is drawn in the chrome's dim, so \
         the agent's reading arrived without arriving"
    );
    // Nothing else moved: the reader's words above the edge keep their ink.
    let (_, _, origin) = arriving.gutter();
    assert_eq!(arriving.fg(origin + 2, word.y - 1), rig.theme.chrome.fg);

    rig.advance(RESOLVE_ARRIVING);
    let settled = rig.paint(&mut frame, PANE, Pointing::default());
    assert!(
        (word.x..word.right()).all(|x| settled.fg(x, word.y) == rig.theme.note_seen.fg),
        "the crossfade ran its length and the word did not settle on the seen state's ink"
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
    assert!(noted.text(noted.after_notes(y)).contains("line 6"));

    // The agent resolves it: the rows become the agent's line, arriving.
    rig.agent()
        .rewrite(&left_as("n1", "short", Status::Resolved, Some(REPLY)))
        .expect("rewrite");
    rig.reload();
    let (_, _, origin) = noted.gutter();
    // Halfway in, the line is evolving: shade blocks where its words will be.
    rig.advance(RESOLVE_ARRIVING / 2);
    let arriving = rig.paint(&mut frame, PANE, Pointing::default());
    assert!(
        shading(&arriving, y) > 0,
        "the agent's line landed drawn rather than evolving:
{}",
        arriving.rows().join(
            "
"
        )
    );

    // The beat: the line holds, readable, in the answer's own ink.
    rig.advance(RESOLVE_ARRIVING / 2 + ARRIVING_FRAME);
    let holding = rig.paint(&mut frame, PANE, Pointing::default());
    let under = holding.notes_under(y);
    assert_eq!(under.len(), 1, "{under:?}");
    assert!(under[0].starts_with("swapped for"), "{:?}", under[0]);
    let arrow = holding.reply_row(y);
    assert_eq!(
        holding.text(arrow).chars().nth(usize::from(origin)),
        Some('↳')
    );
    assert_eq!(
        holding.fg(origin, arrow),
        rig.theme.note_reply.fg,
        "the answer is not in the reply's ink once it has arrived"
    );
    assert_ne!(holding.fg(origin, arrow), Some(dim));
    assert!(holding.text(holding.after_notes(y)).contains("line 6"));

    // The beat, held with no motion over it, and then the dissolve: halfway
    // through, the line is going and the diff has not closed up yet.
    rig.advance(RESOLVE_BEAT);
    rig.advance(LEAVING / 2);
    let dissolving = rig.paint(&mut frame, PANE, Pointing::default());
    assert_ne!(
        dissolving.text(arrow),
        holding.text(arrow),
        "halfway through the dissolve the agent's line is drawn whole"
    );
    assert!(
        dissolving
            .text(dissolving.after_notes(y))
            .contains("line 6"),
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

    // The pane's own prune: the file goes on the frame that drops the rows,
    // which is what leaves the reader having watched the line before the note
    // left.
    assert!(
        files_in(rig.store.dir()).is_empty(),
        "the departure ran and the file stayed"
    );

    // Once: a resolved file still in the store, which is a removal the store
    // refused or another pane's copy, does not run the departure again.
    rig.agent()
        .put(&left_as("n1", "short", Status::Resolved, Some(REPLY)))
        .expect("put it back");
    rig.reload();
    let again = rig.paint(&mut frame, PANE, Pointing::default());
    assert_eq!(again.rows(), plain.rows(), "a resolved note departed twice");
    assert!(!rig.effects.is_running());
}

#[test]
fn the_beat_between_a_resolve_and_its_sweep_runs_no_effect() {
    // What makes the hold affordable, and the only thing that does. `patience`
    // asks for a frame every `ARRIVING_FRAME` while any effect is running, so a
    // beat spent inside one effect is the beat's length divided by 16ms in
    // paints of a surface that is not moving. The line arrives, the pane goes
    // quiet holding it, and the sweep is armed by the deadline that ends the
    // beat.
    let scratch = fixture("notes-resolve-beat");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    let plain = rig.paint(&mut frame, PANE, Pointing::default());
    let y = plain.row_of(EDITED);

    rig.store
        .put(&left_as("n1", "short", Status::Seen, None))
        .expect("put");
    rig.reload();
    rig.agent()
        .rewrite(&left_as("n1", "short", Status::Resolved, Some(REPLY)))
        .expect("rewrite");
    rig.reload();

    // The arrival, and then nothing: the effect over the rows is retired at its
    // own length rather than at the departure's.
    rig.advance(RESOLVE_ARRIVING);
    let held = rig.paint(&mut frame, PANE, Pointing::default());
    assert!(
        held.notes_under(y)[0].starts_with("swapped for"),
        "the agent's line is not drawn once its arrival has run: {:?}",
        held.notes_under(y)
    );
    assert!(
        !rig.effects.is_running(),
        "an effect is still running once the agent's line has arrived, so the \
         beat is paid for at one frame every {ARRIVING_FRAME:?}"
    );
    // And what the loop is offered instead: the sweep's own moment, rather than
    // the frame an effect would have asked for.
    let sweeps = rig.clock + RESOLVE_BEAT;
    assert_eq!(
        rig.ledger.ends_in(),
        Some(sweeps),
        "the ledger offers the loop something other than the beat's end, so the \
         pane wakes through a stretch nothing is drawing"
    );

    // Three quarters of the way through the beat, still quiet and still drawn.
    rig.advance(RESOLVE_BEAT * 3 / 4);
    let late = rig.paint(&mut frame, PANE, Pointing::default());
    assert!(
        late.notes_under(y)[0].starts_with("swapped for"),
        "the agent's line left inside the beat: {:?}",
        late.notes_under(y)
    );
    assert!(!rig.effects.is_running(), "the beat armed an effect");
    assert_eq!(rig.ledger.ends_in(), Some(sweeps), "the beat's end moved");

    // The beat's end is the deadline the loop waits on, and it arms the sweep.
    rig.advance(RESOLVE_BEAT / 4);
    assert!(
        rig.effects.is_running(),
        "the beat ran out and nothing swept the rows away"
    );
    let armed = rig.paint(&mut frame, PANE, Pointing::default());
    assert_eq!(
        armed.notes_under(y),
        late.notes_under(y),
        "the frame that arms the sweep has already run some of it, so the line \
         starts leaving before it was ever drawn settled"
    );
    rig.advance(LEAVING / 2);
    let dissolving = rig.paint(&mut frame, PANE, Pointing::default());
    assert_ne!(
        dissolving.notes_under(y),
        late.notes_under(y),
        "halfway through the sweep the rows are drawn whole"
    );

    // And the rows are dropped on the frame after it, as they always were.
    rig.advance(LEAVING / 2);
    let gone = rig.paint(&mut frame, PANE, Pointing::default());
    assert_eq!(
        gone.rows(),
        plain.rows(),
        "the departure ended and something was left drawn"
    );
    assert!(!rig.effects.is_running());
}

#[test]
fn a_beat_that_has_run_names_its_sweep_once_however_often_the_pane_settles() {
    // On the ledger rather than through a paint, because the effects cannot see
    // this: `arm` replaces the effect over a note's rows rather than stacking on
    // it, so a sweep re-armed on every frame keeps the count at one and restarts
    // its own dissolve, and the reader watches the rows leave and come back.
    let now = Instant::now();
    let mut ledger = Ledger::default();
    ledger.reload(vec![note("n1", 5, EDITED, "short")], now);
    ledger.reload(
        vec![left_as("n1", "short", Status::Resolved, Some(REPLY))],
        now,
    );

    // Through the beat the loop is offered its end, and settling inside it
    // sweeps nothing.
    let sweeps = now + RESOLVED_DEPARTURE - LEAVING;
    assert_eq!(ledger.ends_in(), Some(sweeps));
    assert!(
        ledger.settle(now + RESOLVE_ARRIVING).sweeping.is_empty(),
        "the rows were swept while the agent's line was still being read"
    );

    // The turn that meets the end names the note, and no turn after it does.
    assert_eq!(ledger.settle(sweeps).sweeping, vec!["n1".to_owned()]);
    for again in 1..=4 {
        assert!(
            ledger.settle(sweeps).sweeping.is_empty(),
            "settle {again} turns past the beat's end named the sweep again"
        );
    }
    assert_eq!(
        ledger.ends_in(),
        Some(now + RESOLVED_DEPARTURE),
        "the rows are dropped at something other than the departure's own end"
    );
}

#[test]
fn departures_in_different_phases_each_keep_their_own_clock() {
    // One ledger holding a resolve mid-beat, a resolve already sweeping and a
    // withdrawal, which is every phase a `Departing` has. `ends_in` folds them to
    // the earliest, so a note in one phase must not be able to move another's
    // moment, and a settle must name only what is due on it.
    let now = Instant::now();
    let mut ledger = Ledger::default();
    ledger.reload(
        vec![
            note("n1", 5, EDITED, "first"),
            note("n2", 6, "line 6", "second"),
        ],
        now,
    );

    // n1 resolves, and a moment later n2 is withdrawn: two departures, two
    // clocks, and the withdrawal's is the nearer one.
    ledger.reload(
        vec![
            left_as("n1", "first", Status::Resolved, Some(REPLY)),
            note("n2", 6, "line 6", "second"),
        ],
        now,
    );
    let withdrawn = now + RESOLVE_ARRIVING;
    ledger.reload(
        vec![left_as("n1", "first", Status::Resolved, Some(REPLY))],
        withdrawn,
    );
    assert_eq!(
        ledger.ends_in(),
        Some(withdrawn + LEAVING),
        "the ledger offers something other than the nearer of the two clocks"
    );

    // The withdrawal ends first and takes nothing of the resolve's with it.
    let settled = ledger.settle(withdrawn + LEAVING);
    assert!(settled.changed);
    assert!(
        settled.sweeping.is_empty() && settled.prune.is_empty(),
        "a withdrawal ending swept or pruned the resolve still holding its line"
    );
    let sweeps = now + RESOLVED_DEPARTURE - LEAVING;
    assert_eq!(
        ledger.ends_in(),
        Some(sweeps),
        "the resolve's beat moved when the note beside it left"
    );
    assert_eq!(ledger.drawn().len(), 1, "{:?}", ledger.drawn());

    // And the resolve runs its own course from there.
    assert_eq!(ledger.settle(sweeps).sweeping, vec!["n1".to_owned()]);
    assert_eq!(
        ledger.settle(now + RESOLVED_DEPARTURE).prune,
        vec!["n1".to_owned()]
    );
    assert!(
        ledger.ends_in().is_none(),
        "a clock outlived every departure"
    );
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
    // And it travels: halfway through, the half the sweep started on is the
    // emptier one. Without this the gate holds for a dissolve in any direction,
    // or none.
    let width = dissolving.text(y + 2).chars().count() as u16;
    let middle = (origin + width) / 2;
    let cleared = |painted: &Painted, from: u16, to: u16| {
        (y + 1..painted.after_notes(y))
            .flat_map(|row| (from..to).map(move |x| (x, row)))
            .filter(|(x, row)| painted.cell(*x, *row).symbol() == " ")
            .count()
    };
    assert!(
        cleared(&dissolving, origin, middle) > cleared(&dissolving, middle, width),
        "the sweep did not clear the rows' left half ahead of their right:
{}",
        dissolving.rows().join(
            "
"
        )
    );
    assert!(
        dissolving
            .text(dissolving.after_notes(y))
            .contains("line 6")
    );

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
    // Once the answer has evolved in: its first frames are shade blocks, which
    // is the arrival, and what this gate is about is which rows are left.
    rig.advance(RESOLVE_ARRIVING);
    let arriving = rig.paint(&mut frame, PANE, Pointing::default());
    assert_eq!(
        arriving
            .text(arriving.reply_row(y))
            .chars()
            .nth(usize::from(origin)),
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
    rig.advance(RESOLVE_ARRIVING);
    let truth = rig.paint(&mut frame, PANE, Pointing::default());
    assert_eq!(
        truth
            .text(truth.reply_row(y))
            .chars()
            .nth(usize::from(origin)),
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
    let long = format!("{REPLY}, and the second clause wraps at every width here");
    rig.store
        .put(&left_as("n1", BODY, Status::Seen, Some(&long)))
        .expect("put");
    rig.reload();
    // Past the answer's arrival, so the cells hold what the renderer drew
    // rather than the shade blocks it evolves through.
    rig.advance(RESOLVE_ARRIVING);
    // The third pane is short enough that the bar is drawn and narrow enough to
    // have no trailing margin, so only the bar's own narrowing keeps the rows
    // off its column and the clause below is exercised rather than skipped.
    // Two rows taller than the bar alone needs: the enclosure costs a note two
    // rows more than the bar did, and rows past the region's floor are not the
    // cells' to cover.
    let short = Rect::new(0, 0, 40, 22);
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
            note_rows(&painted, "n1"),
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
        // Over every row of the answer, not the arrow's alone: a wrapped reply
        // is one `Reply` row and the rest `Blank`, and the rect is their union.
        assert!(
            reply.height > 1,
            "the answer did not wrap at {} columns, so the union over its              continuations is not exercised",
            pane.width
        );
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
fn a_reply_landing_on_an_open_note_evolves_in() {
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
    rig.advance(RESOLVE_ARRIVING / 2);
    let arriving = rig.paint(&mut frame, PANE, Pointing::default());
    let cells = note_cells(&arriving.laid, &arriving.view);
    let reply = cells[0].reply.expect("the reply's cells");
    assert!(
        shaded(&arriving, reply) > 0,
        "halfway in, the agent's line is drawn rather than evolving:
{}",
        arriving.rows().join(
            "
"
        )
    );
    // The note's own rows do not move: only the answer's cells are the
    // effect's, which is what the reply's rect is for.
    assert_eq!(arriving.fg(origin + 2, y + 2), rig.theme.chrome.fg);
    assert_eq!(shading(&arriving, y), shaded(&arriving, reply));

    rig.advance(RESOLVE_ARRIVING);
    let settled = rig.paint(&mut frame, PANE, Pointing::default());
    let under = settled.notes_under(y);
    assert_eq!(under.len(), 2, "{under:?}");
    assert!(under[1].starts_with("swapped for"), "{:?}", under[1]);
    assert_eq!(
        settled.fg(reply.x + 2, reply.y),
        rig.theme.note_reply.fg,
        "the answer settled in an ink that is not its own"
    );
    assert_ne!(settled.fg(reply.x + 2, reply.y), Some(dim));
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
    rig.advance(RESOLVE_ARRIVING);
    let departing = rig.paint(&mut frame, PANE, Pointing::default());
    assert_eq!(
        departing
            .text(departing.reply_row(y))
            .chars()
            .nth(usize::from(origin)),
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

    // And the one left on n1 is the whole note's: at its first frame the evolve
    // holds every cell of the rows in the answer's ink, the body's included,
    // where a word's or a line's effect would reach the word alone.
    let from = theme.note_reply.fg.expect("the answer's ink");
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

/// The ink the word `needle` is drawn in on `painted`, read from the cell its
/// last character stands in.
fn word_ink(painted: &Painted, needle: &str) -> Option<Color> {
    let diff = painted.laid.diff;
    for y in diff.top..diff.top + diff.rows {
        let row: Vec<char> = painted.text(y).chars().collect();
        let want: Vec<char> = needle.chars().collect();
        // By character rather than by byte: the row carries `▎`, which is three.
        if let Some(at) = row
            .windows(want.len())
            .position(|run| run == want.as_slice())
        {
            let last = at + want.len() - 1;
            return painted.fg(u16::try_from(last).expect("a sane column"), y);
        }
    }
    panic!("no row draws {needle:?}:\n{}", painted.rows().join("\n"));
}

#[test]
fn the_four_states_draw_their_word_and_their_bar_in_four_different_inks() {
    // One fixture per state, since a state is a fact about the note and the diff
    // together and no one frame holds all four.
    let mut inks: Vec<(&str, Option<Color>, Option<Color>)> = Vec::new();

    for (word, status) in [("open", Status::Open), ("seen", Status::Seen)] {
        let scratch = fixture(&format!("notes-ink-{word}"));
        let worktree = scratch.worktree();
        let mut frame = worktree.frame();
        frame.advance().expect("advance");
        let mut rig = Rig::open(&scratch);
        rig.store
            .put(&left_as("n1", "the reader's words", status, None))
            .expect("put");
        rig.reload();
        let painted = rig.paint(&mut frame, PANE, Pointing::default());
        let y = painted.row_of(EDITED);
        let (_, _, origin) = painted.gutter();
        inks.push((word, word_ink(&painted, word), painted.fg(origin, y + 1)));
    }

    // The line is edited under the note, so its stored text is no longer there.
    {
        let scratch = fixture("notes-ink-changed");
        scratch.edit_line(PATH, 4, "edited out from under the note");
        let worktree = scratch.worktree();
        let mut frame = worktree.frame();
        frame.advance().expect("advance");
        let mut rig = Rig::open(&scratch);
        rig.store
            .put(&note("n1", 5, EDITED, "the reader's words"))
            .expect("put");
        rig.reload();
        let painted = rig.paint(&mut frame, PANE, Pointing::default());
        let y = painted.row_of("edited out from under the note");
        let (_, _, origin) = painted.gutter();
        inks.push((
            "changed",
            word_ink(&painted, "changed"),
            painted.fg(origin, y + 1),
        ));
    }

    // The file leaves the diff entirely, so the note draws under the heading.
    {
        let scratch = fixture("notes-ink-gone");
        let worktree = scratch.worktree();
        let mut frame = worktree.frame();
        frame.advance().expect("advance");
        let mut rig = Rig::open(&scratch);
        rig.store
            .put(&note("n1", 5, EDITED, "the reader's words"))
            .expect("put");
        rig.reload();
        scratch.remove(PATH);
        frame.advance().expect("advance after the delete");
        let painted = rig.paint(&mut frame, PANE, Pointing::default());
        let heading = painted.row_of("watch.rs");
        let (_, _, origin) = painted.gutter();
        inks.push((
            "gone",
            word_ink(&painted, "gone"),
            painted.fg(origin, heading + 1),
        ));
    }

    assert_eq!(inks.len(), 4);
    for (word, ink, bar) in &inks {
        assert!(ink.is_some(), "{word} drew its word in no ink at all");
        assert_eq!(
            ink, bar,
            "{word} draws its word and its bar in different inks, so the bar says \
             nothing the word does not"
        );
    }
    for (at, (first, ink, _)) in inks.iter().enumerate() {
        for (second, other, _) in inks.iter().skip(at + 1) {
            assert_ne!(
                ink, other,
                "{first} and {second} draw in one ink on the pane, so a reader has to \
                 read the word to tell them apart"
            );
        }
    }
}

#[test]
fn the_box_frame_takes_the_notes_own_ink_and_not_the_chromes() {
    // The frame is one of the four things the report named, and it drew in the
    // key that paints every other piece of furniture on the pane.
    let scratch = fixture("notes-frame-ink");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    let painted = rig.paint(&mut frame, PANE, Pointing::default());
    let y = painted.row_of(EDITED);
    let (left, _, origin) = painted.gutter();
    assert!(rig.press_opens(&painted, left + 1, y));
    let opened = rig.paint(&mut frame, PANE, Pointing::default());

    let edge = opened
        .view
        .rows
        .iter()
        .position(|row| {
            matches!(
                row,
                Row::Box {
                    part: BoxPart::Top { .. }
                }
            )
        })
        .expect("the box's top edge");
    let at = opened.laid.diff.top + u16::try_from(edge).expect("a sane row");
    assert_eq!(
        opened.fg(origin, at),
        rig.theme.note_frame.fg,
        "the box's frame is not in the note's own ink:\n{}",
        opened.rows().join("\n")
    );
    assert_ne!(
        rig.theme.note_frame.fg, rig.theme.chrome_dim.fg,
        "the palette draws the note's frame in the chrome's dim, so this gate \
         cannot tell the two apart"
    );
}

/// B21: the agent "resolves it with a line the reader watches arrive before the
/// note leaves." Resolve-then-list in one breath is the ordinary shape of an
/// agent working a queue, and a reader whose pane was closed while it worked
/// meets every resolve at startup instead. Neither may cost the line, so the
/// pane is what removes a resolved file, at the end of the departure it drew.
#[test]
fn the_pane_draws_a_resolve_before_it_takes_the_file() {
    let scratch = fixture("notes-resolve-pruned");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);

    rig.store
        .put(&left_as("n1", "short", Status::Seen, None))
        .expect("put");
    rig.reload();
    let noted = rig.paint(&mut frame, PANE, Pointing::default());
    let y = noted.row_of(EDITED);
    assert_eq!(noted.notes_under(y).len(), 1);

    // The agent resolves. Whatever it does next, the file is still there for
    // this pane to draw: the server's half of that is gated in `tests/mcp.rs`.
    rig.agent()
        .rewrite(&left_as("n1", "short", Status::Resolved, Some(REPLY)))
        .expect("rewrite");
    assert_eq!(files_in(rig.store.dir()), ["n1.note"]);

    // The pane wakes whenever it wakes, and the line is still there to draw.
    // It arrives over its own length, so the beat after is where it reads.
    rig.reload();
    rig.advance(RESOLVE_ARRIVING + ARRIVING_FRAME);
    let departing = rig.paint(&mut frame, PANE, Pointing::default());
    let under = departing.notes_under(y);
    assert!(
        under
            .first()
            .is_some_and(|row| row.starts_with("swapped for")),
        "the note left without the agent's line ever being drawn: {under:?}"
    );

    // And the pane is what takes the file, once it has run the departure.
    rig.advance(RESOLVED_DEPARTURE);
    rig.paint(&mut frame, PANE, Pointing::default());
    assert!(
        files_in(rig.store.dir()).is_empty(),
        "the departure ran and nobody took the file"
    );
}

/// A note's wrap measures the paragraph two ways, and a tab is the one
/// character they price differently: `width_of` gives it the nothing the buffer
/// draws a control character as, while the cut comes from `split_at`, which
/// walks it out to its stop the way a line of the diff is drawn. A body is then
/// cut against columns its rows never spend, and can break onto one carrying
/// nothing but the status word. A reader pastes indented code into the box,
/// which is the thing there is to paste.
///
/// A tab wraps as what it draws or the two are still disagreeing, so the tabbed
/// body below and the same text with those tabs already spelled as the spaces
/// they advance to are one drawing and one set of rows.
#[test]
fn a_tabbed_note_wraps_as_the_columns_its_tabs_are_drawn_in() {
    let rows_for = |name: &str, body: &str| {
        let scratch = fixture(name);
        let worktree = scratch.worktree();
        let mut frame = worktree.frame();
        frame.advance().expect("advance");
        let mut rig = Rig::open(&scratch);
        rig.store.put(&note("n1", 5, EDITED, body)).expect("put");
        rig.reload();
        let painted = rig.paint(&mut frame, NARROW, Pointing::default());
        painted.notes_under(painted.row_of("checked_mul"))
    };

    // Each tab sits on a four-column boundary already, so the stop it advances
    // to is a full four spaces and the two bodies are the same drawing.
    let tabbed = rows_for(
        "notes-tabbed",
        "aaaa\tbbbb\tcccc\tdddd\teeee\tffff\tgggg\thhhh\tiiii\tjjjj",
    );
    let spelled = rows_for(
        "notes-spelled",
        "aaaa    bbbb    cccc    dddd    eeee    ffff    gggg    hhhh    iiii    jjjj",
    );

    assert!(
        !tabbed
            .iter()
            .any(|row| row.trim_end().is_empty() || row.trim_end() == "open"),
        "the body broke onto a row that draws nothing but its word: {tabbed:?}"
    );
    assert_eq!(
        tabbed, spelled,
        "a tab wrapped and drew as something other than the columns it advances \
         to:\n{tabbed:?}\n{spelled:?}"
    );
}

/// A file three lines longer than its committed form, with old line nine
/// rewritten, so a note on the index side is numbered nine and sits at
/// working-tree line twelve. The two numbers fall in different slices of the
/// strip, which is the only shape that can tell the walk from reading the stored
/// number straight.
fn shifted(name: &str) -> Scratch {
    let scratch = Scratch::new(name);
    scratch.write(PATH, numbered_lines(12));
    scratch.commit_all("baseline");
    let mut lines: Vec<String> = ["new a", "new b", "new c"]
        .iter()
        .map(|&s| s.to_owned())
        .chain((1..=12).map(|i| format!("line {i}")))
        .collect();
    lines[11] = "rewritten nine".to_owned();
    scratch.write(PATH, format!("{}\n", lines.join("\n")));
    scratch
}

/// The heat strip's inks on the row `y`, in column order.
fn strip_on(painted: &Painted, y: u16) -> Vec<Option<Color>> {
    let width = painted.backend.buffer().area.width;
    (0..width)
        .filter(|x| painted.cell(*x, y).symbol() == "\u{25a0}")
        .map(|x| painted.fg(x, y))
        .collect()
}

/// The one mark glyph on row `y`, or `None` where the slot is blank.
fn mark_on(painted: &Painted, y: u16) -> Option<char> {
    let width = painted.backend.buffer().area.width;
    let found: Vec<char> = (0..width)
        .map(|x| painted.cell(x, y))
        .filter_map(|cell| cell.symbol().chars().next())
        .filter(|glyph| ['\u{270e}', '\u{21b3}', '\u{2713}'].contains(glyph))
        .collect();
    assert!(found.len() <= 1, "row {y} drew {found:?}");
    found.into_iter().next()
}

#[test]
fn a_note_marks_its_file_in_the_list_and_in_the_diff() {
    // Both regions draw a file row through one drawer, so the mark has to reach
    // both or the map and the thing it maps disagree.
    let scratch = fixture("notes-mark-both");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    rig.store.put(&note("n1", 5, EDITED, BODY)).expect("put");
    rig.reload();
    let painted = rig.paint(&mut frame, PANE, Pointing::default());

    assert_eq!(mark_on(&painted, painted.row_of(PATH)), Some('\u{270e}'));
    assert!(
        painted.laid.list.rows > 0,
        "the fixture drew no list region"
    );
    assert_eq!(mark_on(&painted, painted.laid.list.top), Some('\u{270e}'));
}

#[test]
fn an_answered_note_takes_the_reply_glyph_and_a_resolved_one_takes_its_own() {
    let scratch = fixture("notes-mark-states");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);

    for (status, reply, glyph) in [
        (Status::Open, None, '\u{270e}'),
        (Status::Seen, None, '\u{270e}'),
        (Status::Seen, Some("which margin?"), '\u{21b3}'),
        (Status::Resolved, Some("fixed"), '\u{2713}'),
    ] {
        rig.store
            .put(&left_as("n1", "short", status, reply))
            .expect("put");
        rig.reload();
        let painted = rig.paint(&mut frame, PANE, Pointing::default());
        assert_eq!(
            mark_on(&painted, painted.row_of(PATH)),
            Some(glyph),
            "{status:?} with reply {reply:?}"
        );
    }
}

#[test]
fn a_resolved_note_keeps_the_rows_mark_and_lets_go_of_the_strip() {
    // The one place the two surfaces are deliberately out of step: the row says a
    // conversation just ended here, and the strip says where an outstanding one
    // is, which a resolved note is not.
    let scratch = fixture("notes-mark-resolved");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    let tint = rig.theme.heat_note.fg;

    rig.store
        .put(&left_as("n1", "short", Status::Seen, None))
        .expect("put");
    rig.reload();
    let open = rig.paint(&mut frame, PANE, Pointing::default());
    let inked = strip_on(&open, open.row_of(PATH))
        .into_iter()
        .filter(|ink| *ink == tint)
        .count();
    assert_eq!(
        inked, 1,
        "an open note tinted no slice, so the gate below \
                          cannot tell a resolve from a fixture that never worked"
    );

    rig.store
        .put(&left_as("n1", "short", Status::Resolved, Some("fixed")))
        .expect("put");
    rig.reload();
    let done = rig.paint(&mut frame, PANE, Pointing::default());
    let y = done.row_of(PATH);
    assert_eq!(mark_on(&done, y), Some('\u{2713}'), "the row lost its mark");
    assert_eq!(
        strip_on(&done, y)
            .into_iter()
            .filter(|ink| *ink == tint)
            .count(),
        0,
        "a resolved note is still tinting a slice"
    );
}

#[test]
fn a_note_on_a_removed_line_lands_in_the_slice_its_line_sits_in() {
    // An old-side note is numbered by the index while the strip is projected onto
    // the working tree's length. Here the two numbers are nine and twelve, which
    // fall in different slices: a fixture where they agree cannot tell the walk
    // from reading the stored number straight.
    let scratch = shifted("notes-mark-old-side");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    let tint = rig.theme.heat_note.fg;

    let mut old_side = note("n1", 9, "line 9", BODY);
    old_side.side = Side::Old;
    rig.store.put(&old_side).expect("put");
    rig.reload();
    let painted = rig.paint(&mut frame, PANE, Pointing::default());
    let inks = strip_on(&painted, painted.row_of(PATH));
    let noted: Vec<usize> = inks
        .iter()
        .enumerate()
        .filter(|(_, ink)| **ink == tint)
        .map(|(at, _)| at)
        .collect();

    // Twenty-four source buckets over a fifteen-line file, halved onto twelve
    // slices: working-tree line twelve is slice eight and the stored nine is six.
    assert_eq!(
        noted,
        vec![8],
        "the old-side note landed on {noted:?}; slice six is the stored number \
         used straight and slice eight is the line it sits on"
    );
}

#[test]
fn a_file_holding_two_notes_draws_the_worse_of_them() {
    // The fold no single-note fixture can see: with one note every precedence
    // rule draws the same row, so a `worse` that returned its receiver, or the
    // milder of the two, would pass every other gate here.
    let scratch = fixture("notes-mark-precedence");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);

    // Written in both orders, because a fold that keeps whichever it met first
    // is right half the time and this is the half that would hide it.
    for (first, second) in [("n1", "n2"), ("n2", "n1")] {
        for id in ["n1", "n2"] {
            let _ = rig.store.remove(id);
        }
        let waiting = left_as(first, "waiting", Status::Seen, None);
        let answered = left_as(second, "answered", Status::Seen, Some("which margin?"));
        rig.store.put(&waiting).expect("put");
        rig.store.put(&answered).expect("put");
        rig.reload();
        let painted = rig.paint(&mut frame, PANE, Pointing::default());
        assert_eq!(
            mark_on(&painted, painted.row_of(PATH)),
            Some('\u{21b3}'),
            "a waiting note and an answered one, written {first} then {second}, \
             drew something other than the answer"
        );
    }

    // And resolved is the mildest of the three: a file still holding an
    // unanswered note says so rather than announcing the departure.
    for id in ["n1", "n2"] {
        let _ = rig.store.remove(id);
    }
    let waiting = left_as("n1", "waiting", Status::Seen, None);
    let done = left_as("n2", "done", Status::Resolved, Some("fixed"));
    rig.store.put(&waiting).expect("put");
    rig.store.put(&done).expect("put");
    rig.reload();
    let painted = rig.paint(&mut frame, PANE, Pointing::default());
    assert_eq!(
        mark_on(&painted, painted.row_of(PATH)),
        Some('\u{270e}'),
        "a resolved note outranked a waiting one"
    );
}

/// Which slices of the heading's strip carry the note's ink.
fn tinted(painted: &Painted, tint: Option<Color>) -> Vec<usize> {
    strip_on(painted, painted.row_of(PATH))
        .iter()
        .enumerate()
        .filter(|(_, ink)| **ink == tint)
        .map(|(at, _)| at)
        .collect()
}

#[test]
fn an_old_side_note_answers_a_removed_line_and_not_the_addition_beside_it() {
    // `Hunk::positions` advances `old` only on a line the index side has, so the
    // addition in a rewritten block carries the index number of the context line
    // that follows it, and the walk emits it first. A lookup that does not ask the
    // kind therefore answers with the addition.
    //
    // A note reaches that shape by going stale: it was pinned to a removed line,
    // the agent edited under it, and its stored number now names a line the index
    // still has and the diff no longer removes. What catches it is the stale note
    // tinting nothing at all; through the addition it would tint the very slice
    // the genuine note below tints, so position alone cannot tell them apart.
    let scratch = Scratch::new("notes-mark-phantom");
    scratch.write(PATH, numbered_lines(12));
    scratch.commit_all("baseline");
    scratch.edit_line(PATH, 4, "rewritten five");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    let tint = rig.theme.heat_note.fg;

    // The stale note alone. Index line six is a context line, and the addition
    // above it answers to six with a working-tree position of five.
    let mut stale = note("n2", 6, "line 6", BODY);
    stale.side = Side::Old;
    rig.store.put(&stale).expect("put");
    rig.reload();
    let painted = rig.paint(&mut frame, PANE, Pointing::default());
    assert_eq!(
        tinted(&painted, tint),
        Vec::<usize>::new(),
        "the stale note tinted a slice, so it answered through the addition that \
         carries index line six rather than through a line the index removed"
    );

    // Added rather than swapped, because removing one starts a departure that
    // keeps drawing. Index line five is the line the rewrite removed and it sits
    // at working-tree line five: twelve lines over twelve slices puts it in slice
    // four, which is the slice the stale note would have taken.
    let mut removed = note("n1", 5, "line 5", BODY);
    removed.side = Side::Old;
    rig.store.put(&removed).expect("put");
    rig.reload();
    let painted = rig.paint(&mut frame, PANE, Pointing::default());
    assert_eq!(
        tinted(&painted, tint),
        vec![4],
        "a note on the line the rewrite removed did not tint its own slice, so the \
         assertion above passes on a strip that cannot draw one at all"
    );

    assert_eq!(mark_on(&painted, painted.row_of(PATH)), Some('\u{270e}'));
}

#[test]
fn a_note_on_a_path_in_both_runs_marks_one_entry_wherever_the_list_is_looking() {
    // Which run owns a note is resolved for every path in both runs that holds
    // one, rather than only for a path the diff walk reaches. Pinned, the walk
    // reaches one file, and a path in both runs that is not the pinned one has
    // neither entry in that range, so a rule asked only there leaves one note
    // marking the file twice while the footer says one. The answer cannot depend
    // on where either region is looking, and this sweep is what says so.
    //
    // The windows have to genuinely differ or the sweep is one fixture run four
    // times: with few enough files the list draws every row at once, `last_top` is
    // zero, and `following_top` returns its argument, so every request converges
    // on the same window. The filler files below are what stop that.
    let scratch = in_both_runs("notes-mark-pinned", 5, "staged six");
    for at in 0..7 {
        scratch.write(&format!("src/filler{at}.rs"), numbered_lines(6));
    }
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.show_staged(true);
    frame.advance().expect("advance");

    let runs: Vec<usize> = frame
        .files()
        .iter()
        .enumerate()
        .filter(|(_, change)| change.paths().any(|path| path == PATH))
        .map(|(at, _)| at)
        .collect();
    assert_eq!(runs.len(), 2, "the fixture is not a path in both runs");
    let pinned = (0..frame.files().len())
        .find(|at| !runs.contains(at))
        .expect("no file to pin that is not one of the two runs");

    let notes = vec![note("n1", 8, "line 8", "context in both")];
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    // Four windows that are four windows: a request the file count can honour and
    // one it cannot, each with the list following the diff and not following it.
    // `list_rows` is under what the rows would want, so the clamp and the snap
    // both have somewhere to move the answer to.
    let mut both_drawn = 0;
    let mut windows: Vec<(Option<String>, usize)> = Vec::new();
    for (top, follows) in [(0, false), (9, false), (0, true), (9, true)] {
        let viewport = Viewport {
            position: vigia::Position {
                file: pinned,
                row: 0,
            },
            anchored: false,
            diff_rows: 12,
            width: 80,
            wrap: false,
            list_top: top,
            list_rows: 4,
            list_follows: follows,
            measured: true,
            landing: false,
            highlight: false,
            // The pin, which is what narrowed the walk to one file.
            single: true,
        };
        let view = View::collect_noted(
            &mut frame,
            &mut highlighter,
            &history,
            viewport,
            &notes,
            true,
            None,
        )
        .expect("collect");

        let listed: Vec<&vigia::FileEntry> =
            view.list.iter().filter_map(vigia::ListRow::entry).collect();
        windows.push((listed.first().map(|entry| entry.path.clone()), listed.len()));
        let drawn = listed.iter().filter(|entry| entry.path == PATH).count();
        let marked = listed
            .iter()
            .filter(|entry| entry.path == PATH && entry.notes.mark.is_some())
            .count();

        // Holds in every window, drawn whole or not: one note, one entry.
        assert!(
            marked <= 1,
            "at row {top} following {follows}, one note marked {marked} entries of \
             a path in both runs while the pane was pinned elsewhere"
        );
        if drawn == 2 {
            both_drawn += 1;
            assert_eq!(
                marked, 1,
                "at row {top} following {follows}, both entries were drawn and \
                 {marked} carried the note"
            );
        }
    }

    // Non-vacuity, both halves. The sweep has to reach a window that draws both
    // entries, or `marked <= 1` is satisfied by drawing neither; and the four
    // requests have to land on more than one window, or this is one fixture run
    // four times.
    windows.dedup();
    assert!(
        windows.len() > 1,
        "every request landed on one window, so neither the clamp nor the snap was exercised: {windows:?}"
    );
    assert!(
        both_drawn > 0,
        "no window in the sweep drew both entries, so the count above never had \
         two rows to choose between"
    );
}

/// A screen that opens inside a note counts no diff line for it. A continuation
/// is the one display row the screenful still counts, since part of its line's
/// text is drawn there; a note leaves the line it hangs under wholly above.
#[test]
fn a_screen_opening_inside_a_note_counts_no_line_for_it() {
    let scratch = fixture("shown-inside-a-note");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    let short = Rect::new(0, 0, 60, 12);
    // Long enough that its rows outlast the line they hang under as the window
    // moves down over them.
    let long = "checked_mul on a Duration cannot overflow here, so the unwrap_or \
                is unreachable and saturating_mul is what this wants instead.";
    rig.store.put(&note("n1", 5, EDITED, long)).expect("put");
    rig.reload();

    let height = body_layout(
        short,
        &rig.app.chrome("fixture", None, Pointing::default(), 0, ""),
        1,
        1,
    )
    .diff;
    // Down until the window opens inside the note. Found by moving rather than
    // by naming a row, because how many rows the note takes is the pane's to
    // decide and a number here would be a second copy of that arithmetic.
    let mut opened = None;
    for _ in 0..12 {
        let painted = rig.paint(&mut frame, short, Pointing::default());
        if matches!(painted.view.rows.first(), Some(Row::Note { .. })) {
            opened = Some(painted);
            break;
        }
        rig.app
            .apply(Action::Scroll(1), &mut frame, height)
            .expect("scroll");
    }
    let opened = opened.expect("the window never opened inside the note");
    assert!(
        !opened
            .view
            .rows
            .iter()
            .any(|row| matches!(row, Row::Line { text, .. } if text == EDITED)),
        "the pinned line is still drawn, so this is not a screen that opens \
         inside the note and the count below proves nothing"
    );
    assert_eq!(
        opened.view.shown(),
        opened
            .view
            .rows
            .iter()
            .filter(|row| !row.is_display())
            .count(),
        "the screenful counted a line for a note whose own line is wholly above it"
    );
}

#[test]
fn a_press_on_a_notes_left_side_withdraws_it_and_its_rows_leave() {
    let scratch = fixture("notes-side-withdraw");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    rig.store.put(&note("n1", 5, EDITED, BODY)).expect("put");
    rig.reload();
    let noted = rig.paint(&mut frame, PANE, Pointing::default());
    let y = noted.row_of(EDITED);
    let (_, _, origin) = noted.gutter();
    let rows = side_rows(&noted, "n1");
    assert!(
        rows.iter().all(|row| *row > y),
        "the note is not drawn under its own line, so this gate presses elsewhere"
    );

    // A body row rather than the run's first, which is the one a press that
    // ignored the row it landed on would reach anyway.
    let side = *rows
        .iter()
        .find(|row| noted.lead_at(**row) == Some(NoteLead::Body))
        .expect("a body row");
    assert_eq!(
        rig.edge_press(&noted, origin, side),
        Some(true),
        "the press on the note's left side took nothing back"
    );
    assert!(
        files_in(rig.store.dir()).is_empty(),
        "the file outlived the press"
    );

    // The rows leave over `LEAVING` and are dropped on the frame after, which is
    // the departure a withdrawal already had.
    rig.advance(LEAVING);
    let clear = rig.paint(&mut frame, PANE, Pointing::default());
    let plain = {
        let mut bare = Rig::open(&scratch);
        bare.paint(&mut frame, PANE, Pointing::default())
    };
    assert_eq!(
        clear.rows(),
        plain.rows(),
        "the note taken back left something drawn"
    );
}

#[test]
fn a_pointer_on_a_notes_left_side_marks_the_cell_it_rests_on() {
    let scratch = fixture("notes-side-mark");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    rig.store.put(&note("n1", 5, EDITED, BODY)).expect("put");
    rig.reload();
    rig.advance(RESOLVE_ARRIVING);
    let noted = rig.paint(&mut frame, PANE, Pointing::default());
    let y = noted.row_of(EDITED);
    let (_, _, origin) = noted.gutter();
    let rows = side_rows(&noted, "n1");
    let side = *rows
        .iter()
        .find(|row| noted.lead_at(**row) == Some(NoteLead::Body))
        .expect("a body row");

    let hovering = Pointing {
        hovered: Some(Hovered::NoteEdge(side)),
        ..Pointing::default()
    };
    let marked = rig.paint(&mut frame, PANE, hovering);
    assert_eq!(
        marked.cell(origin, side).symbol(),
        "✕",
        "the pointer's own cell is not marked:\n{}",
        marked.rows().join("\n")
    );
    assert_eq!(
        marked.fg(origin, side),
        Theme::default().bar_hover.fg,
        "the mark is not in the pointer's ink"
    );
    for row in &rows {
        assert!(
            *row == side || marked.cell(origin, *row).symbol() != "✕",
            "row {row} of the note is marked and the pointer is not on it:\n{}",
            marked.rows().join("\n")
        );
    }

    // And a line of the diff is no target, which is what keeps this a mark on
    // the note rather than one on the column the note happens to start in.
    let over_line = rig.paint(
        &mut frame,
        PANE,
        Pointing {
            hovered: Some(Hovered::NoteEdge(y)),
            ..Pointing::default()
        },
    );
    assert_eq!(
        over_line.text(y),
        noted.text(y),
        "the pointer marked a line of the diff"
    );
}

#[test]
fn a_press_on_the_side_of_a_note_resolved_since_the_paint_leaves_the_resolve_alone() {
    let scratch = fixture("notes-side-stale");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    rig.store.put(&note("n1", 5, EDITED, "short")).expect("put");
    rig.reload();
    let marked = rig.paint(&mut frame, PANE, Pointing::default());
    let (_, _, origin) = marked.gutter();
    let side = side_rows(&marked, "n1")[0];

    // The agent resolves it while the screen still shows it open, and the
    // reader's press lands on that screen.
    rig.agent()
        .rewrite(&left_as("n1", "short", Status::Resolved, Some(REPLY)))
        .expect("rewrite");
    assert_eq!(
        rig.edge_press(&marked, origin, side),
        Some(false),
        "the press took back a note the agent had already resolved"
    );
    assert_eq!(
        files_in(rig.store.dir()).len(),
        1,
        "the press deleted the agent's resolve and its line"
    );

    // And a press on a note nobody resolved still takes it back, so the case
    // above is the resolve being honoured rather than a press that removes
    // nothing at all.
    rig.store
        .put(&note("n2", 6, "line 6", "short"))
        .expect("put");
    rig.reload();
    let second = rig.paint(&mut frame, PANE, Pointing::default());
    let live = side_rows(&second, "n2")[0];
    assert_eq!(
        rig.edge_press(&second, second.gutter().2, live),
        Some(true),
        "the press no longer takes back a note the agent has not touched"
    );
}

#[test]
fn the_left_side_takes_back_a_note_the_agent_replied_to_and_the_reply_goes_with_it() {
    let scratch = fixture("notes-side-replied");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    rig.store
        .put(&left_as("n1", "short", Status::Seen, Some(REPLY)))
        .expect("put");
    rig.reload();
    rig.advance(RESOLVE_ARRIVING);
    let answered = rig.paint(&mut frame, PANE, Pointing::default());
    let y = answered.row_of(EDITED);
    let (_, _, origin) = answered.gutter();

    // The answer's own row, which is the half of the exchange the agent wrote:
    // the side runs its whole height, and the answer goes with the note.
    let arrow = answered.reply_row(y);
    assert_eq!(
        answered.cell(origin, arrow).symbol(),
        "↳",
        "this gate is not pressing the answer's own row"
    );
    assert_eq!(
        rig.edge_press(&answered, origin, arrow),
        Some(true),
        "a note the agent answered cannot be taken back"
    );
    assert!(
        files_in(rig.store.dir()).is_empty(),
        "the file outlived the press"
    );
    rig.advance(LEAVING);
    let clear = rig.paint(&mut frame, PANE, Pointing::default());
    assert!(
        !clear
            .rows()
            .iter()
            .any(|row| row.contains("saturating_mul")),
        "the agent's line is still drawn under a note that is gone:\n{}",
        clear.rows().join("\n")
    );
}

#[test]
fn the_bar_rung_takes_the_mark_and_the_press_where_no_enclosure_fits() {
    // The rung below the enclosure, where the note's whole left side is the one
    // `▎` at the content origin.
    let narrow = Rect::new(0, 0, 14, 24);
    let scratch = fixture("notes-side-bar");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    rig.store.put(&note("n1", 5, EDITED, "yyy")).expect("put");
    rig.reload();
    rig.advance(RESOLVE_ARRIVING);
    let painted = rig.paint(&mut frame, narrow, Pointing::default());
    let side = side_rows(&painted, "n1")[0];
    assert_eq!(
        painted.lead_at(side),
        Some(NoteLead::Bar),
        "the pane drew an enclosure, so this gate is not on the rung it is named \
         for:\n{}",
        painted.rows().join("\n")
    );

    let origin = painted.gutter().2;
    let marked = rig.paint(
        &mut frame,
        narrow,
        Pointing {
            hovered: Some(Hovered::NoteEdge(side)),
            ..Pointing::default()
        },
    );
    assert_eq!(
        marked.cell(origin, side).symbol(),
        "✕",
        "the bar rung's own cell is not marked:\n{}",
        marked.rows().join("\n")
    );
    assert_eq!(
        rig.edge_press(&painted, origin, side),
        Some(true),
        "the rung below the enclosure has no press"
    );
    assert!(
        files_in(rig.store.dir()).is_empty(),
        "the file outlived the press"
    );
}

#[test]
fn a_press_on_the_left_side_begins_no_selection_and_the_body_beside_it_still_does() {
    let scratch = fixture("notes-side-selection");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut rig = Rig::open(&scratch);
    rig.store.put(&note("n1", 5, EDITED, BODY)).expect("put");
    rig.reload();
    let noted = rig.paint(&mut frame, PANE, Pointing::default());
    let (_, _, origin) = noted.gutter();
    let side = *side_rows(&noted, "n1")
        .iter()
        .find(|row| noted.lead_at(**row) == Some(NoteLead::Body))
        .expect("a body row");

    // The loop answers the side and never reaches the wash with it, so the
    // gate is that the side answers and the columns beside it do not.
    assert!(
        edge_at(&noted.view, noted.laid, &press(origin, side)).is_some(),
        "the note's left side is not a press"
    );
    assert!(
        edge_at(&noted.view, noted.laid, &press(origin + 2, side)).is_none(),
        "the press reaches past the one column the side is"
    );
    assert!(
        press_at(&noted.view, noted.laid, &press(origin, side)).is_none(),
        "the gutter's press and this one share a cell"
    );

    // And the body beside it still begins a drag, so what changed is the one
    // column and not the row.
    let (standing, _) = selection_after(&press(origin + 2, side), noted.laid, None);
    assert!(
        standing.is_some(),
        "the note's body no longer begins a selection"
    );
    let sent = noted
        .view
        .lines_in((
            usize::from(side - noted.laid.diff.top),
            usize::from(side - noted.laid.diff.top),
        ))
        .expect("the row resolves");
    assert_eq!(
        sent,
        vec![BODY.to_owned()],
        "a drag over the note's body no longer copies the note"
    );
}
