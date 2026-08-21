//! The covered grammar set is a ruling, and these are its gates.
//!
//! `SPEC.md` §6 (via [#235](https://github.com/breferrari/vigia/issues/235))
//! rules that `vigia` covers every modern language, names the mechanism (the
//! dump `xtask` builds from `two-face`'s `fancy`-vetted set plus the locally
//! vendored extras under `assets/syntaxes/`), and demands four things no
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
//! 4. **The committed dump was built from the committed sources.** The checks
//!    above read the dump and the sources separately, so a grammar edited
//!    without rerunning `xtask` satisfied both while the shipped bytes still
//!    held the old one. `xtask` writes two records beside the sources in the
//!    same run as the dump — `NOTICE.md`'s per-file hashes and
//!    `GRAMMARS.txt`'s sorted name list, both beside the dump so they ship
//!    with it — and these gates recompute both.
//!
//! The dump is read here exactly as the crate reads it — same bytes, same
//! loader — so what these gates pass is what a reader gets.

use std::collections::HashSet;

use syntect::parsing::syntax_definition::{Pattern, SyntaxDefinition};
use syntect::parsing::{Regex, SyntaxSet};

/// Whether this test is running inside the repository rather than inside a
/// published `.crate`.
///
/// **The two gates below read `assets/syntaxes/`, which is outside the
/// package**, and `vigia-core` deliberately publishes its own test suite
/// (see its manifest). So a consumer running `cargo test` on the published
/// crate has the dump and no sources, which is not the same state as a
/// contributor deleting the sources, and must not be asserted against as
/// though it were. The repository is identified by a file only it has;
/// `SPEC.md` sits two levels up from this crate and never ships in the
/// package.
///
/// This is a skip with a provable condition rather than a shrug, and it is
/// the narrow form: inside the repository the gates always run, so nothing a
/// contributor can do makes them quietly stop.
fn in_repository() -> bool {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../SPEC.md")
        .exists()
}

/// The dump the crate embeds, loaded the way `Highlighter::new` loads it.
fn embedded() -> SyntaxSet {
    syntect::dumps::from_uncompressed_data(include_bytes!("../assets/syntaxes.bin"))
        .expect("the embedded dump deserialises")
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
    // The locally vendored tail (assets/syntaxes/), each pinned upstream.
    ("V", "V"),
    ("v.mod", "V Module"),
    ("Gleam", "Gleam"),
    ("PowerShell", "PowerShell"),
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

/// The committed roster: every grammar in the dump, sorted, one per line,
/// written by `xtask` in the same run that writes the dump.
///
/// This is the "may not silently change" half of the pin, and it replaced a
/// count. A count plus [`RULED`]'s named rows protected the 56 grammars
/// somebody had thought to name: an upgrade swapping one of the other 161 for
/// a different grammar left the count identical and every named row present,
/// so the pin's own claim — that a silent drop or gain goes red — was false
/// for three quarters of the set. A committed list is diffable, so the change
/// arrives in review as the grammar it is rather than as a number moving.
/// [`RULED`] stays because it makes a different claim: it maps what a reader
/// calls a format onto the grammar that has to exist for it.
const ROSTER: &str = include_str!("../assets/GRAMMARS.txt");

/// The grammars that come from `assets/syntaxes/` rather than from
/// `two-face`, by the name they carry in the dump.
///
/// It exists so the gates below can act on the **absence** of their sources.
/// Deleting the extras directory without rebuilding used to satisfy every
/// gate by early return: the walk found nothing to check and the committed
/// dump still held them, so a dump shipping four grammars with no sources was
/// invisible.
const VENDORED: [&str; 4] = ["Gleam", "PowerShell", "V", "V Module"];

#[test]
fn every_ruled_format_is_in_the_dump_and_the_roster_is_pinned() {
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

    let mut present: Vec<&str> = set.syntaxes().iter().map(|s| s.name.as_str()).collect();
    present.sort_unstable();
    let pinned: Vec<&str> = ROSTER.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(
        present, pinned,
        "the dump's grammars and the committed roster disagree; run `cargo run \
         -p xtask` and read the diff of assets/syntaxes/GRAMMARS.txt before \
         committing it, because that diff is the change"
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
    if !in_repository() {
        return;
    }
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/syntaxes");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        // No extras directory means the dump should be exactly two-face's
        // set, whose guarantee is upstream's. That is a legal state — but
        // only if the dump agrees, so the absence is asserted rather than
        // returned on.
        assert_no_orphan_vendored_grammars();
        return;
    };

    // Collected once, then every collected path is checked or the loop
    // panics, so nothing can be skipped by construction. The vendored tail is
    // four grammars today; a count pin here would just restate the roster the
    // pin gate already holds.
    let sources: Vec<_> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "sublime-syntax"))
        .collect();

    // An empty directory is the same state as a missing one, and it was the
    // hole the `Err` arm alone left open: `read_dir` returns `Ok` for it, so
    // both gates walked an empty list and passed vacuously while the dump
    // still shipped the grammars those sources had built.
    if sources.is_empty() {
        assert_no_orphan_vendored_grammars();
        return;
    }

    for path in &sources {
        let text = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let stem = path.file_stem().and_then(|s| s.to_str());
        let def = SyntaxDefinition::load_from_str(&text, true, stem)
            .unwrap_or_else(|e| panic!("{} does not parse: {e}", path.display()));

        if let Some(flm) = &def.first_line_match
            && let Some(err) = Regex::try_compile(flm)
        {
            panic!(
                "{} carries a first_line_match the shipped engine cannot                  compile: /{flm}/ -> {err}",
                path.display(),
            );
        }
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
    }
}

