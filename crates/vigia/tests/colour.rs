//! The colour-depth ladder: what this terminal can show, and what a palette
//! becomes when it can show less than the palette was written in.
//!
//! Two halves, and only the first is arithmetic.
//!
//! **Detection** is a precedence table, and a table is exactly the shape that goes
//! wrong by a rule being right in isolation and in the wrong place. So it is driven
//! one row per rung, each with the rungs above it deliberately set to something
//! else, rather than by asserting each variable alone against an empty environment.
//!
//! **Degradation** has one property that matters more than the rest and it is not
//! "the output is expressible". A depth that mapped every colour to black would pass
//! that and destroy the product. What has to survive every rung is the *distinction*:
//! `SPEC.md` §5 spends the diff signal on colour, so added and removed may never
//! land on the same cell colour however few colours are left.

use ratatui::style::{Color, Modifier, Style};
use vigia::{DEPTH_VAR, Depth, Theme};

/// An environment built from pairs, so a case reads as the thing it is testing.
///
/// Takes a slice rather than touching the process, because `cargo test` runs these
/// on threads of one process and `set_var` is both racy and, since Rust 2024,
/// `unsafe`. `Depth::from_env` exists in this shape for exactly that reason.
fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + use<> {
    let owned: Vec<(String, String)> = pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    move |key| {
        owned
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.to_owned())
    }
}

/// One row of the precedence table: why, the platform, the environment, the rung.
///
/// Named because `clippy::type_complexity` is denied and a four-part tuple in a
/// slice reaches it. The alias is also the better documentation: the table is read
/// row-wise and the names say what each column is for.
type Precedence<'a> = (&'a str, bool, &'a [(&'a str, &'a str)], Depth);

/// One row of the hue table: what it is, the colour, and the names it may take.
type Hue<'a> = (&'a str, (u8, u8, u8), &'a [Color]);

fn depth(windows: bool, pairs: &[(&str, &str)]) -> Depth {
    Depth::from_env(windows, env(pairs)).expect("a depth")
}

