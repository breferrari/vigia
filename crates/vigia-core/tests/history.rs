//! I10, as an invariant over the public store rather than over its arithmetic.
//!
//! > Churn history is bounded by a fixed window and a fixed cap on tracked
//! > paths, independent of how many files the session changed. A path that ages
//! > out of the window is dropped entirely.
//!
//! The unit tests inside `src/history.rs` cover the rules one step at a time:
//! one path aging out, one eviction picking the right victim, one bucket
//! rolling. This file covers the claim those rules add up to, and it covers it
//! **at the scale I10 is written against**, which is the part no arithmetic test
//! reaches. `SPEC.md` §5.2 names the case in as many words: *a bulk operation
//! touching ten thousand files must not grow it past the cap.*
//!
//! Every gate here drives the clock by handing an [`Instant`] in rather than by
//! sleeping. The window is two minutes long, so a suite that waited for real
//! time could not assert the time rule at all, and one that shortened the window
//! for testing would be gating a constant the product does not ship.
//!
//! Structural throughout: counts and bounds, hardware-independent, no slack.
//! There is nothing to time here, because I10 is a claim about size.

use std::time::Instant;

use vigia_core::{
    HISTORY_BUCKET, HISTORY_BUCKETS, HISTORY_PATHS, HISTORY_WINDOW, History, Recency,
};

/// Paths a bulk operation invents, well past the cap.
///
/// `SPEC.md` §5.2's own number. It matters that it is far above
/// [`HISTORY_PATHS`] rather than just above: a fixture that only grazed the cap
/// would pass against an off-by-one in the eviction rule.
const BULK: usize = 10_000;

/// A base far enough ahead that a gate can step forward through a whole window
/// without leaving the monotonic clock's range.
fn base() -> Instant {
    Instant::now() + HISTORY_WINDOW * 4
}

fn bulk_paths() -> Vec<String> {
    (0..BULK)
        .map(|n| format!("src/generated/f{n}.rs"))
        .collect()
}

/// The case `SPEC.md` §5.2 names, driven rather than approximated.
///
/// Both halves are asserted, and the second is the one that matters. A store
/// nothing ever filled satisfies a cap the way an empty room satisfies a fire
/// code, so the bound is only evidence when the eviction counter says the cap
/// was doing the bounding.
#[test]
fn ten_thousand_distinct_paths_leave_the_store_at_the_cap() {
    let now = base();
    let mut history = History::starting_at(now);
    let paths = bulk_paths();

    history.record(paths.iter().map(String::as_str), now);

    assert_eq!(
        history.tracked(),
        HISTORY_PATHS,
        "{BULK} paths left {} tracked, so glance history is bounded by what the \
         session touched rather than by anything fixed",
        history.tracked()
    );
    assert_eq!(
        history.stats().evicted_by_cap as usize,
        BULK - HISTORY_PATHS,
        "the cap held without evicting, so it was never what bounded the store"
    );
    assert_eq!(history.stats().recorded as usize, BULK);
}

/// The bound has to hold *throughout*, not only when the dust settles.
///
/// A store that collected everything and pruned at the end would pass the gate
/// above while allocating for ten thousand paths, which is the cost I10 exists
/// to prevent rather than to clean up after.
#[test]
fn the_store_never_grows_past_the_cap_at_any_point_during_the_bulk() {
    let now = base();
    let mut history = History::starting_at(now);

    for (n, path) in bulk_paths().iter().enumerate() {
        history.record([path.as_str()], now);
        assert!(
            history.tracked() <= HISTORY_PATHS,
            "after {n} paths the store held {}, over the cap of {HISTORY_PATHS}",
            history.tracked()
        );
    }
}

