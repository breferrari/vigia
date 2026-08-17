//! Which drawing glyphs this terminal's font can be asked for, and what the
//! sparkline becomes at each rung.
//!
//! `SPEC.md` §10's Windows bullet asked for this and said it could not be built:
//! *"`Depth` is a ladder with a gate on every rung, and there is no equivalent
//! for characters."*
//!
//! ## That sentence is true of probing and false of deciding
//!
//! No terminal reports which glyphs its font carries. There is no escape
//! sequence for it, and the cursor-position trick detects a glyph's *width*
//! rather than its presence, so a missing glyph rendered as tofu measures one
//! column exactly as a present one does.
//!
//! But [`Depth`](crate::Depth) never asked a terminal anything either. It reads
//! `TERM_PROGRAM` and `TERM`, which is the terminal **naming itself**, and looks
//! the name up in a table of what somebody checked. That mechanism transfers
//! whole. What does not transfer is the direction of the safe error: an
//! over-claimed colour paints a colour nobody chose, where an over-claimed glyph
//! paints **tofu**, which is worse than flat. So every rung above the floor here
//! rests on a positive answer.
//!
//! ## The rungs are two densities, not three capabilities
//!
//! | Rung | Buckets per cell | Levels above the baseline |
//! |---|---|---|
//! | [`Glyphs::Block`] | 1 | 8 |
//! | [`Glyphs::Braille`] | 2 | 3 |
//! | [`Glyphs::Octant`] | 2 | 3 |
//!
//! Braille and octants are the same 2x4 grid and differ only in whether the dots
//! are dots or solid; their arithmetic is identical. **So this deliberately does
//! not derive `Ord`**, where [`Depth`](crate::Depth) does: there `>=` is a
//! capability test and the ladder is totally ordered, and here `Octant` is not
//! "more" than `Braille`, it is the same rung spelled differently for a font
//! that has one and not the other.
//!
//! ## What was measured, because this bullet has been wrong twice
//!
//! Probed through `System.Windows.Media.GlyphTypeface`, 2026-08-17:
//!
//! - **Consolas carries none of U+2800**, and Lucida Console and Courier New
//!   carry none either. Consolas is what the legacy console draws with, so
//!   Windows outside Windows Terminal takes the floor.
//! - **Cascadia Mono and Cascadia Code carry all of it.** That is what Windows
//!   Terminal ships, and `WT_SESSION` is how it says so.
//! - **No font measured carries U+1CD00**, the octants, including the newest
//!   Cascadia; `microsoft/cascadia-code#711` is still open. The terminals that
//!   draw them do so natively rather than from the font, which nothing in the
//!   environment distinguishes. So [`Glyphs::Octant`] is reachable **only**
//!   through [`GLYPHS_VAR`] and detection never returns it.
//!
//! The Windows rung is therefore the exact inverse of [`Depth`](crate::Depth)'s,
//! which claims 24-bit for the whole platform. That is not an inconsistency:
//! colour aged out on that console and glyphs did not, because the font did not
//! age with it.

use std::fmt;

use ratatui::symbols;

use crate::colour::names;

/// Environment variable that overrides detection outright.
pub const GLYPHS_VAR: &str = "VIGIA_GLYPHS";

/// Rows of dots a 2x4 cell has.
const DENSE_ROWS: usize = 4;

/// Which glyphs the sparkline may be drawn from.
///
/// See the module docs for why this is not `Ord` where
/// [`Depth`](crate::Depth) is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Glyphs {
    /// The eighth-blocks `▁▂▃▄▅▆▇█`, one bucket per cell at eight levels.
    ///
    /// **The default, and the one answer that is never actively wrong.** Two of
    /// the eight are inside CP437 and six are not, which `SPEC.md` §10 records:
    /// a legacy console loses the ramp's *shape* here, and would lose the
    /// element entirely at any rung above.
    #[default]
    Block,
    /// The braille patterns U+2800..U+28FF, two buckets per cell.
    ///
    /// What detection returns wherever the font stack is known to carry them.
    Braille,
    /// The Unicode 16 octants U+1CD00.., two buckets per cell, solid.
    ///
    /// **Never returned by [`Glyphs::from_env`] without the override**, because
    /// no font measured carries them. See the module docs.
    Octant,
}

