//! Compiling grammars ahead of the reader, and what that is and is not worth.
//!
//! `syntect` defers a grammar's patterns to `fancy_regex` on first use, so the
//! first parse under one costs 74-362ms where loading all seventy-five costs
//! 318µs. `Highlighter::warm_ahead` pays that where nothing is waiting on it.
//!
//! **The claim being tested is deliberately weak, and the weakness is the
//! finding.** There is no such thing as a warm grammar: compilation is per
//! *pattern*, so warming on one file leaves a different file of the same
//! language still paying. Measured in release, warming on one document and then
//! parsing another of the same language: `.rs` 41.41ms residual, `.md` 95.04ms,
//! `.html` 201.20ms. So what is asserted here is that warming helps **the
//! content it warmed on**, which is the only thing that is true, and
//! `SPEC.md` §10 records the rest.

mod support;

use support::{Scratch, absolute_gates_apply, exclusively_timed, time};
use vigia_core::{Highlighter, WARM_BYTES, WARM_FILES, WARM_PER_GRAMMAR, WARM_TOTAL};

#[test]
fn a_path_with_no_grammar_is_skipped_before_it_is_read() {
    // The same answer `syntax_for` gives the frame path for a file type nothing
    // recognises, and it has to be reachable without panicking because the
    // warmer walks whatever status named. Skipped **before** the read, which is
    // what makes an unknown file type free rather than merely harmless.
    let scratch = Scratch::large_diff("warm-no-grammar", 1, 4);
    std::fs::write(
        scratch.path_of("data.unknownext"),
        "some bytes
",
    )
    .expect("write");
    let highlighter = Highlighter::new();

    let warmed = highlighter
        .warm_ahead(
            scratch.root().to_path_buf(),
            vec!["data.unknownext".to_owned()],
        )
        .join()
        .expect("the warmer thread");

    assert_eq!(
        warmed, 0,
        "the warmer counted a file `syntect` has no grammar for, so it read one it could not have compiled anything from"
    );
}

#[test]
fn an_empty_changed_set_warms_nothing() {
    let highlighter = Highlighter::new();
    let handle = highlighter.warm_ahead(std::path::PathBuf::from("."), Vec::new());
    assert_eq!(handle.join().expect("the warmer thread"), 0);
}

#[test]
fn many_files_of_one_language_warm_only_a_few() {
    // **The bound that decides what the warmer costs.** Compiling a grammar is
    // one file's work and every later file of the same language buys only a
    // decaying residual, so a changed set of eighty-four Rust files must not be
    // eighty-four parses. Measured before this cap existed: sixty-four files was
    // **1.053s** of held core, about 96% of it re-parsing an already-compiled
    // grammar next to the frame path.
    let files = WARM_FILES + 20;
    let scratch = Scratch::large_diff("warm-one-language", files, 4);
    let highlighter = Highlighter::new();
    let paths: Vec<String> = (0..files).map(|n| format!("src/mod_{n}.rs")).collect();

    let warmed = highlighter
        .warm_ahead(scratch.root().to_path_buf(), paths)
        .join()
        .expect("the warmer thread");

    assert_eq!(
        warmed, WARM_PER_GRAMMAR,
        "the warmer parsed {warmed} of {files} files that all share one grammar,          over the {WARM_PER_GRAMMAR} it is allowed"
    );
}

#[test]
fn the_path_cap_stops_the_walk_before_a_language_it_has_not_reached() {
    // The outer bound, and it needs a fixture that can *see* it: with every path
    // sharing one grammar the per-grammar cap binds first and a missing path cap
    // would be invisible. So the run is `WARM_FILES` Rust files, which contribute
    // exactly `WARM_PER_GRAMMAR` parses, followed by a Markdown file that is
    // reachable only if the walk runs past its cap.
    let scratch = Scratch::large_diff("warm-path-cap", WARM_FILES, 4);
    std::fs::write(
        scratch.path_of("README.md"),
        "# heading

text
",
    )
    .expect("write");
    let highlighter = Highlighter::new();

    let mut paths: Vec<String> = (0..WARM_FILES).map(|n| format!("src/mod_{n}.rs")).collect();
    paths.push("README.md".to_owned());

    let warmed = highlighter
        .warm_ahead(scratch.root().to_path_buf(), paths)
        .join()
        .expect("the warmer thread");

    assert_eq!(
        warmed, WARM_PER_GRAMMAR,
        "the warmer parsed {warmed} files, so it walked past the {WARM_FILES}          paths it is allowed and reached the Markdown file behind them"
    );
}

