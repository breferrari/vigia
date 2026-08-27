//! Which drawing glyphs this terminal's font can be asked for, and what the
//! sparkline becomes at each rung.
//!
//! No terminal reports its font's coverage, and a tofu box measures one column
//! exactly as a real glyph does. So this reads `TERM_PROGRAM`/`TERM` against a
//! table of what was probed by hand, and every rung above the floor needs a
//! positive answer: an over-claimed colour paints the wrong colour, an
//! over-claimed glyph paints tofu.
//!
//! | Rung | Buckets per cell | Levels above the baseline |
//! |---|---|---|
//! | [`Glyphs::Block`] | 1 | 8 |
//! | [`Glyphs::Braille`] | 2 | 3 |
//! | [`Glyphs::Octant`] | 2 | 3 |
//!
//! Braille and octants are the same 2x4 grid with identical arithmetic, differing
//! only in whether the dots are dots. So this does not derive `Ord`, where
//! [`Depth`](crate::Depth) does: `Octant` is not more than `Braille`.
//!
//! No measured font carries U+1CD00; terminals that draw octants do so natively,
//! which nothing in the environment distinguishes, so [`Glyphs::Octant`] is
//! reachable only through [`GLYPHS_VAR`] and detection never returns it.

use std::fmt;

use ratatui::symbols;

use crate::colour::{names, override_of};

/// Environment variable that overrides detection outright.
pub const GLYPHS_VAR: &str = "VIGIA_GLYPHS";

/// A churn bucket's height, emptiest first.
pub(crate) const SPARK_RAMP: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// A bucket nothing happened in.
const SPARK_TRACK: char = '_';

/// Rows of dots a 2x4 cell has.
const DENSE_ROWS: usize = 4;

/// Which glyphs the sparkline may be drawn from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Glyphs {
    /// The eighth-blocks `▁▂▃▄▅▆▇█`, one bucket per cell at eight levels.
    #[default]
    Block,
    /// The braille patterns U+2800..U+28FF, two buckets per cell.
    Braille,
    /// The Unicode 16 octants U+1CD00.., two buckets per cell, solid.
    Octant,
}

/// A [`GLYPHS_VAR`] this does not understand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlyphsError {
    /// What was found in the variable.
    pub value: String,
}

impl fmt::Display for GlyphsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{GLYPHS_VAR}: {:?} is not one of auto, block, braille, octant",
            self.value
        )
    }
}

impl std::error::Error for GlyphsError {}

impl Glyphs {
    /// Decide the rung from the environment.
    pub fn from_env(
        windows: bool,
        lookup: impl Fn(&str) -> Option<String>,
    ) -> Result<Self, GlyphsError> {
        // Through [`override_of`], which owns the set-but-empty rule for both
        // ladders: it is a discovered PowerShell gotcha rather than a choice, and
        // a copy here would be a second place for a later correction to miss.
        if let Some((raw, value)) = override_of(&lookup, GLYPHS_VAR) {
            match value.as_str() {
                "block" => return Ok(Self::Block),
                "braille" => return Ok(Self::Braille),
                "octant" => return Ok(Self::Octant),
                "auto" => {}
                _ => return Err(GlyphsError { value: raw }),
            }
        }

        // `NO_COLOR` is not consulted: it asks for no *colour*, and a glyph is
        // not colour. Same reading that keeps `Depth::None` carrying bold.
        let term = lookup("TERM").unwrap_or_default().to_ascii_lowercase();
        if term == "dumb" || term == "linux" {
            return Ok(Self::Block);
        }

        if let Some(glyphs) = octant_of(&lookup, &term) {
            return Ok(glyphs);
        }

        if let Some(glyphs) = lookup("TERM_PROGRAM")
            .as_deref()
            .map(str::trim)
            .and_then(program_glyphs)
        {
            return Ok(glyphs);
        }
        if windows && lookup("WT_SESSION").is_some() {
            return Ok(Self::Braille);
        }
        if BRAILLE_TERMS.iter().any(|name| names(&term, name)) {
            return Ok(Self::Braille);
        }
        if windows {
            return Ok(Self::Block);
        }
        Ok(Self::Braille)
    }

    /// [`Glyphs::from_env`] against this process.
    pub fn detect() -> Result<Self, GlyphsError> {
        Self::from_env(cfg!(windows), |key| std::env::var(key).ok())
    }

