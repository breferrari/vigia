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

use std::time::Duration;
use std::time::Instant;

use vigia_core::{
    GRAPH_COLUMNS, GRAPH_PERIOD, HISTORY_BUCKET, HISTORY_BUCKETS, HISTORY_PATHS, HISTORY_SAMPLES,
    HISTORY_WINDOW, History, Recency,
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
/// covers, and the strip would silently describe a shorter span than the
/// gradient's boundary uses. Cheap to check and impossible to notice by reading.
///
/// **This is the *drawn* grid, and since
/// [#198](https://github.com/breferrari/vigia/issues/198) it is no longer the one
/// the window rolls on.** That is the sample grid, which is private and therefore
/// out of this crate's reach; it is asserted at compile time beside
/// `SAMPLES_PER_BUCKET` instead, along with the exactness of the projection
/// between the two.
#[test]
fn the_window_is_exactly_the_buckets_it_is_divided_into() {
    assert_eq!(
        HISTORY_BUCKET * HISTORY_BUCKETS as u32,
        HISTORY_WINDOW,
        "the buckets do not tile the window, so a sample can fall outside every \
         one of them"
    );
}

/// Everything a path has inside the window, across every drawn column.
///
/// Named for the reason [`base`] and [`bulk_paths`] are: the gates below both ask
/// "did anything leak or vanish", and a spelled-out fold in each is the third
/// copy this file would carry.
fn total(drawn: &[u16; HISTORY_BUCKETS]) -> u32 {
    drawn.iter().map(|&count| u32::from(count)).sum()
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
    let now = base();
    let mut history = History::starting_at(now);

    // Well inside one drawn bucket, and spread far enough apart that a
    // one-second sampling grid puts them in different samples.
    let writes = 5;
    for step in 0..writes {
        history.record(["src/a.rs"], now + HISTORY_BUCKET / 8 * step);
    }

    let drawn = history.churn("src/a.rs").expect("the path is tracked");
    let newest = u32::from(drawn[HISTORY_BUCKETS - 1]);
    assert_eq!(
        newest, writes,
        "the newest column holds {newest} of {writes} writes made inside it, so \
         the projection is dropping samples rather than summing them: {drawn:?}"
    );
    assert_eq!(
        total(&drawn),
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
    let now = base();
    let mut history = History::starting_at(now);
    history.record(["src/a.rs"], now);

    for step in 1..HISTORY_BUCKETS {
        // Nothing written, only time passing, which is what `record` with an
        // empty iterator means.
        history.record(std::iter::empty(), now + HISTORY_BUCKET * step as u32);
        let drawn = history.churn("src/a.rs").expect("still inside the window");
        let at = HISTORY_BUCKETS - 1 - step;
        assert_eq!(
            drawn[at], 1,
            "after {step} bucket(s) the write is not in column {at}: {drawn:?}"
        );
        assert_eq!(
            total(&drawn),
            1,
            "the write was duplicated or lost while sliding: {drawn:?}"
        );
    }
}

#[test]
fn each_drawn_bucket_covers_a_whole_share_of_the_window() {
    // **The period the spec names, gated where it is computed.** `SPEC.md`
    // §11.1 says a drawn bucket is ten seconds and a band column eight, and both
    // are divisions of one window by one constant. A `const` block already
    // refuses a division that is not exact; this refuses one that is exact and
    // wrong, which is the case that would leave the spec's numbers false while
    // everything still compiled.
    assert_eq!(HISTORY_BUCKET, Duration::from_secs(10));
    assert_eq!(GRAPH_PERIOD, Duration::from_secs(8));

    // And the two tile the same window, which is what makes them comparable at
    // all: the band is finer than the strip beside it, and both are coarser than
    // the rate the store samples at.
    assert_eq!(HISTORY_BUCKET * HISTORY_BUCKETS as u32, HISTORY_WINDOW);
    assert_eq!(GRAPH_PERIOD * GRAPH_COLUMNS as u32, HISTORY_WINDOW);
    assert!(
        GRAPH_PERIOD < HISTORY_BUCKET,
        "the band stopped being finer than the sparkline, which is the whole \
         reason they are two elements"
    );
}

/// The sparkline's bucket count is a ceiling, not a choice, and the ceiling is
/// computed here rather than written down.
///
/// **[#161](https://github.com/breferrari/vigia/issues/161) asked for the drawn
/// bucket count to become a rung of the width ladder, and for the sparkline that
/// is refused by arithmetic rather than by preference.** A wider slot cannot buy
/// more of the window, because the widest rung already draws all of it. It can
/// only buy a finer division of the same window, and the division cannot go finer
/// than a band column without the two elements reading one store at crossed
/// scales, which is what the gate above forbids.
///
/// So the largest count available is the largest divisor of [`HISTORY_SAMPLES`]
/// whose period still exceeds [`GRAPH_PERIOD`], and the sparkline is already
/// sitting on it. **Searched rather than restated**, because the claim is that no
/// larger count exists: writing the answer down would assert the number instead
/// of the argument, and the number would go on passing if either constant moved
/// underneath it.
#[test]
fn the_sparkline_is_already_at_the_ceiling_the_band_sets() {
    let ceiling = (1..=HISTORY_SAMPLES)
        .filter(|count| HISTORY_SAMPLES % count == 0)
        .filter(|count| HISTORY_WINDOW / *count as u32 > GRAPH_PERIOD)
        .max()
        .expect("some division of the window is coarser than a band column");

    assert_eq!(
        HISTORY_BUCKETS, ceiling,
        "the sparkline draws {HISTORY_BUCKETS} buckets where {ceiling} is the \
         largest division of the window that stays coarser than a band column, \
         so it has either room left to grow or has already passed the band"
    );
}

/// The newest drawn bucket of a path, which is where a write just landed.
fn newest(history: &History, path: &str) -> u16 {
    *history
        .churn(path)
        .expect("the path is tracked")
        .last()
        .expect("a window has buckets")
}

/// A write weighs the bytes it moved, not the fact that it happened.
///
/// **[#232](https://github.com/breferrari/vigia/issues/232), reported from a live
/// pane.** A sample used to be a count of files written, so a worktree where one
/// file is saved repeatedly put exactly one in every sample and made itself the
/// peak, and both the band and the sparkline drew every active column at full
/// height. The element could say *when* and never *how much*, which is the
/// opposite of the "change density over time" `SPEC.md` §5.1 names it for.
///
/// The first write of a path is a baseline rather than a change, so the gate
/// takes three: one to establish the size, then a small edit and a large one.
#[test]
fn a_larger_write_weighs_more_than_a_smaller_one() {
    let now = base();
    let mut history = History::starting_at(now);

    history.record_sized([("src/a.rs", Some(1_000)), ("src/b.rs", Some(1_000))], now);
    let baseline = (newest(&history, "src/a.rs"), newest(&history, "src/b.rs"));
    assert_eq!(
        baseline,
        (1, 1),
        "a first write has no earlier size to differ from, so it weighs the \
         floor; charging a file's whole size on first sight would spike the peak \
         on the first save of a session"
    );

    // `a` gains ten bytes and `b` gains a thousand, in one tick, so nothing but
    // the weight can separate them.
    history.record_sized([("src/a.rs", Some(1_010)), ("src/b.rs", Some(2_000))], now);
    let (small, large) = (newest(&history, "src/a.rs"), newest(&history, "src/b.rs"));

    assert!(
        large > small,
        "a thousand-byte write drew {large} against a ten-byte write's {small}, \
         so the store is still counting writes rather than weighing them"
    );
    // And the magnitudes are the deltas rather than some rank of them, which is
    // what makes the drawn heights proportional instead of merely ordered.
    assert_eq!((small, large), (1 + 10, 1 + 1_000));
}

/// A file that shrinks moved as much as one that grew.
///
/// Deleting five thousand bytes is the larger of the two edits a reader can make
/// and signing the difference would draw it as the quieter one.
#[test]
fn a_write_that_shrinks_a_file_weighs_what_it_removed() {
    let now = base();
    let mut history = History::starting_at(now);

    history.record_sized([("src/a.rs", Some(5_000))], now);
    history.record_sized([("src/a.rs", Some(1_000))], now);

    assert_eq!(newest(&history, "src/a.rs"), 1 + 4_000);
}

/// A size that could not be read still counts the write.
///
/// A file can vanish between the watch naming it and the `stat`, and it was
/// still written. It weighs the floor rather than nothing, which is exactly what
/// [`History::record`] does for every caller that supplies no size at all, so the
/// unsized entry point keeps the behaviour every gate written before #232 holds.
#[test]
fn a_write_whose_size_cannot_be_read_still_counts_one() {
    let now = base();
    let mut sized = History::starting_at(now);
    let mut plain = History::starting_at(now);

    sized.record_sized([("src/a.rs", None)], now);
    sized.record_sized([("src/a.rs", None)], now);
    plain.record(["src/a.rs"], now);
    plain.record(["src/a.rs"], now);

    assert_eq!(newest(&sized, "src/a.rs"), 2);
    assert_eq!(
        sized.churn("src/a.rs"),
        plain.churn("src/a.rs"),
        "an unreadable size stopped behaving like the unsized entry point, so \
         every gate written against `record` is measuring something else now"
    );
}

/// The peak follows the weight, which is what makes the drawn heights differ.
///
/// The store's own half of #232: heights are scaled against the busiest drawn
/// bucket, so a peak that stayed at the file count would leave every active
/// column at the ceiling however the samples were weighed.
#[test]
fn the_peak_follows_the_weight_rather_than_the_write_count() {
    let now = base();
    let mut history = History::starting_at(now);

    history.record_sized([("src/a.rs", Some(100))], now);
    let flat = history.peak();
    history.record_sized([("src/a.rs", Some(5_000))], now);

    assert!(
        history.peak() > flat,
        "a four-thousand-nine-hundred-byte write left the peak at {flat}, so \
         every column would still draw against a denominator that cannot move"
    );
}
