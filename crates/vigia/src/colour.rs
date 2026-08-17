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
    /// 2. **`NO_COLOR`**, present at all. The convention's own wording is "present
    ///    and not an empty string", and this is deliberately the looser reading:
    ///    `NO_COLOR=` in an environment file is a reader asking for no colour, and
    ///    honouring the letter over the intent hands them colour they went out of
    ///    their way to switch off. Below the override so that `VIGIA_COLOR` can
    ///    still ask for colour in one pane of a session that sets it globally.
    /// 3. **`TERM=dumb`**, which is a terminal saying it cannot do this.
    /// 4. **`COLORTERM`** of `truecolor` or `24bit`. The strongest positive signal
    ///    for 24-bit, and the only one that is a convention rather than a name.
    /// 5. **`TERM_PROGRAM`**, which is a terminal naming itself. See
    ///    [`program_depth`] for why a name outranks the `TERM` entry under it, and
    ///    why this is the rung that matters on macOS.
    /// 6. **Windows with `WT_SESSION`**, which is Windows Terminal identifying
    ///    itself. It has done 24-bit since it shipped, and it is the same class of
    ///    evidence the rung above is: a terminal that says which one it is.
    /// 7. **`TERM` promising 24-bit**, either in terminfo's own `-direct` spelling
    ///    or by naming a terminal that has only ever drawn it. See [`term_depth`].
    /// 8. **`TERM` containing `256color`**, which is terminfo's spelling for the
    ///    rung below.
    /// 9. **Windows otherwise**, at 24-bit. This is §10's *"legacy conhost
    ///    degrades"* aged out: every Windows console that can run this draws
    ///    24-bit, and a reader on something genuinely older says so with
    ///    `VIGIA_COLOR`.
    /// 10. **Sixteen**, which is what the palette was before any of this existed
    ///     and is the one answer that is never actively wrong.
    pub fn from_env(
        windows: bool,
        lookup: impl Fn(&str) -> Option<String>,
    ) -> Result<Self, DepthError> {
        // **Set-but-empty is the same as unset**, which `VIGIA_THEME` has always
        // said and this did not. Without the filter, `VIGIA_COLOR=""` reaches the
        // refusal arm below and stops the shell from starting, over a variable
        // nobody gave a value to.
        //
        // Reachable without trying. `$env:X = ''` in PowerShell leaves the variable
        // **set and empty**, and a child process sees it: verified on 7.6.3, where
        // `GetEnvironmentVariable` returns an empty string rather than null. (The
        // sibling spelling `$env:X = $null` does remove it there, which is worth
        // knowing because the two look interchangeable and are not.) Every shell
        // has some way to leave an empty value behind, and a reader who cleared a
        // variable has said "decide for me", not "here is a value you will not
        // recognise".
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

        let term = lookup("TERM").unwrap_or_default();
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
        //
        // That ruling was written for `WT_SESSION` and it generalises: the two
        // rungs here are both a terminal saying which one it is, and `TERM` below
        // them is a reader's `TERM` *setting*, which every layer in a session is
        // free to rewrite and which several do.
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
            // **Truecolour, and this used to be 256.** The conservative answer was
            // wrong in a way only a screen could show: the xterm cube's darkest
            // axis levels are 0 and 95 with nothing between, so a *subtle* colour
            // has nowhere to land. A row wash of `#1b3d29` quantises to `#005f00`,
            // which is a saturated primary rather than a tint, and a reader looking
            // at it asks whether the colour is right. It is not, and no better
            // index exists.
            //
            // So 256 is not a safe default, it is a different wrong one, and the
            // thing it was protecting against has aged out: §10's "legacy conhost
            // degrades" is about consoles before Windows 10 1703, which has not
            // been a supported target for years. Every Windows console that can run
            // this draws 24-bit.
            //
            // A reader on something genuinely older says so with `VIGIA_COLOR`,
            // which is one rung above this and exists for exactly the cases
            // detection cannot see.
            return Ok(Self::Truecolor);
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
            //
            // **`Ansi256` is on this side of the line and used to be on the other.**
            // See the module docs: the cube cannot express a subtle wash, the grey
            // ramp cannot tell an addition from a removal, and a slab is worse than
            // no tint. Nothing else changes at this rung, so the foreground is
            // quantised exactly as it was.
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
///
/// **This is the rung that matters on macOS**, and it was missing for a phase.
/// `COLORTERM` is the only convention for claiming 24-bit and nothing propagates
/// it: `ssh` forwards `TERM` and not `COLORTERM`, and a multiplexer replaces `TERM`
/// with its own entry. `tmux` at its default `default-terminal screen` is the
/// ordinary case, and `screen` contains neither `256color` nor anything else the
/// chain used to read, so a reader in the two-pane arrangement this whole tool is
/// designed for landed on sixteen. Sixteen **drops backgrounds**, so `dark` drew
/// every diff row unwashed and the tool looked like it was ignoring the palette.
/// Reported from a real macOS pane, where the giveaway was the `@@` header: at
/// sixteen the mockup's `#58a6ff` blue resolves to `LightCyan`, and it was cyan.
///
/// `TERM_PROGRAM` survives all of that, because it describes the process that owns
/// the screen rather than the entry the innermost layer decided to advertise.
///
/// **Only positive answers.** A program not in this table returns nothing and falls
/// through to `TERM`, rather than capping anything: the table is evidence about
/// terminals someone checked, not a claim about the ones nobody has.
///
/// `Apple_Terminal` is the one entry that is **not** truecolour, and it is the
/// reason this returns a rung rather than a bool. Terminal.app has never drawn
/// 24-bit: it accepts the sequences and rounds them to its own palette, so
/// claiming truecolour there is the over-claim [`Depth::Ansi16`]'s own doc warns
/// about, where an under-claim only looks flatter than it had to.
///
/// A reader on a build older than its terminal's 24-bit support says so with
/// `VIGIA_COLOR`, which is the same escape hatch the Windows rung leans on and
/// exists for exactly the cases detection cannot see.
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
///
/// Three readings, and the first two are the ones that were missing:
///
/// 1. **`-direct`** is terminfo's own spelling for direct colour, which is what
///    24-bit is called by the database that has an opinion. `xterm-direct`,
///    `tmux-direct`, `alacritty-direct`, and the numbered `xterm-direct2`. A reader
///    who set one has said the thing `COLORTERM` says, in the vocabulary of the
///    variable they were setting.
/// 2. **A terminal naming itself.** `xterm-kitty`, `xterm-ghostty`, `alacritty`,
///    `wezterm`, `foot`, `contour`, `rio` all ship their own entry and none of them
///    contains `256color`, so every one of them fell to sixteen whenever
///    `COLORTERM` had not survived. They have drawn 24-bit for their whole
///    existence; there is no version of any of them where this is an over-claim.
/// 3. **`256color`**, terminfo's spelling for the rung below, which is what this
///    function read and all it read.
///
/// **Promotes only**, which is `SPEC.md` §11.1's standing rule for `TERM`: an entry
/// that names none of these yields nothing and leaves the floor where it is.
fn term_depth(term: &str) -> Option<Depth> {
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
///
/// Deliberately short. Every entry is a terminal that ships its own `TERM` and has
/// never had a 16-colour era, which is what makes promoting on the name safe; a
/// terminal that merely *usually* has truecolour belongs behind `COLORTERM` or
/// `VIGIA_COLOR`, not here.
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
///
/// **Set-but-empty is the same as unset**, and that rule is here rather than in
/// each ladder because it is a discovered gotcha rather than a choice: `$env:X =
/// ''` in PowerShell leaves the variable **set and empty** and a child process
/// sees it, verified on 7.6.3 where `GetEnvironmentVariable` returns an empty
/// string rather than null. (The sibling spelling `$env:X = $null` does remove
/// it, which is worth knowing because the two look interchangeable.) Without
/// this filter a reader who *cleared* a variable stops the shell from starting,
/// over a value nobody gave. Written twice, a later correction to it (a BOM, a
/// different whitespace class) would land in one ladder and not the other.
///
/// Both spellings come back because both are needed and neither derives from the
/// other: the **normalised** one is matched against, and the **raw** one is what
/// a refusal quotes, since a reader who typed `Braille ` needs to see what they
/// typed rather than what it was folded to.
///
/// `NO_COLOR` deliberately does not come through here and the asymmetry is the
/// point: it has no valid values at all, so presence is the whole signal and an
/// empty one still means what it says.
pub(crate) fn override_of(
    lookup: &impl Fn(&str) -> Option<String>,
    var: &str,
) -> Option<(String, String)> {
    let raw = lookup(var).filter(|value| !value.trim().is_empty())?;
    let normalised = raw.trim().to_ascii_lowercase();
    Some((raw, normalised))
}

/// Whether `term` is `name` or one of its variants.
///
/// Entries are suffixed rather than substringed: `foot-extra` and `alacritty-direct`
/// are the same terminal, and a bare `contains` would also match a `TERM` that
/// merely has the word in it. The boundary is the `-` the database itself uses.
///
/// **Shared with [`glyphs`](crate::glyphs), which is the one thing the two
/// ladders do share.** The *tables* are deliberately separate, because a colour
/// depth and a font's coverage are different questions about the same name; how
/// a `TERM` entry is matched against a table is the same question in both, and
/// two copies of this would be two chances to disagree about `foot-extra`.
pub(crate) fn names(term: &str, name: &str) -> bool {
    term.strip_prefix(name)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('-'))
}

/// The six levels each axis of the xterm colour cube can take.
const CUBE: [u8; 6] = [0, 95, 135, 175, 215, 255];

/// Below this chroma there is no hue worth preserving.
///
/// **One constant, because two functions claim to share it.** [`to_indexed`] uses
/// it to veto the grey ramp and [`to_ansi16`] uses it to take a grey outright, and
/// each one's doc says "the same threshold as" the other. Written as two bare
/// literals with opposite comparisons, that agreement was a claim nothing held:
/// change one and they diverge silently in the band between.
const ACHROMATIC: u8 = 32;

/// How far a colour is from grey.
fn chroma(r: u8, g: u8, b: u8) -> u8 {
    r.max(g).max(b) - r.min(g).min(b)
}

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
    let cube_err = distance((r, g, b), (CUBE[ri], CUBE[gi], CUBE[bi]));

    // **The ramp is only a candidate for something that is actually grey**, and
    // leaving that out is a bug that reaches the screen. A row wash like `#1b3d29`
    // is a desaturated green, so on raw distance the nearest grey beats the nearest
    // cube entry: the cube's green axis jumps 0, 95, 135 while the ramp steps by
    // ten, and a colour sitting between two cube levels is closer to a grey than to
    // either. The wash then draws as a **neutral band**, which is a tint that has
    // lost the only thing it was for.
    //
    // Same chroma threshold as [`to_ansi16`], and the same reasoning: below it
    // there is no hue to preserve and the ramp is the better answer; above it, hue
    // is the whole point and a grey cannot carry it however near it measures.
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