/// The other half of the abort-on-first-use guarantee, over the **whole**
/// dump rather than the extras: `find_syntax_by_first_line` compiles each
/// grammar's `first_line_match` behind the same lazy `expect` as any match
/// pattern, `two-face`'s vetting covers match patterns, and a first-line
/// regex is exactly the kind nothing compiles until an extensionless file
/// turns up in a diff.
#[test]
fn every_first_line_regex_in_the_dump_compiles() {
    let set = embedded();
    let mut checked = 0usize;
    for syntax in set.syntaxes() {
        if let Some(flm) = &syntax.first_line_match {
            if let Some(err) = Regex::try_compile(flm) {
                panic!(
                    "{}'s first_line_match cannot compile under the shipped                      engine, which aborts the monitor on the first                      extensionless file: /{flm}/ -> {err}",
                    syntax.name,
                );
            }
            checked += 1;
        }
    }
    assert!(
        checked > 0,
        "no grammar in the dump carries a first_line_match,          so this gate checked nothing and the dump is not the one it expects"
    );
}

/// The committed dump was built from the committed sources.
///
/// The pattern gate checks the *sources* and the pin gate checks the *dump*;
/// neither ties one to the other, so a source edited without rerunning
/// `cargo run -p xtask` passed both while the shipped dump still carried the
/// old grammar. `xtask` writes an FNV-1a 64 of each source into
/// `NOTICE.md`'s Sources table in the same run that writes the dump, and this
/// recomputes them: table and sources must match exactly, both directions.
#[test]
fn the_committed_dump_matches_its_sources() {
    if !in_repository() {
        return;
    }
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/syntaxes");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        assert_no_orphan_vendored_grammars();
        return;
    };
    let paths: Vec<std::path::PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "sublime-syntax"))
        .collect();
    // The empty directory, which is the same state as a missing one and which
    // the `Err` arm alone could not see.
    if paths.is_empty() {
        assert_no_orphan_vendored_grammars();
        return;
    }

    let mut sources: Vec<(String, u64)> = paths
        .iter()
        .map(|p| {
            let bytes = std::fs::read(p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()));
            (
                p.file_name()
                    .expect("a file has a name")
                    .to_string_lossy()
                    .into_owned(),
                fnv1a64(&bytes),
            )
        })
        .collect();
    sources.sort();

    // The notice lives beside the dump rather than beside the sources, because
    // it has to ship: a crates.io consumer and a release archive both get the
    // grammars, so they both have to get the attribution. `include_str!` would
    // read it at compile time, and a plain read is used instead so that this
    // gate fails with the path when the file has moved rather than failing the
    // build for every consumer.
    let notice_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/NOTICE.md");
    let notice = std::fs::read_to_string(&notice_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", notice_path.display()));
    let mut listed: Vec<(String, u64)> = notice
        .lines()
        .filter_map(|line| {
            let rest = line.strip_prefix("- `")?;
            let (name, hash) = rest.split_once("`: ")?;
            Some((name.to_owned(), u64::from_str_radix(hash.trim(), 16).ok()?))
        })
        .collect();
    listed.sort();

    assert_eq!(
        sources, listed,
        "the vendored sources and NOTICE.md's Sources table disagree, so the          committed dump was not built from these sources; run `cargo run -p          xtask` and commit what it writes"
    );

    // The hashes above tie the sources to `NOTICE.md`, which `xtask` writes in
    // the same run as the dump. This ties [`VENDORED`] to the sources, which
    // is the half nothing else could see: the roster is hand-written, so a
    // fifth vendored grammar nobody added to it would simply never be checked
    // by the absence direction.
    let mut parsed: Vec<String> = paths
        .iter()
        .map(|path| {
            let text = std::fs::read_to_string(path)
                .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
            let stem = path.file_stem().and_then(|s| s.to_str());
            SyntaxDefinition::load_from_str(&text, true, stem)
                .unwrap_or_else(|e| panic!("{} does not parse: {e}", path.display()))
                .name
        })
        .collect();
    parsed.sort();
    let mut roster: Vec<String> = VENDORED.iter().map(|name| (*name).to_owned()).collect();
    roster.sort();
    assert_eq!(
        parsed, roster,
        "VENDORED does not name exactly the grammars under assets/syntaxes/, \
         so the orphan check that runs when those sources are gone would be \
         looking for the wrong set"
    );

    // **And every vendored grammar's attribution reaches the notice.** The
    // dump is a copy of somebody else's work, so the upstream it came from and
    // the licence it came under have to travel with it. Both live in the
    // vendored file's header, which is outside the package and reaches nobody
    // who installs the crate; the notice is the copy that ships. A name in a
    // list is not attribution, which is what this file said for one release.
    for path in &paths {
        let text = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let name = path
            .file_name()
            .expect("a file has a name")
            .to_string_lossy();
        for field in ["upstream", "pinned"] {
            let value = text
                .lines()
                .find_map(|line| {
                    line.trim_start()
                        .strip_prefix('#')?
                        .trim_start()
                        .strip_prefix(&format!("{field}:"))
                        .map(str::trim)
                })
                .unwrap_or_else(|| {
                    panic!("{} has no `{field}:` line in its provenance header", name)
                });
            assert!(
                notice.contains(value),
                "{name} is vendored at {field} {value}, and the shipped notice \
                 does not mention it; run `cargo run -p xtask` and commit what \
                 it writes"
            );
        }
    }

    // **And the licence text, per upstream, driven from the grammars rather
    // than from the licence files.**
    //
    // The direction matters and the first spelling of this had it backwards: it
    // walked `licences/` and checked each file reached the notice, which is a
    // check that cannot see a *missing* licence. Delete one and regenerate, and
    // the walk simply had one less thing to look at. Driving it from the
    // vendored grammars is what makes "somebody added a grammar and not its
    // licence" the red case.
    //
    // A licence file is named after its upstream repository, so two grammars
    // from one repository share one file, and the association needs no second
    // list to maintain. `bat` is the exception with a reason: `two-face`
    // reproduces a `LICENSE` per grammar for everything in its curation, so a
    // second copy here would be two texts to drift apart.
    let mut upstreams: Vec<(String, String)> = Vec::new();
    for path in &paths {
        let text = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let upstream = text
            .lines()
            .find_map(|line| {
                line.trim_start()
                    .strip_prefix('#')?
                    .trim_start()
                    .strip_prefix("upstream:")
                    .map(str::trim)
            })
            .expect("a vendored grammar with no upstream in its header");
        let name = path
            .file_name()
            .expect("a file has a name")
            .to_string_lossy()
            .into_owned();
        upstreams.push((upstream.to_owned(), name));
    }

    for (upstream, grammar) in &upstreams {
        let repo = upstream
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .expect("an upstream URL has a last segment");

        // The grammars that came through `bat` are covered by the two-face
        // listing, and the check is that the listing really does carry one for
        // this grammar rather than that we trust it to.
        if upstream.contains("sharkdp/bat") {
            let stem = grammar.trim_end_matches(".sublime-syntax");
            assert!(
                notice.contains(&format!("{stem}/LICENSE")),
                "{grammar} comes through bat, whose per-grammar licences the \
                 two-face listing reproduces, and the shipped notice has no \
                 {stem}/LICENSE in it"
            );
            continue;
        }

        let licence = dir.join("licences").join(format!("{repo}.LICENSE"));
        let text = std::fs::read_to_string(&licence).unwrap_or_else(|e| {
            panic!(
                "{grammar} is vendored from {upstream} and {} is not readable \
                 ({e}), so the dump would ship a copy of somebody's grammar \
                 without their licence",
                licence.display()
            )
        });
        // The copyright line for MIT, the title line for Apache: whichever this
        // licence carries, a substantial line of it has to be in the notice
        // rather than a summary of it.
        let anchor = text
            .lines()
            .find(|line| line.contains("Copyright") || line.contains("Apache License"))
            .map(str::trim)
            .expect("a licence with neither a copyright line nor a title");
        assert!(
            notice.contains(anchor),
            "{grammar}'s licence is committed at {} but its text is not in the \
             shipped notice; run `cargo run -p xtask` and commit what it writes",
            licence.display()
        );
    }

    // And to the **dump**, which is the artefact that actually ships.
    let names: HashSet<String> = embedded()
        .syntaxes()
        .iter()
        .map(|syntax| syntax.name.clone())
        .collect();
    for vendored in VENDORED {
        assert!(
            names.contains(vendored),
            "{vendored} is vendored under assets/syntaxes/ but is not in the              committed dump; run `cargo run -p xtask` and commit what it writes"
        );
    }
}

