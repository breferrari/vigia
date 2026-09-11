//! Reading the drawn screen, and the key space a sweep over the keymap walks:
//! what more than one test binary needs.

// Each test binary uses a different subset, and a binary that used all of it
// would be a binary asking every question this file answers.
#![allow(dead_code)]

use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::Color;
use vigia::{Action, FileEntry, ListRow, Regions, Theme, View, action_for};

/// Columns of row `y` between `from` and `to` whose glyph is one of `symbols`
/// **and** whose foreground is one of `colours`.
pub fn rows_of(buf: &Buffer, area: ratatui::layout::Rect) -> Vec<String> {
    (area.top()..area.bottom())
        .map(|y| {
            let mut row = String::new();
            let mut covered = 0usize;
            for x in area.left()..area.right() {
                if covered > 0 {
                    covered -= 1;
                    continue;
                }
                let symbol = buf[(x, y)].symbol();
                row.push_str(symbol);
                covered = ratatui::text::Span::raw(symbol).width().saturating_sub(1);
            }
            row.trim_end().to_owned()
        })
        .collect()
}

pub fn columns_in(
    buf: &Buffer,
    y: u16,
    columns: std::ops::Range<u16>,
    colours: &[Color],
    symbols: &[char],
) -> Vec<u16> {
    columns
        .filter(|x| {
            let cell = &buf[(*x, y)];
            symbols
                .iter()
                .any(|glyph| cell.symbol() == glyph.to_string())
                && cell.style().fg.is_some_and(|fg| colours.contains(&fg))
        })
        .collect()
}

/// Every colour the heat strip can draw a slice in, its track included.
pub fn heat_colours(theme: &Theme) -> Vec<Color> {
    [
        theme.heat_track,
        theme.heat_added,
        theme.heat_added_warm,
        theme.heat_added_hot,
        theme.heat_removed,
        theme.heat_removed_warm,
        theme.heat_removed_hot,
        theme.heat_mixed,
        theme.heat_mixed_warm,
        theme.heat_mixed_hot,
    ]
    .iter()
    .filter_map(|style| style.fg)
    .collect()
}

/// Every colour a sparkline **bar** can be drawn in.
pub fn spark_colours(theme: &Theme) -> Vec<Color> {
    [theme.spark, theme.spark_warm, theme.spark_hot]
        .into_iter()
        .filter_map(|style| style.fg)
        .collect()
}

/// Every file the pinned list draws, skipping the run separators.
pub fn listed_files(view: &View) -> impl Iterator<Item = &FileEntry> {
    view.list.iter().filter_map(ListRow::entry)
}

/// Every key event a sweep over the keymap walks.
///
/// One list rather than one per sweeping binary: a gate whose candidate space
/// is narrower than the keymap passes over the binding it cannot reach, and two
/// hand-kept spaces drift without a compile error to say so.
pub fn candidate_keys() -> Vec<KeyEvent> {
    let mut codes: Vec<KeyCode> = (b' '..=b'~').map(|c| KeyCode::Char(c as char)).collect();
    codes.extend([
        KeyCode::Up,
        KeyCode::Down,
        KeyCode::Left,
        KeyCode::Right,
        KeyCode::Home,
        KeyCode::End,
        KeyCode::PageUp,
        KeyCode::PageDown,
        KeyCode::Enter,
        KeyCode::Esc,
        KeyCode::Backspace,
        KeyCode::Tab,
        KeyCode::Delete,
        KeyCode::Insert,
    ]);
    codes.extend((1..=12).map(KeyCode::F));
    let mods = [
        KeyModifiers::NONE,
        KeyModifiers::SHIFT,
        KeyModifiers::CONTROL,
        KeyModifiers::ALT,
    ];
    codes
        .into_iter()
        .flat_map(|code| mods.iter().map(move |m| KeyEvent::new(code, *m)))
        .collect()
}

