//! Which colour every drawn thing gets.
//!
//! `SPEC.md` §11.1 rules that the engine emits **meanings** and the shell colours
//! them, so this is the only place in the project that knows what green is. §5
//! makes shape and colour the whole differentiator, which is what makes a palette
//! load-bearing rather than decoration.
//!
//! ## Two axes, and both have to allow a thing before it is drawn
//!
//! A **palette** decides what may be drawn. A [`Depth`] decides how finely it can
//! be expressed. They are separate because they answer different questions and get
//! different answers: [`Theme::ansi`] refuses a row tint at *every* depth, because
//! a tint has to assume a background and that palette's whole contract is that it
//! assumes none, while [`Theme::dark`] draws one wherever the depth can express it.
//!
//! ## Why `ansi` is still the default
//!
//! It is the only palette that is correct on a terminal whose background nothing
//! has detected. The sixteen names resolve to whatever the reader's own scheme
//! says, so `vigia` matches the pane beside it instead of fighting it, and a
//! reader on a light terminal is not handed a screen authored for a dark one.
//!
//! The cost is real and is the reason `dark` exists: `ansi` cannot draw the row
//! tint `assets/preview.svg` promises, so on the default palette the diff signal
//! is still the sigil column that §11.1 records as a loss. A reader who knows
//! their own background gets the picture by naming it.
//!
//! ## Configuring it
//!
//! `SPEC.md` §11.2 B6 is ruled here: no config file and no flags. `VIGIA_THEME`
//! names a built-in or points at a file, `VIGIA_COLOR` overrides the depth. A
//! monitor is launched from the shell rc that opens the pane, which is where a
//! setting made once belongs, and a config file with one thing in it invites a
//! second thing.

use std::fmt;
use std::path::{Path, PathBuf};

use ratatui::style::{Color, Modifier, Style};
use vigia_core::{Class, Recency};

use crate::colour::Depth;
use crate::render::Heat;

/// Environment variable naming a built-in palette, or a file holding one.
pub const THEME_VAR: &str = "VIGIA_THEME";