/// The dump holds none of [`VENDORED`], which is what "there are no extras"
/// has to mean for the early returns above to be honest.
fn assert_no_orphan_vendored_grammars() {
    let names: HashSet<String> = embedded()
        .syntaxes()
        .iter()
        .map(|syntax| syntax.name.clone())
        .collect();
    for vendored in VENDORED {
        assert!(
            !names.contains(vendored),
            "assets/syntaxes/ is gone but the committed dump still ships              {vendored}, so it carries a grammar with no source and no              attribution; run `cargo run -p xtask` and commit what it writes"
        );
    }
}

/// FNV-1a 64, byte-identical to `xtask`'s copy and duplicated for the same
/// reason the pattern check is: the two crates cannot share it without a new
/// workspace member.
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for &b in bytes {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// One ruled format's snippet: a path, optionally a first line, a few lines
/// of representative source, and the least number of distinct non-Plain
/// classes those lines must reach.
struct Snippet {
    format: &'static str,
    path: &'static str,
    first_line: Option<&'static str>,
    lines: &'static [&'static str],
    min_classes: usize,
}

const fn snip(
    format: &'static str,
    path: &'static str,
    lines: &'static [&'static str],
    min_classes: usize,
) -> Snippet {
    Snippet {
        format,
        path,
        first_line: None,
        lines,
        min_classes,
    }
}

