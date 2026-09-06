//! The box a press on a content row's gutter opens, what Enter and Esc do with
//! it, and what the pane keeps of the store between wakes (`SPEC.md` §11.2
//! B21).

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{
    Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};
use ratatui::layout::{Margin, Position, Rect};
use ratatui_textarea::{CursorMove, Input, TextArea};
use tachyonfx::{CellFilter, Effect, Interpolation, fx};
use vigia_core::{Listing, Note, Result, Status, Store};

use crate::input::Regions;
use crate::render::NoteCells;
use crate::theme::{self, Theme};
use crate::view::{Anchor, View};
use crate::{ARRIVING, NOTICE_ARRIVING, NOTICE_LINGER};

/// How long the box takes to arrive, and to leave on Esc: what a changed file
/// takes.
pub const BOX_ARRIVING: Duration = ARRIVING;

/// How long the agent's line, or a word the agent moved, takes to crossfade in:
/// an announcement's own arrival, since that is what it is.
pub const RESOLVE_ARRIVING: Duration = NOTICE_ARRIVING;

/// How long a note's rows take to dissolve, whichever way the note leaves.
pub const LEAVING: Duration = NOTICE_ARRIVING;

/// The whole of a resolve's departure, after which the rows are dropped: one
/// notice's time on the footer, its two ends included. One table with the
/// footer's, so the pane keeps one rhythm.
pub const RESOLVED_DEPARTURE: Duration = NOTICE_LINGER;

/// How long a resolve's line holds between arriving and dissolving.
pub const RESOLVE_BEAT: Duration = RESOLVED_DEPARTURE
    .saturating_sub(RESOLVE_ARRIVING)
    .saturating_sub(LEAVING);

/// The row of `view` a press on a content row's gutter landed on, or `None` for
/// any other event: `regions` says the pointer is on the gutter, and `view` says
/// whether that row is a line. A press this answers is a note and never a
/// selection, which is B20 and B21 sharing no cell.
#[must_use]
pub fn press_at(view: &View, regions: Regions, event: &Event) -> Option<usize> {
    let Event::Mouse(mouse) = event else {
        return None;
    };
    if mouse.kind != MouseEventKind::Down(MouseButton::Left) {
        return None;
    }
    let row = regions.gutter_at(mouse.column, mouse.row)?;
    let offset = usize::from(row.saturating_sub(regions.diff.top));
    view.anchor_at(offset).map(|_| offset)
}

/// Whether the pane leaves a content row room for the box between its two
/// sides. A mode the reader cannot see is one they cannot leave on purpose, so
/// a press on a pane this narrow opens nothing rather than taking the keys
/// while nothing on screen says where they are going.
#[must_use]
pub fn has_room(regions: Regions) -> bool {
    let (_, gutter) = regions.diff.gutter;
    usize::from(regions.diff.text.saturating_sub(gutter)) > crate::view::BOX_FRAME
}

/// What a press at row `offset` of `view` opens the box on: the line's anchor,
/// and the open note already pinned there when there is one, whose text the
/// box takes. `None` off a content row.
#[must_use]
pub fn opening(view: &View, offset: usize, notes: &[Note]) -> Option<(Anchor, Option<Note>)> {
    let anchor = view.anchor_at(offset)?;
    let existing = view
        .marked_at(offset)
        .into_iter()
        .find_map(|id| {
            notes
                .iter()
                .find(|note| note.id == id && note.status != Status::Resolved)
        })
        .cloned();
    Some((anchor, existing))
}

/// The box the reader types a note into, open under one line of the diff. It
/// owns the keys while the reader's hand is in it, and its rows stay drawn a
/// moment after Esc while they leave.
#[derive(Debug, Clone)]
pub struct NoteBox {
    anchor: Anchor,
    /// The note the box reopened, when it is over one.
    over: Option<String>,
    editor: TextArea<'static>,
    /// When its rows are dropped, once Esc has sent it away.
    leaving: Option<Instant>,
}