/// Declare the palette once, and derive everything that has to agree with it.
///
/// The struct, the key list a theme file is parsed against, the setter that list
/// dispatches through, and the walk [`Theme::resolve`] uses all come from this one
/// place. **"Every field has a key" is then true by construction** rather than by a
/// test that rots the first time someone adds a field and forgets the table, which
/// is the failure this shape exists to make impossible.
macro_rules! palette {
    ($($(#[$doc:meta])* $field:ident),* $(,)?) => {
        /// Every colour the shell draws with.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct Theme {
            $($(#[$doc])* pub $field: Style,)*
        }

        impl Theme {
            /// Every key a theme file may set, in declaration order.
            pub const KEYS: &'static [&'static str] = &[$(stringify!($field)),*];

            /// Set one key, or say it is not one.
            fn set(&mut self, key: &str, style: Style) -> bool {
                match key {
                    $(stringify!($field) => { self.$field = style; true }),*,
                    _ => false,
                }
            }

            /// Every style through `f`, which is how a palette reaches a [`Depth`].
            fn map(self, f: impl Fn(Style) -> Style) -> Self {
                Self { $($field: f(self.$field),)* }
            }
        }
    };
}

palette! {
    /// The header and footer lines.
    chrome,
    /// Secondary text on those lines: key hints, counts, the readouts.
    chrome_dim,

    /// A changed file's path, at the recency the reader should read it as.
    ///
    /// Three styles rather than one, because `SPEC.md` §5 makes intensity carry
    /// recency: the mockup's dimmed `Cargo.toml` is how the eye finds what moved
    /// without reading anything. Resolved through [`Theme::recency`].
    path,
    /// A path that changed inside the glance window but not in the last tick.
    path_live,
    /// A path nothing has written since `vigia` started watching.
    path_cold,
    /// The `● just changed` label on a file that moved in the last tick.
    pulse,
    /// A churn sparkline's blocks.
    spark,

    /// A heat-strip slice nothing changed in.
    ///
    /// A track rather than a gap, which is what `assets/preview.svg` draws: an
    /// empty slice is dark, not absent, so the strip's own length stays legible
    /// and a reader can see *how much* of the file is untouched.
    heat_track,
    /// A heat-strip slice holding additions.
    heat_added,
    /// The same, busier.
    heat_added_warm,
    /// The same, in this file's busiest band.
    heat_added_hot,
    /// A heat-strip slice holding removals.
    heat_removed,
    /// The same, busier.
    heat_removed_warm,
    /// The same, in this file's busiest band.
    heat_removed_hot,
    /// A heat-strip slice holding both.
    heat_mixed,
    /// The same, busier.
    heat_mixed_warm,
    /// The same, in this file's busiest band.
    heat_mixed_hot,

    /// The letter naming what happened to a file.
    kind,
    /// A hunk's `@@` header.
    hunk,
    /// Line numbers.
    gutter,

    /// An added line's sigil.
    added,
    /// A removed line's sigil.
    removed,
    /// An unchanged line shown for orientation.
    context,

    /// The wash behind a whole added line.
    ///
    /// `SPEC.md` §5.1's tinted row, and it is a **background**: a palette that
    /// leaves it unset draws no tint, which is what [`Theme::ansi`] does
    /// deliberately. Dropped below [`Depth::Ansi256`], where an ANSI background is
    /// a slab rather than a wash.
    added_row,
    /// The same, behind a removed line.
    removed_row,
    /// The sigil cell of an added line, which is §5.1's left bar.
    ///
    /// The mockup's bar is three pixels of a nine-pixel cell, so it has no terminal
    /// equivalent that does not spend a column, and I6 forbids spending one on
    /// decoration. The sigil cell is the one cell that already means *this line
    /// changed*, so it carries the bar by inverting: the diff hue behind, the row's
    /// own wash in front.
    added_bar,
    /// The same, on a removed line.
    removed_bar,

    /// A stand-in for content there is no diff for: binary, conflict.
    note,
    /// Something went wrong and the reader should know.
    alert,

    /// `fn`, `if`, `pub`, `mut`.
    keyword,
    /// A type's name.
    type_name,
    /// A function's name.
    function,
    /// A binding, a parameter, a field.
    variable,
    /// A named constant, and a language literal.
    constant,
    /// A string literal.
    string,
    /// A numeric literal.
    number,
    /// A comment.
    comment,
}

/// Shorthand for the palettes below, which are mostly foregrounds.
const fn fg(colour: Color) -> Style {
    Style::new().fg(colour)
}

/// Shorthand for a 24-bit foreground.
const fn rgb(r: u8, g: u8, b: u8) -> Style {
    fg(Color::Rgb(r, g, b))
}

impl Theme {
    /// A built-in palette by name.
    pub fn named(name: &str) -> Option<Self> {
        match name {
            "ansi" => Some(Self::ansi()),
            "dark" => Some(Self::dark()),
            "light" => Some(Self::light()),
            _ => None,
        }
    }

    /// Every built-in name, for an error message that can list them.
    pub const NAMES: [&'static str; 3] = ["ansi", "dark", "light"];

    /// This palette, in colours `depth` can actually show.
    ///
    /// Walked **once**, at startup, and the result stored. Nothing here runs on the
    /// frame path, so I9 never sees it and the renderer keeps drawing with plain
    /// [`Style`] values that already mean what this terminal can show.
    pub fn resolve(self, depth: Depth) -> Self {
        self.map(|style| depth.resolve(style))
    }

    /// The style a file heading is drawn in at `recency`.
    ///
    /// **Three steps, not a fade, and the palette is not what decides that.**
    /// This used to say a real gradient needed the truecolour ramp
    /// [#11](https://github.com/breferrari/vigia/issues/11) brings. It does not,
    /// and `SPEC.md` §11.1 now carries the correction: the number of rungs belongs
    /// to [`Recency`], which has three variants because the store can answer
    /// exactly three questions about a path, and whose `Cold` means *untracked*
    /// rather than *old*. A wider palette draws the same three rungs in better
    /// colours. A fourth would have to mean *how far through the window*, which
    /// nothing computes and which cannot be drawn honestly on a shell that only
    /// wakes when a file changes.
    ///
    /// What #11 did buy is real: these were three intensities borrowed from the
    /// modifier set, and they are now three chosen luminances wherever the depth
    /// can express them.
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
    /// addition from removal by position is the strip's entire job. Yellow is also
    /// what a reader already reads as "both" from every diff tool they have used.
    pub fn heat(&self, heat: Heat) -> Style {
        match heat {
            Heat::Cool => self.heat_track,
            Heat::Added { heavy: false } => self.heat_added,
            Heat::Added { heavy: true } => self.heat_added_hot,
            Heat::Removed { heavy: false } => self.heat_removed,
            Heat::Removed { heavy: true } => self.heat_removed_hot,
            Heat::Mixed { heavy: false } => self.heat_mixed,
            Heat::Mixed { heavy: true } => self.heat_mixed_hot,
        }
    }

    /// The style a run of `class` is drawn in.
    ///
    /// [`Class::Plain`] takes [`Theme::context`] whatever line it lands on, and
    /// that is `SPEC.md` §11.1's ruling rather than an oversight: the mockup
    /// colours added, removed and context lines identically and leaves the diff
    /// signal to the sigil, so unclassified text on an added line is *not* green.
    /// What the picture uses instead is the row tint, which this palette may or may
    /// not carry and which the depth may or may not be able to express.
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

    /// The wash and the bar for a changed line, or nothing where the palette
    /// declines to draw one.
    ///
    /// Two styles rather than one lookup because they land on different cells: the
    /// wash covers the row and the bar covers only the sigil.
    pub fn row(&self, added: bool) -> (Style, Style) {
        if added {
            (self.added_row, self.added_bar)
        } else {
            (self.removed_row, self.removed_bar)
        }
    }

    /// The sixteen named colours, which is what shipped before there was a choice.
    ///
    /// Every colour here is a **name**, never an index or an RGB triple, and that
    /// is the whole point: a name resolves to whatever the reader's terminal scheme
    /// says it is, so this palette inherits their scheme instead of arguing with
    /// it. It is the only one that is correct on a background nothing detected.
    ///
    /// It draws **no row tint at any depth**. A tint has to assume a background,
    /// this palette assumes none, and a solid ANSI block behind syntax-highlighted
    /// text destroys the colours on it. That is `SPEC.md` §11.1's recorded loss,
    /// and on this palette it stands.
    pub fn ansi() -> Self {
        Self {
            chrome: fg(Color::Cyan).add_modifier(Modifier::BOLD),
            chrome_dim: fg(Color::DarkGray),
            // Three rungs of one ramp: bright and bold, bright, then plain.
            // `Gray` rather than `DarkGray` for the coldest, deliberately. Every
            // file in an already-dirty worktree is cold until something writes to
            // it, so this is what the **first** frame of a session looks like, and
            // a path drawn in the same near-invisible grey as a comment would open
            // the tool on a screen nobody can read.
            path: fg(Color::White).add_modifier(Modifier::BOLD),
            path_live: fg(Color::White),
            path_cold: fg(Color::Gray),
            // Cyan rather than a diff colour. The pulse says *when*, and green or
            // red beside a path would read as *what*, which the sigil column
            // already means two rows below.
            pulse: fg(Color::Cyan),
            spark: fg(Color::Cyan),
            // The strip is a map of the file, so an untouched slice has to be
            // visible as untouched rather than absent. DarkGray is the dimmest
            // thing every terminal still draws.
            heat_track: fg(Color::DarkGray),
            // Two stops of hue where the other palettes have three. Sixteen names
            // hold a normal and a bright of each colour and no third, so the middle
            // stop is the normal one and the ramp reads as two. Written out rather
            // than left to the depth ladder, because this palette is authored *in*
            // names and has nothing to quantise.
            heat_added: fg(Color::Green),
            heat_added_warm: fg(Color::Green),
            heat_added_hot: fg(Color::LightGreen),
            heat_removed: fg(Color::Red),
            heat_removed_warm: fg(Color::Red),
            heat_removed_hot: fg(Color::LightRed),
            heat_mixed: fg(Color::Yellow),
            heat_mixed_warm: fg(Color::Yellow),
            heat_mixed_hot: fg(Color::LightYellow),
            kind: fg(Color::Yellow),
            hunk: fg(Color::Blue),
            gutter: fg(Color::DarkGray),
            added: fg(Color::Green),
            removed: fg(Color::Red),
            // Reset rather than a colour: context is most of the screen, and the
            // reader's own foreground is the least distracting thing it can be.
            context: fg(Color::Reset),
            // Unset, which is what "this palette draws no tint" is spelled as.
            added_row: Style::new(),
            removed_row: Style::new(),
            added_bar: Style::new(),
            removed_bar: Style::new(),
            note: fg(Color::Magenta),
            alert: fg(Color::Red).add_modifier(Modifier::BOLD),
            // The mockup's hues, mapped onto the sixteen names every terminal
            // resolves. String, number and comment are not in the picture and are
            // chosen to sit clear of the diff colours: a green string on a red
            // removal would read as an addition.
            keyword: fg(Color::LightRed),
            type_name: fg(Color::LightYellow),
            function: fg(Color::LightMagenta),
            variable: fg(Color::LightBlue),
            constant: fg(Color::Yellow),
            string: fg(Color::LightGreen),
            number: fg(Color::LightCyan),
            // The mockup draws comments no differently from its own dimmed text,
            // and a comment is the one thing on a diff line a reader routinely
            // wants to skip.
            comment: fg(Color::DarkGray),
        }
    }

    /// `assets/preview.svg`, as a palette.
    ///
    /// Every value here is **read out of the picture** rather than chosen, per
    /// `SPEC.md` §5.1's rule that a published artifact answering an open question
    /// is the answer. The picture's own class names map onto these directly:
    /// `.fg` `#e6edf3`, `.dim` `#7d8590`, `.faint` `#6e7681`, `.grn` `#3fb950`,
    /// `.red` `#f85149`, `.cyn` `#39c5cf`, `.kw` `#ff7b72`, `.fnn` `#d2a8ff`,
    /// `.typ` `#ffa657`, `.var` `#79c0ff`, `.con` `#e3b341`, with the row washes
    /// `#0f2c1c` and `#2d1416` and the track `#21262d` taken from the rects.
    ///
    /// Where the picture ramps and we need a third stop it is interpolated in the
    /// picture's own direction: brighter is busier, which is what its sparkline
    /// does.
    ///
    /// **This is the palette that assumes a dark terminal**, and that is why it is
    /// not the default. On a light background its foregrounds are unreadable, and
    /// nothing here can detect which one a reader has.
    pub fn dark() -> Self {
        Self {
            chrome: rgb(0x39, 0xc5, 0xcf).add_modifier(Modifier::BOLD),
            chrome_dim: rgb(0x6e, 0x76, 0x81),
            path: rgb(0xe6, 0xed, 0xf3).add_modifier(Modifier::BOLD),
            path_live: rgb(0xe6, 0xed, 0xf3),
            path_cold: rgb(0x7d, 0x85, 0x90),
            pulse: rgb(0x39, 0xc5, 0xcf),
            // Cyan, where the picture's sparkline is green. The picture draws a
            // *ramp* there and we draw one colour, so it does not answer this; what
            // does is that green already means addition two rows down, and a churn
            // sparkline is about *when*, not *what*.
            spark: rgb(0x39, 0xc5, 0xcf),
            heat_track: rgb(0x21, 0x26, 0x2d),
            heat_added: rgb(0x3f, 0xb9, 0x50),
            heat_added_warm: rgb(0x56, 0xd3, 0x64),
            heat_added_hot: rgb(0x7e, 0xe7, 0x87),
            heat_removed: rgb(0xda, 0x36, 0x33),
            heat_removed_warm: rgb(0xf8, 0x51, 0x49),
            heat_removed_hot: rgb(0xff, 0x7b, 0x72),
            heat_mixed: rgb(0xbb, 0x80, 0x09),
            heat_mixed_warm: rgb(0xe3, 0xb3, 0x41),
            heat_mixed_hot: rgb(0xf2, 0xcc, 0x60),
            kind: rgb(0xe3, 0xb3, 0x41),
            hunk: rgb(0x58, 0xa6, 0xff),
            gutter: rgb(0x6e, 0x76, 0x81),
            added: rgb(0x3f, 0xb9, 0x50),
            removed: rgb(0xf8, 0x51, 0x49),
            context: rgb(0xe6, 0xed, 0xf3),
            // The two rects the picture draws behind changed lines, and the two
            // bars at their left edge. Backgrounds, so the depth ladder drops them
            // below 256 on its own.
            added_row: Style::new().bg(Color::Rgb(0x0f, 0x2c, 0x1c)),
            removed_row: Style::new().bg(Color::Rgb(0x2d, 0x14, 0x16)),
            added_bar: Style::new()
                .fg(Color::Rgb(0x0f, 0x2c, 0x1c))
                .bg(Color::Rgb(0x3f, 0xb9, 0x50)),
            removed_bar: Style::new()
                .fg(Color::Rgb(0x2d, 0x14, 0x16))
                .bg(Color::Rgb(0xf8, 0x51, 0x49)),
            note: rgb(0xd2, 0xa8, 0xff),
            alert: rgb(0xf8, 0x51, 0x49).add_modifier(Modifier::BOLD),
            keyword: rgb(0xff, 0x7b, 0x72),
            type_name: rgb(0xff, 0xa6, 0x57),
            function: rgb(0xd2, 0xa8, 0xff),
            variable: rgb(0x79, 0xc0, 0xff),
            constant: rgb(0xe3, 0xb3, 0x41),
            string: rgb(0xa5, 0xd6, 0xff),
            number: rgb(0x79, 0xc0, 0xff),
            comment: rgb(0x6e, 0x76, 0x81),
        }
    }

    /// The same design against a light terminal.
    ///
    /// Not an inversion of [`Theme::dark`], which does not work: flipping a
    /// luminance ramp leaves saturated hues that were chosen to glow on black
    /// sitting at the same lightness as white paper. The hues are re-picked at
    /// light-background luminance instead, and the **direction of every ramp
    /// flips**: on black, busier is brighter; on white, busier is darker, because
    /// that is which end has contrast to spend.
    pub fn light() -> Self {
        Self {
            chrome: rgb(0x0a, 0x62, 0x6b).add_modifier(Modifier::BOLD),
            chrome_dim: rgb(0x59, 0x63, 0x6e),
            path: rgb(0x1f, 0x23, 0x28).add_modifier(Modifier::BOLD),
            path_live: rgb(0x1f, 0x23, 0x28),
            path_cold: rgb(0x81, 0x8b, 0x98),
            pulse: rgb(0x0a, 0x62, 0x6b),
            spark: rgb(0x0a, 0x62, 0x6b),
            // Light enough to read as a track on white, dark enough to be visible.
            heat_track: rgb(0xd0, 0xd7, 0xde),
            heat_added: rgb(0x4a, 0xc2, 0x6b),
            heat_added_warm: rgb(0x2d, 0xa4, 0x4e),
            heat_added_hot: rgb(0x11, 0x63, 0x29),
            heat_removed: rgb(0xff, 0x81, 0x82),
            heat_removed_warm: rgb(0xcf, 0x22, 0x2e),
            heat_removed_hot: rgb(0x82, 0x07, 0x1e),
            heat_mixed: rgb(0xd4, 0xa7, 0x2c),
            heat_mixed_warm: rgb(0xbf, 0x87, 0x00),
            heat_mixed_hot: rgb(0x7d, 0x4e, 0x00),
            kind: rgb(0xbf, 0x87, 0x00),
            hunk: rgb(0x05, 0x50, 0xae),
            gutter: rgb(0x81, 0x8b, 0x98),
            added: rgb(0x1a, 0x7f, 0x37),
            removed: rgb(0xcf, 0x22, 0x2e),
            context: rgb(0x1f, 0x23, 0x28),
            added_row: Style::new().bg(Color::Rgb(0xda, 0xfb, 0xe1)),
            removed_row: Style::new().bg(Color::Rgb(0xff, 0xeb, 0xe9)),
            added_bar: Style::new()
                .fg(Color::Rgb(0xda, 0xfb, 0xe1))
                .bg(Color::Rgb(0x1a, 0x7f, 0x37)),
            removed_bar: Style::new()
                .fg(Color::Rgb(0xff, 0xeb, 0xe9))
                .bg(Color::Rgb(0xcf, 0x22, 0x2e)),
            note: rgb(0x82, 0x50, 0xdf),
            alert: rgb(0xcf, 0x22, 0x2e).add_modifier(Modifier::BOLD),
            keyword: rgb(0xcf, 0x22, 0x2e),
            type_name: rgb(0x95, 0x38, 0x00),
            function: rgb(0x82, 0x50, 0xdf),
            variable: rgb(0x05, 0x50, 0xae),
            constant: rgb(0x66, 0x39, 0xba),
            string: rgb(0x0a, 0x30, 0x69),
            number: rgb(0x05, 0x50, 0xae),
            comment: rgb(0x59, 0x63, 0x6e),
        }
    }
}

impl Default for Theme {
    /// [`Theme::ansi`]. See the module docs for why it is that one.
    fn default() -> Self {
        Self::ansi()
    }
}

/// Anything that stops a theme from being understood.
///
/// Every variant carries the 1-based line it was found on, because a theme file is
/// the one input here a reader wrote by hand and "something is wrong somewhere" is
/// not an answer they can act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeError {
    /// A key that is not one of [`Theme::KEYS`].
    UnknownKey {
        /// Where.
        line: usize,
        /// What was written.
        key: String,
    },
    /// A colour that is not a hex triple, an index, a name, or `default`.
    UnknownColour {
        /// Where.
        line: usize,
        /// What was written.
        value: String,
    },
    /// A trailing word that is not a modifier.
    UnknownModifier {
        /// Where.
        line: usize,
        /// What was written.
        value: String,
    },
    /// A line with no `=` in it.
    MissingSeparator {
        /// Where.
        line: usize,
        /// What was written.
        text: String,
    },
    /// A `base` naming a palette that does not exist.
    UnknownBase {
        /// Where.
        line: usize,
        /// What was written.
        name: String,
    },
    /// A `base` after the first key it would have overwritten.
    LateBase {
        /// Where.
        line: usize,
    },
    /// The file could not be read at all.
    Unreadable {
        /// Which file.
        path: PathBuf,
        /// Why not.
        why: String,
    },
}

impl fmt::Display for ThemeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownKey { line, key } => write!(
                f,
                "line {line}: {key:?} is not a theme key. `vigia` has {} of them, \
                 and the list is in the theme documentation",
                Theme::KEYS.len()
            ),
            Self::UnknownColour { line, value } => write!(
                f,
                "line {line}: {value:?} is not a colour. Write #rrggbb, a palette \
                 index 0 to 255, one of the sixteen colour names, or `default`"
            ),
            Self::UnknownModifier { line, value } => write!(
                f,
                "line {line}: {value:?} is not a modifier. Write bold, dim, italic, \
                 underline or reverse"
            ),
            Self::MissingSeparator { line, text } => {
                write!(f, "line {line}: {text:?} has no `=` in it")
            }
            Self::UnknownBase { line, name } => write!(
                f,
                "line {line}: there is no built-in theme called {name:?}. There are \
                 three: {}",
                Theme::NAMES.join(", ")
            ),
            Self::LateBase { line } => write!(
                f,
                "line {line}: `base` has to come before any key it would overwrite, \
                 because it replaces the whole palette"
            ),
            Self::Unreadable { path, why } => {
                write!(f, "{}: {why}", path.display())
            }
        }
    }
}