/// A representative snippet per ruled format, with the bar set to **what the
/// snippet actually reaches**, measured rather than chosen.
///
/// A floor of "three or so" is the loose bound this repo already has a rule
/// about: a bound no input can approach is decorative, and a Kotlin snippet
/// reaching six classes against a bar of three still passes with half its
/// highlighting gone. Pinned at the measured number, any class a grammar or a
/// scope-table change stops producing turns this red, and the number moves
/// the way the count pin moves: deliberately, in the commit that changed the
/// answer.
const SNIPPETS: &[Snippet] = &[
    snip(
        "TypeScript",
        "src/app.ts",
        &[
            "// greet",
            "const n: number = 42;",
            "export function greet(name: string): string {",
            "  return name;",
            "}",
        ],
        6,
    ),
    snip(
        "TSX",
        "src/App.tsx",
        &[
            "// view",
            "const n = 42;",
            "export const App = () => <div className=\"a\">{n}</div>;",
        ],
        7,
    ),
    snip(
        "JSX",
        "src/App.jsx",
        &[
            "// view",
            "const n = 42;",
            "export const App = () => <div className=\"a\">{n}</div>;",
        ],
        7,
    ),
    snip(
        "Kotlin",
        "Main.kt",
        &[
            "// entry",
            "fun main() {",
            "    val n = 42",
            "    println(\"hello\")",
            "}",
        ],
        6,
    ),
    snip(
        "Swift",
        "Sources/App.swift",
        &[
            "// app",
            "func greet(name: String) -> Int {",
            "    let n = 42",
            "    return n",
            "}",
        ],
        4,
    ),
    snip(
        "Dart",
        "lib/main.dart",
        &[
            "// entry",
            "int main() {",
            "  var n = 42;",
            "  print('hi');",
            "}",
        ],
        6,
    ),
    snip(
        "PowerShell",
        "deploy.ps1",
        &[
            "# deploy",
            "function Get-Thing {",
            "    $count = 42",
            "    Write-Output \"hi\"",
            "}",
        ],
        7,
    ),
    snip(
        "Elixir",
        "lib/app.ex",
        &[
            "# mod",
            "defmodule App do",
            "  def go, do: IO.puts(\"hi\")",
            "end",
        ],
        6,
    ),
    snip(
        "Julia",
        "calc.jl",
        &[
            "# calc",
            "function f(x)",
            "    return x * 42",
            "end",
            "s = \"hi\"",
        ],
        5,
    ),
    snip(
        "Zig",
        "src/main.zig",
        &[
            "// entry",
            "const std = @import(\"std\");",
            "pub fn main() void {",
            "    const n: u32 = 42;",
            "    _ = n;",
            "}",
        ],
        7,
    ),
    snip(
        "Nim",
        "app.nim",
        &[
            "# app",
            "proc greet(name: string): int =",
            "  result = 42",
            "echo \"hi\"",
        ],
        6,
    ),
    snip(
        "Crystal",
        "app.cr",
        &[
            "# app",
            "def greet(name : String)",
            "  42",
            "end",
            "puts \"hi\"",
        ],
        7,
    ),
    snip(
        "F#",
        "App.fs",
        &[
            "// app",
            "let greet name =",
            "    let n = 42",
            "    sprintf \"hi %s\" name",
        ],
        3,
    ),
    snip(
        "Solidity",
        "Token.sol",
        &[
            "// SPDX-License-Identifier: MIT",
            "pragma solidity ^0.8.0;",
            "contract Token {",
            "    uint256 total = 42;",
            "}",
        ],
        5,
    ),
    snip(
        "V",
        "cli/main.v",
        &[
            "// entry",
            "fn main() {",
            "    n := 42",
            "    println('hi')",
            "}",
        ],
        6,
    ),
    snip(
        "Odin",
        "main.odin",
        &[
            "package main",
            "// entry",
            "main :: proc() {",
            "    n := 42",
            "    s := \"hi\"",
            "}",
        ],
        6,
    ),
    snip(
        "Elm",
        "src/Main.elm",
        &[
            "-- main",
            "module Main exposing (main)",
            "answer : Int",
            "answer = 42",
            "greet = \"hi\"",
        ],
        6,
    ),
    snip(
        "Gleam",
        "src/app.gleam",
        &[
            "// app",
            "pub fn main() {",
            "  let n = 42",
            "  io.println(\"hi\")",
            "}",
        ],
        6,
    ),
    snip(
        "SCSS",
        "styles/site.scss",
        &[
            "// vars",
            "$primary: #333;",
            ".card {",
            "  color: $primary;",
            "  margin: 4px;",
            "}",
        ],
        5,
    ),
    snip(
        "Sass",
        "styles/site.sass",
        &["// vars", "$primary: #333", ".card", "  color: $primary"],
        4,
    ),
    snip(
        "Less",
        "styles/site.less",
        &["// vars", "@primary: #333;", ".card { color: @primary; }"],
        4,
    ),
    snip(
        "Vue",
        "src/App.vue",
        &[
            "<template>",
            "  <div class=\"app\">hi</div>",
            "</template>",
            "<script>",
            "export default { n: 42 }",
            "</script>",
        ],
        5,
    ),
    snip(
        "Svelte",
        "src/App.svelte",
        &[
            "<script>",
            "  let n = 42;",
            "</script>",
            "<div class=\"app\">{n}</div>",
        ],
        5,
    ),
    snip(
        "TOML",
        "Cargo.toml",
        &[
            "# manifest",
            "[package]",
            "name = \"vigia\"",
            "version = \"0.19.0\"",
        ],
        3,
    ),
    snip(
        "INI",
        "config.ini",
        &[
            "; config",
            "[server]",
            "host = \"localhost\"",
            "port = 8080",
        ],
        3,
    ),
    snip(
        "Protobuf",
        "api.proto",
        &[
            "// api",
            "syntax = \"proto3\";",
            "message Ping {",
            "  int32 id = 1;",
            "}",
        ],
        6,
    ),
    snip(
        "GraphQL",
        "schema.graphql",
        &["# schema", "type Query {", "  user(id: ID!): String", "}"],
        4,
    ),
    snip(
        "Terraform",
        "infra/main.tf",
        &[
            "# infra",
            "resource \"aws_s3_bucket\" \"b\" {",
            "  bucket = \"name\"",
            "  size   = 42",
            "}",
        ],
        6,
    ),
    snip(
        "Dockerfile",
        "Dockerfile",
        &[
            "# build",
            "FROM rust:1.85",
            "RUN cargo build --release",
            "ENV PORT=8080",
        ],
        3,
    ),
    snip(
        "CMake",
        "CMakeLists.txt",
        &[
            "# build",
            "cmake_minimum_required(VERSION 3.20)",
            "project(vigia)",
            "set(SRC \"main.c\")",
        ],
        4,
    ),
    snip(
        "Nix",
        "default.nix",
        &[
            "# drv",
            "{ pkgs }:",
            "pkgs.stdenv.mkDerivation {",
            "  name = \"vigia-0.1\";",
            "}",
        ],
        4,
    ),
    snip(
        "env",
        ".env",
        &["# secrets", "PORT=8080", "NAME=\"vigia\""],
        5,
    ),
    snip(
        "gitignore",
        ".gitignore",
        &["# artifacts", "target/", "*.log"],
        3,
    ),
    snip(
        "go.mod",
        "go.mod",
        &[
            "// module",
            "module example.com/app",
            "go 1.22",
            "require example.com/dep v1.2.3",
        ],
        4,
    ),
    // The first-line step, through the public path: an extensionless script
    // resolves by its shebang or not at all.
    Snippet {
        format: "shebang script",
        path: "scripts/deploy",
        first_line: Some("#!/usr/bin/env bash"),
        lines: &[
            "#!/usr/bin/env bash",
            "# deploy",
            "NAME=\"vigia\"",
            "echo \"$NAME\"",
        ],
        min_classes: 5,
    },
    // The nearest-grammar approximations (SPEC §6 records each gap): the bar
    // is the base grammar's, because that is the whole of what they buy.
    snip(
        "Astro (as HTML)",
        "src/pages/index.astro",
        &["<!-- page -->", "<div class=\"app\">hi</div>"],
        4,
    ),
    snip(
        "Bicep (as JavaScript)",
        "infra/main.bicep",
        &["// infra", "var total = 42", "param name string = 'x'"],
        6,
    ),
    snip(
        "MDX (as Markdown)",
        "docs/post.mdx",
        &["# A heading", "Some **bold** and `code` here."],
        3,
    ),
    snip(
        "Mojo (as Python)",
        "kernels/matmul.mojo",
        &["# kernel", "def matmul(n):", "    return n * 42"],
        5,
    ),
];