/// The box as the walk places it: an anchor in a note's shape, so it resolves
/// against the diff by the note's own rules, and the text the rows draw.
pub struct Standing<'b> {
    /// The anchor, with nothing written in it.
    pub note: Note,
    /// The note the box is open over, whose rows the box stands in for.
    pub over: Option<&'b str>,
    /// The reader's text, one entry per line the editor holds.
    pub lines: &'b [String],
    /// The editor's cursor, as a line and a character within it.
    pub cursor: (usize, usize),
}

impl NoteBox {
    /// Open the box on `anchor`, holding `existing`'s text when the line
    /// already carries a note, with the caret at the end.
    #[must_use]
    pub fn open(anchor: Anchor, existing: Option<&Note>) -> Self {
        let lines = existing
            .map(|note| note.body.split('\n').map(str::to_owned).collect())
            .unwrap_or_default();
        let mut editor = TextArea::new(lines);
        editor.move_cursor(CursorMove::Bottom);
        editor.move_cursor(CursorMove::End);
        Self {
            anchor,
            over: existing.map(|note| note.id.clone()),
            editor,
            leaving: None,
        }
    }

    /// The line the box is open under.
    #[must_use]
    pub fn anchor(&self) -> &Anchor {
        &self.anchor
    }

    /// The note the box reopened, by id.
    #[must_use]
    pub fn over(&self) -> Option<&str> {
        self.over.as_deref()
    }

    /// The reader's text, one entry per line the editor holds.
    #[must_use]
    pub fn lines(&self) -> &[String] {
        self.editor.lines()
    }

    /// The editor's cursor, as a line and a character within it.
    #[must_use]
    pub fn cursor(&self) -> (usize, usize) {
        let cursor = self.editor.cursor();
        (cursor.0, cursor.1)
    }

    /// What Enter would write: the lines joined, trimmed at both ends.
    #[must_use]
    pub fn body(&self) -> String {
        self.editor.lines().join("\n").trim().to_owned()
    }

    /// Hand the editor one key; `true` when the text changed.
    pub fn edit(&mut self, input: Input) -> bool {
        self.editor.input(input)
    }

    /// Insert pasted text at the caret; `true` when anything went in.
    pub fn paste(&mut self, text: &str) -> bool {
        self.editor.insert_str(text)
    }

    /// Whether the reader's hand is still in it, so keys are its.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.leaving.is_none()
    }

    /// Send it away: the keys are the pane's again at once, and the rows stay
    /// drawn until `until` while they leave.
    pub fn close(&mut self, until: Instant) {
        self.leaving = Some(until);
    }

    /// When its rows are dropped, once it is leaving.
    #[must_use]
    pub fn ends_in(&self) -> Option<Instant> {
        self.leaving
    }

    /// The box in the walk's terms.
    #[must_use]
    pub fn stand_in(&self) -> Standing<'_> {
        Standing {
            note: Note {
                id: String::new(),
                path: self.anchor.path.clone(),
                side: self.anchor.side,
                line: self.anchor.line,
                text: self.anchor.text.clone(),
                body: String::new(),
                status: Status::Open,
                reply: None,
                written: SystemTime::UNIX_EPOCH,
            },
            over: self.over.as_deref(),
            lines: self.editor.lines(),
            cursor: self.cursor(),
        }
    }
}

/// What one terminal event means while the box is open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoxRoute {
    /// Enter: write the note and close the box.
    Send,
    /// Esc, or a press outside the box: close it, write nothing, and do
    /// nothing else with the event.
    Cancel,
    /// A key for the editor, Alt+Enter's newline included.
    Edit(Input),
    /// Pasted text for the editor.
    Paste(String),
    /// A press inside the box, or a key's release: nothing.
    Inert,
    /// Not the box's to answer: the wheel, a resize, focus and the pointer
    /// moving, which the pane takes as it always does.
    Through,
}

