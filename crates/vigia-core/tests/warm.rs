//! Compiling grammars ahead of the reader, and what that is and is not worth.
//!
//! `syntect` defers a grammar's patterns to `fancy_regex` on first use, so the
//! first parse under one costs 74-362ms where loading all seventy-five costs
//! 318µs. [`vigia_core::warm`] pays that where nothing is waiting on it.
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
use vigia_core::{Highlighter, WARM_FILES, WARM_PER_GRAMMAR};

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
        "the warmer counted a file `syntect` has no grammar for, so it read one          it could not have compiled anything from"
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
        "the warmer counted {warmed} files where two of the four exist, so a          missing path is either fatal or is spending the per-grammar budget"
    );
}

#[test]
fn warming_moves_a_grammars_compile_off_the_parse_that_follows_it() {
    // The only claim `warm` makes, and the one that has to be measured rather
    // than asserted structurally: there is no observable state to check, because
    // the compiled patterns live in `syntect`'s own `OnceCell`s.
    //
    // Two highlighters, so each starts with a genuinely cold `SyntaxSet`. Sharing
    // one would make the second measurement free for a reason that is not the
    // code, which is the trap this whole issue was about.
    if !absolute_gates_apply("cargo test --release -p vigia-core --test warm") {
        return;
    }
    // Taken for the reason `crates/vigia/tests/budgets.rs` takes it: this binary
    // also runs `the_warmer_is_bounded_by_its_file_cap`, which builds an
    // eighty-four file fixture and warms sixty-four of them on a thread. A wall
    // clock measured beside that is measuring the neighbour.
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
