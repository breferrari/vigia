//! What one press on a content row's gutter does to the store (`SPEC.md` §11.2
//! B21).

use std::time::SystemTime;

use ratatui::crossterm::event::{Event, MouseButton, MouseEventKind};
use vigia_core::{Note, Result, Status, Store};

use crate::input::Regions;
use crate::view::View;

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
        for id in &marked {
            if let Err(e) = store.remove(id) {
                return Some(Err(e));
            }
        }
        return Some(Ok(Toggled::Withdrawn(marked.len())));
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