/// The time rule, which is the half a cap cannot provide.
///
/// A worktree that goes quiet must empty the store rather than hold its last
/// picture for ever: `SPEC.md` §5.2 asks for history that survives a file
/// settling, and I10 is what stops "survives" from meaning "never leaves".
#[test]
fn a_window_of_silence_empties_the_store() {
    let now = base();
    let mut history = History::starting_at(now);
    let paths = bulk_paths();
    history.record(paths.iter().map(String::as_str), now);
    assert_eq!(history.tracked(), HISTORY_PATHS);

    // One tick, naming nothing, a whole window later. This is the staging case
    // and the idle case at once: the wake that arrives after a quiet spell.
    history.record(std::iter::empty(), now + HISTORY_WINDOW);

    assert_eq!(
        history.tracked(),
        0,
        "{} paths survived a whole window of silence",
        history.tracked()
    );
    assert!(history.stats().evicted_by_window >= HISTORY_PATHS as u64);
    assert_eq!(history.peak(), 0, "the shared scale outlived its samples");
}

/// The same claim, reached one bucket at a time.
///
/// **This is a different code path from the gate above and mutation testing is
/// what found that.** A gap of a whole window or more is a special case: nothing
/// tracked can have a sample left, so the store clears outright rather than
/// shifting anything. Every gate here jumped a full window, so the ordinary path
/// (shift the buckets, drop whatever came out empty) was never run, and breaking
/// it left the whole suite green.
///
/// A monitor beside a working agent takes exactly this shape: ticks arrive
/// steadily, each one a fraction of the window, and a path ages out through
/// accumulated shifts rather than through one long silence.
#[test]
fn a_path_ages_out_through_ordinary_ticks_and_not_only_through_one_long_gap() {
    let now = base();
    let mut history = History::starting_at(now);
    history.record(["src/lib.rs"], now);

    // One bucket per tick, each naming a *different* file, so the store stays
    // busy and only the first path is aging.
    for step in 1..=HISTORY_BUCKETS as u32 {
        history.record(["src/other.rs"], now + HISTORY_BUCKET * step);
        assert!(
            history.tracked() > 0,
            "the store emptied at step {step}, so this walked off the end \
             instead of aging one path out"
        );
    }

    assert_eq!(
        history.churn("src/lib.rs"),
        None,
        "the path still has buckets after the window slid past it a step at a \
         time, so eviction only happens on the whole-window shortcut"
    );
    assert_eq!(history.recency("src/lib.rs"), Recency::Cold);
    assert_eq!(
        history.tracked(),
        1,
        "the aged-out path is still occupying a slot the cap counts"
    );
    assert!(history.stats().evicted_by_window > 0);
}

/// A path that ages out is **dropped**, not drawn empty.
///
/// The distinction is the whole of I10's second sentence, and it is
/// observable: a path drawn empty would still occupy a slot the cap counts, so
/// a store that zeroed instead of removing would be bounded by paths-ever-seen
/// rather than by the window.
#[test]
fn a_path_that_ages_out_is_dropped_rather_than_kept_empty() {
    let now = base();
    let mut history = History::starting_at(now);
    history.record(["src/lib.rs"], now);
    assert!(history.churn("src/lib.rs").is_some());

    history.record(["src/other.rs"], now + HISTORY_WINDOW);

    assert_eq!(history.churn("src/lib.rs"), None);
    assert_eq!(history.recency("src/lib.rs"), Recency::Cold);
    assert_eq!(
        history.tracked(),
        1,
        "the aged-out path is still occupying a slot the cap counts"
    );
}

/// Churn survives a file settling, which is the reason this store exists at all.
///
/// `SPEC.md` §5.2: the frame path's cache empties the moment a path stops being
/// changed, and a sparkline built on that would show nothing worth glancing at.
/// This is the property that had to be added rather than reused.
#[test]
fn a_path_keeps_its_churn_after_it_stops_changing() {
    let now = base();
    let mut history = History::starting_at(now);
    history.record(["src/lib.rs"], now);

    // Several ticks later, about other files entirely: the shape of an agent
    // that moved on to something else.
    for step in 1..=4 {
        history.record(["src/other.rs"], now + HISTORY_BUCKET * step);
    }

    let buckets = history
        .churn("src/lib.rs")
        .expect("the settled file lost its history");
    assert!(
        buckets.iter().any(|&count| count > 0),
        "the settled file is tracked but its window is empty"
    );
    assert_eq!(
        history.recency("src/lib.rs"),
        Recency::Live,
        "a settled file inside the window is live, not cold and not pulsing"
    );
}

