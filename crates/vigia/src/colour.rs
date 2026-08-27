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
//! | [`Depth::Ansi256`] | quantised to the 256-colour palette | **dropped** |
//! | [`Depth::Ansi16`] | nearest of the sixteen names | dropped |
//! | [`Depth::None`] | dropped | dropped |
//!
//! **A background needs 24-bit and a foreground does not**, and that asymmetry is
//! the one thing here worth arguing. `SPEC.md` §5.1 records that an ANSI background
//! is a solid block rather than a tint: the darkest available green behind a line of
//! code is a slab, not the wash `assets/preview.svg` draws, and a slab is worse than
//! nothing because it destroys the syntax colours sitting on it.
//!
//! **That argument was applied at sixteen and it holds one rung higher, which took a
//! screen to establish.** The 256-colour cube has six levels per axis and its darkest
//! two are 0 and 95, so a *subtle* colour has nowhere to land: the wash `#1b3d29`
//! quantises to `#005f00` and `#45222a` to `#5f0000`, which are saturated primaries
//! at roughly two and a half times the authored luminance. On a hunk or two that
//! reads as a strong tint. On a newly added file, where every row is an addition, it
//! is a screen of flat green, and `SPEC.md` §5's whole claim is that colour carries
//! signal. This was already written down as the reason Windows detects 24-bit rather
//! than 256; what was missing was applying it to the rung itself.
//!
//! The grey ramp is not the escape. It is much nearer in distance, and it is where
//! [`to_indexed`] would send a desaturated wash if its chroma gate did not stop it,
//! but `#1b3d29` and `#45222a` both average to the *same* grey. An added row and a
//! removed row would be one colour, which is the one thing §5 says may never happen.
//!
//! So there is no honest wash below 24-bit, and the diff signal narrows back to the
//! sigil column exactly as it did before #11. A reader whose terminal draws more
//! than detection can prove says so with `VIGIA_COLOR`.
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum Depth {
    /// Draw no colour at all. `NO_COLOR`, or `TERM=dumb`.
    None,
    /// The sixteen named colours every terminal resolves. Backgrounds are dropped
    /// here: see the module docs.
    #[default]
    Ansi16,
    /// The xterm 256-colour palette: sixteen names, a 6x6x6 cube, 24 greys.
    Ansi256,
    /// 24-bit colour, which is what every built-in palette is authored in.
    Truecolor,
}