/// A [`GLYPHS_VAR`] this does not understand.
///
/// Refused rather than ignored, which is
/// [`DepthError`](crate::DepthError)'s rule and its reasoning unchanged: a
/// reader who set a variable and got no effect has no way to find out why, and
/// "it was silently discarded" is the one answer they cannot guess.
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
    ///
    /// `windows` is a parameter rather than a `cfg!` for
    /// [`Depth::from_env`](crate::Depth::from_env)'s reason: `SPEC.md` §7's rule
    /// for a decision with no reachable integration path is to extract it and
    /// test the function, and both sides of the platform split have to be
    /// drivable on one machine.
    ///
    /// First answer wins, and the order is the argument:
    ///
    /// 1. **[`GLYPHS_VAR`]**, because a reader who set it has already been told
    ///    wrong by everything below. `auto` falls through rather than naming a
    ///    rung, so an override can be unset in a child shell without unsetting
    ///    the variable.
    /// 2. **`TERM=dumb`**, a terminal saying it cannot do this.
    /// 3. **`TERM=linux`**, the Linux virtual console, whose font is a 256 or
    ///    512 glyph bitmap that has never held braille. This is the one rung
    ///    with no colour analogue, and it is why `btop` carries a `tty` mode
    ///    beside its `braille` and `block` ones.
    /// 4. **`TERM_PROGRAM`**, a terminal naming itself. See [`program_glyphs`].
    /// 5. **Windows with `WT_SESSION`**, which is Windows Terminal identifying
    ///    itself, and it ships Cascadia. Measured: see the module docs.
    /// 6. **`TERM` naming a terminal that ships its own entry.** See
    ///    [`BRAILLE_TERMS`].
    /// 7. **Windows otherwise, at [`Glyphs::Block`]**, because the console this
    ///    reaches draws with Consolas and Consolas has no braille. Measured.
    /// 8. **[`Glyphs::Braille`] otherwise**, which is the broad claim on this
    ///    ladder and the one worth stating a reason for. Braille has been in
    ///    every mainstream Unix monospace face for twenty years, the one Unix
    ///    terminal that cannot draw it is caught at rung 3, and `btop` defaults
    ///    to braille on every platform with no detection at all and puts the
    ///    font in its README prerequisites. A reader whose font is the exception
    ///    says so with [`GLYPHS_VAR`], which is the same escape the Windows rung
    ///    of [`Depth`](crate::Depth) leans on.
    pub fn from_env(
        windows: bool,
        lookup: impl Fn(&str) -> Option<String>,
    ) -> Result<Self, GlyphsError> {
        // **Set-but-empty is the same as unset**, which `VIGIA_COLOR` learned
        // the hard way: PowerShell's `$env:X = ''` leaves the variable set and
        // empty and a child process sees it, so without this filter a reader who
        // cleared the variable stops the shell from starting over a value nobody
        // gave.
        if let Some(raw) = lookup(GLYPHS_VAR).filter(|value| !value.trim().is_empty()) {
            let value = raw.trim().to_ascii_lowercase();
            match value.as_str() {
                "block" | "blocks" => return Ok(Self::Block),
                "braille" => return Ok(Self::Braille),
                "octant" | "octants" => return Ok(Self::Octant),
                "auto" => {}
                _ => return Err(GlyphsError { value: raw }),
            }
        }

        // **`NO_COLOR` is deliberately not consulted.** It asks for no *colour*,
        // and a glyph is not colour: honouring it here would take the sparkline's
        // shape from a reader who asked only for its ink, which is the same
        // reading that keeps `Depth::None` carrying bold.
        let term = lookup("TERM").unwrap_or_default().to_ascii_lowercase();
        if term == "dumb" || term == "linux" {
            return Ok(Self::Block);
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
    ///
    /// **Three at the dense rungs rather than four, and that is derived rather
    /// than chosen.** A 2x4 cell has four dot rows and the bottom one is spent
    /// on the baseline, because the sparkline's track may not be the ramp's own
    /// floor: "one write" and "no writes" is the distinction the element exists
    /// to make, and inside a cell shared by two buckets a per-column track has
    /// nowhere else to live. So a column draws one dot when nothing happened and
    /// four when it is full, which is the separation by height that `_` buys
    /// under the block ramp.
    pub const fn levels(self) -> usize {
        match self {
            Self::Block => 8,
            Self::Braille | Self::Octant => DENSE_ROWS - 1,
        }
    }

    /// The glyph one dense cell draws, its two buckets at `left` and `right`.
    ///
    /// `None` at [`Glyphs::Block`], where a bucket **is** a cell and the
    /// eighth-block ramp indexes it directly with no packing to do.
    ///
    /// Both levels are clamped into `0..=levels()` rather than trusted, because
    /// the caller's arithmetic scales against a peak and a peak is data.
    ///
    /// **The two tables are indexed identically**, which is what makes one
    /// geometry serve both: `ratatui`'s own docs say the symbols are listed
    /// "according to the corresponding bit pattern in row-major order", so bit
    /// `row * 2 + col` with the top row first. Confirmed against both:
    /// `BRAILLE[4]` is `⠂`, dot 2, the second row's left cell, and `OCTANTS[192]`
    /// is `▂`, the bottom row across.
    pub fn cell(self, left: usize, right: usize) -> Option<char> {
        let table: &[char; 256] = match self {
            Self::Block => return None,
            Self::Braille => &symbols::braille::BRAILLE,
            Self::Octant => &symbols::pixel::OCTANTS,
        };
        let mask = self.column(left, 0) | self.column(right, 1);
        Some(table[usize::from(mask)])
    }

    /// One column of a dense cell: the baseline, plus `level` rows climbing it.
    fn column(self, level: usize, col: u8) -> u8 {
        // Row 3 across, which is bits 6 and 7. Always lit, so an empty bucket
        // draws a track rather than a gap.
        let mut mask = 1u8 << (6 + col);
        for step in 0..level.min(self.levels()) as u8 {
            // Rows 2, 1 then 0, which is two bits down the mask per step.
            mask |= 1u8 << (4 - 2 * step + col);
        }
        mask
    }
}

/// What `TERM_PROGRAM` names, and whether that terminal's font has braille.
///
/// **Deliberately a separate table from
/// `colour.rs`'s `program_depth`, and the entries coinciding
/// today is not a reason to share one.** The criterion is different: that one
/// asks how many colours a terminal draws, this one asks what its font carries.
/// `Apple_Terminal` is the proof rather than a hypothetical, because it already
/// answers differently on the two ladders: it is the one entry there that is
/// *not* truecolour, and it is braille here, since Menlo and SF Mono both carry
/// U+2800. A shared table returning one rung could not express that, and a
/// shared table of *names* would silently gain a wrong entry the first time
/// somebody adds a terminal to the other list for the other reason.
///
/// **Only positive answers**, which is that table's discipline and matters more
/// here: a program not named returns nothing and falls through, because this is
/// evidence about terminals somebody checked rather than a claim about the ones
/// nobody has.
fn program_glyphs(program: &str) -> Option<Glyphs> {
    // Lowercased because the values are brand names and are spelled as such.
    match program.to_ascii_lowercase().as_str() {
        "apple_terminal" | "ghostty" | "hyper" | "iterm.app" | "rio" | "tabby" | "vscode"
        | "warpterminal" | "wezterm" => Some(Glyphs::Braille),
        _ => None,
    }
}

/// Terminals whose own terminfo entry is the whole signal.
///
/// Every entry ships its own `TERM`, and every one of them draws braille and
/// always has. Separate from
/// `colour.rs`'s `TRUECOLOR_TERMS` for
/// [`program_glyphs`]'s reason, and the same discipline applies: a terminal that
/// merely *usually* has a braille-carrying font belongs behind [`GLYPHS_VAR`],
/// not here.
///
/// Suffixed rather than substringed through [`names`], so `foot-extra` and
/// `alacritty-direct` are the same terminal and a `TERM` that merely has the
/// word in it is not.
const BRAILLE_TERMS: [&str; 7] = [
    "alacritty",
    "contour",
    "foot",
    "rio",
    "wezterm",
    "xterm-ghostty",
    "xterm-kitty",
];