#[test]
fn a_path_that_is_not_there_is_skipped_rather_than_fatal() {
    // A file can vanish between status naming it and this thread reaching it,
    // which is ordinary beside an agent. The run has to continue: a warmer that
    // gave up on the first missing path would stop warming exactly when the
    // other pane is busiest. A missing path also must not spend the per-grammar
    // budget, since nothing was compiled from it.
    let scratch = Scratch::large_diff("warm-missing", 2, 4);
    let highlighter = Highlighter::new();

    let warmed = highlighter
        .warm_ahead(
            scratch.root().to_path_buf(),
            vec![
                "src/gone.rs".to_owned(),
                "src/mod_0.rs".to_owned(),
                "src/also_gone.rs".to_owned(),
                "src/mod_1.rs".to_owned(),
            ],
        )
        .join()
        .expect("the warmer thread");

    assert_eq!(
        warmed, 2,
        "the warmer counted {warmed} files where two of the four exist, so a missing path is either fatal or is spending the per-grammar budget"
    );
}

#[test]
fn warming_moves_a_grammars_compile_off_the_parse_that_follows_it() {
    // The only claim `warm` makes, and the one that has to be measured rather
    // than asserted structurally: there is no observable state to check, because
    // the compiled patterns live in `syntect`'s own `OnceCell`s.
    //
    // One highlighter, deliberately: the second timing has to meet the patterns
    // the first compiled, and a fresh `SyntaxSet` would be cold again and measure
    // nothing.
    if !absolute_gates_apply("cargo test --release -p vigia-core --test warm") {
        return;
    }
    // Taken for the reason `crates/vigia/tests/budgets.rs` takes it: this binary
    // also runs `many_files_of_one_language_warm_only_a_few`, which builds an
    // eighty-four file fixture, and a polyglot gate that writes twenty-five files
    // and compiles up to twelve grammars on a thread. A wall clock measured
    // beside either is measuring the neighbour.
    //
    // **It works only because they take it too.** A mutual-exclusion protocol one
    // participant observes is not one, and the two heavy gates hold it across
    // their fixture building for exactly that reason.
    let _timed = exclusively_timed();

    let scratch = Scratch::large_diff("warm-cost", 1, 40);
    let root = scratch.root().to_path_buf();
    let paths = vec!["src/mod_0.rs".to_owned()];

    let cold = Highlighter::new();
    let cold_parse = time(|| {
        cold.warm_ahead(root.clone(), paths.clone())
            .join()
            .expect("the warmer thread");
    });

    // The same highlighter, so the second run meets its own compiled patterns.
    let after = time(|| {
        cold.warm_ahead(root.clone(), paths.clone())
            .join()
            .expect("the warmer thread");
    });

    eprintln!("note: a first parse is {cold_parse:?}, the same parse warmed is {after:?}");

    // An order of magnitude, not a ratio tuned to this machine. The measured
    // gap is ~93ms against ~0.5ms, so ten times is loose enough to survive a
    // slow runner and tight enough that a `warm` which quietly stopped
    // compiling anything would fail it.
    assert!(
        after * 10 < cold_parse,
        "a warmed parse took {after:?} against {cold_parse:?} cold, so warming \
         is not moving the grammar compile anywhere"
    );
}

#[test]
fn the_per_grammar_cap_is_per_grammar_and_not_one_shared_counter() {
    // **The bound the docs call "the bound that matters" had no test saying it
    // was per grammar at all.** Replacing the map with a single counter left
    // every other gate here green, because each of their fixtures is one
    // language. `SPEC.md` §7's ASCII-fixture rule one axis over: a
    // single-language fixture cannot tell a per-grammar cap from a global one.
    let scratch = Scratch::large_diff("warm-two-languages", 1, 4);
    let mut paths = Vec::new();
    for n in 0..WARM_PER_GRAMMAR + 1 {
        for (ext, body) in [("rs", "fn a() {}\n"), ("md", "# h\n\ntext\n")] {
            let name = format!("pair_{n}.{ext}");
            std::fs::write(scratch.path_of(&name), body).expect("write");
            paths.push(name);
        }
    }

    let warmed = Highlighter::new()
        .warm_ahead(scratch.root().to_path_buf(), paths)
        .join()
        .expect("the warmer thread");

    assert_eq!(
        warmed,
        2 * WARM_PER_GRAMMAR,
        "two languages offering {WARM_PER_GRAMMAR} files each warmed {warmed}, \
         not {}, so the cap is one shared counter rather than one per grammar",
        2 * WARM_PER_GRAMMAR
    );
}

