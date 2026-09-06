//! What one press on a content row's gutter does to the store, and what the
//! pane keeps of the store between wakes (`SPEC.md` §11.2 B21).

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{Event, MouseButton, MouseEventKind};
use ratatui::layout::Rect;
use tachyonfx::{Effect, Interpolation, fx};
use vigia_core::{Listing, Note, Result, Status, Store};

use crate::input::Regions;
use crate::render::NoteCells;
use crate::theme::{self, Theme};
use crate::view::View;
use crate::{NOTICE_ARRIVING, NOTICE_LINGER};

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

/// What a press on a content row's gutter did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Toggled {
    /// A note with an empty body was written, under this id.
    Written(String),
    /// This many notes on the line were withdrawn.
    Withdrawn(usize),
}

/// Act on a press at row `offset` of `view`: `None` off a content row; on an
/// unmarked line, write the anchor alone; on a marked one, withdraw every note
/// pinned there, which is how one line holds one open note.
///
/// # Errors
///
/// The store could not write or remove a file. A removal that fails partway
/// leaves the earlier ones done, so the caller reads the store back either way.
pub fn toggle(store: &Store, view: &View, offset: usize) -> Option<Result<Toggled>> {
    let anchor = view.anchor_at(offset)?;
    let marked = view.marked_at(offset);
    if !marked.is_empty() {
        let mut withdrawn = 0;
        for id in &marked {
            // The screen the press landed on may be a frame behind the store: a
            // note the agent has resolved since keeps its file, because the resolve
            // landed first and its line is what the next frame shows leaving. The
            // check sits right before the removal and no closer; two processes
            // with no lock between them leave the removal itself as the window a
            // resolve can still slip into, as the store's own rewrite says.
            if matches!(store.get(id), Ok(Some(note)) if note.status == Status::Resolved) {
                continue;
            }
            if let Err(e) = store.remove(id) {
                return Some(Err(e));
            }
            withdrawn += 1;
        }
        return Some(Ok(Toggled::Withdrawn(withdrawn)));
    }
    let note = Note {
        id: Store::new_id(),
        path: anchor.path,
        side: anchor.side,
        line: anchor.line,
        text: anchor.text,
        body: String::new(),
        status: Status::Open,
        reply: None,
        written: SystemTime::now(),
    };
    Some(store.put(&note).map(|()| Toggled::Written(note.id)))
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
    effect: Effect,
    /// When it is retired whether or not it ever drew: a note off screen is
    /// never processed, and an effect never processed never reports itself done.
    until: Instant,
}

impl NoteEffect {
    /// The effect `change` arms, or `None` where the palette has nothing to
    /// fade between and the word simply changes, which is the whole of what a
    /// crossfade says.
    fn armed(change: Change, theme: &Theme, now: Instant) -> Option<Self> {
        let (id, target, effect, length) = match change {
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
            effect: effect?,
            until: now + length,
        })
    }

    /// Whether the effect has run its length, by its own count or by the clock.
    fn spent(&self, now: Instant) -> bool {
        now >= self.until || self.effect.done()
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
        self.running.retain(|armed| !armed.spent(now));
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
                armed.effect.process(since.into(), buf, over);
            }
        }
    }
}
