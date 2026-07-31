//! Colours, in one place so Phase 3 has somewhere to put a palette.
//!
//! The default deliberately uses only the sixteen named ANSI colours. They are
//! the one set every terminal resolves, including the legacy Windows console
//! host that `SPEC.md` §10 leaves as an open question, and they inherit the
//! user's own scheme instead of fighting it. Truecolour, a 256-colour
//! degradation path and anything configurable belong to
//! [#11](https://github.com/breferrari/vigia/issues/11); this type is the seam
//! that work attaches to, not an attempt at it.

use crate::render::Heat;
use ratatui::style::{Color, Modifier, Style};
use vigia_core::{Class, Recency};

/// Every colour the shell draws with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    /// The header and footer lines.
    pub chrome: Style,
    /// Secondary text on those lines: key hints, counts.
    pub chrome_dim: Style,
    /// A changed file's path, at the recency the reader should read it as.
    ///
    /// Three styles rather than one, because `SPEC.md` §5 makes intensity carry
    /// recency: the mockup's dimmed `Cargo.toml` is how the eye finds what moved
    /// without reading anything. Resolved through [`Theme::recency`].
    pub path: Style,
    /// A path that changed inside the glance window but not in the last tick.
    pub path_live: Style,
    /// A path nothing has written since `vigia` started watching.
    pub path_cold: Style,
    /// The `● just changed` label on a file that moved in the last tick.
    pub pulse: Style,
    /// A churn sparkline's blocks.
    pub spark: Style,
    /// A heat-strip slice nothing changed in.
    ///
    /// A track rather than a gap, which is what `assets/preview.svg` draws: an
    /// empty slice is dark, not absent, so the strip's own length stays legible
    /// and a reader can see *how much* of the file is untouched.
    pub heat_track: Style,
    /// A heat-strip slice holding additions.
    pub heat_added: Style,
    /// The same, in this file's busiest half.
    pub heat_added_heavy: Style,
    /// A heat-strip slice holding removals.
    pub heat_removed: Style,
    /// The same, in this file's busiest half.
    pub heat_removed_heavy: Style,
    /// A heat-strip slice holding both.
    pub heat_mixed: Style,
    /// The same, in this file's busiest half.
    pub heat_mixed_heavy: Style,
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

    /// `fn`, `if`, `pub`, `mut`.
    pub keyword: Style,
    /// A type's name.
    pub type_name: Style,
    /// A function's name.
    pub function: Style,
    /// A binding, a parameter, a field.
    pub variable: Style,
    /// A named constant, and a language literal.
    pub constant: Style,
    /// A string literal.
    pub string: Style,
    /// A numeric literal.
    pub number: Style,
    /// A comment.
    pub comment: Style,
}

impl Theme {
    /// The style a file heading is drawn in at `recency`.
    ///
    /// **Three steps, not a fade, and that is a loss rather than a design.**
    /// `SPEC.md` §5.1 asks for a recency *gradient*, and sixteen foreground-only
    /// colours have exactly three intensities to spend: bold, plain and dim.
    /// A real gradient needs the truecolour ramp that
    /// [#11](https://github.com/breferrari/vigia/issues/11) brings, and this is
    /// the seam it attaches to. Recorded out loud for the same reason §11.1
    /// records the diff signal narrowing to the sigil column: §5 makes shape and
    /// colour the whole differentiator, so spending some of it is worth saying.
    pub fn recency(&self, recency: Recency) -> Style {
        match recency {
            Recency::Pulse => self.path,
            Recency::Live => self.path_live,
            Recency::Cold => self.path_cold,
        }
    }