#[test]
fn depth_is_decided_by_the_first_variable_that_answers() {
    // Each row sets everything *below* its own rung to a value that would give a
    // different answer, so a rule consulted out of order fails here rather than
    // passing by coincidence. The bottom rows have nothing left to shadow.
    let cases: &[Precedence<'_>] = &[
        (
            "the override outranks every signal under it",
            false,
            &[
                (DEPTH_VAR, "256"),
                ("NO_COLOR", "1"),
                ("COLORTERM", "truecolor"),
                ("TERM", "xterm-256color"),
            ],
            Depth::Ansi256,
        ),
        (
            "NO_COLOR outranks a terminal claiming 24-bit",
            false,
            &[("NO_COLOR", "1"), ("COLORTERM", "truecolor")],
            Depth::None,
        ),
        (
            // Present at all, whatever it holds. The no-color.org wording is
            // "present and not an empty string", and this is deliberately the
            // looser reading: `NO_COLOR=` in an env file is a reader asking for no
            // colour, and honouring the letter over the intent would hand them
            // colour they went out of their way to switch off.
            "an empty NO_COLOR still means no colour",
            false,
            &[("NO_COLOR", ""), ("COLORTERM", "truecolor")],
            Depth::None,
        ),
        (
            "a dumb terminal outranks a COLORTERM it cannot honour",
            false,
            &[("TERM", "dumb"), ("COLORTERM", "truecolor")],
            Depth::None,
        ),
        (
            "COLORTERM outranks TERM's 256",
            false,
            &[("COLORTERM", "24bit"), ("TERM", "xterm-256color")],
            Depth::Truecolor,
        ),
        (
            // A terminal naming itself outranks the entry the innermost layer of
            // the session decided to advertise, which is the ruling `WT_SESSION`
            // already stands on one rung down. It has to be the case where the
            // two disagree, or it passes against an implementation that never
            // reads the variable.
            "TERM_PROGRAM outranks a TERM that only claims 256",
            false,
            &[("TERM_PROGRAM", "WezTerm"), ("TERM", "xterm-256color")],
            Depth::Truecolor,
        ),
        (
            // The one entry in the program table that is not truecolour, and the
            // reason the table returns a rung rather than a bool. Terminal.app
            // accepts 24-bit sequences and rounds them to its own palette, so
            // claiming truecolour there paints colours nobody chose.
            "Apple_Terminal names a terminal that has never drawn 24-bit",
            false,
            &[("TERM_PROGRAM", "Apple_Terminal"), ("TERM", "xterm-direct")],
            Depth::Ansi256,
        ),
        (
            // The table is evidence about terminals someone checked, not a claim
            // about the ones nobody has. A program it does not know caps nothing.
            "an unknown TERM_PROGRAM falls through rather than capping",
            false,
            &[("TERM_PROGRAM", "something-nobody-has"), ("TERM", "foot")],
            Depth::Truecolor,
        ),
        (
            "COLORTERM outranks a program name",
            false,
            &[
                ("COLORTERM", "truecolor"),
                ("TERM_PROGRAM", "Apple_Terminal"),
            ],
            Depth::Truecolor,
        ),
        (
            // terminfo's own spelling for direct colour, which is the vocabulary
            // of the variable being set. The numbered entries say how many bits
            // each channel gets and are all direct colour.
            "TERM's own spelling of direct colour",
            false,
            &[("TERM", "xterm-direct2")],
            Depth::Truecolor,
        ),
        (
            // None of these contains `256color`, so every one of them fell to
            // sixteen whenever `COLORTERM` had not survived, which is the whole
            // defect. Driven as a rung of the table rather than as its own test
            // so an entry that stops promoting fails here.
            "a terminal that ships its own entry and has never had a 16-colour era",
            false,
            &[("TERM", "xterm-ghostty")],
            Depth::Truecolor,
        ),
        (
            // Suffixed rather than substringed: the boundary is the `-` the
            // database itself uses, so a variant is the same terminal.
            "a variant of one of those entries is the same terminal",
            false,
            &[("TERM", "foot-extra")],
            Depth::Truecolor,
        ),
        (
            // `TERM` promotes only. `sixel` is not a colour claim and the entry
            // names no terminal in the table, so the floor stays where it is
            // rather than the substring rules reaching for it.
            "a TERM that promises neither rung leaves the floor alone",
            false,
            &[("TERM", "xterm-sixel")],
            Depth::Ansi16,
        ),
        (
            "TERM's own spelling of 256",
            false,
            &[("TERM", "screen-256color")],
            Depth::Ansi256,
        ),
        (
            // **Above the `TERM` rung, and the row below is why.** As a bare
            // `WT_SESSION` case this was vacuous: every Windows path returns
            // truecolour, so it passed against an implementation that never read
            // the variable. It has to be the case where the two rungs disagree.
            "Windows Terminal outranks a TERM that only claims 256",
            true,
            &[("WT_SESSION", "abc"), ("TERM", "xterm-256color")],
            Depth::Truecolor,
        ),
        (
            // Git Bash and MSYS export exactly this on Windows. Reading `TERM`
            // first sent the most common shell for this repo to 256, where a
            // subtle wash quantises to a saturated primary.
            "a Windows shell reporting xterm-256color and nothing else",
            true,
            &[("TERM", "xterm-256color")],
            Depth::Ansi256,
        ),
        (
            // Not 256, which this was and which is a different wrong answer rather
            // than a safe one: the xterm cube's darkest axis levels are 0 and 95
            // with nothing between, so a subtle wash quantises to a saturated
            // primary. What 256 was protecting against is consoles older than
            // Windows 10 1703, which is not a supported target.
            "Windows draws 24-bit, and has since 1703",
            true,
            &[],
            Depth::Truecolor,
        ),
        (
            // `TERM` only ever promotes. On Windows it is usually unset, and where
            // something does set it (Git Bash, MSYS) it says `xterm`, which is a
            // name from a world where sixteen was the floor. Reading that as a
            // demotion would take 256 colours away from every Windows console,
            // all of which have them, on the strength of a variable that was
            // describing something else.
            "a Windows TERM that does not claim 256 does not demote below Windows",
            true,
            &[("TERM", "xterm")],
            Depth::Truecolor,
        ),
        ("nothing at all, anywhere else", false, &[], Depth::Ansi16),
    ];

    for (why, windows, pairs, want) in cases {
        assert_eq!(depth(*windows, pairs), *want, "{why}");
    }
}