#[test]
fn a_polyglot_changed_set_is_bounded_in_total_and_not_only_per_language() {
    // The per-grammar cap gives a polyglot tree as many budgets as it has
    // languages. Measured before `WARM_TOTAL` existed: fifty extensions warmed
    // forty-three files in 3.93s of held core, against the 1.053s worst case the
    // per-grammar cap was reasoned about with.
    let scratch = Scratch::large_diff("warm-polyglot", 1, 4);
    let exts = [
        "rs", "md", "py", "js", "go", "toml", "json", "yaml", "c", "cpp", "h", "rb", "sh", "php",
        "java", "cs", "css", "html", "xml", "sql", "lua", "swift", "hs", "clj", "erl",
    ];
    let mut paths = Vec::new();
    for (n, ext) in exts.iter().enumerate() {
        let name = format!("file_{n}.{ext}");
        std::fs::write(scratch.path_of(&name), "text\nmore text\n").expect("write");
        paths.push(name);
    }
    assert!(
        paths.len() > WARM_TOTAL,
        "the fixture offers {} files against a total cap of {WARM_TOTAL}, so it \
         cannot reach the bound it exists to test",
        paths.len()
    );

    let warmed = Highlighter::new()
        .warm_ahead(scratch.root().to_path_buf(), paths)
        .join()
        .expect("the warmer thread");

    // `assert_eq!` rather than `<=`, because a bound is only evidence when
    // something reached it: `warmed <= WARM_TOTAL` is satisfied by a `warm_ahead`
    // whose loop body was deleted. The fixture offers twice the cap, so equality
    // is what says the run both reached it and stopped there.
    assert_eq!(
        warmed, WARM_TOTAL,
        "the warmer parsed {warmed} files across distinct languages against a \
         total cap of {WARM_TOTAL}, so it either walked past the cap or never \
         reached it and this gate is vacuous"
    );
}

#[test]
fn a_file_that_is_not_text_is_skipped_before_it_spends_the_budget() {
    // A UTF-16 BOM is the ordinary way a path with a known extension is not
    // text. It trims to the empty string and can compile nothing, so counting it
    // both overstates the result and burns one of three per-grammar slots. The
    // sibling rule is already asserted for a path that vanished.
    let scratch = Scratch::large_diff("warm-not-text", 5, 4);
    std::fs::write(scratch.path_of("src/utf16.rs"), [0xFFu8, 0xFE, 0x66, 0x00]).expect("write");

    let alone = Highlighter::new()
        .warm_ahead(
            scratch.root().to_path_buf(),
            vec!["src/utf16.rs".to_owned()],
        )
        .join()
        .expect("the warmer thread");
    assert_eq!(
        alone, 0,
        "a file that is not text counted as a warm, so the result counts files \
         opened rather than grammars compiled"
    );

    // And placed first it must not cost a real file its slot.
    let mut paths = vec!["src/utf16.rs".to_owned()];
    paths.extend((0..5).map(|n| format!("src/mod_{n}.rs")));
    let warmed = Highlighter::new()
        .warm_ahead(scratch.root().to_path_buf(), paths)
        .join()
        .expect("the warmer thread");
    assert_eq!(
        warmed, WARM_PER_GRAMMAR,
        "with an unreadable file first the warmer parsed {warmed} real files \
         against {WARM_PER_GRAMMAR}, so it spent a slot on one that compiled \
         nothing"
    );
}

#[test]
fn the_warmer_reads_nothing_outside_the_worktree() {
    // `PathBuf::join` silently discards the root for an absolute path. Not
    // reachable from the shell, which passes what status reported, but
    // `warm_ahead` is public on a public type with no other precondition.
    // The bait lives in its own `Scratch` rather than at a fixed name beside the
    // worktree: the temp directory is shared, and a fixed name is one panic away
    // from being left behind, against the soak's zero-retained-temp-files claim.
    let scratch = Scratch::large_diff("warm-escape", 1, 4);
    let bait = Scratch::large_diff("warm-escape-bait", 1, 4);
    let outside = bait.path_of("src/mod_0.rs");

    let mut spellings = vec![
        // Absolute, which `PathBuf::join` discards the root for.
        outside.to_string_lossy().into_owned(),
        "../outside-the-worktree.rs".to_owned(),
    ];

    // **The spelling a blacklist misses, and it has to name a file that really
    // exists.** A path pointing at nothing is refused and not-found alike, so a
    // gate built on one is green either way: reverting this guard to its earlier
    // `is_absolute() || ParentDir` form left an earlier version of this test
    // passing. `Path::is_absolute` on Windows wants a prefix *and* a root, so
    // stripping the drive from the bait leaves a path that fails that test and
    // that `join` still resolves to the same real file.
    //
    // On Unix an absolute path has no prefix, so the case is already the one a
    // line above and there is nothing extra to spell.
    #[cfg(windows)]
    if let Some(rooted) = outside.to_string_lossy().get(2..) {
        spellings.push(rooted.to_owned());
    }

    let warmed = Highlighter::new()
        .warm_ahead(scratch.root().to_path_buf(), spellings)
        .join()
        .expect("the warmer thread");

    assert_eq!(
        warmed, 0,
        "the warmer read {warmed} files from outside the worktree it was given"
    );
}