/// Where `event` goes while the box is open over `over`, the cells it drew on
/// the last paint. Every key is the box's, which is the mode; the pointer's
/// buttons close it from anywhere else; the rest passes through.
#[must_use]
pub fn box_route(event: &Event, over: Option<Rect>) -> BoxRoute {
    match event {
        Event::Key(key) if key.kind == KeyEventKind::Release => BoxRoute::Inert,
        Event::Key(key) => match key.code {
            KeyCode::Enter if !key.modifiers.contains(KeyModifiers::ALT) => BoxRoute::Send,
            KeyCode::Esc => BoxRoute::Cancel,
            _ => BoxRoute::Edit(Input::from(*key)),
        },
        Event::Paste(text) => BoxRoute::Paste(text.clone()),
        Event::Mouse(mouse) => match mouse.kind {
            MouseEventKind::Down(_) => {
                if over.is_some_and(|over| over.contains(Position::new(mouse.column, mouse.row))) {
                    BoxRoute::Inert
                } else {
                    BoxRoute::Cancel
                }
            }
            _ => BoxRoute::Through,
        },
        Event::Resize(_, _) | Event::FocusGained | Event::FocusLost => BoxRoute::Through,
    }
}

/// What Enter did to the store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Committed {
    /// A note was written, under this id.
    Written(String),
    /// The note the box was over took the new text and is open again.
    Rewritten(String),
    /// The box was emptied over a note, which was withdrawn.
    Withdrawn(String),
    /// The box was empty over nothing, so nothing was written.
    Nothing,
}

/// Write what the box holds: a new note under its anchor, the reopened note
/// rewritten with the new text and its status back to open so the agent reads
/// it again, or that note withdrawn when the box was emptied. Empty text over
/// nothing writes nothing.
///
/// # Errors
///
/// The store could not write or remove the file. The box is the caller's to
/// keep, so nothing the reader typed is lost with it.
pub fn commit(store: &Store, open: &NoteBox) -> Result<Committed> {
    let body = open.body();
    // The screen the press landed on may be a frame behind the store: a resolve
    // that landed since answers the old text and keeps its file, so the new text
    // goes down beside it as a new note rather than over it. The read sits as
    // close to the write below as it can, and the two are still not one act:
    // a resolve landing between them is overwritten, which is the window the
    // store's own rewrite names and no portable primitive closes.
    let standing = open.over.as_deref().and_then(|id| match store.get(id) {
        Ok(Some(note)) if note.status != Status::Resolved => Some(note),
        _ => None,
    });
    match (standing, body.is_empty()) {
        (Some(note), true) => {
            store.remove(&note.id)?;
            Ok(Committed::Withdrawn(note.id))
        }
        (Some(note), false) => {
            let rewritten = Note {
                body,
                status: Status::Open,
                written: SystemTime::now(),
                ..note
            };
            if store.rewrite(&rewritten)? {
                return Ok(Committed::Rewritten(rewritten.id));
            }
            // Withdrawn under the box by another hand; the words are still the
            // reader's to send.
            let fresh = Note {
                id: Store::new_id(),
                ..rewritten
            };
            store.put(&fresh)?;
            Ok(Committed::Written(fresh.id))
        }
        (None, true) => Ok(Committed::Nothing),
        (None, false) => {
            let note = Note {
                id: Store::new_id(),
                path: open.anchor.path.clone(),
                side: open.anchor.side,
                line: open.anchor.line,
                text: open.anchor.text.clone(),
                body,
                status: Status::Open,
                reply: None,
                written: SystemTime::now(),
            };
            store.put(&note)?;
            Ok(Committed::Written(note.id))
        }
    }
}

/// How the box arrives: its border drawn in cell by cell around the ring from
/// the anchor's corner, clockwise, and its text fading up from the chrome's dim
/// behind it, over what a changed file takes. Where the depth has flattened the
/// two inks together the border alone arrives.
#[must_use]
pub fn box_entrance(theme: &Theme) -> Effect {
    let timer = (
        tachyonfx::Duration::from(BOX_ARRIVING),
        Interpolation::QuadOut,
    );
    let border = fx::effect_fn((), timer, |(): &mut (), context, cells| {
        let area = context.area;
        let drawn = ring_drawn(area, context.alpha());
        for (at, cell) in cells {
            if ring_step(area, at).is_some_and(|step| step >= drawn) {
                cell.set_symbol(" ");
            }
        }
    });
    match theme::contrast(theme.chrome_dim, theme.chrome) {
        Some(from) => fx::parallel(&[
            border,
            fx::fade_from_fg(from, timer).with_filter(CellFilter::Inner(Margin::new(1, 1))),
        ]),
        None => border,
    }
}

