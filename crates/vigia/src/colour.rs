//! How many colours this terminal has, and what a palette becomes when it has
//! fewer than the palette was written in.
//!
//! `SPEC.md` §5 makes shape and colour the whole differentiator, so a palette that
//! assumed truecolour would be a palette that silently degrades to noise on the
//! terminals that do not have it. §10 named the case and left it open: *"Truecolor
//! needs Win10+, and legacy conhost degrades."*
//!
//! ## The ladder is a product, not a fallback
//!
//! [`Theme`](crate::Theme) decides **what** may be drawn and this decides **how
//! finely it can be expressed**. Both have to allow an element before it appears,
//! and keeping them separate is what makes this a mechanism rather than a second
//! hand-written palette: a theme is authored once, in the colours it means, and
//! every rung below is derived from it.
//!
//! The rungs are ordered and each loses exactly one thing:
//!
//! | Rung | Foreground | Background |
//! |---|---|---|
//! | [`Depth::Truecolor`] | 24-bit, as authored | as authored |
//! | [`Depth::Ansi256`] | quantised to the 256-colour palette | quantised |
//! | [`Depth::Ansi16`] | nearest of the sixteen names | **dropped** |
//! | [`Depth::None`] | dropped | dropped |
//!
//! **Background is dropped a rung above foreground**, and that is the one asymmetry
//! worth arguing. `SPEC.md` §5.1 records that an ANSI background is a solid block
//! rather than a tint: at sixteen colours the darkest available green behind a line
//! of code is a slab, not the wash `assets/preview.svg` draws. A slab is worse than
//! nothing, because it destroys the syntax colours sitting on it. So the row tint
//! stops one rung earlier than the text does, and the diff signal narrows back to
//! the sigil column exactly as it did before #11.
//!
//! ## Modifiers survive every rung
//!
//! [`Depth::None`] exists for `NO_COLOR`, which asks for no *colour*. Bold is not
//! colour. Dropping it too would take the one distinction left on a monochrome
//! terminal, and there is no reading of the convention that asks for that.
//!
//! ## Quantising happens once
//!
//! [`Theme::resolve`](crate::Theme::resolve) walks the palette a single time at
//! startup and stores the result. Nothing here runs on the frame path, so I9 never
//! sees it, and the renderer keeps drawing with plain [`Style`] values that already
//! mean what this terminal can show.

use std::fmt;

use ratatui::style::{Color, Style};

/// Environment variable that overrides detection outright.
pub const DEPTH_VAR: &str = "VIGIA_COLOR";

/// How many distinct colours this terminal can be asked for.
///
/// Ordered from fewest to most, so `>=` is a capability test and a comparison
/// reads the way the ladder does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum Depth {
    /// Draw no colour at all. `NO_COLOR`, or `TERM=dumb`.
    None,
    /// The sixteen named colours every terminal resolves. Backgrounds are dropped
    /// here: see the module docs.
    ///
    /// The default when nothing says otherwise, and deliberately not the top rung.
    /// An over-claim paints colours a terminal cannot show; an under-claim merely
    /// looks flatter than it had to. See [`Depth::from_env`].
    #[default]
    Ansi16,
    /// The xterm 256-colour palette: sixteen names, a 6x6x6 cube, 24 greys.
    Ansi256,
    /// 24-bit colour, which is what every built-in palette is authored in.
    Truecolor,
}

/// A `VIGIA_COLOR` this does not understand.
///
/// Refused rather than ignored, for the same reason
/// [`ThemeError`](crate::ThemeError) refuses an unknown key: a reader who set a
/// variable and got no effect has no way to find out why, and "it was silently
/// discarded" is the one answer they cannot guess.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepthError {
    /// What was found in the variable.
    pub value: String,
}

impl fmt::Display for DepthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{DEPTH_VAR}: {:?} is not one of auto, never, 16, 256, truecolor",
            self.value
        )
    }
}

impl std::error::Error for DepthError {}