#[test]
fn auto_falls_through_rather_than_naming_a_rung() {
    // The property that makes the override unsettable in a child shell without
    // unsetting the variable. If `auto` were a rung it would pin the depth to
    // whatever that rung is and the fall-through below could not happen.
    assert_eq!(
        depth(false, &[(DEPTH_VAR, "auto"), ("COLORTERM", "truecolor")]),
        Depth::Truecolor
    );
    assert_eq!(depth(false, &[(DEPTH_VAR, "auto")]), Depth::Ansi16);
}

#[test]
fn an_override_is_read_loosely_but_refused_rather_than_ignored() {
    for spelling in ["truecolor", "TrueColour", "  24BIT  "] {
        assert_eq!(
            depth(false, &[(DEPTH_VAR, spelling)]),
            Depth::Truecolor,
            "{spelling:?}"
        );
    }

    // Set but empty is the same as unset, which is not obvious and is reachable
    // without trying: `$env:X = ''` in PowerShell leaves the variable set and
    // empty, and a child process sees an empty string rather than nothing.
    // Without this, clearing a variable that way stops the shell from starting
    // over a value nobody gave.
    assert_eq!(
        depth(false, &[(DEPTH_VAR, ""), ("COLORTERM", "truecolor")]),
        Depth::Truecolor
    );
    assert_eq!(
        depth(false, &[(DEPTH_VAR, "   "), ("COLORTERM", "truecolor")]),
        Depth::Truecolor
    );

    // `NO_COLOR` reads the opposite way on purpose: it has no valid values, so
    // presence is the whole signal and an empty one still means what it says.
    assert_eq!(depth(false, &[("NO_COLOR", "")]), Depth::None);

    // The half that matters. A variable that was set and had no effect is the one
    // failure a reader cannot diagnose by looking at the screen.
    let refused = Depth::from_env(false, env(&[(DEPTH_VAR, "tru")])).expect_err("refused");
    assert_eq!(refused.value, "tru");
    let said = refused.to_string();
    assert!(said.contains("tru"), "{said}");
    assert!(said.contains(DEPTH_VAR), "{said}");
}

/// Every colour any palette can hand the resolver, so a rung is asserted over the
/// whole input space rather than over the cases that occurred to me.
fn every_colour() -> Vec<Color> {
    let mut all = vec![
        Color::Reset,
        Color::Black,
        Color::Red,
        Color::Green,
        Color::Yellow,
        Color::Blue,
        Color::Magenta,
        Color::Cyan,
        Color::Gray,
        Color::DarkGray,
        Color::LightRed,
        Color::LightGreen,
        Color::LightYellow,
        Color::LightBlue,
        Color::LightMagenta,
        Color::LightCyan,
        Color::White,
    ];
    all.extend((0..=255).map(Color::Indexed));
    for r in (0..=255).step_by(17) {
        for g in (0..=255).step_by(17) {
            for b in (0..=255).step_by(17) {
                all.push(Color::Rgb(r as u8, g as u8, b as u8));
            }
        }
    }
    all
}

#[test]
fn every_rung_resolves_to_what_that_rung_can_draw() {
    for colour in every_colour() {
        let style = Style::new().fg(colour).bg(colour);

        let truecolor = Depth::Truecolor.resolve(style);
        assert_eq!(truecolor.fg, Some(colour), "truecolor changed {colour:?}");

        let indexed = Depth::Ansi256.resolve(style);
        assert!(
            !matches!(indexed.fg, Some(Color::Rgb(..))),
            "256 left {colour:?} as RGB"
        );
        assert!(
            !matches!(indexed.bg, Some(Color::Rgb(..))),
            "256 left {colour:?} as an RGB background"
        );

        let sixteen = Depth::Ansi16.resolve(style);
        assert!(
            !matches!(
                sixteen.fg,
                Some(Color::Rgb(..)) | Some(Color::Indexed(16..))
            ),
            "16 left {colour:?} inexpressible, as {:?}",
            sixteen.fg
        );

        let none = Depth::None.resolve(style);
        assert_eq!(none.fg, Some(Color::Reset), "None kept {colour:?}");
    }
}