/// How the box leaves on Esc: its entrance played backwards.
#[must_use]
pub fn box_exit(theme: &Theme) -> Effect {
    box_entrance(theme).reversed()
}

/// How many cells of the ring the sweep has reached at `alpha` of its run.
fn ring_drawn(area: Rect, alpha: f32) -> usize {
    // Rounded rather than floored, so the run's end draws the last cell.
    (alpha * ring_len(area) as f32).round() as usize
}

/// Cells on the edge of `area`.
fn ring_len(area: Rect) -> usize {
    let (width, height) = (usize::from(area.width), usize::from(area.height));
    if width < 2 || height < 2 {
        width * height
    } else {
        2 * (width + height) - 4
    }
}

/// Where `at` stands along the edge of `area`, counted clockwise from its top
/// left corner, or `None` inside it.
fn ring_step(area: Rect, at: Position) -> Option<usize> {
    let (width, height) = (usize::from(area.width), usize::from(area.height));
    let x = usize::from(at.x.checked_sub(area.x)?);
    let y = usize::from(at.y.checked_sub(area.y)?);
    if x >= width || y >= height {
        return None;
    }
    if width < 2 || height < 2 {
        return Some(y * width + x);
    }
    if y == 0 {
        Some(x)
    } else if x == width - 1 {
        Some(width - 1 + y)
    } else if y == height - 1 {
        Some(width - 1 + height - 1 + (width - 1 - x))
    } else if x == 0 {
        Some(2 * (width - 1) + height - 1 + (height - 1 - y))
    } else {
        None
    }
}

/// An effect and when it is retired, which the box holds one of and every
/// note effect is built on.
pub struct Timed {
    effect: Effect,
    /// The clock is the retirement and the effect's own count is the other
    /// half: a thing off screen is never processed, and an effect never
    /// processed never reports itself done.
    until: Instant,
}

impl Timed {
    /// Arm `effect` until `until`.
    #[must_use]
    pub fn new(effect: Effect, until: Instant) -> Self {
        Self { effect, until }
    }

    /// Whether it still has frames to draw, which keeps the frame clock armed.
    #[must_use]
    pub fn is_running(&self) -> bool {
        !self.effect.done()
    }

    /// Whether it has run its length, by its own count or by the clock.
    #[must_use]
    pub fn spent(&self, now: Instant) -> bool {
        now >= self.until || self.effect.done()
    }

    /// Advance it by `since` over `over`, the cells its subject drew this frame.
    pub fn draw(&mut self, since: Duration, buf: &mut Buffer, over: Rect) {
        self.effect.process(since.into(), buf, over);
    }
}

/// What the last listing had to say for itself, so the footer's alert is said
/// when that changes and not on every wake the agent causes.
#[derive(Debug, Default)]
pub struct Alerts {
    skipped: Vec<(PathBuf, String)>,
    failed: Option<String>,
}

