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

use std::time::{Duration, Instant};

use support::Scratch;
use vigia_core::{Highlighter, WARM_FILES};

/// Whether the absolute wall-clock half should assert.
fn absolute_gates_apply() -> bool {
    if cfg!(debug_assertions) {
        eprintln!(
            "note: the absolute half is skipped in a debug build; run \
             `cargo test --release -p vigia-core --test warm` to enforce it"
        );
        false
    } else {
        true
    }
}

fn time(work: impl FnOnce()) -> Duration {
    let began = Instant::now();
    work();
    began.elapsed()
}

#[test]
fn a_path_with_no_grammar_is_a_no_op_rather_than_a_failure() {
    // The same answer `syntax_for` gives the frame path for a file type nothing
    // recognises, and it has to be reachable without panicking because the
    // warmer walks whatever status named.
    let highlighter = Highlighter::new();
    let handle = highlighter.warm_ahead(std::path::PathBuf::from("."), Vec::new());
    assert_eq!(handle.join().expect("the warmer thread"), 0);
}

#[test]
fn the_warmer_is_bounded_by_its_file_cap() {
    // I3's business: a monitor is left open for days and a worktree can be
    // enormous. The cap is what stops "warm the changed set" becoming "read the
    // repository".
    let files = WARM_FILES + 20;
    let scratch = Scratch::large_diff("warm-cap", files, 4);
    let highlighter = Highlighter::new();
    let paths: Vec<String> = (0..files).map(|n| format!("src/mod_{n}.rs")).collect();

    let warmed = highlighter
        .warm_ahead(scratch.path_of("."), paths)
        .join()
        .expect("the warmer thread");

    assert_eq!(
        warmed, WARM_FILES,
        "the warmer read {warmed} of {files} files, so its cap of {WARM_FILES} \
         is not holding"
    );
}

#[test]
fn a_path_that_is_not_there_is_skipped_rather_than_fatal() {
    // A file can vanish between status naming it and this thread reaching it,
    // which is ordinary beside an agent. The run has to continue: a warmer that
    // gave up on the first missing path would stop warming exactly when the
    // other pane is busiest.
    let scratch = Scratch::large_diff("warm-missing", 2, 4);
    let highlighter = Highlighter::new();

    let warmed = highlighter
        .warm_ahead(
            scratch.path_of("."),
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
        "the warmer counted {warmed} files where two of the four exist, so a \
         missing path is not being skipped the way it has to be"
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
    if !absolute_gates_apply() {
        return;
    }

    let scratch = Scratch::large_diff("warm-cost", 1, 40);
    let root = scratch.path_of(".");
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