#[test]
fn a_background_needs_24_bit_where_a_foreground_does_not() {
    // `SPEC.md` §5.1: a quantised background is a solid block rather than a tint,
    // and a slab behind syntax-highlighted text is worse than no tint at all. So the
    // row tint needs the top rung while the text goes all the way down to sixteen.
    let style = Style::new()
        .fg(Color::Rgb(0x3f, 0xb9, 0x50))
        .bg(Color::Rgb(0x1b, 0x3d, 0x29));

    assert!(Depth::Truecolor.resolve(style).bg.is_some());
    assert_eq!(Depth::Ansi256.resolve(style).bg, None);
    assert_eq!(Depth::Ansi16.resolve(style).bg, None);
    assert_eq!(Depth::None.resolve(style).bg, None);

    // The foreground is untouched by any of that: it quantises at every rung and
    // survives to sixteen, which is the asymmetry the name of this test is about.
    assert!(Depth::Ansi256.resolve(style).fg.is_some());
    assert!(Depth::Ansi16.resolve(style).fg.is_some());

    // **What the cube would have done, stated rather than implied.** This is the
    // whole evidence for the rung moving: the wash and its opposite both land on a
    // saturated primary, at roughly two and a half times the authored luminance.
    // Left as a live assertion because `to_indexed` is still the foreground path and
    // a change to it should have to look at this.
    let at_256 = |r, g, b| {
        Depth::Ansi256
            .resolve(Style::new().fg(Color::Rgb(r, g, b)))
            .fg
    };
    assert_eq!(at_256(0x1b, 0x3d, 0x29), Some(Color::Indexed(22)));
    assert_eq!(at_256(0x45, 0x22, 0x2a), Some(Color::Indexed(52)));

    // And dropped, not blanked. `None` inherits whatever the reader's pane is
    // painted in; `Reset` would impose the terminal's default over it.
    assert_ne!(Depth::Ansi16.resolve(style).bg, Some(Color::Reset));
}

#[test]
fn a_terminal_that_can_draw_the_wash_is_detected_as_one_that_can() {
    // **The two halves of this file, joined.** Detection and degradation were each
    // correct alone, and the defect lived in the sentence they make together: a
    // real macOS pane resolved to sixteen, sixteen drops backgrounds by the rule
    // the test above pins, and `dark` therefore drew every changed row unwashed
    // while colouring everything else. From the outside that reads as a palette
    // being ignored, which is what it was reported as.
    //
    // Driven from the environment to the drawn style rather than from either end,
    // because neither end is where it broke.
    //
    // `COLORTERM` is absent in every row on purpose. It is the only convention for
    // claiming 24-bit and nothing propagates it: `ssh` forwards `TERM` alone, and a
    // multiplexer replaces `TERM` with an entry of its own. Its presence is what
    // made this invisible for a phase, so a row that sets it is a row that cannot
    // fail.
    let cases: &[(&str, &[(&str, &str)])] = &[
        // The reported screen: a macOS terminal, `tmux` at its default
        // `default-terminal screen`. `screen` claims nothing at all, so before
        // `TERM_PROGRAM` was read this was sixteen.
        (
            "a macOS pane inside tmux",
            &[("TERM_PROGRAM", "iTerm.app"), ("TERM", "screen")],
        ),
        // The same arrangement under the terminals that ship their own entry, none
        // of which spells `256color`.
        ("Ghostty", &[("TERM", "xterm-ghostty")]),
        ("kitty", &[("TERM", "xterm-kitty")]),
        ("Alacritty", &[("TERM", "alacritty")]),
        ("WezTerm", &[("TERM", "wezterm")]),
    ];

    for (why, pairs) in cases {
        let depth = depth(false, pairs);
        // **24-bit, not "at least 256"**, and that bound is the second half of the
        // same report. A wash needs the top rung now: the cube's darkest two levels
        // are 0 and 95, so `#1b3d29` lands on `#005f00` and a file of pure
        // additions draws as a screen of flat green.
        assert_eq!(
            depth,
            Depth::Truecolor,
            "{why}: detected {depth:?}, and a wash cannot be drawn below 24-bit"
        );
        let theme = Theme::dark().resolve(depth);
        assert!(
            theme.added_row.bg.is_some() && theme.removed_row.bg.is_some(),
            "{why}: `dark` at {depth:?} draws no row wash, so a changed line is \
             the sigil alone"
        );
    }

    // Terminal.app is the case this cannot fix and must not pretend to. It has
    // never drawn 24-bit, so it gets 256 and 256 has no honest wash: the diff
    // signal is the sigil column, which is what `SPEC.md` §11.1 records as the
    // cost of the rung rather than a failure here.
    let depth = depth(
        false,
        &[("TERM_PROGRAM", "Apple_Terminal"), ("TERM", "screen")],
    );
    assert_eq!(depth, Depth::Ansi256);
    assert_eq!(Theme::dark().resolve(depth).added_row.bg, None);
}

