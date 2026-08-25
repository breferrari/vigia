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
//! `SPEC.md` §11.2 B6 is ruled here: **no flags**. `VIGIA_THEME` names a built-in
//! or points at a file, `VIGIA_COLOR` overrides the depth, and `~/.config/vigia/theme`
//! is where a palette set once lives.
//!
//! **This said "no config file" until 2026-08-25 and had been wrong for two
//! phases.** B6's *first* ruling was two variables and no file, on the argument
//! that a config file with one thing in it invites a second thing; it was amended
//! within the day, and this sentence never followed. The amendment's own reasoning
//! is the opposite: a variable has to be re-declared per shell, so a preference
//! set once belongs in a file. And the second thing did arrive, in
//! [#306](https://github.com/breferrari/vigia/issues/306), which put the view
//! defaults in `crate::config` rather than here: `VIGIA_THEME` naming a built-in
//! never opens this file, so keys that are not colours would be lost by a reader
//! choosing a palette for one session.

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
///
/// **One rule rather than one per platform.** Resolved from `HOME` or
/// `USERPROFILE`, which between them cover every target: no XDG matrix, no
/// `%APPDATA%` special case, no discovery crate. `SPEC.md` §11.1 argues the trade;
/// the short version is that a palette is a preference and should follow a reader
/// into every shell, where a colour depth is a fact about one terminal and stays a
/// variable.
pub const THEME_FILE: &str = ".config/vigia/theme";

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
    /// Secondary text on those lines: key hints, the follow marker, the footer's
    /// `N/M` position, the readouts, and the header's mode word **while the watch
    /// is live**, since a lost one takes [`Theme::alert`] instead. It is also the
    /// background a chrome row is painted with before anything is drawn on it.
    ///
    /// **Not the header's changed-file count**, which took the worktree name's
    /// [`Theme::chrome`] when it moved beside it
    /// ([#67](https://github.com/breferrari/vigia/issues/67)), because two
    /// weights inside one clause would draw the seam that move removed.
    ///
    /// The list is meant to be exhaustive for a line of chrome, which is what
    /// makes the exclusion worth stating: a class with a named exception has to
    /// name its members too, or the exception is the only thing anyone can
    /// check. It was not exhaustive when it first named one, and the follow
    /// marker is what it missed.
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
    /// A listed path the pointer is resting on.
    ///
    /// **The pointer's own colour, which is [`Theme::bar_hover`]'s**, so that
    /// one reading means *the pointer is here* wherever it lands: a step button,
    /// a thumb, a listed path. It holds that value in every built-in and keeps a
    /// key of its own for [`Theme::spark_track`]'s reason, which is that a
    /// distinct element has to be movable without moving the others.
    ///
    /// **It is underlined, and that is the whole of what keeps it off the
    /// recency ladder.** §5.3 rules that intensity carries recency *and nothing
    /// else*, so a mark that merely brightened a row would say **recent** about
    /// a file nothing had touched. Nothing else in these palettes underlines
    /// anything, so the separation survives every depth. It is also what a
    /// reader already reads as *the thing under the cursor* from every hyperlink
    /// they have ever pointed at.
    ///
    /// **This used to sit above all three rungs and that was wrong**
    /// ([#193](https://github.com/breferrari/vigia/issues/193)), which is worth
    /// keeping rather than editing out because the reason is not taste. §5.3's
    /// own B10 principle rules that a pointer mark *"must be the quietest thing
    /// still visible in that region"*, on the ground that a mark which can go
    /// **stale** must never be read before the worktree is; the paragraph beside
    /// it made this the brightest text on the pane. Both sentences were written
    /// the same day. The brightness was never doing the anti-collision work, and
    /// the underline above always was.
    ///
    /// **On `ansi` the foreground now equals [`Theme::path_cold`]'s outright**,
    /// which is stated rather than left to be discovered: sixteen names hold
    /// nothing between colour 8 and `Gray`, so a quiet mark on that palette *is*
    /// the cold rung's colour and the modifier is the only channel left. That is
    /// exactly the case §5.3 nominates the underline for.
    ///
    /// **It carries no `BOLD`.** That weight belongs to the row the caret marks,
    /// which is `render.rs`'s `CURRENT_WEIGHT`, and the two are orthogonal on
    /// purpose: colour and underline are the pointer, weight is the file the
    /// diff is inside, so a hovered current row reads as both rather than as
    /// whichever mark won.
    path_hover,
    /// The `●` marking a file that moved in the last tick.
    pulse,
    /// A churn sparkline's blocks, at the quietest of its three stops.
    ///
    /// **Ramped by height since [#196](https://github.com/breferrari/vigia/issues/196),
    /// which closes a departure rather than adding a flourish.**
    /// `assets/preview.svg` has ramped its sparkline across five greens from the
    /// start, tallest brightest, and the shell drew one flat colour. This field's
    /// own docblock used to record that gap and stop there: *"The picture draws a
    /// ramp there and we draw one colour, so it does not answer this."*
    ///
    /// **The hue is not what changed and is still cyan.** Green already means
    /// addition two rows down, and a churn sparkline is about *when* rather than
    /// *what*, which is the ruling that survives. What the picture was right
    /// about is the ramp.
    ///
    /// Three stops rather than the picture's five, resolved through the same
    /// [`Band`] and [`Theme::band`] the heat strip uses, so the two glance
    /// elements on one row ramp through one mechanism and a reader learns the
    /// gradient once. `Band` has three variants because a third is what the
    /// depth ladder can draw, which is [`Theme::heat_added`]'s own reasoning.
    spark,
    /// A sparkline bucket at a third or more of the screen's busiest.
    spark_warm,
    /// A sparkline bucket at two thirds or more of it.
    ///
    /// **Measured against `vigia_core::scale_of`'s figure over every bucket on
    /// screen**, which was the busiest of them until
    /// [#232](https://github.com/breferrari/vigia/issues/232) made a sample weigh
    /// bytes. Not against
    /// this file's own, which is the asymmetry `heat_at`'s docblock already
    /// draws: a sparkline is compared *down* a file list so it shares one scale,
    /// and a heat strip is read *across* one row so it scales to its own file.
    /// The ramp inherits that rather than introducing a second rule.
    spark_hot,
    /// A sparkline bucket nothing was written in.
    ///
    /// A track rather than a gap, which is the rule [`Theme::heat_track`] and
    /// [`Theme::bar_track`] already follow. A file with no history at all is the
    /// *all*-empty case, so at launch a worktree that was already dirty draws a
    /// full track on every row
    /// ([#78](https://github.com/breferrari/vigia/issues/78)).
    ///
    /// **Chosen rather than read off the picture, which is the one value in
    /// [`Theme::dark`] that is.** `assets/preview.svg` drew all eight bucket
    /// positions on all three rows, so nothing in it was ever absent — but its
    /// quiet buckets were `#1f6f3f`, the ramp's own floor colour, which makes
    /// them *written* buckets rather than track. There was no sparkline track in
    /// the file to read. (`#30363d` *is* in that file, as the window border's
    /// stroke, which is worth knowing because grepping the SVG for it finds a
    /// hit that has nothing to do with this element.) So this was picked for the
    /// reason below and the picture was then corrected to draw it, which is why
    /// the artifact and the shell still agree: the cold row's eight bars are
    /// `#30363d` today, deliberately **not** the `#21262d` the heat strip on that
    /// same row uses.
    ///
    /// **One step brighter than the other two tracks, because ink is not
    /// value.** [`Theme::heat_track`] argues `DarkGray` is right on the ground
    /// that "a track is not text: it is a solid block that should sit just above
    /// the background". A sparkline track is `_`, one stroke in a cell, so that
    /// premise does not reach it: the same colour behind a twelfth of the ink
    /// reads as absence, which is the state this element exists to stop looking
    /// like. What should match across the three tracks is perceived **weight**,
    /// and matching the value is what would break it.
    ///
    /// **[`Theme::ansi`] is the exception and it is a forced one.** Sixteen
    /// names hold nothing between colour 8 and `Gray`, and `Gray` is what that
    /// palette draws content in, so the track keeps `DarkGray` and inherits the
    /// risk [#60](https://github.com/breferrari/vigia/issues/60) is open on. That
    /// palette assumes no background, so it cannot be gated the way
    /// `tests/palette.rs` gates `dark` and `light` against the pane behind them;
    /// what it gets instead is a named place in the destructure at the top of
    /// `nothing_a_reader_has_to_read_is_drawn_in_colour_eight`, so the exemption
    /// is a decision the compiler makes someone take rather than a field nobody
    /// added to a list. What is not available is walking the styles **by key
    /// string**: `Theme::KEYS` is public and the values behind those names are
    /// not, which is why that gate is a destructure rather than a loop.
    ///
    /// **Its own key rather than [`Theme::heat_track`]'s**, which is what it
    /// started as and would still be sharing a value with in all three built-ins
    /// had the paragraph above not moved two of them. The precedent for the
    /// separate key is [`Theme::bar_track`], which *does* still hold
    /// [`Theme::heat_track`]'s value everywhere and is separate anyway: a
    /// distinct element gets a distinct key, so a theme author can move one
    /// without moving the others. That the sparkline's then had to move is this
    /// field's own vindication rather than a coincidence.
    ///
    /// It used to add "so a test can tell two tracks apart by style", which was
    /// not true when the values were identical and is only accidentally true
    /// now: what actually separates a spark track from a heat track on one row is
    /// the **glyph**, `_` against `█`, and `tests/render.rs` matches on the pair.
    spark_track,

    /// The filled part of a scrollbar: where in the whole a region is looking.
    ///
    /// **Deliberately not `chrome`**, which every other structural mark uses.
    /// The thumb is drawn as `█`, and `chrome` shares its foreground with
    /// [`Theme::spark`] on every palette here, so a bar drawn in it would be
    /// indistinguishable from a sparkline at full height to anything reading
    /// cells. That is not a hypothetical: `tests/legibility.rs` counts a
    /// sparkline's buckets by colour *and* glyph precisely because the heat strip
    /// already collided with it once, and the first draft of this bar collided
    /// with it again the same day.
    bar,
    /// The same mark while the reader is holding it.
    ///
    /// **Feedback for a gesture in progress, and it reuses a reading the eye has
    /// already learned on this column.** A step button held down and a thumb
    /// being dragged both take this, so *lit* means the same thing in both
    /// places: you are doing this right now. Brighter than [`Theme::bar`] rather
    /// than a different hue, because the element has not changed meaning, only
    /// state.
    ///
    /// It is also what the arrows take while the keys scroll, which is the one
    /// case where nothing is being held at all: `j` moving the viewport down
    /// lights the down arrow for as long as the burst lasts, so the bar answers
    /// the keyboard the way it answers the mouse.
    bar_active,
    /// The same mark while the pointer is merely resting on it.
    ///
    /// **The middle rung of the column's three, and it exists because a click
    /// has to be brighter than a hover.** A step button had somewhere to put a
    /// hover already, since it rests at [`Theme::bar_track`] and only reaches
    /// [`Theme::bar_active`] under a gesture. The **thumb** did not: it rests at
    /// [`Theme::bar`] and is dragged at `bar_active`, with nothing in between,
    /// so a hover drawn in `bar_active` would make a pointer resting on the
    /// thumb indistinguishable from a hand dragging it.
    ///
    /// [#186](https://github.com/breferrari/vigia/issues/186) read that as *no
    /// rung, therefore no mark* and shipped the thumb unmarked. That inverted
    /// the question: if the mark is wanted, the rung is the thing to build.
    /// Between `bar` and `bar_active` on every palette, so the ordering holds on
    /// the button and on the thumb with one entry.
    bar_hover,
    /// The unfilled part, which is drawn rather than left blank.
    ///
    /// A bar with no track is a mark floating in space: a reader cannot tell a
    /// short thumb near the top from a long one without seeing the extent it sits
    /// in. Dimmer than the thumb, for the same reason [`Theme::heat_track`] is
    /// dimmer than a slice.
    bar_track,

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
    /// The kind letter and the run label that mark a **staged** change.
    ///
    /// **Green, because that is git's own vocabulary for staged**: `git status`
    /// paints a staged path green and an unstaged one red, so a reader who has
    /// used git has already learned this. It is `SPEC.md` §5.3's green amendment
    /// ([#313](https://github.com/breferrari/vigia/issues/313)), and what keeps it
    /// from colliding with the diff's own green is *where* it is drawn: it inks a
    /// file row's kind letter and never a content row, so it is never beside a `+`
    /// sigil or a `+42` counter.
    ///
    /// **It was a bar in a column of its own until
    /// [#316](https://github.com/breferrari/vigia/issues/316)**, and the column is
    /// what it cost: taken on every row of *both* runs, so that a run boundary did
    /// not slide the paths beside it, to mark the rows of one. Colour says the same
    /// thing for nothing, needs no glyph the reader's font may not have, and is the
    /// channel `git status` was already using. What it gives up is `NO_COLOR`,
    /// where the run separators carry the fact alone, which is what they already
    /// did below the old gutter's own floor.
    ///
    /// One key for both marks on purpose. The letter's ink and the word in the
    /// run's separator are the same statement said twice, so a theme that moved
    /// one and not the other would be saying it two ways.
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
    ///
    /// `SPEC.md` §5.1's tinted row, and it is a **background**: a palette that
    /// leaves it unset draws no tint, which is what [`Theme::ansi`] does
    /// deliberately. Drawn only at [`Depth::Truecolor`], because every rung below it
    /// turns a wash into a slab: see [`crate::colour`] for the arithmetic and for
    /// why the grey ramp is not the way out.
    added_row,
    /// The same, behind a removed line.
    removed_row,
    /// The pane's leading cell on an added line, which is §5.1's left bar.
    ///
    /// **Background only, and it costs no column**
    /// ([#218](https://github.com/breferrari/vigia/issues/218)). It is one cell of
    /// the blank margin [`crate::render`]'s inset ladder already keeps, and the row
    /// wash already bleeds under, so setting its background spends nothing that was
    /// carrying content. Drawn wherever that margin exists, which is forty-three
    /// columns up, and absent below it, where the wash and the sigil carry the
    /// signal alone.
    ///
    /// **The reason it used to live on the sigil is recorded because it expired
    /// rather than because it was wrong.** It read: *"the mockup's bar is three
    /// pixels of a nine-pixel cell, so it has no terminal equivalent that does not
    /// spend a column, and I6 forbids spending one on decoration"*, and the sigil
    /// cell carried it by inverting. That was the best answer available when the
    /// pane drew from column zero. [#119](https://github.com/breferrari/vigia/issues/119)
    /// gave the pane a margin, the premise stopped being true, and nobody re-read
    /// the refusal. It also never drew: every built-in left this key empty, so the
    /// element §5.1 lists as *a tinted row with a coloured left bar* shipped as the
    /// tint alone for three phases.
    ///
    /// **It reaches `ansi` and every depth, where the wash reaches neither**, and
    /// the distinction is the whole reason this is a separate key. What rules the
    /// wash out below 24-bit is stated about text: *a quantised background is a
    /// solid block, and a block behind highlighted code destroys the colours on
    /// it.* This cell has no code on it. It is one blank column, so there is
    /// nothing for a background to destroy, and the default palette gets a
    /// row-level diff signal §11.1 records it as having lost.
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
    ///
    /// **The sibling of [`Theme::heat`], one element over**, and it goes through
    /// the same [`Theme::band`] so a ramp that ever draws its middle stop where
    /// its top belongs is one line to find rather than two elements to compare.
    /// It takes no `Cool` arm because a sparkline's empty bucket is
    /// [`Theme::spark_track`] and is resolved by the glyph rather than by the
    /// band: `_` against `▁`, which `SPARK_TRACK`'s own docblock rules is what
    /// keeps that distinction a matter of shape.
    pub fn spark_at(&self, band: Band) -> Style {
        self.band(band, self.spark, self.spark_warm, self.spark_hot)
    }

    /// One rung of a three-stop ramp.
    ///
    /// Written once and called four times rather than twelve match arms, so a
    /// ramp that ever draws its middle stop where its top belongs is one line to
    /// find instead of four places to compare. Three of the callers are
    /// [`Theme::heat`]'s arms and the fourth is [`Theme::spark_at`], which is what
    /// made the count in this sentence worth keeping current.
    fn band(&self, band: Band, low: Style, warm: Style, hot: Style) -> Style {
        match band {
            Band::Low => low,
            Band::Warm => warm,
            Band::Hot => hot,
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
    /// wash covers the row and the bar covers the pane's own leading cell. They
    /// also degrade separately, which is why neither can be derived from the other:
    /// a wash is dropped below 24-bit because it sits behind highlighted code, and
    /// a bar is not, because it sits behind nothing.
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
            // **Readable, at the cost of not being dim, and that trade is the
            // ruling.**
            //
            // The sixteen-colour world offers exactly two ways to say "dim", and
            // both were tried here and both were invisible on the same real
            // terminal ([#60](https://github.com/breferrari/vigia/issues/60)).
            //
            // `DarkGray` is ANSI colour 8, which most schemes define *relative to
            // the background* rather than as a colour: "bright black" is commonly a
            // shade just above the pane. `Reset` plus `DIM` is SGR 2, which asks
            // for the reader's own foreground at reduced intensity, and terminals
            // are free to reduce it as far as they like. One of them put the hints,
            // the counts, the readouts, the empty state and every line number a few
            // points off the background; the other did it again.
            //
            // So this palette stops trying. Colour 7 is the reader's ordinary
            // foreground and is readable by construction, which is the property
            // that actually matters: `SPEC.md` §5 makes colour half the
            // differentiator, and §11.1 already argues that a monitor whose state
            // cannot be read has failed twice. The hints are how a reader finds out
            // the tool has an `f` key.
            //
            // What is given up is the visual hierarchy between chrome and content,
            // and `dark` and `light` keep it, because a palette that knows its own
            // background can pick a grey that is genuinely between the two. That is
            // the cost of inheriting a scheme you cannot see.
            chrome_dim: fg(Color::Gray),
            // Three rungs of one ramp: bright and bold, bright, then plain.
            // `Gray` rather than `DarkGray` for the coldest, deliberately. Every
            // file in an already-dirty worktree is cold until something writes to
            // it, so this is what the **first** frame of a session looks like, and
            // a path drawn in the same near-invisible grey as a comment would open
            // the tool on a screen nobody can read.
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
            // `DarkGray`, and the one palette where the track does *not* step
            // towards the foreground the way `dark` and `light` do. Sixteen names
            // hold nothing between colour 8 and `Gray`, and `Gray` is what this
            // palette draws content in, so a step up would put a track at the
            // weight of the counts beside it. The field's own doc carries the
            // rule this is the exception to.
            spark_track: fg(Color::DarkGray),
            // Grey rather than cyan, for the reason the field's own doc gives:
            // the thumb is a full block and cyan is the sparkline's, so the two
            // would be one colour drawing two meanings. Grey is what this palette
            // already uses for everything structural.
            bar: fg(Color::Gray),
            bar_active: fg(Color::White),
            bar_hover: fg(Color::Gray).add_modifier(Modifier::BOLD),
            bar_track: fg(Color::DarkGray),
            // **The one place colour 8 is the right answer**, and the exception
            // proves the rule that sent everything else to `DIM`. A track is not
            // text: it is a solid block that should sit just above the background,
            // and "just above the background" is exactly what most schemes define
            // colour 8 to be. What is fatal for a key hint is correct for this.
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
            // **The bar, in names, and it is the one row-level diff signal this
            // palette can carry.** §11.1 records the loss it is fixing: at sixteen
            // colours the signal degrades to the sigil column, because a wash has
            // to assume a background and this palette assumes none. That argument
            // is about text on a background, and the bar has none: one blank cell
            // of the pane's own margin, so there is nothing behind it to destroy.
            added_bar: Style::new().bg(Color::Green),
            removed_bar: Style::new().bg(Color::Red),
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
            // wants to skip. Dimmed rather than colour 8, for the reason
            // `chrome_dim` gives: a comment should recede, not vanish.
            comment: fg(Color::Gray),
        }
    }

    /// `assets/preview.svg`, as a palette.
    ///
    /// Every value here is **read out of the picture** rather than chosen, per
    /// `SPEC.md` §5.1's rule that a published artifact answering an open question
    /// is the answer. **[`Theme::spark_track`] is the one exception and says so
    /// itself**: the picture had no sparkline track in it to read, so that value
    /// was picked and the picture was corrected to draw it, which is the same
    /// rule applied in the only direction available when the artifact is silent.
    /// The picture's own class names map onto these directly:
    /// `.fg` `#e6edf3`, `.dim` `#7d8590`, `.faint` `#6e7681`, `.grn` `#3fb950`,
    /// `.red` `#f85149`, `.cyn` `#39c5cf`, `.kw` `#ff7b72`, `.fnn` `#d2a8ff`,
    /// `.typ` `#ffa657`, `.var` `#79c0ff`, `.con` `#e3b341`, with the row washes
    /// `#0f2c1c` and `#2d1416` and the heat and scrollbar tracks' `#21262d` taken
    /// from the rects.
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
            chrome_dim: rgb(0x8b, 0x94, 0x9e),
            path: rgb(0xe6, 0xed, 0xf3).add_modifier(Modifier::BOLD),
            path_live: rgb(0xe6, 0xed, 0xf3),
            path_cold: rgb(0x7d, 0x85, 0x90),
            // `bar_hover`'s `#a8b1bb`, which is **8.71:1** on this pane: quieter
            // than `path_live`'s `#e6edf3` and a long way clear of unreadable.
            path_hover: rgb(0xa8, 0xb1, 0xbb).add_modifier(Modifier::UNDERLINED),
            pulse: rgb(0x39, 0xc5, 0xcf),
            // Cyan, where the picture's sparkline is green. What decides the hue
            // is that green already means addition two rows down, and a churn
            // sparkline is about *when*, not *what*. This used to add that the
            // picture "draws a ramp there and we draw one colour, so it does not
            // answer this", which was true of the hue and became false of the
            // ramp: #196 draws one.
            // **The quietest stop keeps today's value**, so a worktree nobody
            // is writing to looks exactly as it did and only the busy buckets
            // gain. Brighter as it climbs, which is this palette's direction for
            // every ramp it has.
            spark: rgb(0x39, 0xc5, 0xcf),
            spark_warm: rgb(0x7a, 0xe9, 0xf0),
            spark_hot: rgb(0xa8, 0xf2, 0xf7),
            // **One step above `heat_track`, which is the rule this field always
            // had and could not satisfy while the thing it was a step above was
            // itself invisible.** A stroke needs more contrast than a block to
            // read as the same weight: `_` is one line in a cell where `▄` is
            // half of one. With `heat_track` at 2.96:1 this is 4.12:1, and it was
            // `#30363d` at **1.55:1** until 2026-08-16, which is below even the
            // block it was supposed to outrank.
            //
            // Found by the gate rather than by eye, which is the point of having
            // one: nobody reported the sparkline, and an empty bucket is exactly
            // the case `SPEC.md` §5.1 rules has to draw a track, so a launched
            // worktree with no history behind it was drawing the blank column
            // [#78](https://github.com/breferrari/vigia/issues/78) exists to remove.
            spark_track: rgb(0x6e, 0x76, 0x81),
            bar: rgb(0x8b, 0x94, 0x9e),
            bar_active: rgb(0xc9, 0xd1, 0xd9),
            bar_hover: rgb(0xa8, 0xb1, 0xbb),
            // **`#57606a`, and it was `#21262d` until 2026-08-16, which was
            // invisible.** Reported from use: the scrollbar's track and its step
            // buttons could not be seen at all, and a button appeared only while
            // it was pressed, because a press draws in `bar` above and everything
            // else on that column drew in this.
            //
            // Measured rather than adjusted by eye: `#21262d` on `#0d1117` is
            // **1.24:1**, where 1.0 is the background exactly. It was never a
            // subtle colour, it was an absent one, and the same value made the
            // empty heat bucket `SPEC.md` §5.1 promises a track for draw nothing.
            //
            // The earlier reading was that the value was right and the defect
            // belonged to the sixteen-colour rung, because these were taken off
            // `assets/preview.svg` where they do read. That is the picture-versus-
            // cell-grid distinction §5.1 already draws: a 1.24:1 edge over many
            // pixels of SVG is perceptible and the same ratio in one terminal cell
            // is not. The mockup is not wrong; reading a cell colour off it is.
            //
            // `#57606a` is **2.96:1**, against the thumb's 6.15:1. Both visible,
            // and the gap between them is what keeps the ruling one line up true:
            // the track is context and the thumb is the reading.
            //
            // **`#656c76` from 2026-08-18, because the column it sits on stopped
            // being the pane** ([#239](https://github.com/breferrari/vigia/issues/239)).
            // Every ratio above is measured against `#0d1117`, which was true of
            // the bar's cell while the row wash stopped one column short of it.
            // The band runs under the bar now, so on a changed row this glyph sits
            // on `added_row` or `removed_row` instead, and `#57606a` there is
            // **1.88:1** and **2.17:1**. That is nearer the `#21262d` this note
            // rejects as absent than the value it chose, so the track would have
            // survived on context rows and faded on the rows a reader is looking
            // at, drawing the bar as a dashed line down the pane.
            //
            //             context   added  removed
            //   #57606a     2.96     1.88     2.17
            //   #656c76     3.57     2.27     2.61
            //   thumb       6.15     3.91     4.51
            //
            // The gap the paragraph above depends on survives and stops varying:
            // **1.72x on every row kind**, where it was 2.08x on a context row and
            // undefined on a washed one, the track having been the pane there.
            //
            // **`#656c76` is a value this palette did not previously hold, and the
            // plan for #239 said no new colour would be needed. That was wrong and
            // this is where it was found.** The intended value was `spark_track`'s
            // own `#6e7681`, on §5.3's rule that an element earns a colour by
            // taking a role rather than by being distinct. It fails `palette.rs`'s
            // pre-existing separation gate, which requires the thumb to outrank the
            // track by half again: it lands at **1.49x** where 1.50 is the floor.
            //
            // The window is genuinely narrow. Once the track has to clear 2.0:1 on
            // the *added* row and stay under the thumb by half again on that same
            // row, it is bounded to roughly 2.0 and 2.61 there, and **no grey this
            // palette already holds sits inside it**: `#57606a` is below the floor
            // at 1.88, and `#6e7681` and everything above it breaks the separation.
            // So the value is chosen by those two constraints rather than by eye,
            // and it is stated as new rather than dressed up as borrowed.
            //
            // (An earlier version of this note called the intended value `#6e7781`
            // and read it as a second, distinct grey one step from `#6e7681`. There
            // is no such value here: `spark_track` is `#6e7681`, and the other was
            // that value with a single digit slipped, `7` for `6` in the fourth
            // place, which is why it read as a neighbour rather than as a typo. The
            // 1.49x was measured against the real one, so the conclusion held while
            // the attribution did not.)
            bar_track: rgb(0x65, 0x6c, 0x76),
            // **Left at `#57606a` deliberately, and not an oversight.** The move
            // above is paid for by the wash, and the heat strip does not sit on
            // one: it draws on list rows and on file headings, which are never
            // washed. Only the diff's bar crosses a changed row.
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
            // The two rects the picture draws behind changed lines, and the two
            // bars at their left edge. Backgrounds, so the depth ladder drops them
            // below 24-bit on its own and these are only ever drawn as authored.
            // **Stronger than the picture's, and that is a correction rather than
            // a liberty.** `assets/preview.svg` washes with `#0f2c1c` and
            // `#2d1416`, which it can, because it also paints its own background
            // `#0d1117` and knows the contrast it is getting. A terminal is not a
            // picture: a reader's pane is whatever they set it to, and every common
            // dark scheme is *lighter* than the mockup's. A wash darker than the
            // background reads as nothing at all, which is what it did on the first
            // terminal it met.
            //
            // So these are lifted until they are green and red rather than
            // marginally-darker, while staying dark enough to keep `#e6edf3` text
            // and the syntax colours legible on top. What the picture specifies is
            // *that the row is washed in the colour of the change*, and that is
            // what survives; the exact triple was a value chosen against a
            // background this palette does not control.
            added_row: Style::new().bg(Color::Rgb(0x1b, 0x3d, 0x29)),
            removed_row: Style::new().bg(Color::Rgb(0x45, 0x22, 0x2a)),
            // **Unset, and that is a ruling reversed rather than a gap.** These
            // used to invert the sigil cell: diff hue behind, the row's wash in
            // front, on the argument that the sigil is the one cell that already
            // means "this line changed" and so could carry §5.1's left bar without
            // spending a column.
            //
            // It reads as a solid block. Inverting the sigil takes the one glyph
            // that carries the diff signal and turns it into a background, and at
            // terminal sizes the `+` stops being a character. Reported from a real
            // screen, which is the only place this was ever going to show.
            //
            // So the sigil keeps its own colour on the wash, which is what every
            // diff tool does and what the mockup itself draws: its bar is a
            // separate 3px sliver *beside* a green `+`, not a recolouring of it.
            // The wash carries the band; the sigil carries the sign.
            //
            // **The bar itself was then refused, on a reason that has since
            // expired** ([#218](https://github.com/breferrari/vigia/issues/218)).
            // It read: *the bar has no terminal equivalent that does not spend a
            // column I6 forbids.* True when the pane drew from column zero, false
            // once [#119](https://github.com/breferrari/vigia/issues/119) gave it a
            // margin, and nobody re-read the refusal. The margin is blank, the wash
            // already bleeds under it, so the bar is a background on a cell that
            // was carrying nothing.
            //
            // The picture's own colours, which is the point: the sliver beside the
            // `+` is `#3fb950` and the one beside the `−` is `#f85149`, the same
            // hues `added` and `removed` draw the sigils in.
            added_bar: Style::new().bg(Color::Rgb(0x3f, 0xb9, 0x50)),
            removed_bar: Style::new().bg(Color::Rgb(0xf8, 0x51, 0x49)),
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
            // `bar_hover`'s `#3d4650`, **9.59:1** on white. The direction flips
            // with the palette and the rule does not: quieter here means lighter
            // than `path`'s `#1f2328`, not darker.
            path_hover: rgb(0x3d, 0x46, 0x50).add_modifier(Modifier::UNDERLINED),
            pulse: rgb(0x0a, 0x62, 0x6b),
            // **Darker as it climbs**, which is the same rule as `dark`'s and
            // not a second one: both move towards the foreground, and on a light
            // background that direction is down.
            //
            // **Today's flat value becomes the middle stop here**, where on
            // `dark` it stays the quietest, and that asymmetry is forced rather
            // than chosen. The 256-colour cube's axes are `0, 95, 135, 175, 215,
            // 255`, so everything below 95 collapses into one cell: a ramp
            // running *down* from `#0a626b` has nowhere to put two more stops and
            // `the_sparkline_ramp_has_three_stops_where_the_depth_can_draw_them`
            // is what said so. Spread around it instead, and the quiet end
            // lightens.
            spark: rgb(0x5a, 0xa6, 0xae),
            spark_warm: rgb(0x0a, 0x62, 0x6b),
            spark_hot: rgb(0x03, 0x28, 0x2e),
            // **Darker** here where `dark`'s is brighter, which is the same rule
            // and not a second one: both move one step *towards* the foreground,
            // and on a light background that direction is down. `#d0d7de` is a
            // block colour on white and a stroke drawn in it is barely there.
            // One step above `heat_track` for the reason `Theme::dark`'s carries;
            // `#afb8c1` was 2.01:1, under the block it is meant to outrank.
            spark_track: rgb(0x7d, 0x85, 0x90),
            bar: rgb(0x59, 0x63, 0x6e),
            bar_active: rgb(0x24, 0x29, 0x2f),
            bar_hover: rgb(0x3d, 0x46, 0x50),
            // `#8c959f` at 3.04:1 on white, where `#d0d7de` was **1.45:1** and
            // invisible for the same reason the dark palette's was. See the note
            // on `Theme::dark`'s `bar_track`; the two were wrong together and are
            // fixed together, and the gap below `bar`'s 6.11:1 matches.
            //
            // **`#7d8590` from 2026-08-18**, and the third time these two move
            // together ([#239](https://github.com/breferrari/vigia/issues/239)).
            // The band runs under the bar, so this glyph sits on `added_row` or
            // `removed_row` on a changed row rather than on white:
            //
            //             context   added  removed
            //   #8c959f     3.04     2.40     2.25
            //   #7d8590     3.73     2.95     2.77
            //   thumb       6.11     4.83     4.54
            //
            // A uniform **1.64x** below the thumb on every row kind. `#7d8590` is
            // `spark_track`'s value here, so no colour is invented on this palette
            // either.
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
            // The same correction one background over: a wash has to be *further*
            // from the pane than the pane is from white, or a reader on an
            // off-white terminal sees nothing. Darker here, where `dark` went
            // lighter, for the reason every ramp in this palette is reversed.
            added_row: Style::new().bg(Color::Rgb(0xc0, 0xf0, 0xcd)),
            removed_row: Style::new().bg(Color::Rgb(0xff, 0xd4, 0xd1)),
            // Set, for the reason `dark` gives at length, in this palette's own
            // diff hues rather than the dark one's: a bar is a background on a
            // blank cell, so what it has to clear is the page behind it.
            added_bar: Style::new().bg(Color::Rgb(0x1a, 0x7f, 0x37)),
            removed_bar: Style::new().bg(Color::Rgb(0xcf, 0x22, 0x2e)),
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
/// Resolved **before the screen is taken**, so a theme that does not parse reports
/// on a terminal the reader can still read. That is the same rule `SPEC.md` §11.1
/// already states for a path that is not a repository, and it exists for the same
/// reason: an error painted inside a TUI that then hands the terminal back is an
/// error nobody sees.
pub fn from_env(
    depth: Depth,
    lookup: impl Fn(&str) -> Option<String>,
) -> Result<Theme, ThemeError> {
    // **Chosen here, resolved once on the way out.** `.resolve(depth)` used to be
    // written on each of three exits, which is a rule spelled out three times: a
    // fourth source returns a `Theme` either way so the compiler stays silent, and
    // the omission is invisible at truecolour, where `resolve` is the identity. It
    // would surface only on somebody else's sixteen-colour terminal.
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
        // Then the file, which is where a preference set once lives. **Absent is
        // not an error and unreadable is**, which is the distinction that matters:
        // nobody has to have one, but a reader who wrote one and got the default
        // silently would have no way to find out why.
        load(&path)?
    } else {
        Theme::default()
    };
    Ok(theme.resolve(depth))
}