impl std::error::Error for ThemeError {}

/// The palette this process should draw with.
///
/// Resolved **before the screen is taken**, so a theme that does not parse reports
/// on a terminal the reader can still read. That is the same rule `SPEC.md` §11.1
/// already states for a path that is not a repository, and it exists for the same
/// reason: an error painted inside a TUI that then hands the terminal back is an
/// error nobody sees.
pub fn from_env(
    depth: Depth,
    lookup: impl Fn(&str) -> Option<String>,
) -> Result<Theme, ThemeError> {
    let Some(named) = lookup(THEME_VAR).filter(|value| !value.trim().is_empty()) else {
        return Ok(Theme::default().resolve(depth));
    };
    let named = named.trim();

    // A built-in wins over a file of the same name. The three names are short,
    // ordinary words and a file called `dark` in the working directory should not
    // silently take over what `VIGIA_THEME=dark` has always meant.
    if let Some(theme) = Theme::named(named) {
        return Ok(theme.resolve(depth));
    }
    load(Path::new(named)).map(|theme| theme.resolve(depth))
}

/// Read and parse a theme file.
pub fn load(path: &Path) -> Result<Theme, ThemeError> {
    let source = std::fs::read_to_string(path).map_err(|why| ThemeError::Unreadable {
        path: path.to_owned(),
        why: why.to_string(),
    })?;
    parse(&source)
}

