//! What a `hide` pattern keeps out beyond the walk: `SPEC.md` §11.1.
//!
//! The walk's own filter is gated in `vigia-core/tests/hidden.rs`. These are the
//! two places a hidden path can still arrive from somewhere else: the wake's
//! burst, which feeds the churn history directly, and the header, which is what
//! tells the reader anything is being kept from them at all.

use std::time::Instant;

use vigia::{shown, sized};
use vigia_core::{Hidden, History};

const KEPT: &str = "src/lib.rs";
const GENERATED: &str = "target/debug/build.log";

fn burst() -> Vec<String> {
    vec![KEPT.to_owned(), GENERATED.to_owned()]
}

/// The acceptance criterion the walk alone cannot meet.
///
/// `History` is fed from the burst and never from `Frame::files`, so a path
/// filtered out of the walk still lands here unless the burst is filtered too. It
/// costs one of I10's 256 tracked paths and holds a sparkline for a row nothing
/// draws.
#[test]
fn a_hidden_path_never_reaches_the_history_store() {
    let workdir = std::env::temp_dir();
    let hide = Hidden::new("^target/").expect("a pattern");

    // Non-vacuity first: without a pattern both paths are tracked, so the
    // assertion below is over a store that would otherwise have held two.
    let mut open = History::new();
    open.record_sized(sized(&workdir, &burst()), Instant::now());
    assert_eq!(
        open.tracked(),
        2,
        "the burst is not reaching the store at all"
    );

    let mut filtered = History::new();
    filtered.record_sized(
        sized(&workdir, &shown(burst(), Some(&hide))),
        Instant::now(),
    );

    assert!(
        filtered.churn(GENERATED).is_none(),
        "a hidden path is in the churn history, so it spends one of I10's 256 \
         tracked paths and carries a sparkline for a row the pane never draws"
    );
    assert!(
        filtered.churn(KEPT).is_some(),
        "the filter took the whole burst rather than the hidden part of it"
    );
    assert_eq!(filtered.tracked(), 1);
}

/// The pattern is the only thing that narrows a burst, so a pane without one
/// pays nothing and sees everything.
#[test]
fn no_pattern_narrows_no_burst() {
    assert_eq!(shown(burst(), None), burst());
}

/// Searched rather than anchored here too, so the burst and the walk agree about
/// what the same pattern means.
#[test]
fn the_burst_and_the_walk_read_one_pattern_the_same_way() {
    let hide = Hidden::new(r"\.lock$").expect("a pattern");
    let paths = vec![
        "deps/pinned.lock".to_owned(),
        "deps/pinned.lock.txt".to_owned(),
    ];
    assert_eq!(
        shown(paths, Some(&hide)),
        vec!["deps/pinned.lock.txt".to_owned()]
    );
}
