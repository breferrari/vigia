//! The glyph ladder: which drawing characters this terminal's font can be asked
//! for, and how a sparkline cell is packed once it can be asked for more.
//!
//! **Detection is a precedence table**, and a table goes wrong by a rule being
//! right in isolation and in the wrong place. So it is driven one row per rung,
//! each with the rungs *above* it deliberately set to something else, rather
//! than by asserting each variable alone against an empty environment. That is
//! `tests/colour.rs`'s shape and its reasoning is unchanged.
//!
//! **What this ladder has that the colour one does not** is a rung that is
//! never a font bet. No font measured carries the Unicode 16 octants, and the
//! engines that draw them rasterise the range themselves (#324), so
//! [`Glyphs::Octant`] is reachable only through [`GLYPHS_VAR`] or an engine
//! naming itself *with a qualifying version*, and a sweep of the whole table
//! asserting that is worth more than any single row: an accidental promotion
//! would paint tofu on every terminal it reached.
//!
//! **The packing half is arithmetic**, and the property that matters most is not
//! "a glyph came back". A packer that returned the full cell for everything
//! would pass that and destroy the element. What has to survive is the
//! *distinction* the sparkline exists to make: one write and no writes are
//! different **heights**, at every rung, which is
//! [#78](https://github.com/breferrari/vigia/issues/78) and `SPARK_TRACK`'s own
//! rule reaching a second geometry.

use vigia::{GLYPHS_VAR, Glyphs};

/// An environment built from pairs, so a case reads as the thing it is testing.
///
/// Takes a slice rather than touching the process, because `cargo test` runs
/// these on threads of one process and `set_var` is both racy and, since Rust
/// 2024, `unsafe`. `Glyphs::from_env` exists in this shape for that reason.
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
struct Row {
    why: &'static str,
    windows: bool,
    env: &'static [(&'static str, &'static str)],
    want: Glyphs,
}