    /// The style one slice of a heat strip is drawn in.
    ///
    /// **Mixed is yellow, and that is a ruling rather than a leftover colour.**
    /// `SPEC.md` §5.1 left the mixed case open because the mockup happens not to
    /// contain one. Every alternative lies: drawing the slice as whichever kind
    /// dominates, or as the rarer one, paints a mixed slice as pure, and telling
    /// addition from removal by position is the strip's entire job. Yellow is
    /// also what a reader already reads as "both" from every diff tool they have
    /// used.
    ///
    /// **Two intensities rather than the picture's three.** The SVG ramps
    /// additions across `#3fb950`, `#56d364` and `#7ee787`; sixteen
    /// foreground-only colours have a normal and a bright of each hue and no
    /// third stop. The same loss §11.1 already records for the diff signal and
    /// the recency ramp, and the same issue closes it:
    /// [#11](https://github.com/breferrari/vigia/issues/11).
    pub fn heat(&self, heat: Heat) -> Style {
        match heat {
            Heat::Cool => self.heat_track,
            Heat::Added { heavy: false } => self.heat_added,
            Heat::Added { heavy: true } => self.heat_added_heavy,
            Heat::Removed { heavy: false } => self.heat_removed,
            Heat::Removed { heavy: true } => self.heat_removed_heavy,
            Heat::Mixed { heavy: false } => self.heat_mixed,
            Heat::Mixed { heavy: true } => self.heat_mixed_heavy,
        }
    }

    /// The style a run of `class` is drawn in.
    ///
    /// [`Class::Plain`] takes [`Theme::context`] whatever line it lands on, and
    /// that is `SPEC.md` §11.1's ruling rather than an oversight: the mockup
    /// colours added, removed and context lines identically and leaves the diff
    /// signal to the sigil, so unclassified text on an added line is *not*
    /// green. What the picture uses instead is a row background tint, which
    /// sixteen foreground-only colours cannot draw, so the signal is thinner
    /// here than it is there until [#11](https://github.com/breferrari/vigia/issues/11)
    /// lands one.
    pub fn class(&self, class: Class) -> Style {
        match class {
            Class::Plain => self.context,
            Class::Keyword => self.keyword,
            Class::Type => self.type_name,
            Class::Function => self.function,
            Class::Variable => self.variable,
            Class::Constant => self.constant,
            Class::String => self.string,
            Class::Number => self.number,
            Class::Comment => self.comment,
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            chrome: Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            chrome_dim: Style::new().fg(Color::DarkGray),
            // Three rungs of one ramp: bright and bold, bright, then plain.
            // `Gray` rather than `DarkGray` for the coldest, deliberately. Every
            // file in an already-dirty worktree is cold until something writes
            // to it, so this is what the **first** frame of a session looks
            // like, and a path drawn in the same near-invisible grey as a
            // comment would open the tool on a screen nobody can read.
            path: Style::new().fg(Color::White).add_modifier(Modifier::BOLD),
            path_live: Style::new().fg(Color::White),
            path_cold: Style::new().fg(Color::Gray),
            // Cyan rather than a diff colour. The pulse says *when*, and green
            // or red beside a path would read as *what*, which the sigil column
            // already means two rows below.
            pulse: Style::new().fg(Color::Cyan),
            spark: Style::new().fg(Color::Cyan),
            // The strip is a map of the file, so an untouched slice has to be
            // visible as untouched rather than absent. DarkGray is the dimmest
            // thing every terminal still draws.
            heat_track: Style::new().fg(Color::DarkGray),
            heat_added: Style::new().fg(Color::Green),
            heat_added_heavy: Style::new().fg(Color::LightGreen),
            heat_removed: Style::new().fg(Color::Red),
            heat_removed_heavy: Style::new().fg(Color::LightRed),
            heat_mixed: Style::new().fg(Color::Yellow),
            heat_mixed_heavy: Style::new().fg(Color::LightYellow),
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

            // The mockup's hues, mapped onto the sixteen names every terminal
            // resolves. `assets/preview.svg` picks salmon for keywords, purple
            // for functions, orange for types, blue for variables and gold for
            // constants, so the bright half of the palette is where most of
            // these land. String, number and comment are not in the picture and
            // are chosen to sit clear of the diff colours: a green string on a
            // red removal would read as an addition.
            keyword: Style::new().fg(Color::LightRed),
            type_name: Style::new().fg(Color::LightYellow),
            function: Style::new().fg(Color::LightMagenta),
            variable: Style::new().fg(Color::LightBlue),
            constant: Style::new().fg(Color::Yellow),
            string: Style::new().fg(Color::LightGreen),
            number: Style::new().fg(Color::LightCyan),
            // The mockup draws comments no differently from its own dimmed
            // text, and a comment is the one thing on a diff line a reader
            // routinely wants to skip.
            comment: Style::new().fg(Color::DarkGray),
        }
    }
}
