//! The colour-depth ladder: what this terminal can show, and what a palette
//! becomes when it can show less than the palette was written in.

use ratatui::style::{Color, Modifier, Style};
use vigia::{DEPTH_VAR, Depth, Theme};

/// An environment built from pairs, so a case reads as the thing it is testing.
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
            // Present at all, whatever it holds.
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
            // Folded, matching the glyph ladder's own `TERM` arm: unfolded, one of the
            // two hears `TERM=DUMB` as a terminal saying it cannot draw and the other
            // did not.
            "a dumb terminal shouting is still a dumb terminal",
            false,
            &[("TERM", "DUMB"), ("COLORTERM", "truecolor")],
            Depth::None,
        ),
        (
            // `term_depth`'s `contains("truecolor")` arm had no case at all: dropping
            // it survived the suite.
            "a TERM that says truecolour in words is believed",
            false,
            &[("TERM", "xterm-truecolor")],
            Depth::Truecolor,
        ),
        (
            "COLORTERM outranks TERM's 256",
            false,
            &[("COLORTERM", "24bit"), ("TERM", "xterm-256color")],
            Depth::Truecolor,
        ),
        (
            // A terminal naming itself outranks the entry the innermost layer of the
            // session decided to advertise, which is the ruling `WT_SESSION` already
            // stands on one rung down.
            "TERM_PROGRAM outranks a TERM that only claims 256",
            false,
            &[("TERM_PROGRAM", "WezTerm"), ("TERM", "xterm-256color")],
            Depth::Truecolor,
        ),
        (
            // The one entry in the program table that is not truecolour, and the reason
            // the table returns a rung rather than a bool.
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
            // None of these contains `256color`, so every one of them fell to sixteen
            // whenever `COLORTERM` had not survived, which is the whole defect.
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
            // Above the `TERM` rung, and the row below is why.
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
            // Not 256, which this was and which is a different wrong answer rather than
            // a safe one: the xterm cube's darkest axis levels are 0 and 95 with
            // nothing between, so a subtle wash quantises to a saturated primary.
            "Windows draws 24-bit, and has since 1703",
            true,
            &[],
            Depth::Truecolor,
        ),
        (
            // `TERM` only ever promotes.
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

    // Set but empty is the same as unset, which is not obvious and is reachable without
    // trying: `$env:X = ''` in PowerShell leaves the variable set and empty, and a
    // child process sees an empty string rather than nothing.
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

    // What the cube would have done, stated rather than implied.
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
    // The two halves of this file, joined.
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
        // 24-bit, not "at least 256", and that bound is the second half of the same
        // report.
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

    // Terminal.app is the case this cannot fix and must not pretend to.
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
    // The named colours are whatever the reader's own scheme says they are, so treating
    // them as fixed RGB and quantising *to* them would hand back a colour that means
    // something else on their terminal.
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
    // The one distinction that may never be lost at any rung that has colour at all.
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
    // weighted rather than plain Euclidean: plain RGB distance treats a blue error as
    // costing what a green one does, and sends a mid-green to cyan.
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
    // This reached a screen.
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
    // The cube corners cannot test this, and a mutation proved it: replacing the axis
    // picker's `abs_diff` with a `saturating_sub` still maps every corner to itself,
    // because at a corner the distance is zero either way.
    let index = |r: u8, g: u8, b: u8| match Depth::Ansi256
        .resolve(Style::new().fg(Color::Rgb(r, g, b)))
        .fg
    {
        Some(Color::Indexed(i)) => i,
        other => panic!("expected an index, got {other:?}"),
    };

    // Saturated on purpose.
    assert_eq!(index(40, 255, 0), 16 + 30, "40 is nearer 0 than 95");
    assert_eq!(index(60, 255, 0), 16 + 36 + 30, "60 is nearer 95 than 0");
    assert_eq!(index(255, 40, 0), 16 + 180, "the same, one axis over");
    assert_eq!(index(255, 0, 40), 16 + 180, "and again");

    // 47 is just below the midpoint of the widest gap and resolves downwards.
    assert_eq!(index(47, 255, 0), 16 + 30, "the midpoint of 0 and 95");
}

#[test]
fn the_mockups_own_hues_keep_their_hue_at_sixteen() {
    // Every colour `assets/preview.svg` draws, through the rung that has the least to
    // work with.
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
        // The greys, which have no hue to keep and are ranked by luma instead.
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

#[test]
fn every_self_naming_term_answers() {
    // The sibling trap, one file over.
    const SELF_NAMING: [&str; 7] = [
        "alacritty",
        "contour",
        "foot",
        "rio",
        "wezterm",
        "xterm-ghostty",
        "xterm-kitty",
    ];
    for term in SELF_NAMING {
        assert_eq!(
            Depth::from_env(false, env(&[("TERM", term)])).expect("a rung"),
            Depth::Truecolor,
            "TERM={term} ships its own entry and has only ever drawn 24-bit"
        );
        // A suffixed variant is the same terminal, which is `names`' whole rule.
        let variant = format!("{term}-direct");
        assert_eq!(
            Depth::from_env(false, env(&[("TERM", variant.as_str())])).expect("a rung"),
            Depth::Truecolor,
            "TERM={variant} is a variant of {term}"
        );
    }
}

#[test]
fn every_named_program_answers() {
    // The other half of the same gap: `program_depth`'s arms were reachable only
    // through rows that used them as a *higher* signal something else had to
    // lose to, so deleting `tabby` and five others survived.
    for program in [
        "ghostty",
        "Hyper",
        "iTerm.app",
        "rio",
        "Tabby",
        "vscode",
        "WarpTerminal",
        "WezTerm",
    ] {
        assert_eq!(
            Depth::from_env(false, env(&[("TERM_PROGRAM", program), ("TERM", "screen")]))
                .expect("a rung"),
            Depth::Truecolor,
            "TERM_PROGRAM={program} names a terminal that draws 24-bit"
        );
    }
    assert_eq!(
        Depth::from_env(
            false,
            env(&[("TERM_PROGRAM", "Apple_Terminal"), ("TERM", "screen")])
        )
        .expect("a rung"),
        Depth::Ansi256,
        "Terminal.app rounds 24-bit to its own palette, which is why this table \
         returns a rung"
    );
}

#[test]
fn a_256color_entry_is_matched_anywhere_in_the_name() {
    // `contains` rather than `ends_with`, and swapping them survived.
    for term in [
        "screen-256color",
        "screen-256color-bce",
        "screen-256color-bce-s",
        "xterm-256color-italic",
    ] {
        assert_eq!(
            Depth::from_env(false, env(&[("TERM", term)])).expect("a rung"),
            Depth::Ansi256,
            "TERM={term} names the 256-colour rung"
        );
    }
}

#[test]
fn wt_session_is_read_only_on_windows() {
    // Pinning what happens today rather than asserting it is right.
    assert_eq!(
        Depth::from_env(false, env(&[("WT_SESSION", "abc")])).expect("a rung"),
        Depth::Ansi16,
        "off Windows the session variable is not read, so the floor applies"
    );
    assert_eq!(
        Depth::from_env(true, env(&[("WT_SESSION", "abc")])).expect("a rung"),
        Depth::Truecolor,
        "on Windows it is the terminal naming itself"
    );
    // And the case a reader actually hits: WSL in Windows Terminal, whose TERM
    // is what decides today.
    assert_eq!(
        Depth::from_env(
            false,
            env(&[("WT_SESSION", "abc"), ("TERM", "xterm-256color")])
        )
        .expect("a rung"),
        Depth::Ansi256,
        "WSL in Windows Terminal takes its rung from TERM"
    );
}