/// The precedence table, richest signal first.
///
/// Every row below the first sets the signals **above** it to something that
/// would answer differently, so a rung that stopped being consulted in the right
/// order fails here rather than in a screenshot.
const TABLE: [Row; 19] = [
    Row {
        why: "the override outranks every signal under it",
        windows: true,
        env: &[
            (GLYPHS_VAR, "block"),
            ("TERM_PROGRAM", "ghostty"),
            ("TERM_PROGRAM_VERSION", "1.3.1"),
            ("WT_SESSION", "1"),
            ("TERM", "xterm-kitty"),
        ],
        want: Glyphs::Block,
    },
    Row {
        why: "the override can ask for a rung detection never returns",
        windows: false,
        env: &[(GLYPHS_VAR, "octant"), ("TERM", "linux")],
        want: Glyphs::Octant,
    },
    Row {
        why: "auto falls through rather than naming a rung",
        windows: false,
        env: &[(GLYPHS_VAR, "auto"), ("TERM", "linux")],
        want: Glyphs::Block,
    },
    Row {
        why: "a set-but-empty override is the same as unset",
        windows: false,
        env: &[(GLYPHS_VAR, "   "), ("TERM", "linux")],
        want: Glyphs::Block,
    },
    Row {
        why: "a terminal saying it cannot draw takes the floor",
        windows: false,
        env: &[
            ("TERM", "dumb"),
            ("TERM_PROGRAM", "ghostty"),
            ("TERM_PROGRAM_VERSION", "1.3.1"),
        ],
        want: Glyphs::Block,
    },
    Row {
        why: "the linux console has no braille in its bitmap font",
        windows: false,
        env: &[
            ("TERM", "linux"),
            ("TERM_PROGRAM", "ghostty"),
            ("TERM_PROGRAM_VERSION", "1.3.1"),
        ],
        want: Glyphs::Block,
    },
    Row {
        why: "ghostty at 1.2+ draws octants itself, and says so with a version",
        windows: false,
        env: &[
            ("TERM_PROGRAM", "ghostty"),
            ("TERM_PROGRAM_VERSION", "1.3.1-arch2"),
            ("TERM", "xterm-ghostty"),
        ],
        want: Glyphs::Octant,
    },
    Row {
        why: "a ghostty before 1.2 takes octants from a font that lacks them",
        windows: false,
        env: &[
            ("TERM_PROGRAM", "ghostty"),
            ("TERM_PROGRAM_VERSION", "1.1.3"),
            ("TERM", "xterm-ghostty"),
        ],
        want: Glyphs::Braille,
    },
    Row {
        why: "kitty at 0.40+ draws octants itself",
        windows: false,
        env: &[("TERM", "xterm-kitty"), ("TERM_PROGRAM_VERSION", "0.42.1")],
        want: Glyphs::Octant,
    },
    Row {
        why: "a kitty with no version to show stays braille, which fails safe",
        windows: false,
        env: &[("TERM", "xterm-kitty")],
        want: Glyphs::Braille,
    },
    Row {
        why: "VTE says its version in its own convention, 7802 for 0.78.2",
        windows: false,
        env: &[("VTE_VERSION", "7802"), ("TERM", "xterm-256color")],
        want: Glyphs::Octant,
    },
    Row {
        why: "a VTE before 0.78 stays braille",
        windows: false,
        env: &[("VTE_VERSION", "7403"), ("TERM", "xterm-256color")],
        want: Glyphs::Braille,
    },
    Row {
        why: "a multiplexer hides the terminal, so the octant rung is never chosen behind one",
        windows: false,
        env: &[
            ("TERM", "tmux-256color"),
            ("TERM_PROGRAM", "ghostty"),
            ("TERM_PROGRAM_VERSION", "1.3.1"),
        ],
        want: Glyphs::Braille,
    },
    Row {
        why: "a named program outranks the TERM under it",
        windows: false,
        env: &[("TERM_PROGRAM", "iTerm.app"), ("TERM", "screen")],
        want: Glyphs::Braille,
    },
    Row {
        why: "Apple_Terminal is braille here where it is not truecolour there",
        windows: false,
        env: &[("TERM_PROGRAM", "Apple_Terminal"), ("TERM", "screen")],
        want: Glyphs::Braille,
    },
    Row {
        why: "Windows Terminal says so with WT_SESSION, and ships Cascadia",
        windows: true,
        env: &[("WT_SESSION", "abc"), ("TERM", "screen")],
        want: Glyphs::Braille,
    },
    Row {
        why: "a terminal that ships its own entry is the whole signal",
        windows: false,
        env: &[("TERM", "foot-extra")],
        want: Glyphs::Braille,
    },
    Row {
        why: "the Windows console draws with Consolas, which has no braille",
        windows: true,
        env: &[("TERM", "xterm-256color")],
        want: Glyphs::Block,
    },
    Row {
        why: "anything else takes braille, which is where this class has sat for years",
        windows: false,
        env: &[("TERM", "screen-256color")],
        want: Glyphs::Braille,
    },
];

#[test]
fn the_precedence_table_answers_row_by_row() {
    for row in &TABLE {
        let got = Glyphs::from_env(row.windows, env(row.env))
            .unwrap_or_else(|error| panic!("{}: {error}", row.why));
        assert_eq!(got, row.want, "{}: env {:?}", row.why, row.env);
    }
}

#[test]
fn an_override_that_is_not_a_rung_is_refused() {
    // Refused rather than ignored, which is `VIGIA_COLOR`'s rule: a reader who
    // set a variable and got no effect cannot guess that it was discarded.
    let error = Glyphs::from_env(false, env(&[(GLYPHS_VAR, "sixel")]))
        .expect_err("an unknown rung is refused");
    assert_eq!(error.value, "sixel");
    let said = error.to_string();
    // The message names every value that would have worked, because a refusal
    // that does not is a refusal the reader has to go and look something up for.
    for spelling in ["auto", "block", "braille", "octant"] {
        assert!(
            said.contains(spelling),
            "the refusal does not name {spelling}: {said}"
        );
    }
    assert!(said.contains(GLYPHS_VAR), "the refusal names the variable");
}