/// Parse every snippet through the crate's own public path — the same
/// resolution, the same scope table, the same parser a frame uses — and
/// assert the spread. A resolution-only test would pass while a language
/// draws plain, which is #235's second half.
#[test]
fn every_ruled_format_reaches_a_spread_of_classes() {
    use vigia_core::{Class, Highlighter, Hunk, Line, LineKind};

    // One highlighter for the whole table: the cache keys on path and every
    // snippet's path is distinct, so sharing it also exercises the cache the
    // way a frame full of files does.
    let mut highlighter = Highlighter::eager();
    let mut failures = Vec::new();
    for snippet in SNIPPETS {
        let hunk = Hunk {
            old_start: 0,
            old_lines: 0,
            new_start: 1,
            new_lines: snippet.lines.len() as u32,
            lines: snippet
                .lines
                .iter()
                .map(|text| Line {
                    kind: LineKind::Added,
                    text: (*text).to_owned(),
                })
                .collect(),
        };

        let mut classes: Vec<Class> = Vec::new();
        {
            let mut pass = highlighter.pass();
            for index in 0..hunk.lines.len() {
                for span in pass.spans(snippet.path, 0, &hunk, index, snippet.first_line) {
                    if span.class != Class::Plain && !classes.contains(&span.class) {
                        classes.push(span.class);
                    }
                }
            }
        }

        if classes.len() < snippet.min_classes {
            failures.push(format!(
                "{} ({}): {} distinct classes {:?}, needs {}",
                snippet.format,
                snippet.path,
                classes.len(),
                classes,
                snippet.min_classes,
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "formats the ruling covers that do not actually colour:\n{}",
        failures.join("\n"),
    );
}