/// Where a gesture's launch state is set, as the reason it is or is not a key of
/// the view-defaults config file.
#[derive(Debug)]
pub enum Place {
    /// The file's own key for it, which `config::KEYS` has to carry, beside the
    /// gesture that flips the same state for a session.
    Key {
        /// What a reader writes in the file.
        key: &'static str,
        /// What a reader presses, as `README.md`'s key table spells it.
        gesture: &'static str,
    },
    /// A toggle kept out of the file on purpose. Carries why, because a toggle
    /// absent from the file is absent by ruling or by oversight and nothing else
    /// tells the two apart.
    Excluded(&'static str),
    /// Not a view toggle: what it does is no state a launch could start in.
    /// Carries why, so the classification can be argued with rather than only read.
    Neither(&'static str),
}

/// Where each action's launch state is set.
///
/// Exhaustive, with no wildcard arm, and that is the gate rather than any
/// assertion over it: a gesture added later stops this module compiling, and with
/// it every binary that reads it, until somebody has said which of the three it
/// is. `sheet.rs::reach_of` holds the gestures sheet the same way, one surface
/// over.
pub fn place_of(action: &Action) -> Place {
    match action {
        // The gestures sheet's `view` section once follow is taken out of it.
        Action::ToggleRail => Place::Key {
            key: "rail",
            gesture: "r",
        },
        Action::ToggleSingle => Place::Key {
            key: "single",
            gesture: "s",
        },
        Action::ToggleOverview => Place::Key {
            key: "overview",
            gesture: "o",
        },
        Action::ToggleStaged => Place::Key {
            key: "staged",
            gesture: "a",
        },
        Action::ToggleWrap => Place::Key {
            key: "wrap",
            gesture: "w",
        },
        // The sheet's `notes` section.
        Action::ToggleNotes => Place::Key {
            key: "notes",
            gesture: "c",
        },
        // The exclusion list.
        Action::ToggleStanding => Place::Excluded(
            "the pane opens on what the agent in the other pane just wrote, and a \
             file able to open it somewhere else would make the thesis a setting. \
             `b` is how a session says otherwise, which is a session's choice \
             about a session. `SPEC.md` §11.2 B6",
        ),
        // A key of the file since `SPEC.md` §11.2 B22, whose exclusion
        // `REVOCATIONS.md` holds: remembering what this reader pressed is not a
        // file configuring I5 away for one who never asked.
        Action::ToggleFollow => Place::Key {
            key: "follow",
            gesture: "f",
        },
        Action::ToggleSheet | Action::CloseSheet => Place::Neither(
            "the sheet is drawn over the pane and put away again, so there is no \
             pane a launch could start inside one of",
        ),
        Action::ToggleMenu
        | Action::CloseMenu
        | Action::MenuMove(_)
        | Action::MenuFlip
        | Action::MenuRow(_) => Place::Neither(
            "the config menu is the sheet's shape one overlay over: drawn over the              pane and put away again, and where its caret sits is no state a launch              could start in. `SPEC.md` §11.2 B22",
        ),
        // The gesture is `m` for both, because the menu is one gesture for every
        // row rather than a key each. Neither comes out of the keymap sweep, so
        // this arm documents rather than gates.
        Action::ToggleIcons => Place::Key {
            key: "icons",
            gesture: "m",
        },
        Action::ToggleLinks => Place::Key {
            key: "links",
            gesture: "m",
        },
        Action::TogglePersist => Place::Key {
            key: "persist",
            gesture: "m",
        },
        Action::MenuReset => Place::Neither(
            "putting every toggle back is an act rather than a state, so there is no              pane a launch could start inside one of",
        ),
        Action::Scroll(_)
        | Action::ScrollList(_)
        | Action::Page(_)
        | Action::HalfPage(_)
        | Action::File(_)
        | Action::Top
        | Action::Bottom
        | Action::ListTo(_)
        | Action::ListRow(_)
        | Action::DiffTo(_) => Place::Neither(
            "a move, and where the pane sits is I5's to decide rather than a \
             setting's",
        ),
        Action::Quit | Action::Escape | Action::Redraw => {
            Place::Neither("nothing they leave behind is a state a launch could hold")
        }
    }
}

/// Every action a key of the pane produces, one per variant.
///
/// Taken from the keymap rather than listed here, so a toggle bound to a new key
/// is walked without this file being told about it.
pub fn actions_keys_reach() -> Vec<Action> {
    let mut seen = std::collections::HashSet::new();
    let mut found: Vec<Action> = Vec::new();
    for event in candidate_keys() {
        let Some(action) = action_for(&Event::Key(event), Regions::default()) else {
            continue;
        };
        if seen.insert(std::mem::discriminant(&action)) {
            found.push(action);
        }
    }
    found
}