#[test]
fn octants_come_only_from_a_version_qualified_engine() {
    // **The sweep is the point, not any single row**, and since
    // [#324](https://github.com/breferrari/vigia/issues/324) its claim is
    // narrower than the absolute it replaced: octants are never a *font* bet,
    // so with the override and every engine-version signal stripped, no row of
    // the table may promote. What remains reachable is exactly the engines
    // that rasterise the range themselves, named with a qualifying version,
    // which the positive rows above pin one by one. The old sweep said
    // "detection never returns octants"; that sentence was a fact about fonts
    // quoted as a fact about rendering, and SPEC.md section 10 carries the
    // correction.
    for row in &TABLE {
        let stripped: Vec<(&str, &str)> = row
            .env
            .iter()
            .copied()
            .filter(|(key, _)| {
                *key != GLYPHS_VAR && *key != "TERM_PROGRAM_VERSION" && *key != "VTE_VERSION"
            })
            .collect();
        for windows in [false, true] {
            let got = Glyphs::from_env(windows, env(&stripped)).expect("a rung");
            assert_ne!(
                got,
                Glyphs::Octant,
                "{} on windows={windows} detected octants with no version to stand on",
                row.why
            );
        }
    }
}

#[test]
fn the_rungs_are_two_densities() {
    assert_eq!(Glyphs::Block.density(), 1);
    assert_eq!(Glyphs::Braille.density(), 2);
    // Braille and octants are the same 2x4 grid, so they must cost the same
    // columns. A row that reserved for one and drew the other would slide.
    assert_eq!(Glyphs::Octant.density(), Glyphs::Braille.density());
    assert_eq!(Glyphs::Octant.levels(), Glyphs::Braille.levels());

    // **Three above the baseline, not four, and the number is the whole price of
    // the track rule.** A 2x4 cell has four dot rows and the bottom one is spent
    // so that "one write" and "no writes" differ in *height*. Asserting only
    // that the two dense rungs agree would let both drift to four together,
    // which is the one change that would silently put the track back on the
    // ramp's floor and leave colour alone carrying the distinction.
    assert_eq!(Glyphs::Braille.levels(), 3);
    assert_eq!(Glyphs::Block.levels(), 8);
}

#[test]
fn the_block_rung_answers_from_the_eighth_blocks() {
    // **Total at the block rung too**, which is what lets the drawer have no
    // branch in it. Level zero is the track and every level above indexes the
    // ramp, so one function spells both rungs and `right` is simply unread where
    // a cell holds a single bucket.
    assert_eq!(Glyphs::Block.glyph(0, 0), '_');
    assert_eq!(Glyphs::Block.glyph(1, 0), '▁');
    assert_eq!(Glyphs::Block.glyph(8, 0), '█');
    // `right` is ignored rather than packed, since a block cell holds one.
    assert_eq!(Glyphs::Block.glyph(4, 3), Glyphs::Block.glyph(4, 0));
}

#[test]
fn an_empty_cell_draws_the_baseline_and_not_a_gap() {
    // #78 at this rung: a gap would make the strip's own length ambiguous, so
    // the bottom dot row is lit even where nothing happened.
    assert_eq!(Glyphs::Braille.glyph(0, 0), '⣀');
    assert_eq!(Glyphs::Octant.glyph(0, 0), '▂');
}

#[test]
fn one_write_and_no_writes_are_different_heights() {
    // **The distinction the element exists to make**, and the reason the
    // baseline costs a level. If these two were equal the rung would be spending
    // colour alone on it, which `SPARK_TRACK`'s docblock refuses.
    for glyphs in [Glyphs::Braille, Glyphs::Octant] {
        let empty = glyphs.glyph(0, 0);
        let one = glyphs.glyph(1, 0);
        assert_ne!(empty, one, "{glyphs:?} draws one write as empty");
    }
}

#[test]
fn a_dense_column_climbs_one_row_per_level() {
    // One column climbing while the other stays empty. **Both baseline dots are
    // lit in every one of these**, which is the track rule rather than an
    // artifact: the empty neighbour still draws its own baseline, so a cell
    // never has a blank half.
    let left: Vec<char> = (0..=3)
        .map(|level| Glyphs::Braille.glyph(level, 0))
        .collect();
    assert_eq!(left, vec!['⣀', '⣄', '⣆', '⣇']);

    // And the right column is the mirror, which is what makes one cell carry two
    // buckets that can be told apart.
    let right: Vec<char> = (0..=3)
        .map(|level| Glyphs::Braille.glyph(0, level))
        .collect();
    assert_eq!(right, vec!['⣀', '⣠', '⣰', '⣸']);
}