/// `rela` under the reader's home directory, if there is one.
///
/// `HOME` first, because it is set on every Unix and by Git Bash on Windows too,
/// then `USERPROFILE`. Read through the same lookup its callers use, so a test can
/// place a home directory without touching the process environment.
///
/// **Takes the relative path since [#306](https://github.com/breferrari/vigia/issues/306)**,
/// because there are two files under that directory now and the empty-versus-unset
/// rule below is the kind that is got wrong once per copy. It was got wrong once
/// already, in this function, before there was a second caller to get it wrong
/// again. `crate::config::CONFIG_FILE` is the other one.
pub(crate) fn home_file(rela: &str, lookup: &impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    // **Each candidate is emptied-checked before the next is tried**, which is the
    // whole of this function and was wrong on the first write. Filtering after the
    // fallback reads naturally and is a different rule: `HOME=""` is `Some("")`, so
    // `or_else` never fires, and the filter then discards it having already skipped
    // `USERPROFILE`. A reader with an empty `HOME` would get no theme file and no
    // way to know why.
    //
    // The same empty-versus-unset trap `VIGIA_COLOR` had. Twice in one file is
    // enough to say it out loud: an environment variable has three states, not
    // two, and the third is the one that only shows up on somebody else's machine.
    ["HOME", "USERPROFILE"]
        .into_iter()
        .filter_map(lookup)
        .find(|home| !home.trim().is_empty())
        .map(|home| Path::new(home.trim()).join(rela))
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
    // **A BOM is stripped, and `trim` will not do it.** U+FEFF is `Cf` rather
    // than `White_Space`, so it survives every trim here and lands inside the
    // first key: a theme file saved by Notepad, whose UTF-8 default writes one,
    // stops the shell from starting with an error naming an invisible byte.
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
        // Comments are stripped from the *value* only, and only after the `=`, so
        // `added = #3fb950 # the picture's green` works and a bare `#` line is
        // still a comment.
        // Comments are handled token-wise inside the value rather than by cutting
        // the line here: see `words_of` for the two rules that were wrong first.
        let value = value.trim();

        if key == "base" {
            if touched {
                return Err(ThemeError::LateBase { line });
            }
            // A second `base` used to be accepted with the last one winning, which
            // is the silent-discard this parser refuses everywhere else: `touched`
            // is only set by an ordinary key, so two `base` lines never tripped it.
            if based {
                return Err(ThemeError::RepeatedBase { line });
            }
            based = true;
            // Through `words_of` like every other value, so the documented
            // comment idiom works on the one line every theme file starts with.
            // Reading the raw value made `base = dark # the picture` report that
            // there is no theme called "dark # the picture".
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
///
/// Every configuration format that writes colours in hex has this collision: `#`
/// starts a comment and also starts a value. **Two rules were tried before this
/// one and both were wrong in a way that reading did not show.**
///
/// *"A `#` preceded by whitespace is a comment"* eats `added_row = on #0f2c1c`,
/// where a background with no foreground puts a space before the only colour on
/// the line.
///
/// *"A `#` followed by six hex digits is a colour, otherwise a comment"* fixes that
/// and then does something worse: `added = #gg0000` is a **typo**, and this rule
/// reads it as a comment, leaves the value empty, and parses successfully with the
/// key silently set to nothing. A theme file that reports no error and changes
/// nothing is the exact failure the refuse-unknown-keys rule exists to prevent,
/// arrived at from the other direction.
///
/// The rule that holds is positional: a `#` token begins a comment only once
/// **something real already precedes it** in the value. The first token of a value
/// is therefore always an attempted colour and reaches [`colour_of`], which
/// rejects it by name.
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
///
/// **A value with nothing in it is an error, not an empty style.** `added =` is
/// something a reader meant to finish, and accepting it would set the key to no
/// colour at all, which is a theme file that changes something invisibly rather
/// than not changing it.
fn style_of(value: &str, line: usize, current: Style) -> Result<Style, ThemeError> {
    // **Seeded from the key's current value, not from nothing.** `added = bold`
    // reads as "make additions bold" and used to mean "make additions bold and
    // colourless", because the style was built from `Style::new()` and `set`
    // replaces the whole thing. That is the same invisible change
    // [`ThemeError::MissingValue`] exists to prevent, reached from the other
    // direction: a line that says one thing and silently does two.
    //
    // Only the fields the value names are overwritten, so a value that names a
    // colour still replaces the colour.
    let mut style = current;
    let tokens = words_of(value);
    if tokens.is_empty() {
        return Err(ThemeError::MissingValue { line });
    }
    let mut words = tokens.into_iter().peekable();

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
