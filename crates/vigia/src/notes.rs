//! The box a press on a content row's gutter opens, what Enter and Esc do with
//! it, and what the pane keeps of the store between wakes (`SPEC.md` §11.2
//! B21).

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{
    Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};
use ratatui::layout::{Position, Rect};
use ratatui::style::Style;
use ratatui_textarea::{CursorMove, Input, TextArea};
use tachyonfx::Effect;
use tachyonfx::pattern::AnyPattern;
use vigia_core::{CONTEXT, Listing, Note, Origin, Result, Status, Store};

use crate::input::Regions;
use crate::motion::{self, BOX_ARRIVING, LEAVING, RESOLVE_ARRIVING, RESOLVED_DEPARTURE, Timed};
use crate::render::NoteCells;
use crate::theme::{self, Theme};
use crate::view::{Anchor, View};

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
    /// The run the reader pressed in, which a note has no field for: the store
    /// holds one placement and this holds a gesture.
    pub origin: Origin,
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
    ///
    /// Tabs are expanded on the way in rather than left for the wrap, which
    /// prices one at its stop while the buffer draws it as nothing and would
    /// stand the caret a column off its own character. A paste is the only way
    /// one arrives, since `Tab` is the editor's own soft indent.
    pub fn paste(&mut self, text: &str) -> bool {
        self.editor.insert_str(crate::render::detabbed(text))
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
            origin: self.anchor.origin,
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

/// The working-tree lines within [`CONTEXT`] of `centre` in `path`, numbered,
/// and none when the file cannot be read.
///
/// Both rungs to the agent read it, so a note it meets over the socket and the
/// same note it lists over MCP show it one neighbourhood. Only the working-tree
/// side: a removed line is nowhere in the file, and what the server puts around
/// one comes from the diff it already holds.
#[must_use]
pub fn around(workdir: &Path, path: &str, centre: u32) -> Vec<(u32, String)> {
    let Ok(text) = fs::read_to_string(workdir.join(path)) else {
        return Vec::new();
    };
    let first = centre.saturating_sub(CONTEXT).max(1);
    let last = centre.saturating_add(CONTEXT);
    text.lines()
        .enumerate()
        .map(|(at, line)| (u32::try_from(at + 1).unwrap_or(u32::MAX), line))
        .filter(|(number, _)| (first..=last).contains(number))
        .map(|(number, line)| (number, line.to_owned()))
        .collect()
}

/// The radial edge's softness where a note's cells arrive, in cells.
pub const TRANSITION: f32 = 10.0;

/// Columns the sweep's leading edge is soft over as it clears a note away.
pub const SWEEP: u32 = 35;

/// How a note's cells arrive, and how they leave.
fn evolving(ink: Style, over: Duration) -> Effect {
    motion::evolving(ink, over, TRANSITION)
}

fn sweeping(over: Duration) -> Effect {
    motion::sweeping(over, SWEEP)
}

/// How the box arrives: the reader's words evolving in behind its frame's ink,
/// over what a changed file takes.
#[must_use]
pub fn box_entrance(theme: &Theme) -> Effect {
    evolving(theme.note_frame, BOX_ARRIVING)
}

/// How the box leaves on Esc.
#[must_use]
pub fn box_exit() -> Effect {
    sweeping(BOX_ARRIVING)
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
    /// The agent resolved it: the agent's line arrives over the rows, which
    /// then hold it until [`Change::Swept`].
    Resolved(String),
    /// A resolve's beat has run and its rows are going.
    Swept(String),
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
    /// When the sweep over its rows is armed, while a resolve is still holding
    /// the agent's line; `None` once it has been armed, and from the start for a
    /// departure with no line to show first.
    ///
    /// The beat is a deadline rather than a `sleep` inside the effect because an
    /// effect that has not finished keeps the loop asking for a frame every
    /// `ARRIVING_FRAME`: a minute held inside one motion is a minute of paints
    /// of a surface that is not moving.
    holds: Option<Instant>,
    /// When its rows are dropped and the diff below closes up.
    ends: Instant,
}

/// What one settle came to: whether the rows the next collect places moved, the
/// resolves whose beat has run, and the resolved notes whose departure has run
/// and whose files the caller now owns the removal of.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Settled {
    /// A departure ended, so the drawn set is not what it was.
    pub changed: bool,
    /// Ids whose beat ended this turn, so the sweep over their rows is armed.
    pub sweeping: Vec<String>,
    /// Resolved ids whose files are to be removed, in the order they ended.
    pub prune: Vec<String>,
}

