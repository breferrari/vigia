//! Colours, in one place so Phase 3 has somewhere to put a palette.
//!
//! The default deliberately uses only the sixteen named ANSI colours. They are
//! the one set every terminal resolves, including the legacy Windows console
//! host that `SPEC.md` §10 leaves as an open question, and they inherit the
//! user's own scheme instead of fighting it. Truecolour, a 256-colour
//! degradation path and anything configurable belong to
//! [#11](https://github.com/breferrari/vigia/issues/11); this type is the seam
//! that work attaches to, not an attempt at it.

use ratatui::style::{Color, Modifier, Style};

/// Every colour the shell draws with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    /// The header and footer lines.
    pub chrome: Style,
    /// Secondary text on those lines: key hints, counts.
    pub chrome_dim: Style,
    /// A changed file's path.
    pub path: Style,
    /// The letter naming what happened to a file.
    pub kind: Style,
    /// A hunk's `@@` header.
    pub hunk: Style,
    /// Line numbers.
    pub gutter: Style,
    /// An added line.
    pub added: Style,
    /// A removed line.
    pub removed: Style,
    /// An unchanged line shown for orientation.
    pub context: Style,
    /// A stand-in for content there is no diff for: binary, conflict.
    pub note: Style,
    /// Something went wrong and the reader should know.
    pub alert: Style,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            chrome: Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            chrome_dim: Style::new().fg(Color::DarkGray),
            path: Style::new().fg(Color::White).add_modifier(Modifier::BOLD),
            kind: Style::new().fg(Color::Yellow),
            hunk: Style::new().fg(Color::Blue),
            gutter: Style::new().fg(Color::DarkGray),
            added: Style::new().fg(Color::Green),
            removed: Style::new().fg(Color::Red),
            // Reset rather than a colour: context is most of the screen, and
            // the reader's own foreground is the least distracting thing it can
            // be. Naming it explicitly still gives #11 something to override.
            context: Style::new().fg(Color::Reset),
            note: Style::new().fg(Color::Magenta),
            alert: Style::new().fg(Color::Red).add_modifier(Modifier::BOLD),
        }
    }
}