/// A `VIGIA_COLOR` this does not understand.
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
    pub fn from_env(
        windows: bool,
        lookup: impl Fn(&str) -> Option<String>,
    ) -> Result<Self, DepthError> {
        // Through [`override_of`], which owns the set-but-empty rule for both
        // ladders and carries the PowerShell gotcha behind it.
        if let Some((raw, value)) = override_of(&lookup, DEPTH_VAR) {
            match value.as_str() {
                "never" | "none" | "0" => return Ok(Self::None),
                "16" | "ansi" => return Ok(Self::Ansi16),
                "256" => return Ok(Self::Ansi256),
                "truecolor" | "truecolour" | "24bit" => return Ok(Self::Truecolor),
                "auto" => {}
                _ => return Err(DepthError { value: raw }),
            }
        }

        // `NO_COLOR` deliberately does **not** share that rule, and the asymmetry is
        // the point rather than an oversight. It has no valid values at all, so
        // presence is the whole signal and an empty one still means what it says.
        // `VIGIA_COLOR` has nothing *but* values, so an empty one means nothing was
        // chosen.

        if lookup("NO_COLOR").is_some() {
            return Ok(Self::None);
        }

        // **Folded once, here.** `term_depth` folds again below and this arm did
        // not fold at all, so `TERM=DUMB` was a terminal saying it cannot draw
        // that this ladder heard and the glyph ladder did not. `TERM` is
        // conventionally lower case and the forgiving reading is the right one
        // when the cost of mishearing is colour a reader switched off.
        let term = lookup("TERM").unwrap_or_default().to_ascii_lowercase();
        if term == "dumb" {
            return Ok(Self::None);
        }

        let colorterm = lookup("COLORTERM").unwrap_or_default().to_ascii_lowercase();
        if colorterm == "truecolor" || colorterm == "24bit" {
            return Ok(Self::Truecolor);
        }

        // **Above `TERM`, and that order is the fix rather than a tidy-up.** Git
        // Bash and MSYS export `TERM=xterm-256color` on Windows, so reading `TERM`
        // first sent the most common shell for this repo to 256, where a subtle
        // wash has nowhere to land and quantises to a saturated primary. A
        // terminal that names itself is better evidence than a variable describing
        // a terminfo entry.
        if let Some(depth) = lookup("TERM_PROGRAM")
            .as_deref()
            .map(str::trim)
            .and_then(program_depth)
        {
            return Ok(depth);
        }
        if windows && lookup("WT_SESSION").is_some() {
            return Ok(Self::Truecolor);
        }

        if let Some(depth) = term_depth(&term) {
            return Ok(depth);
        }
        if windows {
            // **Truecolour, not 256.** The conservative answer is wrong in a
            // way only a screen shows: the xterm cube's darkest
            // axis levels are 0 and 95 with nothing between, so a *subtle* colour
            // has nowhere to land. A row wash of `#1b3d29` quantises to `#005f00`,
            // which is a saturated primary rather than a tint, and a reader looking
            // at it asks whether the colour is right. It is not, and no better
            // index exists.
            return Ok(Self::Truecolor);
        }
        Ok(Self::Ansi16)
    }

    /// [`Depth::from_env`] against this process.
    pub fn detect() -> Result<Self, DepthError> {
        Self::from_env(cfg!(windows), |key| std::env::var(key).ok())
    }

    /// `style`, in colours this depth can actually show.
    pub fn resolve(self, style: Style) -> Style {
        let mut out = style;
        out.fg = style.fg.map(|colour| self.colour(colour));
        out.bg = match self {
            // Dropped rather than mapped to `Reset`. `None` means "leave whatever is
            // behind this cell", which is the reader's own background; `Reset` means
            // "the terminal's default", which is not the same thing inside a pane
            // that has been given one.
            Self::Truecolor => style.bg,
            _ => Option::None,
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

/// What `TERM_PROGRAM` names, and what that terminal can draw.
fn program_depth(program: &str) -> Option<Depth> {
    // Lowercased because the values are brand names and are spelled as such:
    // `Apple_Terminal`, `iTerm.app`, `WezTerm`, `WarpTerminal`, `Hyper`, `Tabby`,
    // beside a lowercase `vscode` and `ghostty`. Matching them as written is a
    // table that is wrong the first time a vendor re-capitalises.
    match program.to_ascii_lowercase().as_str() {
        "apple_terminal" => Some(Depth::Ansi256),
        "ghostty" | "hyper" | "iterm.app" | "rio" | "tabby" | "vscode" | "warpterminal"
        | "wezterm" => Some(Depth::Truecolor),
        _ => None,
    }
}

/// What a `TERM` entry promises, or nothing when it promises neither rung.
fn term_depth(term: &str) -> Option<Depth> {
    // **Folded again, and deliberately.** The one caller folds before calling,
    // so this is idempotent and redundant on that path; it stays because the
    // alternative is a precondition living in a caller rather than in a
    // signature, on a private function whose next caller has no way to know.
    // It runs once at startup, so the cost is not a consideration.
    let term = term.to_ascii_lowercase();
    // `contains` rather than `ends_with`, for `xterm-direct2` and friends: the
    // database numbers the direct entries by how many bits they hand each channel,
    // and all of them are direct colour.
    if term.contains("-direct") || term.contains("truecolor") {
        return Some(Depth::Truecolor);
    }
    if TRUECOLOR_TERMS.iter().any(|name| names(&term, name)) {
        return Some(Depth::Truecolor);
    }
    if term.contains("256color") {
        return Some(Depth::Ansi256);
    }
    None
}

/// Terminals whose own terminfo entry is the whole signal, since none of them
/// spells `256color` and all of them draw 24-bit.
const TRUECOLOR_TERMS: [&str; 7] = [
    "alacritty",
    "contour",
    "foot",
    "rio",
    "wezterm",
    "xterm-ghostty",
    "xterm-kitty",
];

/// What a rung-override variable was set to, raw and normalised, or nothing.
pub(crate) fn override_of(
    lookup: &impl Fn(&str) -> Option<String>,
    var: &str,
) -> Option<(String, String)> {
    let raw = lookup(var).filter(|value| !value.trim().is_empty())?;
    let normalised = raw.trim().to_ascii_lowercase();
    Some((raw, normalised))
}

/// Whether `term` is `name` or one of its variants.
pub(crate) fn names(term: &str, name: &str) -> bool {
    term.strip_prefix(name)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('-'))
}

/// The six levels each axis of the xterm colour cube can take.
const CUBE: [u8; 6] = [0, 95, 135, 175, 215, 255];

/// Below this chroma there is no hue worth preserving.
const ACHROMATIC: u8 = 32;

/// How far a colour is from grey.
fn chroma(r: u8, g: u8, b: u8) -> u8 {
    r.max(g).max(b) - r.min(g).min(b)
}

/// Nearest xterm palette index to a 24-bit colour.
pub fn to_indexed(r: u8, g: u8, b: u8) -> u8 {
    let axis = |v: u8| {
        CUBE.iter()
            .enumerate()
            .min_by_key(|(_, level)| v.abs_diff(**level))
            .map_or(0, |(i, _)| i)
    };
    let (ri, gi, bi) = (axis(r), axis(g), axis(b));
    let cube = 16 + 36 * ri + 6 * gi + bi;
    let cube_err = distance((r, g, b), (CUBE[ri], CUBE[gi], CUBE[bi]));

    // **The ramp is only a candidate for something that is actually grey**, and
    // leaving that out is a bug that reaches the screen. A row wash like `#1b3d29`
    // is a desaturated green, so on raw distance the nearest grey beats the nearest
    // cube entry: the cube's green axis jumps 0, 95, 135 while the ramp steps by
    // ten, and a colour sitting between two cube levels is closer to a grey than to
    // either. The wash then draws as a **neutral band**, which is a tint that has
    // lost the only thing it was for.
    if chroma(r, g, b) >= ACHROMATIC {
        return cube as u8;
    }

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
pub fn to_ansi16(r: u8, g: u8, b: u8) -> Color {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let chroma = chroma(r, g, b);
    // Green carries most of perceived brightness, blue least. The 2/5/1 split is
    // the cheap integer form of that and only has to rank, never to be accurate.
    let luma = (2 * u32::from(r) + 5 * u32::from(g) + u32::from(b)) / 8;

    if chroma < ACHROMATIC {
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
fn distance(a: (u8, u8, u8), b: (u8, u8, u8)) -> u32 {
    let d = |x: u8, y: u8| {
        let e = u32::from(x.abs_diff(y));
        e * e
    };
    d(a.0, b.0) + d(a.1, b.1) + d(a.2, b.2)
}