/// What the pane holds of the store: the notes as last listed, the ones leaving,
/// and the resolved ones already gone from the screen whose files have not been
/// removed yet, so nothing resolved is ever drawn twice.
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
                // The sweep comes out of the departure rather than after it,
                // which is the rule the footer's linger keeps too.
                self.departing.push(Departing {
                    note,
                    holds: Some(now + RESOLVED_DEPARTURE - LEAVING),
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
                holds: None,
                ends: now + LEAVING,
            });
        }
        self.listed = listed;
        changes
    }

    /// End every beat that has run, drop every departure that has ended, and
    /// name the resolved ones whose files are now the caller's to remove.
    ///
    /// The pane is what knows a resolve has been drawn, so the pane is what
    /// prunes: the server leaves a resolved file where it is precisely because
    /// it cannot tell a note the reader watched leave from one written while
    /// there was no pane open at all. A resolved id is remembered either way, so
    /// a removal that fails costs the file rather than a second departure.
    pub fn settle(&mut self, now: Instant) -> Settled {
        let before = self.departing.len();
        let departed = &mut self.departed;
        let mut prune = Vec::new();
        self.departing.retain(|gone| {
            if now < gone.ends {
                return true;
            }
            if gone.note.status == Status::Resolved {
                departed.insert(gone.note.id.clone());
                prune.push(gone.note.id.clone());
            }
            false
        });
        // After the drop and not before it, so a turn that finds both deadlines
        // spent takes the rows away rather than arming a sweep over rows that
        // are no longer drawn. The loop is offered the beat's end as its own
        // deadline, so that turn is a pane which was blocked past the whole
        // departure, or one that met the resolve already older than it.
        let mut sweeping = Vec::new();
        for gone in &mut self.departing {
            if gone.holds.is_some_and(|until| now >= until) {
                gone.holds = None;
                sweeping.push(gone.note.id.clone());
            }
        }
        Settled {
            changed: self.departing.len() != before,
            sweeping,
            prune,
        }
    }

    /// When a beat next runs out or a departure next ends, which is the next
    /// frame something here changes on its own; `None` with nothing leaving, so
    /// an idle pane owns no clock for this.
    #[must_use]
    pub fn ends_in(&self) -> Option<Instant> {
        self.departing
            .iter()
            .map(|gone| gone.holds.unwrap_or(gone.ends))
            .min()
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

/// How a word the agent moved arrives: from an announcement's ink into the
/// chrome's dim it is drawn in, the way the footer's text does. `None` where
/// the depth has flattened the two together, and then the word simply changes,
/// which is the whole of what a crossfade says.
#[must_use]
pub fn word_arrival(theme: &Theme) -> Option<Effect> {
    let from = theme::contrast(theme.note, theme.chrome_dim)?;
    Some(motion::fading(
        from,
        RESOLVE_ARRIVING,
        AnyPattern::default(),
        false,
    ))
}

/// How a resolve's departure begins: the agent's line arriving over the rows.
///
/// The beat after it and the sweep that ends it are the ledger's, not this
/// effect's, so the pane runs no motion while the line is simply being read.
#[must_use]
pub fn resolve_arrival(theme: &Theme) -> Effect {
    evolving(theme.note_reply, RESOLVE_ARRIVING)
}

/// How a note's rows leave: swept away, with no line from the agent to show
/// first. A resolve's own sweep, once its beat has run, and the whole of a
/// withdrawal's departure and of one whose file vanished.
#[must_use]
pub fn leaving() -> Effect {
    sweeping(LEAVING)
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
    /// The motion `change` arms, or `None` for the one change that needs two
    /// inks to say anything: a word crossfading on a palette that has none.
    fn armed(change: Change, theme: &Theme, now: Instant) -> Option<Self> {
        let (id, target, motion) = match change {
            // The rows arrive in the ink of the state they arrive in, which for
            // a note the reader just wrote or rewrote is always open.
            Change::Written(id) => (
                id,
                Target::Rows,
                evolving(theme.note_open, RESOLVE_ARRIVING),
            ),
            Change::Seen(id) => (id, Target::Word, word_arrival(theme)?),
            Change::Replied(id) => (
                id,
                Target::Reply,
                evolving(theme.note_reply, RESOLVE_ARRIVING),
            ),
            Change::Resolved(id) => (id, Target::Rows, resolve_arrival(theme)),
            Change::Swept(id) | Change::Left(id) => (id, Target::Rows, leaving()),
        };
        Some(Self {
            id,
            target,
            timed: Timed::armed(motion, now),
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
