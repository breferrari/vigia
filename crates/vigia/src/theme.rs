//! Which colour every drawn thing gets.

use std::fmt;
use std::path::{Path, PathBuf};

use ratatui::style::{Color, Modifier, Style};
use vigia_core::{Class, Recency};

use crate::colour::Depth;
use crate::render::{Band, Heat};

/// Environment variable naming a built-in palette, or a file holding one.
pub const THEME_VAR: &str = "VIGIA_THEME";

/// Where a theme is read from when nothing overrides it, under the home
/// directory.
pub const THEME_FILE: &str = ".config/vigia/theme";

/// Declare the palette once, and derive everything that has to agree with it.
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

            /// Read one key, so a value can be layered on what is already there.
            fn get(&self, key: &str) -> Option<Style> {
                match key {
                    $(stringify!($field) => Some(self.$field)),*,
                    _ => None,
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
    /// Secondary text on those lines: key hints, the follow marker, the footer's `N/M`
    /// position, the readouts, and the header's mode word while the watch is live,
    /// since a lost one takes [`Theme::alert`] instead.
    chrome_dim,

    /// A changed file's path, at the recency the reader should read it as.
    path,
    /// A path that changed inside the glance window but not in the last tick.
    path_live,
    /// A path nothing has written since `vigia` started watching.
    path_cold,
    /// A listed path the pointer is resting on.
    path_hover,
    /// The `●` marking a file that moved in the last tick.
    pulse,
    /// A churn sparkline's blocks, at the quietest of its three stops.
    spark,
    /// A sparkline bucket at a third or more of the screen's busiest.
    spark_warm,
    /// A sparkline bucket at two thirds or more of it.
    spark_hot,
    /// A sparkline bucket nothing was written in.
    spark_track,

    /// The filled part of a scrollbar: where in the whole a region is looking.
    bar,
    /// The same mark while the reader is holding it.
    bar_active,
    /// The same mark while the pointer is merely resting on it.
    bar_hover,
    /// The unfilled part, which is drawn rather than left blank.
    bar_track,

    /// A heat-strip slice nothing changed in.
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
    /// The kind letter and the run label that mark a staged change.
    staged,
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
    added_row,
    /// The same, behind a removed line.
    removed_row,
    /// The hotter wash behind the bytes of an added line that actually changed,
    /// when the line pairs with a removal ([`vigia_core::Line::emph`]).
    added_word,
    /// The same, inside a removed line.
    removed_word,
    /// The line-number cells of an added line: a tone one step darker than the
    /// wash, so the gutter reads as a column without spending a border
    /// (crush's two-tone gutter, same ruling as above).
    added_gutter,
    /// The same, on a removed line.
    removed_gutter,
    /// The pane's leading cell on an added line, which is §5.1's left bar.
    added_bar,
    /// The same, on a removed line.
    removed_bar,
    /// The wash over rows a drag has selected. It stands in for the diff wash
    /// rather than layering over it, so the sigil column carries added against
    /// removed while it is up, which is the degradation §5.1 already rules for a
    /// palette that washes nothing.
    selection,

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
    pub fn resolve(self, depth: Depth) -> Self {
        let mut out = self.map(|style| depth.resolve(style));
        // A depth that cannot carry a background drops the selection's, and unlike
        // the diff washes it has no bar to degrade onto: a reader would drag and see
        // nothing while `y` went on sending. Reversing is what `ansi` already does
        // for the same reason, and it needs no colour at all.
        if out.selection.bg.is_none() {
            out.selection = out.selection.add_modifier(Modifier::REVERSED);
        }
        out
    }

    /// The style a file heading is drawn in at `recency`.
    pub fn recency(&self, recency: Recency) -> Style {
        match recency {
            Recency::Pulse => self.path,
            Recency::Live => self.path_live,
            Recency::Cold => self.path_cold,
        }
    }

    /// The style one slice of a heat strip is drawn in.
    pub fn heat(&self, heat: Heat) -> Style {
        match heat {
            Heat::Cool => self.heat_track,
            Heat::Added(band) => self.band(
                band,
                self.heat_added,
                self.heat_added_warm,
                self.heat_added_hot,
            ),
            Heat::Removed(band) => self.band(
                band,
                self.heat_removed,
                self.heat_removed_warm,
                self.heat_removed_hot,
            ),
            Heat::Mixed(band) => self.band(
                band,
                self.heat_mixed,
                self.heat_mixed_warm,
                self.heat_mixed_hot,
            ),
        }
    }

    /// Which cyan a written sparkline bucket takes, by how busy it is.
    pub fn spark_at(&self, band: Band) -> Style {
        self.band(band, self.spark, self.spark_warm, self.spark_hot)
    }

    /// The sparkline stops interpolated into an eight-step ramp, or `None`
    /// where interpolation is meaningless.
    pub fn spark_ramp(&self) -> Option<[Color; 8]> {
        let rgb = |style: Style| match style.fg {
            Some(Color::Rgb(r, g, b)) => Some((r, g, b)),
            _ => None,
        };
        let low = rgb(self.spark)?;
        let warm = rgb(self.spark_warm)?;
        let hot = rgb(self.spark_hot)?;
        let lerp = |a: (u8, u8, u8), b: (u8, u8, u8), t: f32| {
            let (la, aa, ba) = oklab_of(a);
            let (lb, ab, bb) = oklab_of(b);
            let mix = |x: f32, y: f32| x + (y - x) * t;
            rgb_of(mix(la, lb), mix(aa, ab), mix(ba, bb))
        };
        // Stops at 0, 4 and 7: the warm key sits where Band::of's middle third sits on
        // the eight-level height ramp.
        let colour = |(r, g, b)| Color::Rgb(r, g, b);
        let mut out = [Color::Reset; 8];
        out[0] = colour(low);
        out[4] = colour(warm);
        out[7] = colour(hot);
        for at in [1, 2, 3] {
            out[at] = lerp(low, warm, at as f32 / 4.0);
        }
        for at in [5, 6] {
            out[at] = lerp(warm, hot, (at - 4) as f32 / 3.0);
        }
        Some(out)
    }

    /// One rung of a three-stop ramp.
    fn band(&self, band: Band, low: Style, warm: Style, hot: Style) -> Style {
        match band {
            Band::Low => low,
            Band::Warm => warm,
            Band::Hot => hot,
        }
    }

    /// The style a run of `class` is drawn in.
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
    pub fn row(&self, added: bool) -> (Style, Style) {
        if added {
            (self.added_row, self.added_bar)
        } else {
            (self.removed_row, self.removed_bar)
        }
    }

    /// The sixteen named colours, which is what shipped before there was a choice.
    pub fn ansi() -> Self {
        Self {
            chrome: fg(Color::Cyan).add_modifier(Modifier::BOLD),
            // Readable, at the cost of not being dim, and that trade is the
            // ruling.
            chrome_dim: fg(Color::Gray),
            // Three rungs of one ramp: bright and bold, bright, then plain. `Gray`
            // rather than `DarkGray` for the coldest, deliberately.
            path: fg(Color::White).add_modifier(Modifier::BOLD),
            path_live: fg(Color::White),
            path_cold: fg(Color::Gray),
            // `bar_hover`'s colour without its `BOLD`: the bar needs the weight
            // to separate a hovered button from `bar_track` on a palette with
            // nothing between them, and a path has the underline instead.
            path_hover: fg(Color::Gray).add_modifier(Modifier::UNDERLINED),
            // Cyan rather than a diff colour. The pulse says *when*, and green or
            // red beside a path would read as *what*, which the sigil column
            // already means two rows below.
            pulse: fg(Color::Cyan),
            // Two stops of hue where the other palettes have three, exactly as
            // `heat_added` below and for the same reason: sixteen names hold a
            // normal and a bright of each colour and no third, so the middle
            // stop is the normal one and the ramp reads as two.
            spark: fg(Color::Cyan),
            spark_warm: fg(Color::Cyan),
            spark_hot: fg(Color::LightCyan),
            // `DarkGray`, and the one palette where the track does *not* step towards
            // the foreground the way `dark` and `light` do.
            spark_track: fg(Color::DarkGray),
            // Grey rather than cyan, for the reason the field's own doc gives: the
            // thumb is a full block and cyan is the sparkline's, so the two would be
            // one colour drawing two meanings.
            bar: fg(Color::Gray),
            bar_active: fg(Color::White),
            bar_hover: fg(Color::Gray).add_modifier(Modifier::BOLD),
            bar_track: fg(Color::DarkGray),
            // The one place colour 8 is the right answer, and the exception proves the
            // rule that sent everything else to `DIM`.
            heat_track: fg(Color::DarkGray),
            // Two stops of hue where the other palettes have three. Sixteen names hold
            // a normal and a bright of each colour and no third, so the middle stop is
            // the normal one and the ramp reads as two.
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
            staged: fg(Color::Green),
            hunk: fg(Color::Blue),
            // Same rule as `chrome_dim`, and the same two reports.
            gutter: fg(Color::Gray),
            added: fg(Color::Green),
            removed: fg(Color::Red),
            // Reset rather than a colour: context is most of the screen, and the
            // reader's own foreground is the least distracting thing it can be.
            context: fg(Color::Reset),
            // Unset, which is what "this palette draws no tint" is spelled as.
            added_row: Style::new(),
            removed_row: Style::new(),
            // Unset with them: no wash means no word patch and no gutter tone.
            added_word: Style::new(),
            removed_word: Style::new(),
            added_gutter: Style::new(),
            removed_gutter: Style::new(),
            // The bar, in names, and it is the one row-level diff signal this palette
            // can carry. §11.1 records the loss it is fixing: at sixteen colours the
            // signal degrades to the sigil column, because a wash has to assume a
            // background and this palette assumes none.
            added_bar: Style::new().bg(Color::Green),
            removed_bar: Style::new().bg(Color::Red),
            // Reversed rather than a background, for the reason the row washes
            // above are unset here: a background has to assume one, and this
            // palette is the one that cannot. Reversing swaps whatever the
            // terminal's own scheme put there, so it is visible on both.
            selection: Style::new().add_modifier(Modifier::REVERSED),
            note: fg(Color::Magenta),
            alert: fg(Color::Red).add_modifier(Modifier::BOLD),
            // The mockup's hues, mapped onto the sixteen names every terminal resolves.
            keyword: fg(Color::LightRed),
            type_name: fg(Color::LightYellow),
            function: fg(Color::LightMagenta),
            variable: fg(Color::LightBlue),
            constant: fg(Color::Yellow),
            string: fg(Color::LightGreen),
            number: fg(Color::LightCyan),
            // The mockup draws comments no differently from its own dimmed text, and a
            // comment is the one thing on a diff line a reader routinely wants to skip.
            comment: fg(Color::Gray),
        }
    }

    /// `assets/preview.svg`, as a palette.
    pub fn dark() -> Self {
        Self {
            chrome: rgb(0x39, 0xc5, 0xcf).add_modifier(Modifier::BOLD),
            chrome_dim: rgb(0x8b, 0x94, 0x9e),
            path: rgb(0xe6, 0xed, 0xf3).add_modifier(Modifier::BOLD),
            path_live: rgb(0xe6, 0xed, 0xf3),
            path_cold: rgb(0x7d, 0x85, 0x90),
            // `bar_hover`'s `#a8b1bb`, which is 8.71:1 on this pane: quieter
            // than `path_live`'s `#e6edf3` and a long way clear of unreadable.
            path_hover: rgb(0xa8, 0xb1, 0xbb).add_modifier(Modifier::UNDERLINED),
            pulse: rgb(0x39, 0xc5, 0xcf),
            // Cyan, where the picture's sparkline is green. What decides the hue is
            // that green already means addition two rows down, and a churn sparkline is
            // about *when*, not *what*.
            spark: rgb(0x39, 0xc5, 0xcf),
            spark_warm: rgb(0x7a, 0xe9, 0xf0),
            spark_hot: rgb(0xa8, 0xf2, 0xf7),
            // One step above `heat_track`, which is the rule this field always had and
            // could not satisfy while the thing it was a step above was itself
            // invisible.
            spark_track: rgb(0x6e, 0x76, 0x81),
            bar: rgb(0x8b, 0x94, 0x9e),
            bar_active: rgb(0xc9, 0xd1, 0xd9),
            bar_hover: rgb(0xa8, 0xb1, 0xbb),
            bar_track: rgb(0x65, 0x6c, 0x76),
            // The move above is paid for by the wash, and the heat strip does not sit
            // on one: it draws on list rows and on file headings, which are never
            // washed.
            heat_track: rgb(0x57, 0x60, 0x6a),
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
            // The dark palette's own green, which `added` also takes: one hue for
            // the idea of *this is in*, spent in two places that never share a row.
            staged: rgb(0x3f, 0xb9, 0x50),
            hunk: rgb(0x58, 0xa6, 0xff),
            gutter: rgb(0x7d, 0x85, 0x90),
            added: rgb(0x3f, 0xb9, 0x50),
            removed: rgb(0xf8, 0x51, 0x49),
            context: rgb(0xe6, 0xed, 0xf3),
            // The two rects the picture draws behind changed lines, and the two bars at
            // their left edge. Backgrounds, so the depth ladder drops them below 24-bit
            // on its own and these are only ever drawn as authored.
            added_row: Style::new().bg(Color::Rgb(0x1b, 0x3d, 0x29)),
            removed_row: Style::new().bg(Color::Rgb(0x45, 0x22, 0x2a)),
            // The washes stepped hotter, same hue, roughly delta's line-to-emph
            // ratio; and stepped darker for the gutter, crush's two-tone.
            added_word: Style::new().bg(Color::Rgb(0x2e, 0x6b, 0x41)),
            removed_word: Style::new().bg(Color::Rgb(0x7e, 0x2f, 0x3a)),
            added_gutter: Style::new().bg(Color::Rgb(0x14, 0x2e, 0x1f)),
            removed_gutter: Style::new().bg(Color::Rgb(0x33, 0x1a, 0x20)),
            // Unset, and that is a ruling rather than a gap.
            added_bar: Style::new().bg(Color::Rgb(0x3f, 0xb9, 0x50)),
            removed_bar: Style::new().bg(Color::Rgb(0xf8, 0x51, 0x49)),
            // The row washes' own luminance in a hue neither uses, so a selected
            // removal cannot read as an addition and the ink it covers is no
            // worse off than the diff already leaves it.
            selection: Style::new().bg(Color::Rgb(0x2c, 0x36, 0x4e)),
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
    pub fn light() -> Self {
        Self {
            chrome: rgb(0x0a, 0x62, 0x6b).add_modifier(Modifier::BOLD),
            chrome_dim: rgb(0x59, 0x63, 0x6e),
            path: rgb(0x1f, 0x23, 0x28).add_modifier(Modifier::BOLD),
            path_live: rgb(0x1f, 0x23, 0x28),
            path_cold: rgb(0x81, 0x8b, 0x98),
            path_hover: rgb(0x3d, 0x46, 0x50).add_modifier(Modifier::UNDERLINED),
            pulse: rgb(0x0a, 0x62, 0x6b),
            // Darker as it climbs, which is the same rule as `dark`'s and
            // not a second one: both move towards the foreground, and on a light
            // background that direction is down.
            spark: rgb(0x5a, 0xa6, 0xae),
            spark_warm: rgb(0x0a, 0x62, 0x6b),
            spark_hot: rgb(0x03, 0x28, 0x2e),
            // Darker here where `dark`'s is brighter, which is the same rule and not a
            // second one: both move one step *towards* the foreground, and on a light
            // background that direction is down.
            spark_track: rgb(0x7d, 0x85, 0x90),
            bar: rgb(0x59, 0x63, 0x6e),
            bar_active: rgb(0x24, 0x29, 0x2f),
            bar_hover: rgb(0x3d, 0x46, 0x50),
            bar_track: rgb(0x7d, 0x85, 0x90),
            // Light enough to read as a track on white, dark enough to be visible.
            // Unmoved for the reason `Theme::dark`'s twin gives: the heat strip
            // never draws on a washed row.
            heat_track: rgb(0x8c, 0x95, 0x9f),
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
            staged: rgb(0x1a, 0x7f, 0x37),
            hunk: rgb(0x05, 0x50, 0xae),
            gutter: rgb(0x81, 0x8b, 0x98),
            added: rgb(0x1a, 0x7f, 0x37),
            removed: rgb(0xcf, 0x22, 0x2e),
            context: rgb(0x1f, 0x23, 0x28),
            // The same correction one background over: a wash has to be *further* from
            // the pane than the pane is from white, or a reader on an off-white
            // terminal sees nothing.
            added_row: Style::new().bg(Color::Rgb(0xc0, 0xf0, 0xcd)),
            removed_row: Style::new().bg(Color::Rgb(0xff, 0xd4, 0xd1)),
            // Hotter is *more saturated* here, the direction every ramp in this
            // palette reverses; the gutter steps the same way.
            added_word: Style::new().bg(Color::Rgb(0x93, 0xe0, 0xab)),
            removed_word: Style::new().bg(Color::Rgb(0xff, 0xb3, 0xb0)),
            added_gutter: Style::new().bg(Color::Rgb(0xa9, 0xe4, 0xba)),
            removed_gutter: Style::new().bg(Color::Rgb(0xf7, 0xbf, 0xc0)),
            // Set, for the reason `dark` gives at length, in this palette's own
            // diff hues rather than the dark one's: a bar is a background on a
            // blank cell, so what it has to clear is the page behind it.
            added_bar: Style::new().bg(Color::Rgb(0x1a, 0x7f, 0x37)),
            removed_bar: Style::new().bg(Color::Rgb(0xcf, 0x22, 0x2e)),
            // The dark palette's hue re-picked at light-background luminance, as
            // every other pair here is.
            selection: Style::new().bg(Color::Rgb(0xcf, 0xe0, 0xf7)),
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

/// sRGB bytes to Oklab, through linear light.
fn oklab_of((r, g, b): (u8, u8, u8)) -> (f32, f32, f32) {
    let linear = |c: u8| {
        let c = c as f32 / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let (r, g, b) = (linear(r), linear(g), linear(b));
    let l = (0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    (
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    )
}

/// Oklab back to sRGB bytes, clamped into gamut.
fn rgb_of(l: f32, a: f32, b: f32) -> Color {
    let l_ = l + 0.396_337_78 * a + 0.215_803_76 * b;
    let m_ = l - 0.105_561_346 * a - 0.063_854_17 * b;
    let s_ = l - 0.089_484_18 * a - 1.291_485_5 * b;
    let (l_, m_, s_) = (l_ * l_ * l_, m_ * m_ * m_, s_ * s_ * s_);
    let r = 4.076_741_7 * l_ - 3.307_711_6 * m_ + 0.230_969_94 * s_;
    let g = -1.268_438 * l_ + 2.609_757_4 * m_ - 0.341_319_38 * s_;
    let b = -0.004_196_086_3 * l_ - 0.703_418_6 * m_ + 1.707_614_7 * s_;
    let byte = |c: f32| {
        let c = c.clamp(0.0, 1.0);
        let c = if c <= 0.003_130_8 {
            12.92 * c
        } else {
            1.055 * c.powf(1.0 / 2.4) - 0.055
        };
        (c * 255.0).round() as u8
    };
    Color::Rgb(byte(r), byte(g), byte(b))
}

impl Default for Theme {
    /// [`Theme::ansi`]. See the module docs for why it is that one.
    fn default() -> Self {
        Self::ansi()
    }
}

/// Anything that stops a theme from being understood.
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
    /// A key with nothing after its `=`.
    MissingValue {
        /// Where.
        line: usize,
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
    /// A second `base` in one file.
    RepeatedBase {
        /// Where.
        line: usize,
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
            Self::MissingValue { line } => write!(
                f,
                "line {line}: this key has nothing after its `=`. Write a colour, \
                 `on` and a colour, or a modifier"
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
            Self::RepeatedBase { line } => write!(
                f,
                "line {line}: `base` is already set. One palette to start from, or                  the second silently discards the first"
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
/// # Errors
///
/// The theme file named by the environment cannot be read, or does not parse.
pub fn from_env(
    depth: Depth,
    lookup: impl Fn(&str) -> Option<String>,
    detected: Option<crate::terminal::Background>,
) -> Result<Theme, ThemeError> {
    // Chosen here, resolved once on the way out.
    let theme = if let Some(named) = lookup(THEME_VAR).filter(|value| !value.trim().is_empty()) {
        let named = named.trim();
        // A built-in wins over a file of the same name. The three names are short,
        // ordinary words and a file called `dark` in the working directory should
        // not silently take over what `VIGIA_THEME=dark` has always meant.
        match Theme::named(named) {
            Some(built_in) => built_in,
            None => load(Path::new(named))?,
        }
    } else if let Some(path) = home_file(THEME_FILE, &lookup).filter(|path| path.is_file()) {
        // Then the file, which is where a preference set once lives.
        load(&path)?
    } else {
        match detected {
            Some(crate::terminal::Background::Dark) => Theme::dark(),
            Some(crate::terminal::Background::Light) => Theme::light(),
            None => Theme::default(),
        }
    };
    Ok(theme.resolve(depth))
}

/// `rela` under the reader's home directory, if there is one.
pub(crate) fn home_file(rela: &str, lookup: &impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    // Each candidate is emptied-checked before the next is tried, which is the whole of
    // this function and was wrong on the first write.
    ["HOME", "USERPROFILE"]
        .into_iter()
        .filter_map(lookup)
        .find(|home| !home.trim().is_empty())
        .map(|home| Path::new(home.trim()).join(rela))
}

/// Read and parse a theme file.
///
/// # Errors
///
/// The file cannot be read, or it does not parse.
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
/// Hand-rolled rather than TOML, and that is a dependency decision. `toml` is
/// not in the lock file; taking it means `toml`, `toml_edit`, `winnow` and
/// `serde_spanned`, none of which `SPEC.md` names, for a grammar that is one line
/// shape. CLAUDE.md's rule is that a dependency reaches the spec before it reaches
/// a manifest, and this surface does not earn the argument.
///
/// An unknown key is refused rather than ignored. A silently dropped key is a
/// theme that does nothing, and "it was discarded" is the one explanation a reader
/// cannot arrive at by looking at their screen.
///
/// # Errors
///
/// A line names an unknown key, colour, modifier or base, or is missing a value or its
/// separator.
pub fn parse(source: &str) -> Result<Theme, ThemeError> {
    // A BOM is stripped, and `trim` will not do it.
    let source = source.strip_prefix('\u{FEFF}').unwrap_or(source);

    let mut theme = Theme::default();
    let mut touched = false;
    let mut based = false;

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
        let value = value.trim();

        if key == "base" {
            if touched {
                return Err(ThemeError::LateBase { line });
            }
            // Accepting a second `base` with the last one winning is the
            // silent-discard this parser refuses everywhere else, and `touched`
            // does not catch it: only an ordinary key sets that.
            if based {
                return Err(ThemeError::RepeatedBase { line });
            }
            based = true;
            // Through `words_of` like every other value, so the documented comment
            // idiom works on the one line every theme file starts with.
            let name = words_of(value).first().copied().unwrap_or_default();
            theme = Theme::named(name).ok_or_else(|| ThemeError::UnknownBase {
                line,
                name: name.to_owned(),
            })?;
            continue;
        }

        let Some(current) = theme.get(key) else {
            return Err(ThemeError::UnknownKey {
                line,
                key: key.to_owned(),
            });
        };
        let style = style_of(value, line, current)?;
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

/// The words of a value, stopping where a trailing comment starts.
fn words_of(value: &str) -> Vec<&str> {
    let mut out = Vec::new();
    for word in value.split_whitespace() {
        if word.starts_with('#') && !out.is_empty() && !is_hex(word) {
            break;
        }
        out.push(word);
    }
    out
}

/// `#rrggbb`, exactly.
fn is_hex(word: &str) -> bool {
    word.len() == 7
        && word.starts_with('#')
        && word.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit)
}

/// `[<colour>] [on <colour>] [<modifier>...]`.
fn style_of(value: &str, line: usize, current: Style) -> Result<Style, ThemeError> {
    // Seeded from the key's current value, not from nothing. `added = bold` reads as
    // "make additions bold"; built from `Style::new()`, where `set` replaces the whole
    // thing, it means "make additions bold and colourless".
    let mut style = current;
    let tokens = words_of(value);
    if tokens.is_empty() {
        return Err(ThemeError::MissingValue { line });
    }
    let mut words = tokens.into_iter().peekable();

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
    matches!(word, "bold" | "dim" | "italic" | "underline" | "reverse")
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