#[test]
fn no_colour_still_leaves_the_modifiers() {
    // `NO_COLOR` asks for no colour. Bold is not colour, and on a monochrome
    // terminal it is the only distinction left, so taking it would be the one
    // reading of the convention nobody asked for.
    let style = Style::new()
        .fg(Color::Rgb(0xff, 0x00, 0x00))
        .add_modifier(Modifier::BOLD | Modifier::DIM);
    let flat = Depth::None.resolve(style);
    assert_eq!(flat.fg, Some(Color::Reset));
    assert!(flat.add_modifier.contains(Modifier::BOLD));
    assert!(flat.add_modifier.contains(Modifier::DIM));
}

#[test]
fn the_ramp_is_never_matched_against_the_sixteen_names() {
    // The named colours are whatever the reader's own scheme says they are, so
    // treating them as fixed RGB and quantising *to* them would hand back a colour
    // that means something else on their terminal. At 256 an index below 16 is
    // already expressible and must survive untouched.
    for i in 0..16u8 {
        assert_eq!(
            Depth::Ansi256
                .resolve(Style::new().fg(Color::Indexed(i)))
                .fg,
            Some(Color::Indexed(i))
        );
        assert_eq!(
            Depth::Ansi16.resolve(Style::new().fg(Color::Indexed(i))).fg,
            Some(Color::Indexed(i))
        );
    }

    // And nothing quantised ever lands in that range, at either rung.
    for colour in every_colour() {
        if let Some(Color::Indexed(i)) = Depth::Ansi256
            .resolve(Style::new().fg(colour))
            .fg
            .filter(|_| matches!(colour, Color::Rgb(..)))
        {
            assert!(i >= 16, "{colour:?} quantised onto the reader's own {i}");
        }
    }
}

#[test]
fn the_quantiser_prefers_the_grey_ramp_where_the_grey_ramp_is_nearer() {
    // The cube's darkest three levels are 95 apart and the ramp's steps are 10, so
    // a near-grey belongs on the ramp. Picking the cube unconditionally is the
    // common shortcut and it is what turns a dim grey comment into black.
    let grey = Depth::Ansi256.resolve(Style::new().fg(Color::Rgb(0x6e, 0x76, 0x81)));
    let Some(Color::Indexed(i)) = grey.fg else {
        panic!("expected an index, got {:?}", grey.fg)
    };
    assert!((232..=255).contains(&i), "landed on {i}, not the grey ramp");

    // A saturated colour is not near any grey and must land on the cube.
    let green = Depth::Ansi256.resolve(Style::new().fg(Color::Rgb(0x3f, 0xb9, 0x50)));
    let Some(Color::Indexed(i)) = green.fg else {
        panic!("expected an index")
    };
    assert!((16..232).contains(&i), "landed on {i}, not the cube");
}

#[test]
fn the_quantiser_is_exact_on_the_colours_the_palette_already_holds() {
    // Every cube corner round-trips, which is the property that says the axis
    // quantiser is picking nearest rather than truncating. Truncation passes at 0
    // and fails everywhere else, so the whole cube is swept.
    const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
    for (ri, r) in LEVELS.iter().enumerate() {
        for (gi, g) in LEVELS.iter().enumerate() {
            for (bi, b) in LEVELS.iter().enumerate() {
                let want = 16 + 36 * ri + 6 * gi + bi;
                let got = Depth::Ansi256.resolve(Style::new().fg(Color::Rgb(*r, *g, *b)));
                // A cube corner that is also a grey is legitimately either, since
                // both are exact, so those four are only required to be exact.
                if r == g && g == b {
                    continue;
                }
                assert_eq!(
                    got.fg,
                    Some(Color::Indexed(want as u8)),
                    "#{r:02x}{g:02x}{b:02x}"
                );
            }
        }
    }
}

