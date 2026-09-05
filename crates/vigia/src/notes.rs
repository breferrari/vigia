//! What one press on a content row's gutter does to the store, and what the
//! pane keeps of the store between wakes (`SPEC.md` §11.2 B21).

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{Event, MouseButton, MouseEventKind};
use tachyonfx::{Effect, Interpolation, fx};
use vigia_core::{Note, Result, Status, Store};

use crate::input::Regions;
use crate::render::NoteCells;
use crate::theme::Theme;
use crate::view::View;

/// How long the agent's line, or a word the agent moved, takes to crossfade in.
pub const RESOLVE_ARRIVING: Duration = Duration::from_millis(750);

/// How long a resolve's line holds before the rows dissolve.
pub const RESOLVE_BEAT: Duration = Duration::from_millis(3000);

/// How long a note's rows take to dissolve, whichever way the note leaves.
pub const LEAVING: Duration = Duration::from_millis(750);

/// The whole of a resolve's departure, after which the rows are dropped.
pub const RESOLVED_DEPARTURE: Duration = RESOLVE_ARRIVING
    .saturating_add(RESOLVE_BEAT)
    .saturating_add(LEAVING);

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
    /// These notes on the line were withdrawn.
    Withdrawn(Vec<String>),
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
        for id in &marked {
            if let Err(e) = store.remove(id) {
                return Some(Err(e));
            }
        }
        return Some(Ok(Toggled::Withdrawn(
            marked.into_iter().map(str::to_owned).collect(),
        )));
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

/// The one footer alert for files a listing skipped, said when the skipped set
/// differs from `last` and not on every wake the agent causes; `last` is
/// brought up to date. `None` when nothing changed, an emptied set included:
/// files that read again are not news.
#[must_use]
pub fn skipped_alert(skipped: &[(PathBuf, String)], last: &mut Vec<PathBuf>) -> Option<String> {
    let now: Vec<PathBuf> = skipped.iter().map(|(path, _)| path.clone()).collect();
    if now == *last {
        return None;
    }
    *last = now;
    let (path, why) = skipped.first()?;
    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    Some(match skipped.len() - 1 {
        0 => format!("skipped the note file {name}: {why}"),
        more => format!("skipped the note file {name} and {more} more: {why}"),
    })
}

/// What a reload of the store found had moved, each naming the note's id, for
/// the effect that draws it moving.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// The agent listed the note, so its word climbed from *open* to *seen*.
    Seen(String),
    /// The agent answered without closing the note, so its line is new.
    Replied(String),
    /// The agent resolved it: it departs with the agent's line.
    Resolved(String),
    /// It is leaving without the agent's line: the reader withdrew it, another
    /// pane did, or the server pruned it before this pane saw it resolved.
    Left(String),
}

/// A note on its way off the screen: drawn as it was until `ends`, then dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Departing {
    /// The note as it draws while it goes.
    pub note: Note,
    /// When its rows are dropped and the diff below closes up.
    pub ends: Instant,
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
                if before.reply.is_none() && note.reply.is_some() {
                    changes.push(Change::Replied(note.id.clone()));
                }
            }
            listed.push(note);
        }
        // Gone from the store without this pane's press: pruned by the server,
        // withdrawn by another pane, or resolved and pruned inside one wake.
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

    /// The reader withdrew `ids`: each departs without the agent's line. An id
    /// not listed, already leaving among them, is left as it is.
    pub fn withdraw(&mut self, ids: &[String], now: Instant) -> Vec<Change> {
        let (leaving, staying): (Vec<Note>, Vec<Note>) = std::mem::take(&mut self.listed)
            .into_iter()
            .partition(|note| ids.contains(&note.id));
        self.listed = staying;
        leaving
            .into_iter()
            .map(|note| {
                let id = note.id.clone();
                self.departing.push(Departing {
                    note,
                    ends: now + LEAVING,
                });
                Change::Left(id)
            })
            .collect()
    }

    /// Drop every departure that has ended, remembering a resolved one until the
    /// server prunes its file. `true` when something was dropped, so the caller
    /// knows the rows it hands the next collect have changed.
    pub fn settle(&mut self, now: Instant) -> bool {
        let before = self.departing.len();
        for gone in std::mem::take(&mut self.departing) {
            if now < gone.ends {
                self.departing.push(gone);
            } else if gone.note.status == Status::Resolved {
                self.departed.insert(gone.note.id);
            }
        }
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

    /// The notes on their way off the screen.
    pub fn departing(&self) -> impl Iterator<Item = &Departing> {
        self.departing.iter()
    }

    fn is_departing(&self, id: &str) -> bool {
        self.departing.iter().any(|gone| gone.note.id == id)
    }
}

/// How the agent's line, and a word the agent moved, arrive on a note's rows:
/// from an announcement's ink into the chrome's dim the rows are drawn in, the
/// way the footer's text does. `None` where the depth has flattened the two
/// together, since there is no gradient between a colour and itself.
#[must_use]
pub fn note_arrival(theme: &Theme) -> Option<Effect> {
    let from = theme.note.fg?;
    (from != theme.chrome_dim.fg?).then(|| {
        fx::fade_from_fg(
            from,
            (
                tachyonfx::Duration::from(RESOLVE_ARRIVING),
                Interpolation::SineInOut,
            ),
        )
    })
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
pub enum Target {
    /// Every row the note draws.
    Rows,
    /// The status word alone.
    Word,
    /// The agent's line alone.
    Reply,
}

/// An effect over one note's cells, found by id on every frame that draws it.
pub struct NoteEffect {
    /// Whose cells.
    pub id: String,
    /// Which of them.
    pub target: Target,
    effect: Effect,
    /// When it is retired whether or not it ever drew: a note off screen is
    /// never processed, and an effect never processed never reports itself done.
    pub until: Instant,
}

impl NoteEffect {
    /// The effect `change` arms, or `None` where the palette has nothing to
    /// fade between and the word simply changes, which is the whole of what a
    /// crossfade says.
    #[must_use]
    pub fn armed(change: Change, theme: &Theme, now: Instant) -> Option<Self> {
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
    #[must_use]
    pub fn spent(&self, now: Instant) -> bool {
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
    /// replaces that effect rather than stacking on it.
    pub fn arm(&mut self, changes: Vec<Change>, theme: &Theme, now: Instant) {
        for change in changes {
            let Some(armed) = NoteEffect::armed(change, theme, now) else {
                continue;
            };
            self.running
                .retain(|running| !(running.id == armed.id && running.target == armed.target));
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

    /// The effects running, oldest first.
    pub fn iter(&self) -> impl Iterator<Item = &NoteEffect> {
        self.running.iter()
    }
}