    /// Buckets one drawn cell carries.
    pub const fn density(self) -> usize {
        match self {
            Self::Block => 1,
            Self::Braille | Self::Octant => 2,
        }
    }

    /// Ramp levels a bucket may take, the baseline excluded.
    pub const fn levels(self) -> usize {
        match self {
            Self::Block => 8,
            Self::Braille | Self::Octant => DENSE_ROWS - 1,
        }
    }

    /// The glyph one cell draws, for the buckets at `left` and `right`.
    pub fn glyph(self, left: usize, right: usize) -> char {
        let table: &[char; 256] = match self {
            Self::Block => {
                // The ramp has no rung for "nothing happened", by
                // [`SPARK_TRACK`]'s own ruling, so the floor is a different
                // glyph rather than the ramp's lowest.
                return match left.min(self.levels()) {
                    0 => SPARK_TRACK,
                    level => SPARK_RAMP[level - 1],
                };
            }
            Self::Braille => &symbols::braille::BRAILLE,
            Self::Octant => &symbols::pixel::OCTANTS,
        };
        table[usize::from(Self::column(left, 0) | Self::column(right, 1))]
    }

    /// One column of a dense cell: the baseline, plus `level` rows climbing it.
    const fn column(level: usize, col: u8) -> u8 {
        /// Left column, emptiest first: baseline, then rows 2, 1 and 0.
        const CLIMB: [u8; DENSE_ROWS] = [0b0100_0000, 0b0101_0000, 0b0101_0100, 0b0101_0101];
        // Clamped rather than trusted: the caller scales against a peak, and a
        // peak is data. Shifting by an unclamped level would index a bit that
        // means another row entirely.
        let level = if level < DENSE_ROWS {
            level
        } else {
            DENSE_ROWS - 1
        };
        CLIMB[level] << col
    }
}
/// The engines that draw the octant range themselves, by version.
fn octant_of(lookup: &impl Fn(&str) -> Option<String>, term: &str) -> Option<Glyphs> {
    if term.starts_with("tmux") || term.starts_with("screen") {
        return None;
    }
    let version = |key: &str| lookup(key).and_then(|raw| version_of(&raw));
    let program = lookup("TERM_PROGRAM")
        .map(|p| p.trim().to_ascii_lowercase())
        .unwrap_or_default();

    // One row per engine that self-draws the range and says which version it
    // is, so the table the docblock promises to grow grows by a row. `foot`
    // joins the day it exports a version; see above for why not before.
    for (named, term_entry, floor) in [
        ("ghostty", "xterm-ghostty", (1, 2)),
        ("kitty", "xterm-kitty", (0, 40)),
    ] {
        if (program == named || names(term, term_entry))
            && version("TERM_PROGRAM_VERSION").is_some_and(|v| v >= floor)
        {
            return Some(Glyphs::Octant);
        }
    }
    // VTE's own convention: a single number, 7802 for 0.78.2. Carried by every
    // VTE terminal (GNOME Terminal, Ptyxis, and the rest of that family).
    if let Some(raw) = lookup("VTE_VERSION") {
        if raw.trim().parse::<u32>().is_ok_and(|v| v >= 7800) {
            return Some(Glyphs::Octant);
        }
    }
    None
}

/// `"1.3.1-arch2"` to `(1, 3)`: the leading major and minor, or `None`.
fn version_of(raw: &str) -> Option<(u32, u32)> {
    let mut parts = raw.trim().split(['.', '-']);
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    Some((major, minor))
}

/// What `TERM_PROGRAM` names, and whether that terminal's font has braille.
fn program_glyphs(program: &str) -> Option<Glyphs> {
    // Lowercased because the values are brand names and are spelled as such.
    match program.to_ascii_lowercase().as_str() {
        "apple_terminal" | "ghostty" | "hyper" | "iterm.app" | "rio" | "tabby" | "vscode"
        | "warpterminal" | "wezterm" => Some(Glyphs::Braille),
        _ => None,
    }
}

/// Terminals whose own terminfo entry is the whole signal.
const BRAILLE_TERMS: [&str; 7] = [
    "alacritty",
    "contour",
    "foot",
    "rio",
    "wezterm",
    "xterm-ghostty",
    "xterm-kitty",
];