#[test]
fn degrading_never_collapses_a_hue_onto_its_opposite() {
    // The one distinction that may never be lost at any rung that has colour at
    // all. `SPEC.md` §5.1 already records that the diff signal narrows to the sigil
    // column at sixteen colours; what it must not do is narrow to nothing, which is
    // what an addition and a removal drawn the same colour would mean.
    //
    // Driven from the mockup's own hues, which is what `dark` is authored in.
    let added = Color::Rgb(0x3f, 0xb9, 0x50);
    let removed = Color::Rgb(0xf8, 0x51, 0x49);
    let mixed = Color::Rgb(0xe3, 0xb3, 0x41);

    for rung in [Depth::Truecolor, Depth::Ansi256, Depth::Ansi16] {
        let of = |c| rung.resolve(Style::new().fg(c)).fg;
        assert_ne!(
            of(added),
            of(removed),
            "{rung:?} collapsed added onto removed"
        );
        assert_ne!(of(added), of(mixed), "{rung:?} collapsed added onto mixed");
        assert_ne!(
            of(removed),
            of(mixed),
            "{rung:?} collapsed removed onto mixed"
        );
    }
}

#[test]
fn a_green_stays_green_and_a_red_stays_red_at_sixteen() {
    // Stronger than "they differ", and it is the reason the distance function is
    // weighted rather than plain Euclidean: plain RGB distance treats a blue error
    // as costing what a green one does, and sends a mid-green to cyan. A reader
    // glancing at a diff reads hue, not identity.
    let green = Depth::Ansi16.resolve(Style::new().fg(Color::Rgb(0x3f, 0xb9, 0x50)));
    assert!(
        matches!(green.fg, Some(Color::Green | Color::LightGreen)),
        "{:?}",
        green.fg
    );

    let red = Depth::Ansi16.resolve(Style::new().fg(Color::Rgb(0xf8, 0x51, 0x49)));
    assert!(
        matches!(red.fg, Some(Color::Red | Color::LightRed)),
        "{:?}",
        red.fg
    );

    let yellow = Depth::Ansi16.resolve(Style::new().fg(Color::Rgb(0xe3, 0xb3, 0x41)));
    assert!(
        matches!(yellow.fg, Some(Color::Yellow | Color::LightYellow)),
        "{:?}",
        yellow.fg
    );
}

#[test]
fn a_tint_keeps_its_hue_at_256_rather_than_landing_on_the_grey_ramp() {
    // **This reached a screen.** A row wash is a desaturated colour by design, and
    // on raw distance the nearest grey beats the nearest cube entry for one: the
    // cube's axes jump 0, 95, 135 while the ramp steps by ten, so anything sitting
    // between two cube levels measures closer to a grey than to either. The wash
    // then draws as a neutral band, which is a tint that has lost the only thing it
    // was for, and a reader reports that the background colour "does not happen".
    //
    // **Driven as foregrounds, and the fixture is still the washes.** 256 no longer
    // draws a background at all, so this is now a property of the quantiser rather
    // than of anything on screen. The wash colours stay the fixture because they are
    // the most desaturated things any palette here holds, which makes them the case
    // the grey ramp is most likely to steal; a test that swapped them for ordinary
    // syntax hues would keep passing and stop being about anything.
    for (name, style) in [
        ("dark added", Theme::dark().added_row),
        ("dark removed", Theme::dark().removed_row),
        ("light added", Theme::light().added_row),
        ("light removed", Theme::light().removed_row),
    ] {
        let want = style.bg.expect("a wash");
        let as_fg = Style::new().fg(want);
        let got = Depth::Ansi256.resolve(as_fg).fg.expect("a colour");
        let Color::Indexed(i) = got else {
            panic!("{name} did not quantise: {got:?}")
        };
        assert!(
            (16..232).contains(&i),
            "{name} {want:?} landed on index {i}, which is the grey ramp"
        );
    }
}

