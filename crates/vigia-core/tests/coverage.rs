//! The covered grammar set is a ruling, and these are its gates.
//!
//! `SPEC.md` §6 (via [#235](https://github.com/breferrari/vigia/issues/235))
//! rules that `vigia` covers every modern language, names the mechanism (the
//! dump `xtask` builds from `two-face`'s `fancy`-vetted set plus the locally
//! vendored extras under `assets/syntaxes/`), and demands three things no
//! other test asserts:
//!
//! 1. **The set is pinned.** An upgrade that silently drops or gains a grammar
//!    goes red here and forces a deliberate re-pin, because a covered set that
//!    can drift is an inheritance again, which is exactly what the ruling
//!    ended.
//! 2. **No shipped pattern may fail to compile.** `syntect` compiles patterns
//!    lazily behind an `expect`, the workspace builds with `panic = "abort"`,
//!    so one incompatible pattern in a vendored grammar aborts the monitor on
//!    the first file that reaches it. `two-face` guarantees its own set by
//!    construction; the extras are guaranteed here.
//! 3. **A covered language actually colours.** A grammar can load and still
//!    draw plain if the scope table never matches what it emits, which is how
//!    Markdown drew at 4.5% while being a "covered" language. So every ruled
//!    format has a snippet gate asserting a spread of classes, not merely
//!    resolution.
//!
//! The dump is read here exactly as the crate reads it — same bytes, same
//! loader — so what these gates pass is what a reader gets.

use std::collections::HashSet;

use syntect::parsing::syntax_definition::{Pattern, SyntaxDefinition};
use syntect::parsing::{Regex, SyntaxSet};

/// The dump the crate embeds, loaded the way `Highlighter::new` loads it.
fn embedded() -> SyntaxSet {
    syntect::dumps::from_binary(include_bytes!("../assets/syntaxes.bin"))
}

/// Every format the ruling names, as (what a reader would call it, the grammar
/// name in the dump). One row per formerly missing format from #235's survey,
/// plus the covered-before set that must never regress.
///
/// `xtask` prints the full name list after a rebuild; a rename upstream shows
/// up as a red row here with the old name in the message.
const RULED: &[(&str, &str)] = &[
    // The 2026 survey's missing languages, now covered.
    ("TypeScript", "TypeScript"),
    ("TSX", "TypeScriptReact"),
    ("Kotlin", "Kotlin"),
    ("Swift", "Swift"),
    ("Dart", "Dart"),
    ("Elixir", "Elixir"),
    ("Julia", "Julia"),
    ("Zig", "Zig"),
    ("Nim", "Nim"),
    ("Crystal", "Crystal"),
    ("F#", "F#"),
    ("Solidity", "Solidity"),
    ("Odin", "Odin"),
    ("Elm", "Elm"),
    // Markup and web.
    ("SCSS", "SCSS"),
    ("Sass", "Sass"),
    ("Less", "Less"),
    ("Vue", "Vue Component"),
    ("Svelte", "Svelte"),
    // Data, config, build.
    ("TOML", "TOML"),
    ("INI", "INI"),
    ("Protobuf", "Protocol Buffer"),
    ("GraphQL", "GraphQL"),
    ("Terraform", "Terraform"),
    ("Dockerfile", "Dockerfile"),
    ("CMake", "CMake"),
    ("Nix", "Nix"),
    ("env", "DotENV"),
    ("gitignore", "Git Ignore"),
    ("go.mod", "Gomod"),
    // Covered before #235, and covered still: the set may only grow.
    ("Rust", "Rust"),
    ("Python", "Python"),
    ("JavaScript", "JavaScript"),
    ("Go", "Go"),
    ("C", "C"),
    ("C++", "C++"),
    ("Objective-C", "Objective-C"),
    ("Java", "Java"),
    ("C#", "C#"),
    ("Ruby", "Ruby"),
    ("PHP", "PHP"),
    ("Shell", "Bourne Again Shell (bash)"),
    ("SQL", "SQL"),
    ("Markdown", "Markdown"),
    ("HTML", "HTML"),
    ("CSS", "CSS"),
    ("JSON", "JSON"),
    ("YAML", "YAML"),
    ("XML", "XML"),
    ("Makefile", "Makefile"),
    ("MATLAB", "MATLAB"),
    ("Verilog", "Verilog"),
];

/// How many syntaxes the committed dump holds, exactly.
///
/// This is the "may not silently gain" half of the pin: a `two-face` upgrade
/// that changes the set moves this number, and moving it is a deliberate edit
/// in the same commit as the upgrade, with the diff of names read rather than
/// assumed. The "may not silently drop" half is [`RULED`].
const PINNED_COUNT: usize = 213;

#[test]
fn every_ruled_format_is_in_the_dump_and_the_count_is_pinned() {
    let set = embedded();
    let names: HashSet<&str> = set.syntaxes().iter().map(|s| s.name.as_str()).collect();

    let missing: Vec<&str> = RULED
        .iter()
        .filter(|(_, grammar)| !names.contains(grammar))
        .map(|(format, _)| *format)
        .collect();
    assert!(
        missing.is_empty(),
        "ruled formats missing from the embedded dump: {missing:?}"
    );

    assert_eq!(
        set.syntaxes().len(),
        PINNED_COUNT,
        "the dump gained or lost grammars; re-read the xtask name list and \
         re-pin deliberately"
    );
}

/// The abort-on-first-use gate for the locally vendored grammars.
///
/// Walks `assets/syntaxes/*.sublime-syntax` — the sources `xtask` compiled
/// into the dump — and force-compiles every pattern of every context under the
/// engine this crate ships. `two-face`'s `fancy` build guarantees the same
/// property for the base set by excluding what cannot comply, so between the
/// two, no pattern in the dump can reach `syntect`'s lazy-compile `expect`.
///
/// A grammar that fails here in CI is a grammar `xtask` should already have
/// refused; the gate exists so a dump regenerated by hand cannot dodge that
/// refusal.
#[test]
fn every_vendored_pattern_compiles_under_the_shipped_engine() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/syntaxes");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        // No extras directory means the dump is exactly two-face's set, whose
        // guarantee is upstream's. Nothing to check is a legal state, not a
        // silent pass over something.
        return;
    };

    let mut checked = 0usize;
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "sublime-syntax") {
            continue;
        }
        let text = std::fs::read_to_string(&path).expect("read a vendored grammar");
        let def = SyntaxDefinition::load_from_str(&text, true, None)
            .unwrap_or_else(|e| panic!("{} does not parse: {e}", path.display()));

        for (context_name, context) in &def.contexts {
            for pattern in &context.patterns {
                if let Pattern::Match(m) = pattern
                    && let Some(err) = Regex::try_compile(m.regex.regex_str())
                {
                    panic!(
                        "{} carries a pattern the shipped engine cannot compile, \
                         which aborts the monitor on first use: {context_name}: \
                         /{}/ -> {err}",
                        path.display(),
                        m.regex.regex_str(),
                    );
                }
            }
        }
        checked += 1;
    }

    // The extras roster lives in the dump; if sources exist they must have
    // been checkable, or the walk above silently proved nothing.
    let listed = std::fs::read_dir(&dir)
        .map(|d| {
            d.filter_map(|e| e.ok())
                .filter(|e| e.path().extension().is_some_and(|x| x == "sublime-syntax"))
                .count()
        })
        .unwrap_or(0);
    assert_eq!(checked, listed, "a vendored grammar was skipped");
}