/// The buckets have to tile the window exactly.
///
/// [`HISTORY_BUCKET`] is an integer division of [`HISTORY_WINDOW`], so a window
/// that is not a multiple of the bucket count leaves a remainder no bucket
/// covers: a sample could then fall outside every one of them, and the strip
/// would silently describe a shorter span than the gradient's boundary uses.
/// Cheap to check and impossible to notice by reading.
#[test]
fn the_window_is_exactly_the_buckets_it_is_divided_into() {
    assert_eq!(
        HISTORY_BUCKET * HISTORY_BUCKETS as u32,
        HISTORY_WINDOW,
        "the buckets do not tile the window, so a sample can fall outside every \
         one of them"
    );
}

/// A drawn column is the **sum** of the samples under it, not one of them.
///
/// **The gate [#198](https://github.com/breferrari/vigia/issues/198) exists
/// for.** The store samples fifteen times finer than the sparkline draws, and
/// the projection is what hides that. Two writes a second apart used to land in
/// one bucket and add up because the bucket *was* the sample; now they land in
/// two samples of one column, and only summing keeps the answer the same.
///
/// Every other spelling of the projection fails here and passes everything else:
/// taking the newest sample draws one, taking the max draws one, taking the
/// first draws one. That is why this is asserted on the total rather than on
/// the shape.
///
/// Driven entirely through the public store, so it says nothing about how the
/// samples are held and keeps saying it if that changes again.
#[test]
fn a_drawn_bucket_is_the_sum_of_everything_written_inside_it() {
    let now = Instant::now();
    let mut history = History::starting_at(now);

    // Well inside one drawn bucket, and spread far enough apart that a
    // one-second sampling grid puts them in different samples.
    let writes = 5;
    for step in 0..writes {
        history.record(["src/a.rs"], now + HISTORY_BUCKET / 8 * step);
    }

    let drawn = history.churn("src/a.rs").expect("the path is tracked");
    let newest = drawn[HISTORY_BUCKETS - 1];
    assert_eq!(
        newest,
        u16::try_from(writes).expect("a small count"),
        "the newest column holds {newest} of {writes} writes made inside it, so \
         the projection is dropping samples rather than summing them: {drawn:?}"
    );
    assert_eq!(
        drawn.iter().map(|&count| u32::from(count)).sum::<u32>(),
        writes,
        "writes leaked into a column they were not made in: {drawn:?}"
    );
}

/// One [`HISTORY_BUCKET`] of elapsed time moves a write exactly one column.
///
/// The other half of the projection's contract, and the one that would break
/// silently: a store sampling finer than it draws could roll on the fine grid and
/// still report the coarse one, but only if the two agree about how many samples
/// a column is. If they ever disagree, a write slides by the wrong number of
/// columns and the sparkline becomes a different picture of the same worktree.
///
/// Asserted at the boundary rather than at a fraction of it, because that is
/// where an off-by-one lives.
#[test]
fn a_bucket_of_elapsed_time_slides_the_column_by_one() {
    let now = Instant::now();
    let mut history = History::starting_at(now);
    history.record(["src/a.rs"], now);

    for step in 1..HISTORY_BUCKETS {
        // Nothing written, only time passing, which is what `record` with an
        // empty iterator means.
        history.record(
            std::iter::empty::<&str>(),
            now + HISTORY_BUCKET * step as u32,
        );
        let drawn = history.churn("src/a.rs").expect("still inside the window");
        let at = HISTORY_BUCKETS - 1 - step;
        assert_eq!(
            drawn[at], 1,
            "after {step} bucket(s) the write is not in column {at}: {drawn:?}"
        );
        assert_eq!(
            drawn.iter().map(|&count| u32::from(count)).sum::<u32>(),
            1,
            "the write was duplicated or lost while sliding: {drawn:?}"
        );
    }
}