#[test]
fn a_channel_between_two_levels_takes_the_nearer_one() {
    // **The cube corners cannot test this**, and a mutation proved it: replacing the
    // axis picker's `abs_diff` with a `saturating_sub` still maps every corner to
    // itself, because at a corner the distance is zero either way. It is only wrong
    // in between, where `saturating_sub` reads every level above the value as an
    // equally good zero and picks the last one instead of the nearest.
    //
    // The levels are 0, 95, 135, 175, 215, 255, so the first gap is the wide one and
    // the place to sit in.
    let index = |r: u8, g: u8, b: u8| match Depth::Ansi256
        .resolve(Style::new().fg(Color::Rgb(r, g, b)))
        .fg
    {
        Some(Color::Indexed(i)) => i,
        other => panic!("expected an index, got {other:?}"),
    };

    // **Saturated on purpose.** A dark near-neutral like `(40, 0, 0)` is genuinely
    // nearer a dark grey than it is to black, so the grey ramp wins it and the cube
    // arithmetic never runs. Pinning one channel at full keeps every case in the
    // cube, where the axis picker is the only thing deciding.
    //
    // The cube is `16 + 36*r + 6*g + b` over level indices, so one channel moves
    // the answer by a known stride and the arithmetic stays legible in the result.
    assert_eq!(index(40, 255, 0), 16 + 30, "40 is nearer 0 than 95");
    assert_eq!(index(60, 255, 0), 16 + 36 + 30, "60 is nearer 95 than 0");
    assert_eq!(index(255, 40, 0), 16 + 180, "the same, one axis over");
    assert_eq!(index(255, 0, 40), 16 + 180, "and again");

    // 47 is just below the midpoint of the widest gap and resolves downwards.
    assert_eq!(index(47, 255, 0), 16 + 30, "the midpoint of 0 and 95");
}

#[test]
fn the_mockups_own_hues_keep_their_hue_at_sixteen() {
    // Every colour `assets/preview.svg` draws, through the rung that has the least
    // to work with. This is what justifies the two constants in `to_ansi16` that a
    // reader would otherwise have to take on faith: the chroma cut at a third, and
    // the bright threshold at 0xc0. Both were chosen against this table and both
    // have a row here that moves if they change.
    let cases: &[Hue<'_>] = &[
        (
            "addition",
            (0x3f, 0xb9, 0x50),
            &[Color::Green, Color::LightGreen],
        ),
        (
            "removal",
            (0xf8, 0x51, 0x49),
            &[Color::Red, Color::LightRed],
        ),
        (
            "keyword salmon",
            (0xff, 0x7b, 0x72),
            &[Color::Red, Color::LightRed],
        ),
        // The row that decided the cut. At `chroma / 2` this loses its red bit by a
        // single unit and draws blue, which is a purple keyword colour turning into
        // the variable colour sitting next to it on the same line.
        (
            "function purple",
            (0xd2, 0xa8, 0xff),
            &[Color::Magenta, Color::LightMagenta],
        ),
        (
            "type orange",
            (0xff, 0xa6, 0x57),
            &[Color::Yellow, Color::LightYellow],
        ),
        (
            "constant gold",
            (0xe3, 0xb3, 0x41),
            &[Color::Yellow, Color::LightYellow],
        ),
        (
            "accent cyan",
            (0x39, 0xc5, 0xcf),
            &[Color::Cyan, Color::LightCyan],
        ),
        // The greys, which have no hue to keep and are ranked by luma instead. The
        // spread across three distinct greys is the assertion: chrome, faint text
        // and a heat track that all drew the same grey would be a strip a reader
        // cannot separate from the counters beside it.
        ("page background", (0x0d, 0x11, 0x17), &[Color::Black]),
        (
            "heat track",
            (0x21, 0x26, 0x2d),
            &[Color::Black, Color::DarkGray],
        ),
        ("dim chrome", (0x6e, 0x76, 0x81), &[Color::DarkGray]),
        (
            "faint text",
            (0x7d, 0x85, 0x90),
            &[Color::Gray, Color::DarkGray],
        ),
        ("foreground", (0xe6, 0xed, 0xf3), &[Color::White]),
    ];

    for (what, (r, g, b), allowed) in cases {
        let got = Depth::Ansi16
            .resolve(Style::new().fg(Color::Rgb(*r, *g, *b)))
            .fg
            .expect("a colour");
        assert!(
            allowed.contains(&got),
            "{what} #{r:02x}{g:02x}{b:02x} drew {got:?}, wanted one of {allowed:?}"
        );
    }
}

#[test]
fn the_rungs_are_ordered_so_a_comparison_reads_as_a_capability() {
    assert!(Depth::None < Depth::Ansi16);
    assert!(Depth::Ansi16 < Depth::Ansi256);
    assert!(Depth::Ansi256 < Depth::Truecolor);
    // The safe answer is the default, not the ambitious one: an over-claim paints
    // colours a terminal cannot show, an under-claim only looks flatter.
    assert_eq!(Depth::default(), Depth::Ansi16);
}