/// Parse a theme, which is a `base` and a list of overrides.
///
/// The grammar is one key per line and nothing else:
///
/// ```text
/// # a comment
/// base       = dark
/// added_row  = on #0f2c1c
/// path       = #e6edf3 bold
/// context    = default
/// ```
///
/// A value is `[<colour>] [on <colour>] [<modifier>...]`, where a colour is
/// `#rrggbb`, a palette index `0` to `255`, one of the sixteen names, or `default`.
///
/// **Hand-rolled rather than TOML, and that is a dependency decision.** `toml` is
/// not in the lock file; taking it means `toml`, `toml_edit`, `winnow` and
/// `serde_spanned`, none of which `SPEC.md` names, for a grammar that is one line
/// shape. CLAUDE.md's rule is that a dependency reaches the spec before it reaches
/// a manifest, and this surface does not earn the argument.
///
/// **An unknown key is refused rather than ignored.** A silently dropped key is a
/// theme that does nothing, and "it was discarded" is the one explanation a reader
/// cannot arrive at by looking at their screen.
pub fn parse(source: &str) -> Result<Theme, ThemeError> {
    let mut theme = Theme::default();
    let mut touched = false;

    for (index, raw) in source.lines().enumerate() {
        let line = index + 1;
        let text = raw.trim();
        // A blank line, or one that is nothing but a comment. Checked on the raw
        // line rather than after any `#` handling, because a value's first token
        // may legitimately begin with `#` and a key never does.
        if text.is_empty() || text.starts_with('#') {
            continue;
        }

        let Some((key, value)) = text.split_once('=') else {
            return Err(ThemeError::MissingSeparator {
                line,
                text: text.to_owned(),
            });
        };
        let key = key.trim();
        // Comments are stripped from the *value* only, and only after the `=`, so
        // `added = #3fb950 # the picture's green` works and a bare `#` line is
        // still a comment.
        let value = strip_comment(value.trim());

        if key == "base" {
            if touched {
                return Err(ThemeError::LateBase { line });
            }
            theme = Theme::named(value).ok_or_else(|| ThemeError::UnknownBase {
                line,
                name: value.to_owned(),
            })?;
            continue;
        }

        let style = style_of(value, line)?;
        if !theme.set(key, style) {
            return Err(ThemeError::UnknownKey {
                line,
                key: key.to_owned(),
            });
        }
        touched = true;
    }

    Ok(theme)
}

