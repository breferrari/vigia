//! The glyph ladder: which drawing characters this terminal's font can be asked
//! for, and how a sparkline cell is packed once it can be asked for more.
//!
//! **Detection is a precedence table**, and a table goes wrong by a rule being
//! right in isolation and in the wrong place. So it is driven one row per rung,
//! each with the rungs *above* it deliberately set to something else, rather
//! than by asserting each variable alone against an empty environment. That is
//! `tests/colour.rs`'s shape and its reasoning is unchanged.
//!
//! **What this ladder has that the colour one does not** is a rung nothing can
//! reach by detection at all. No font measured carries the Unicode 16 octants,
//! so [`Glyphs::Octant`] is reachable only through [`GLYPHS_VAR`], and a sweep
//! of the whole table asserting that is worth more than any single row: an
//! accidental promotion would paint tofu on every terminal it reached.
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
const TABLE: [Row; 12] = [
    Row {
        why: "the override outranks every signal under it",
        windows: true,
        env: &[
            (GLYPHS_VAR, "block"),
            ("TERM_PROGRAM", "ghostty"),
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
        env: &[("TERM", "dumb"), ("TERM_PROGRAM", "ghostty")],
        want: Glyphs::Block,
    },
    Row {
        why: "the linux console has no braille in its bitmap font",
        windows: false,
        env: &[("TERM", "linux"), ("TERM_PROGRAM", "ghostty")],
        want: Glyphs::Block,
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
        why: "anything else takes braille, which is where btop has sat for years",
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
fn detection_never_returns_octants() {
    // **The sweep is the point, not any single row.** No font measured carries
    // U+1CD00, including the newest Cascadia, so a signal that promoted to
    // octants would paint tofu everywhere it reached and no gate over one
    // terminal would see it. Every row of the table, on both platforms, with the
    // override absent.
    for row in &TABLE {
        let stripped: Vec<(&str, &str)> = row
            .env
            .iter()
            .copied()
            .filter(|(key, _)| *key != GLYPHS_VAR)
            .collect();
        for windows in [false, true] {
            let got = Glyphs::from_env(windows, env(&stripped)).expect("a rung");
            assert_ne!(
                got,
                Glyphs::Octant,
                "{} on windows={windows} detected octants",
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
}

#[test]
fn the_octant_column_climbs_one_row_per_level() {
    // **Octants had no geometry gate at all**, and that is the shape this whole
    // ladder is most exposed to: `the_tables_index_the_same_way` below used to
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
    // The corners are the check that they agree: empty and full are the two
    // shapes whose meaning is fixed rather than conventional.
    assert_eq!(Glyphs::Braille.glyph(3, 3), '⣿');
    assert_eq!(Glyphs::Octant.glyph(3, 3), '█');
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