#[test]
fn a_full_cell_is_every_dot() {
    assert_eq!(Glyphs::Braille.glyph(3, 3), '⣿');
    assert_eq!(Glyphs::Octant.glyph(3, 3), '█');
}

#[test]
fn a_level_past_the_ramp_is_clamped_rather_than_wrapping() {
    // The caller scales against a peak and a peak is data, so this is reachable
    // by arithmetic rather than only by a programming error. Wrapping the shift
    // would index the table at a bit that means something else entirely.
    for glyphs in [Glyphs::Braille, Glyphs::Octant] {
        assert_eq!(glyphs.glyph(99, 99), glyphs.glyph(3, 3), "{glyphs:?}");
    }
    // **The block rung was missing from this loop, and it is the one that
    // panics.** The dense rungs index a 256-entry table through a `u8` mask, so
    // an unclamped level is merely a wrong glyph; `SPARK_RAMP` has eight entries
    // and `glyph` is public, so `Block.glyph(9, 0)` without the clamp is an
    // out-of-bounds index on a released API.
    assert_eq!(Glyphs::Block.glyph(99, 0), Glyphs::Block.glyph(8, 0));
    assert_eq!(Glyphs::Block.glyph(9, 0), '█');
}

#[test]
fn the_octant_column_climbs_one_row_per_level() {
    // **Octants had no geometry gate at all**, and that is the shape this whole
    // ladder is most exposed to: `the_two_tables_agree_about_geometry` below used to
    // assert only that the two rungs draw *different* characters, which any
    // indexing satisfies. Mirroring the octant cell's halves passed the entire
    // suite, so the strip could read backwards in time at that rung and nothing
    // would say so.
    let left: Vec<char> = (0..=3)
        .map(|level| Glyphs::Octant.glyph(level, 0))
        .collect();
    assert_eq!(
        left,
        vec!['\u{2582}', '\u{1cdbb}', '\u{1cdbf}', '\u{1cdc0}']
    );

    let right: Vec<char> = (0..=3)
        .map(|level| Glyphs::Octant.glyph(0, level))
        .collect();
    assert_eq!(
        right,
        vec!['\u{2582}', '\u{1cdcb}', '\u{1cdd3}', '\u{1cdd5}']
    );
}

#[test]
fn the_two_tables_agree_about_geometry() {
    // One geometry drives both, so every level pair must land on the same
    // *shape* in each table even though the glyphs differ. If the two were
    // indexed differently, a reader switching rungs would see the bars flip.
    // **This asserted nothing for two rounds and the second one caught it.** It
    // began as `assert_ne!(braille, octant)`, which is true by construction
    // because the two tables are disjoint Unicode blocks, and the mirror checks
    // that replaced it duplicate the two climb gates above. What is actually
    // worth holding is **injectivity**: sixteen level pairs must land on sixteen
    // distinct characters at each rung, or two different histories draw the same
    // cell and the strip is ambiguous in a way no colour can recover.
    for glyphs in [Glyphs::Braille, Glyphs::Octant] {
        let mut seen = std::collections::BTreeMap::new();
        for left in 0..=glyphs.levels() {
            for right in 0..=glyphs.levels() {
                let glyph = glyphs.glyph(left, right);
                if let Some(other) = seen.insert(glyph, (left, right)) {
                    panic!(
                        "{glyphs:?}: {other:?} and {:?} both draw {glyph:?}",
                        (left, right)
                    );
                }
            }
        }
        assert_eq!(
            seen.len(),
            (glyphs.levels() + 1) * (glyphs.levels() + 1),
            "{glyphs:?} does not spell every level pair distinctly"
        );
    }
}