impl Alerts {
    /// The one footer alert `listing` earns, when it differs from the last: the
    /// first file skipped and how many more, or why the store could not be read.
    /// `None` when nothing changed, a store that reads whole again included:
    /// files that read again are not news.
    pub fn of(&mut self, listing: &Result<Listing>) -> Option<String> {
        let listing = match listing {
            Ok(listing) => listing,
            Err(e) => {
                let told = format!("could not read the notes: {e}");
                if self.failed.as_ref() == Some(&told) {
                    return None;
                }
                self.failed = Some(told.clone());
                return Some(told);
            }
        };
        self.failed = None;
        // Compared sorted, because the store lists its files in the directory's
        // order and a write beside them can move that; copied only on a change.
        let mut seen: Vec<&(PathBuf, String)> = listing.skipped.iter().collect();
        seen.sort();
        if seen.len() == self.skipped.len() && seen.iter().zip(&self.skipped).all(|(a, b)| *a == b)
        {
            return None;
        }
        self.skipped = seen.into_iter().cloned().collect();
        let (path, why) = self.skipped.first()?;
        let name = path.file_name().map_or_else(
            || path.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
        Some(match listing.skipped.len() - 1 {
            0 => format!("skipped the note file {name}: {why}"),
            more => format!("skipped the note file {name} and {more} more: {why}"),
        })
    }
}

/// What a reload of the store found had moved, each naming the note's id, for
/// the effect that draws it moving.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// The reader pressed Enter, so the note's rows are new under its line.
    Written(String),
    /// The agent listed the note, so its word climbed from *open* to *seen*.
    Seen(String),
    /// The agent answered without closing the note, so its line is new or has
    /// changed.
    Replied(String),
    /// The agent resolved it: it departs with the agent's line.
    Resolved(String),
    /// It is leaving without the agent's line: its file is gone from the store,
    /// by the reader's press here, another pane's, or the server's prune before
    /// this pane saw it resolved.
    Left(String),
}

/// A note on its way off the screen: drawn as it was until `ends`, then dropped.
#[derive(Debug)]
struct Departing {
    /// The note as it draws while it goes.
    note: Note,
    /// When its rows are dropped and the diff below closes up.
    ends: Instant,
}

/// What the pane holds of the store: the notes as last listed, the ones leaving,
/// and the resolved ones already gone from the screen whose files the server has
/// not pruned yet, so nothing resolved is ever drawn twice.
#[derive(Debug, Default)]
pub struct Ledger {
    listed: Vec<Note>,
    departing: Vec<Departing>,
    departed: HashSet<String>,
}

impl Ledger {
    /// Take a fresh listing and answer what moved since the last one, starting a
    /// departure for every note that is leaving. A note already leaving is not
    /// brought back by a listing that still holds it: whichever of a withdrawal
    /// and a resolve landed first is the one drawn.
    pub fn reload(&mut self, notes: Vec<Note>, now: Instant) -> Vec<Change> {
        let mut changes = Vec::new();
        let previous = std::mem::take(&mut self.listed);
        // Only ids the store still holds stay remembered, so the set is bounded by
        // the store rather than by the session.
        self.departed
            .retain(|id| notes.iter().any(|note| note.id == *id));
        let mut listed = Vec::with_capacity(notes.len());
        for note in notes {
            if self.is_departing(&note.id) || self.departed.contains(&note.id) {
                continue;
            }
            if note.status == Status::Resolved {
                changes.push(Change::Resolved(note.id.clone()));
                self.departing.push(Departing {
                    note,
                    ends: now + RESOLVED_DEPARTURE,
                });
                continue;
            }
            if let Some(before) = previous.iter().find(|before| before.id == note.id) {
                if before.status == Status::Open && note.status == Status::Seen {
                    changes.push(Change::Seen(note.id.clone()));
                }
                if note.reply.is_some() && before.reply != note.reply {
                    changes.push(Change::Replied(note.id.clone()));
                }
            }
            listed.push(note);
        }
        // Gone from the store, whichever hand removed it: the reader's press here
        // reads back the same way as another pane's or the server's prune.
        for gone in previous {
            if listed.iter().any(|note| note.id == gone.id) || self.is_departing(&gone.id) {
                continue;
            }
            changes.push(Change::Left(gone.id.clone()));
            self.departing.push(Departing {
                note: gone,
                ends: now + LEAVING,
            });
        }
        self.listed = listed;
        changes
    }

    /// Drop every departure that has ended, remembering a resolved one until the
    /// server prunes its file. `true` when something was dropped, so the caller
    /// knows the rows it hands the next collect have changed.
    pub fn settle(&mut self, now: Instant) -> bool {
        let before = self.departing.len();
        let departed = &mut self.departed;
        self.departing.retain(|gone| {
            if now < gone.ends {
                return true;
            }
            if gone.note.status == Status::Resolved {
                departed.insert(gone.note.id.clone());
            }
            false
        });
        self.departing.len() != before
    }