impl Depth {
    /// Decide the depth from the environment.
    ///
    /// `windows` is a parameter rather than a `cfg!` so the precedence table can be
    /// driven from both sides on one machine. `SPEC.md` §7's rule for a decision
    /// with no reachable integration path: extract it, and test the function.
    ///
    /// First answer wins, and the order is the argument:
    ///
    /// 1. **`VIGIA_COLOR`**, because a reader who set it has already been told
    ///    wrong by everything below. `auto` falls through rather than meaning a
    ///    rung, which is what makes it possible to unset the override in a child
    ///    shell without unsetting the variable.
    /// 2. **`NO_COLOR`**, non-empty, per the convention. Below the override so that
    ///    `VIGIA_COLOR` can still ask for colour in one pane of a session that sets
    ///    it globally.
    /// 3. **`TERM=dumb`**, which is a terminal saying it cannot do this.
    /// 4. **`COLORTERM`** of `truecolor` or `24bit`. The only positive signal for
    ///    24-bit that exists; there is no terminfo capability for it that is
    ///    reliably present.
    /// 5. **`TERM` containing `256color`**, which is terminfo's own spelling.
    /// 6. **Windows with `WT_SESSION`**, which is Windows Terminal identifying
    ///    itself. It has done 24-bit since it shipped.
    /// 7. **Windows otherwise**, at 256. This is §10's *"legacy conhost degrades"*
    ///    written as a rung: conhost accepts 24-bit sequences and approximates them,
    ///    so claiming truecolour there produces colours nobody chose.
    /// 8. **Sixteen**, which is what the palette was before any of this existed and
    ///    is the one answer that is never actively wrong.
    pub fn from_env(
        windows: bool,
        lookup: impl Fn(&str) -> Option<String>,
    ) -> Result<Self, DepthError> {
        if let Some(raw) = lookup(DEPTH_VAR) {
            let value = raw.trim().to_ascii_lowercase();
            match value.as_str() {
                "never" | "none" | "0" => return Ok(Self::None),
                "16" | "ansi" => return Ok(Self::Ansi16),
                "256" => return Ok(Self::Ansi256),
                "truecolor" | "truecolour" | "24bit" => return Ok(Self::Truecolor),
                "auto" => {}
                _ => return Err(DepthError { value: raw }),
            }
        }

        if lookup("NO_COLOR").is_some_and(|value| !value.is_empty()) {
            return Ok(Self::None);
        }

        let term = lookup("TERM").unwrap_or_default();
        if term == "dumb" {
            return Ok(Self::None);
        }

        let colorterm = lookup("COLORTERM").unwrap_or_default().to_ascii_lowercase();
        if colorterm == "truecolor" || colorterm == "24bit" {
            return Ok(Self::Truecolor);
        }
        if term.contains("256color") {
            return Ok(Self::Ansi256);
        }
        if windows {
            // `WT_SESSION` is set by Windows Terminal and by nothing else, so its
            // absence is not evidence of conhost: it is also absent under every
            // third-party host. 256 is the answer that is safe for both.
            return Ok(if lookup("WT_SESSION").is_some() {
                Self::Truecolor
            } else {
                Self::Ansi256
            });
        }
        Ok(Self::Ansi16)
    }

    /// [`Depth::from_env`] against this process.
    pub fn detect() -> Result<Self, DepthError> {
        Self::from_env(cfg!(windows), |key| std::env::var(key).ok())
    }

    /// `style`, in colours this depth can actually show.
    ///
    /// Modifiers pass through untouched at every rung: see the module docs.
    pub fn resolve(self, style: Style) -> Style {
        let mut out = style;
        out.fg = style.fg.map(|colour| self.colour(colour));
        out.bg = match self {
            // Dropped rather than mapped to `Reset`. `None` means "leave whatever is
            // behind this cell", which is the reader's own background; `Reset` means
            // "the terminal's default", which is not the same thing inside a pane
            // that has been given one.
            Self::None | Self::Ansi16 => Option::None,
            _ => style.bg.map(|colour| self.colour(colour)),
        };
        out
    }

    /// One colour, at this depth.
    fn colour(self, colour: Color) -> Color {
        match self {
            Self::None => Color::Reset,
            Self::Truecolor => colour,
            Self::Ansi256 => match colour {
                Color::Rgb(r, g, b) => Color::Indexed(to_indexed(r, g, b)),
                other => other,
            },
            Self::Ansi16 => match colour {
                Color::Rgb(r, g, b) => to_ansi16(r, g, b),
                // Below sixteen the indexed palette *is* the named one, so an index
                // in that range is already expressible and is left alone rather than
                // round-tripped through RGB and back to a possibly different name.
                Color::Indexed(i) if i < 16 => Color::Indexed(i),
                Color::Indexed(i) => {
                    let (r, g, b) = from_indexed(i);
                    to_ansi16(r, g, b)
                }
                other => other,
            },
        }
    }
}

/// The six levels each axis of the xterm colour cube can take.
const CUBE: [u8; 6] = [0, 95, 135, 175, 215, 255];

/// Nearest xterm palette index to a 24-bit colour.
///
/// Two candidates, because the palette has two regions that can win: the 6x6x6
/// cube at 16..232, and the 24-step greyscale ramp at 232..256. A near-grey lands
/// far better on the ramp, whose steps are ten apart, than on the cube, whose
/// darkest three levels are 95 apart. Picking the cube unconditionally is the
/// common shortcut and it is what turns a dim grey comment into black.
///
/// The sixteen named colours are deliberately **not** candidates. They are whatever
/// the reader's terminal theme says they are, so treating them as fixed RGB and
/// matching against them would hand back a colour that means something else here.
pub fn to_indexed(r: u8, g: u8, b: u8) -> u8 {
    let axis = |v: u8| {
        CUBE.iter()
            .enumerate()
            .min_by_key(|(_, level)| v.abs_diff(**level))
            .map_or(0, |(i, _)| i)
    };
    let (ri, gi, bi) = (axis(r), axis(g), axis(b));
    let cube = 16 + 36 * ri + 6 * gi + bi;
    let cube_err = distance(
        (r, g, b),
        (CUBE[ri], CUBE[gi], CUBE[bi]),
    );

    // The ramp runs 8, 18, ... 238, which is `8 + 10 * i` for 24 steps.
    let level = (u32::from(r) + u32::from(g) + u32::from(b)) / 3;
    let step = (level.saturating_sub(8) + 5) / 10;
    let step = step.min(23) as u8;
    let grey = 8 + 10 * u32::from(step);
    let grey_err = distance((r, g, b), (grey as u8, grey as u8, grey as u8));

    if grey_err < cube_err {
        232 + step
    } else {
        cube as u8
    }
}