/// Drop a trailing comment from a value, without eating a leading `#rrggbb`.
///
/// A value's first token may begin with `#`, so a naive split on `#` turns
/// `#3fb950` into an empty value. The rule that works is: a `#` starts a comment
/// only when it is **preceded by whitespace**, which is how every hex-colour
/// configuration format resolves the same collision.
fn strip_comment(value: &str) -> &str {
    let bytes = value.as_bytes();
    for (i, byte) in bytes.iter().enumerate() {
        if *byte == b'#' && i > 0 && bytes[i - 1].is_ascii_whitespace() {
            return value[..i].trim_end();
        }
    }
    value
}

/// `[<colour>] [on <colour>] [<modifier>...]`.
fn style_of(value: &str, line: usize) -> Result<Style, ThemeError> {
    let mut style = Style::new();
    let mut words = value.split_whitespace().peekable();

    // A leading `on` means this value sets only a background, which is how a row
    // wash is written: `added_row = on #0f2c1c`.
    if words.peek().is_some_and(|word| *word != "on") {
        let word = words.next().unwrap_or_default();
        if let Some(colour) = colour_of(word) {
            style = style.fg(colour);
        } else if !is_modifier(word) {
            return Err(ThemeError::UnknownColour {
                line,
                value: word.to_owned(),
            });
        } else {
            // It was a modifier all along, so put it back by applying it here.
            style = apply_modifier(style, word);
        }
    }

    if words.peek() == Some(&"on") {
        words.next();
        let word = words.next().unwrap_or_default();
        let colour = colour_of(word).ok_or_else(|| ThemeError::UnknownColour {
            line,
            value: word.to_owned(),
        })?;
        style = style.bg(colour);
    }

    for word in words {
        if !is_modifier(word) {
            return Err(ThemeError::UnknownModifier {
                line,
                value: word.to_owned(),
            });
        }
        style = apply_modifier(style, word);
    }
    Ok(style)
}