    /// When the next departure ends, which is the next frame something here
    /// changes on its own; `None` with nothing leaving, so an idle pane owns no
    /// clock for this.
    #[must_use]
    pub fn ends_in(&self) -> Option<Instant> {
        self.departing.iter().map(|gone| gone.ends).min()
    }

    /// Every note the next collect places: the ones listed and the ones still
    /// leaving, in that order.
    #[must_use]
    pub fn drawn(&self) -> Vec<Note> {
        self.listed
            .iter()
            .chain(self.departing.iter().map(|gone| &gone.note))
            .cloned()
            .collect()
    }

    fn is_departing(&self, id: &str) -> bool {
        self.departing.iter().any(|gone| gone.note.id == id)
    }
}

/// How the agent's line, and a word the agent moved, arrive on a note's rows:
/// from an announcement's ink into the chrome's dim the rows are drawn in, the
/// way the footer's text does. `None` where the depth has flattened the two
/// together.
#[must_use]
pub fn note_arrival(theme: &Theme) -> Option<Effect> {
    let from = theme::contrast(theme.note, theme.chrome_dim)?;
    Some(fx::fade_from_fg(
        from,
        (
            tachyonfx::Duration::from(RESOLVE_ARRIVING),
            Interpolation::SineInOut,
        ),
    ))
}

/// The departure a resolve runs: the agent's line arrives, holds a beat, and
/// the rows dissolve. Where the line cannot fade it holds for the same length,
/// so the departure is one duration on every palette.
#[must_use]
pub fn resolve_departure(theme: &Theme) -> Effect {
    let arrive = note_arrival(theme).unwrap_or_else(|| {
        fx::sleep((
            tachyonfx::Duration::from(RESOLVE_ARRIVING),
            Interpolation::Linear,
        ))
    });
    fx::sequence(&[
        arrive,
        fx::sleep((
            tachyonfx::Duration::from(RESOLVE_BEAT),
            Interpolation::Linear,
        )),
        leaving(),
    ])
}

/// The departure a withdrawal runs, and a note whose file vanished: the rows
/// dissolve, with no line from the agent to show first.
#[must_use]
pub fn leaving() -> Effect {
    fx::dissolve((tachyonfx::Duration::from(LEAVING), Interpolation::Linear))
}

/// Which of a note's cells an effect runs over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Target {
    /// Every row the note draws.
    Rows,
    /// The status word alone.
    Word,
    /// The agent's line alone.
    Reply,
}

impl NoteCells {
    /// The cells `target` names, when this frame drew them.
    #[must_use]
    pub(crate) fn of(&self, target: Target) -> Option<Rect> {
        match target {
            Target::Rows => Some(self.rows),
            Target::Word => self.word,
            Target::Reply => self.reply,
        }
    }
}

/// An effect over one note's cells, found by id on every frame that draws it.
struct NoteEffect {
    id: String,
    target: Target,
    timed: Timed,
}

impl NoteEffect {
    /// The effect `change` arms, or `None` where the palette has nothing to
    /// fade between and the word simply changes, which is the whole of what a
    /// crossfade says.
    fn armed(change: Change, theme: &Theme, now: Instant) -> Option<Self> {
        let (id, target, effect, length) = match change {
            Change::Written(id) => (id, Target::Rows, note_arrival(theme), RESOLVE_ARRIVING),
            Change::Seen(id) => (id, Target::Word, note_arrival(theme), RESOLVE_ARRIVING),
            Change::Replied(id) => (id, Target::Reply, note_arrival(theme), RESOLVE_ARRIVING),
            Change::Resolved(id) => (
                id,
                Target::Rows,
                Some(resolve_departure(theme)),
                RESOLVED_DEPARTURE,
            ),
            Change::Left(id) => (id, Target::Rows, Some(leaving()), LEAVING),
        };
        Some(Self {
            id,
            target,
            timed: Timed::new(effect?, now + length),
        })
    }
}

/// The effects running over notes' cells, one per note and target.
#[derive(Default)]
pub struct NoteEffects {
    running: Vec<NoteEffect>,
}