/// The RGB an xterm palette index stands for, for indices that have a fixed one.
///
/// Only 16..256 do. Below that the answer belongs to the reader's terminal theme,
/// and this is never called with one: [`Depth::colour`] keeps those as they are.
fn from_indexed(i: u8) -> (u8, u8, u8) {
    if i >= 232 {
        let grey = 8 + 10 * (i - 232);
        return (grey, grey, grey);
    }
    let i = i - 16;
    (
        CUBE[usize::from(i / 36)],
        CUBE[usize::from((i / 6) % 6)],
        CUBE[usize::from(i % 6)],
    )
}

/// The named colour a 24-bit one should be drawn as.
///
/// **Not a nearest-neighbour search, and that is the whole point.** Matching by
/// distance against [`NAMED`] is the obvious implementation and it was written
/// first; it sends the mockup's `#3fb950` addition green to **cyan**, because the
/// palette's `green` is `#008000` with no blue at all while the input has 80 of it,
/// and cyan's 128 is nearer to 80 than zero is. Every weighting tried had a case
/// like it, because the real problem is not the metric: the sixteen entries are
/// *dark saturated primaries* and the colours a modern palette is authored in are
/// light desaturated ones, so the nearest entry to any of them is decided by
/// lightness long before it is decided by hue. A reader glancing at a diff reads
/// hue.
///
/// So hue is chosen first and lightness second, which is the order the eye uses.
///
/// 1. **Achromatic input takes a grey.** Below a chroma of 32 there is no hue to
///    preserve, and the four greys are picked by luma.
/// 2. **Hue is three bits**, one per channel, set when that channel sits in the
///    upper two thirds of the input's own chroma range. A third rather than a half:
///    at a half the mockup's `#d2a8ff` function purple loses its red bit by one
///    unit and draws blue.
/// 3. **Bright is `max >= 0xc0`**, on the raw maximum rather than luma, because the
///    thing that makes a terminal's bright variant right for a colour is that some
///    channel is near full.
///
/// What this deliberately does not promise is that nine syntax classes stay nine
/// colours. They cannot: `#ffa657` and `#e3b341` are both yellow to this, and at
/// sixteen entries that is arithmetic rather than a choice. The `ansi` palette is
/// the answer for a sixteen-colour terminal and hand-picks all nine. What must
/// survive is the diff signal, and `tests/colour.rs` asserts that separately.
pub fn to_ansi16(r: u8, g: u8, b: u8) -> Color {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let chroma = max - min;
    // Green carries most of perceived brightness, blue least. The 2/5/1 split is
    // the cheap integer form of that and only has to rank, never to be accurate.
    let luma = (2 * u32::from(r) + 5 * u32::from(g) + u32::from(b)) / 8;

    if chroma < 32 {
        return match luma {
            0..=0x2f => Color::Black,
            0x30..=0x7f => Color::DarkGray,
            0x80..=0xcf => Color::Gray,
            _ => Color::White,
        };
    }

    let cut = min + chroma / 3;
    let bright = max >= 0xc0;
    // At least one channel is above `cut` and at least one is not, since `max` is
    // always above it and `min` never is, so the all-set and all-clear patterns
    // cannot occur and the match below is total over what can reach it.
    match (r > cut, g > cut, b > cut) {
        (true, false, false) if bright => Color::LightRed,
        (true, false, false) => Color::Red,
        (false, true, false) if bright => Color::LightGreen,
        (false, true, false) => Color::Green,
        (false, false, true) if bright => Color::LightBlue,
        (false, false, true) => Color::Blue,
        (true, true, false) if bright => Color::LightYellow,
        (true, true, false) => Color::Yellow,
        (true, false, true) if bright => Color::LightMagenta,
        (true, false, true) => Color::Magenta,
        (false, true, true) if bright => Color::LightCyan,
        (false, true, true) => Color::Cyan,
        (true, true, true) | (false, false, false) => Color::Gray,
    }
}

/// Squared distance between two colours.
///
/// Used only by [`to_indexed`], to choose between the cube candidate and the grey
/// candidate, and plain Euclidean is right there where it is wrong in
/// [`to_ansi16`]: both candidates are already close to the input by construction,
/// so the comparison is between two small errors of the same kind rather than
/// between a hue and a lightness.
fn distance(a: (u8, u8, u8), b: (u8, u8, u8)) -> u32 {
    let d = |x: u8, y: u8| {
        let e = u32::from(x.abs_diff(y));
        e * e
    };
    d(a.0, b.0) + d(a.1, b.1) + d(a.2, b.2)
}