#[test]
fn a_refusal_quotes_what_was_typed_rather_than_what_it_was_folded_to() {
    // **The raw half of the override, which the shared reader made possible to
    // lose.** `override_of` hands back a normalised spelling to match on and the
    // raw one to quote, and every other gate here passes a value that is already
    // normalised, so a helper that returned only the folded string would satisfy
    // all of them. A reader who typed a capital and a stray space needs to see
    // that, because the whitespace is usually the mistake.
    let error = Glyphs::from_env(false, env(&[(GLYPHS_VAR, "  Sixel  ")]))
        .expect_err("an unknown rung is refused");
    assert_eq!(error.value, "  Sixel  ");
    assert!(
        error.to_string().contains("  Sixel  "),
        "the refusal folded the value before quoting it: {error}"
    );

    // And the match itself is case- and whitespace-insensitive, which is the
    // other half of the same split and would otherwise be untested.
    let rung = Glyphs::from_env(false, env(&[(GLYPHS_VAR, "  BRAILLE  ")])).expect("a rung");
    assert_eq!(rung, Glyphs::Braille);
}

#[test]
fn a_term_merely_containing_a_terminals_name_is_not_it() {
    // Swapping `names()` for a bare `contains` survived the suite. The boundary
    // is the `-` terminfo itself uses, so a variant is the same terminal and a
    // word that merely has the name inside it is not.
    assert_eq!(
        Glyphs::from_env(false, env(&[("TERM", "foot-extra")])).expect("a rung"),
        Glyphs::Braille,
        "a suffixed variant is the same terminal"
    );
    assert_eq!(
        Glyphs::from_env(true, env(&[("TERM", "notfoot")])).expect("a rung"),
        Glyphs::Block,
        "a TERM that merely contains the name is a different terminal"
    );
}

#[test]
fn the_linux_console_is_matched_by_name_and_not_by_prefix() {
    // `starts_with("linux")` survived. `TERM=linuxconsole` is not the VT, and
    // taking the floor for it would cost a reader the rung their font supports.
    assert_eq!(
        Glyphs::from_env(false, env(&[("TERM", "linux")])).expect("a rung"),
        Glyphs::Block
    );
    assert_eq!(
        Glyphs::from_env(false, env(&[("TERM", "linuxconsole")])).expect("a rung"),
        Glyphs::Braille,
        "a TERM merely starting with linux took the console's floor"
    );
    // Same shape one rung up: `dumbterm` is not `dumb`.
    assert_eq!(
        Glyphs::from_env(false, env(&[("TERM", "dumbterm")])).expect("a rung"),
        Glyphs::Braille
    );
}

#[test]
fn a_term_in_capitals_is_the_same_terminal() {
    // Dropping the fold on `TERM` survived. `TERM` is conventionally lower case
    // and this is the forgiving reading, which matters because the cost of
    // getting it wrong is a reader on a VT drawing tofu.
    for spelling in ["LINUX", "Linux", "linux"] {
        assert_eq!(
            Glyphs::from_env(false, env(&[("TERM", spelling)])).expect("a rung"),
            Glyphs::Block,
            "TERM={spelling} is the linux console"
        );
    }
    assert_eq!(
        Glyphs::from_env(false, env(&[("TERM", "XTERM-KITTY")])).expect("a rung"),
        Glyphs::Braille
    );
}

#[test]
fn a_named_program_on_windows_outranks_the_console_floor() {
    // **No row of `TABLE` reaches `TERM_PROGRAM` on Windows**, because the only
    // `windows: true` row above it short-circuits on the override. That left the
    // interesting Windows case ungated: `vscode` rasterises braille itself
    // rather than taking it from Consolas, so it is braille on the platform
    // whose fallthrough is blocks.
    assert_eq!(
        Glyphs::from_env(
            true,
            env(&[("TERM_PROGRAM", "vscode"), ("TERM", "xterm-256color")])
        )
        .expect("a rung"),
        Glyphs::Braille,
        "a terminal that names itself outranks the platform floor under it"
    );
    // And the floor still applies when nothing names itself.
    assert_eq!(
        Glyphs::from_env(true, env(&[("TERM", "xterm-256color")])).expect("a rung"),
        Glyphs::Block
    );
}