impl NoteEffects {
    /// Arm one effect per change. A change landing on cells already moving
    /// replaces the effect over them rather than stacking on it.
    pub fn arm(&mut self, changes: Vec<Change>, theme: &Theme, now: Instant) {
        // The whole of a note's rows outranks a word or a line on them, whichever
        // arrives second, or two effects would draw the same cells at once.
        let whole = |effect: &NoteEffect| effect.target == Target::Rows;
        for change in changes {
            let Some(armed) = NoteEffect::armed(change, theme, now) else {
                continue;
            };
            if !whole(&armed)
                && self
                    .running
                    .iter()
                    .any(|running| running.id == armed.id && whole(running))
            {
                continue;
            }
            self.running.retain(|running| {
                running.id != armed.id || (!whole(&armed) && running.target != armed.target)
            });
            self.running.push(armed);
        }
    }

    /// Retire every effect that has run its length.
    pub fn settle(&mut self, now: Instant) {
        self.running.retain(|armed| !armed.timed.spent(now));
    }

    /// Whether anything is running, which is what keeps the frame clock armed.
    #[must_use]
    pub fn is_running(&self) -> bool {
        !self.running.is_empty()
    }

    /// How many effects are live.
    #[must_use]
    pub fn live(&self) -> usize {
        self.running.len()
    }

    /// Advance every effect by `since` over the cells its note drew this frame,
    /// which `cells` names. A note off screen has no cells and its effect waits.
    pub fn draw(&mut self, since: Duration, buf: &mut Buffer, cells: &[NoteCells]) {
        for armed in &mut self.running {
            let Some(found) = cells.iter().find(|cells| cells.id == armed.id) else {
                continue;
            };
            if let Some(over) = found.of(armed.target) {
                armed.timed.draw(since, buf, over);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    //! The ring the box's border draws itself in along, which is arithmetic no
    //! drawn screen can check: a step counted twice or skipped leaves a cell
    //! that never arrives or one that arrives ahead of its neighbours.

    use super::*;

    /// Every cell of `area`'s edge, by the step the sweep gives it.
    fn steps(area: Rect) -> Vec<Option<usize>> {
        (area.top()..area.bottom())
            .flat_map(|y| (area.left()..area.right()).map(move |x| Position::new(x, y)))
            .map(|at| ring_step(area, at))
            .collect()
    }

    #[test]
    fn the_ring_numbers_its_edge_once_each_and_counts_them_all() {
        // The box is drawn at four rows and any width, and clipped to one row
        // or one column by a pane that leaves it nothing else.
        for area in [
            Rect::new(6, 5, 60, 4),
            Rect::new(0, 0, 1, 1),
            Rect::new(3, 2, 1, 9),
            Rect::new(3, 2, 9, 1),
            Rect::new(0, 0, 2, 2),
        ] {
            let mut on_ring: Vec<usize> = steps(area).into_iter().flatten().collect();
            let len = ring_len(area);
            on_ring.sort_unstable();
            assert_eq!(
                on_ring,
                (0..len).collect::<Vec<_>>(),
                "{area:?} numbers its edge {on_ring:?} rather than every step once"
            );
            // And the sweep reaches all of it: at the end of its run every step
            // is drawn, so no cell of the border is left blank behind it.
            assert_eq!(
                ring_drawn(area, 1.0),
                len,
                "{area:?} ends short of its ring"
            );
            assert_eq!(ring_drawn(area, 0.0), 0, "{area:?} starts part way in");
        }
    }

    #[test]
    fn a_cell_inside_the_box_is_on_no_step_of_the_ring() {
        // The inside is the text's, which fades rather than draws in, so a cell
        // the ring claimed would be blanked while the reader is typing in it.
        let area = Rect::new(6, 5, 60, 4);
        assert_eq!(ring_step(area, Position::new(7, 6)), None);
        assert_eq!(ring_step(area, Position::new(64, 6)), None);
        // And a cell outside it is on none either, whichever side it lies past.
        for outside in [
            Position::new(5, 6),
            Position::new(66, 6),
            Position::new(7, 4),
            Position::new(7, 9),
        ] {
            assert_eq!(ring_step(area, outside), None, "{outside:?}");
        }
    }
}