fn is_modifier(word: &str) -> bool {
    matches!(
        word,
        "bold" | "dim" | "italic" | "underline" | "reverse"
    )
}

fn apply_modifier(style: Style, word: &str) -> Style {
    let modifier = match word {
        "bold" => Modifier::BOLD,
        "dim" => Modifier::DIM,
        "italic" => Modifier::ITALIC,
        "underline" => Modifier::UNDERLINED,
        "reverse" => Modifier::REVERSED,
        _ => return style,
    };
    style.add_modifier(modifier)
}

/// `#rrggbb`, `0` to `255`, one of the sixteen names, or `default`.
fn colour_of(word: &str) -> Option<Color> {
    if let Some(hex) = word.strip_prefix('#') {
        if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let byte = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16).ok();
        return Some(Color::Rgb(byte(0)?, byte(2)?, byte(4)?));
    }
    if let Ok(index) = word.parse::<u8>() {
        return Some(Color::Indexed(index));
    }
    Some(match word {
        "default" | "reset" => Color::Reset,
        "black" => Color::Black,
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" => Color::Magenta,
        "cyan" => Color::Cyan,
        "grey" | "gray" => Color::Gray,
        "bright-black" | "dark-grey" | "dark-gray" => Color::DarkGray,
        "bright-red" => Color::LightRed,
        "bright-green" => Color::LightGreen,
        "bright-yellow" => Color::LightYellow,
        "bright-blue" => Color::LightBlue,
        "bright-magenta" => Color::LightMagenta,
        "bright-cyan" => Color::LightCyan,
        "white" | "bright-white" => Color::White,
        _ => return None,
    })
}