#[test]
fn every_named_program_answers() {
    // **Most of `program_glyphs` was unexercised**, and deleting an entry
    // survived: `TABLE` uses `ghostty` only as the *higher* signal a lower rung
    // must lose to, so it never supplies the answer. A table of terminals
    // somebody checked is worth nothing if the entries are never read, and this
    // is the list a wrong addition would sit in unnoticed.
    for program in [
        "Apple_Terminal",
        "ghostty",
        "Hyper",
        "iTerm.app",
        "rio",
        "Tabby",
        "vscode",
        "WarpTerminal",
        "WezTerm",
    ] {
        // **On Windows, and that is what makes each entry load-bearing.** Off
        // Windows the fallthrough is braille anyway, so deleting a name from the
        // table changes no answer and the gate passes against a table that has
        // lost it. Here the platform floor under the program is blocks, so only
        // the table can supply this answer.
        let rung = Glyphs::from_env(
            true,
            env(&[("TERM_PROGRAM", program), ("TERM", "xterm-256color")]),
        )
        .expect("a rung");
        assert_eq!(
            rung,
            Glyphs::Braille,
            "TERM_PROGRAM={program} should name a terminal whose font has braille"
        );
    }

    // And a program nobody has checked falls through rather than capping
    // anything, which is the table's stated discipline.
    let rung = Glyphs::from_env(
        false,
        env(&[("TERM_PROGRAM", "someterm"), ("TERM", "linux")]),
    )
    .expect("a rung");
    assert_eq!(
        rung,
        Glyphs::Block,
        "an unknown program should fall through to the signals under it"
    );
}

#[test]
fn a_program_name_with_room_around_it_is_still_that_program() {
    // Dropping the trim on `TERM_PROGRAM` survived. A value with surrounding
    // space is what an environment file or a shell export leaves behind, and the
    // cost of not trimming is a reader on a terminal that names itself getting
    // the platform floor instead.
    let rung = Glyphs::from_env(
        true,
        env(&[("TERM_PROGRAM", "  WezTerm  "), ("TERM", "xterm-256color")]),
    )
    .expect("a rung");
    assert_eq!(rung, Glyphs::Braille);
}

#[test]
fn every_self_naming_term_answers() {
    // **The sibling trap `program_glyphs` had, and fixing one did not fix the
    // other.** Off Windows the fallthrough is braille anyway, so breaking any
    // `BRAILLE_TERMS` entry changes no answer there and the table could rot
    // entry by entry. Driven on Windows, where the floor under it is blocks, so
    // only the table can supply this answer.
    for term in [
        "alacritty",
        "contour",
        "foot",
        "rio",
        "wezterm",
        "xterm-ghostty",
        "xterm-kitty",
    ] {
        let rung = Glyphs::from_env(true, env(&[("TERM", term)])).expect("a rung");
        assert_eq!(rung, Glyphs::Braille, "TERM={term} ships its own entry");
        // And a suffixed variant is the same terminal, which is `names`' rule.
        let variant = format!("{term}-direct");
        let rung = Glyphs::from_env(true, env(&[("TERM", variant.as_str())])).expect("a rung");
        assert_eq!(
            rung,
            Glyphs::Braille,
            "TERM={variant} is a variant of {term}"
        );
    }
}

#[test]
fn the_band_follows_the_rung_the_pane_detects() {
    // **[#244](https://github.com/breferrari/vigia/issues/244), reopened.** That
    // row took the churn band off the glyph ladder and pinned it to blocks, on a
    // live report that turned out to have been misquoted: "scattered dots"
    // describes glyph texture and points at a rung, where the reader's actual
    // words describe waves becoming spikes, which is the signal's shape and
    // points at the denominator. That denominator is
    // [#256](https://github.com/breferrari/vigia/issues/256).
    //
    // **The band draws at whatever the pane detects, and that is a product
    // ruling rather than a measured one.** Blocks are more faithful, measured:
    // mean absolute error between the drawn column and the true level, over five
    // series at ten widths from 36 to 124, is 1.2% to 4.0% for blocks against
    // 4.3% to 8.8% for a dense cell. The cost is real, it is known, and the
    // element is a reader-facing option that was removed without being asked
    // for. The ladder is one ladder again.
    //
    // Pinned as a property rather than by naming a rung: nothing in the band may
    // read a glyph set the pane did not detect.
    for pane in [Glyphs::Block, Glyphs::Braille, Glyphs::Octant] {
        assert_eq!(
            pane.density() * pane.levels(),
            match pane {
                Glyphs::Block => 8,
                Glyphs::Braille | Glyphs::Octant => 6,
            },
            "{pane:?} changed what a cell carries, so the band's own gates in \
             masthead.rs need re-deriving rather than this one relaxing"
        );
    }
}