#[test]
fn a_file_cut_mid_character_still_parses() {
    // The cut `WARM_BYTES` makes lands mid-character on anything that is not
    // ASCII, which the `valid_up_to` trim absorbs. Dropping that trim is one
    // edit and nothing else here would notice.
    //
    // **Only the trim, not the bound.** This file is valid UTF-8 throughout, so
    // it warms either way and an unbounded read passes it; the bound is gated by
    // `the_read_is_bounded_by_bytes_and_not_by_the_size_of_the_file`, which is
    // the one that can see it.
    let scratch = Scratch::large_diff("warm-bounded-read", 1, 4);
    let mut text = String::from("// ");
    while text.len() < WARM_BYTES.saturating_sub(2) {
        text.push('a');
    }
    // A four-byte character straddling the cut.
    text.push('\u{1F600}');
    // Far past the bound, so an unbounded read would take all of it.
    while text.len() < WARM_BYTES * 8 {
        text.push_str("\nfn filler() { let s = \"x\"; }");
    }
    std::fs::write(scratch.path_of("src/straddle.rs"), &text).expect("write");

    let warmed = Highlighter::new()
        .warm_ahead(
            scratch.root().to_path_buf(),
            vec!["src/straddle.rs".to_owned()],
        )
        .join()
        .expect("the warmer thread");

    assert_eq!(
        warmed, 1,
        "a file whose bounded read lands mid-character was skipped, so any file \
         with a wide glyph near {WARM_BYTES} bytes warms nothing"
    );
}

#[test]
fn the_read_is_bounded_by_bytes_and_not_by_the_size_of_the_file() {
    // **Two problem sizes whose first `WARM_BYTES` are byte-identical**, which is
    // the only shape that can see this bound. Every structural assertion here is
    // satisfied whether or not the read is capped — a warm counts the same either
    // way — and mutating the `take` away survived all of them. What separates the
    // two is wall clock against file size, so it takes the absolute tier.
    //
    // `SPEC.md` §7's rule about not comparing a part against the whole it belongs
    // to applies directly: the shared term is the first `WARM_BYTES`, and it
    // cancels, so what is left is exactly the tail an unbounded read would take.
    if !absolute_gates_apply("cargo test --release -p vigia-core --test warm") {
        return;
    }
    let _timed = exclusively_timed();

    let scratch = Scratch::large_diff("warm-read-bound", 1, 4);
    let head: String =
        std::iter::repeat_n("fn f() { let s = \"x\"; }\n", WARM_BYTES / 24 + 1).collect::<String>();
    assert!(
        head.len() >= WARM_BYTES,
        "the shared prefix is {} bytes, under the {WARM_BYTES} the read is capped \
         at, so the two fixtures differ inside the window rather than beyond it",
        head.len()
    );

    let mut big = head.clone();
    while big.len() < WARM_BYTES * 256 {
        big.push_str("fn g() { let s = \"y\"; }\n");
    }
    std::fs::write(scratch.path_of("src/small.rs"), &head).expect("write");
    std::fs::write(scratch.path_of("src/big.rs"), &big).expect("write");

    // One highlighter, warmed first, so neither timing carries the one-off
    // grammar compile and what is left is read plus parse.
    let highlighter = Highlighter::new();
    highlighter
        .warm_ahead(
            scratch.root().to_path_buf(),
            vec!["src/mod_0.rs".to_owned()],
        )
        .join()
        .expect("the warmer thread");

    let small = time(|| {
        highlighter
            .warm_ahead(
                scratch.root().to_path_buf(),
                vec!["src/small.rs".to_owned()],
            )
            .join()
            .expect("the warmer thread");
    });
    let large = time(|| {
        highlighter
            .warm_ahead(scratch.root().to_path_buf(), vec!["src/big.rs".to_owned()])
            .join()
            .expect("the warmer thread");
    });

    eprintln!(
        "note: warming {} bytes took {small:?}; warming {} bytes took {large:?}",
        head.len(),
        big.len()
    );

    // A file 256 times larger, read to its end, cannot come out inside four
    // times the cost of the capped one. Loose on purpose: the point is to catch
    // a read proportional to the file, which is two orders of magnitude, not to
    // track microseconds on a shared runner.
    assert!(
        large <= small * 4,
        "warming a {}-byte file took {large:?} against {small:?} for the \
         {}-byte one with the same first {WARM_BYTES} bytes, so the read follows \
         the file rather than the bound",
        big.len(),
        head.len()
    );
}
