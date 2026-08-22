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
    Churn, HISTORY_BUCKET, HISTORY_BUCKETS, HISTORY_PATHS, HISTORY_SAMPLE, HISTORY_SAMPLES,
    HISTORY_WINDOW, History, Recency, scale_of,
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
    assert_eq!(history.scale(), 0, "the shared scale outlived its samples");
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
fn total(drawn: &[u32; HISTORY_BUCKETS]) -> u32 {
    drawn.iter().copied().sum()
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
    let newest = drawn[HISTORY_BUCKETS - 1];
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
    // §5.1 says a source bucket is five seconds, and it is a division of one
    // window by one constant. A `const` block already refuses a division that is
    // not exact; this refuses one that is exact and wrong, which is the case that
    // would leave the spec's numbers false while everything still compiled.
    //
    // **Five since [#234](https://github.com/breferrari/vigia/issues/234)**, from
    // ten, because this constant became the resolution the shell projects *from*
    // rather than the one it draws: the drawn period is this times the source
    // buckets a rung groups, so it is ten seconds at the settled rung still.
    assert_eq!(HISTORY_BUCKET, Duration::from_secs(5));

    // And it tiles the window, which is what makes a drawn bucket mean the same
    // amount of time wherever it sits.
    assert_eq!(HISTORY_BUCKET * HISTORY_BUCKETS as u32, HISTORY_WINDOW);

    // **The band half of this gate is gone with the constants it read**
    // ([#232](https://github.com/breferrari/vigia/issues/232)). It asserted
    // `GRAPH_PERIOD < HISTORY_BUCKET`, "the band stopped being finer than the
    // sparkline, which is the whole reason they are two elements", against a band
    // that drew fifteen fixed columns. The band draws one value per sub-column
    // now, so its period is a property of the pane rather than a constant, and at
    // any wide pane it is the store's own one-second resolution. It is therefore
    // finer than a drawn bucket at every width, and the comparison has nothing
    // left to compare.
}

// **The sparkline's ceiling gate is gone, and #161's ruling with it**
// ([#232](https://github.com/breferrari/vigia/issues/232)).
//
// It searched for the largest division of the window whose period still exceeded
// `GRAPH_PERIOD` and asserted `HISTORY_BUCKETS` was already it, which is how
// [#161](https://github.com/breferrari/vigia/issues/161) closed its sparkline
// half by ruling rather than by a rung: a drawn bucket could not go finer than a
// band column without the two elements drawing one store at crossed scales.
//
// **That bound was a property of the band's fixed period and the band no longer
// has one.** It draws one value per sub-column, so at any wide pane its period
// is the store's own second, which is finer than any bucket the sparkline could
// ask for. The constraint is gone rather than merely restated, so the ruling it
// carried is reopened rather than defended, and the follow-up says so.

/// The newest drawn bucket of a path, which is where a write just landed.
fn newest(history: &History, path: &str) -> u32 {
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
    let flat = history.scale();
    history.record_sized([("src/a.rs", Some(5_000))], now);

    assert!(
        history.scale() > flat,
        "a four-thousand-nine-hundred-byte write left the peak at {flat}, so \
         every column would still draw against a denominator that cannot move"
    );
}

/// Two large writes of different sizes stay different sizes.
///
/// **The ceiling is part of the unit, and changing the unit moved it.** Sixteen
/// bits was ample while a sample counted writes: sixty-five thousand saves inside
/// one second is not a thing anyone can do. It is one ordinary save of a lockfile
/// once the unit is bytes, and two saves either side of that ceiling would both
/// peg, become indistinguishable, and hold the peak at its maximum for the whole
/// two-minute window, scaling every other row on screen to nothing. That is
/// exactly the flattening [#232](https://github.com/breferrari/vigia/issues/232)
/// exists to remove, re-entered through the type rather than through the count.
///
/// Both deltas here are past `u16::MAX`, so this is the gate that separates a
/// `u32` sample from a `u16` one; nothing else in this file does.
#[test]
fn two_large_writes_of_different_sizes_do_not_both_peg() {
    let now = base();
    let mut history = History::starting_at(now);

    history.record_sized([("src/small.rs", Some(0)), ("src/large.rs", Some(0))], now);
    history.record_sized(
        [
            ("src/small.rs", Some(100_000)),
            ("src/large.rs", Some(300_000)),
        ],
        now,
    );

    let (small, large) = (
        newest(&history, "src/small.rs"),
        newest(&history, "src/large.rs"),
    );
    assert!(
        large > small,
        "a 300,000-byte write drew {large} against a 100,000-byte write's \
         {small}, so both saturated and the graph cannot tell them apart"
    );
    assert_eq!((small, large), (1 + 100_000, 1 + 300_000));
}

/// The worktree sum saturates rather than wrapping.
///
/// **The widening put this back in play** ([#232](https://github.com/breferrari/vigia/issues/232)).
/// While a sample was a `u16` and the sum a `u32`, 256 paths at their ceiling
/// could not reach it and a plain `+=` was safe by arithmetic. A sample is a
/// `u32` of bytes now, so two paths at [`Track::bump`]'s own ceiling in one
/// second overflow the sum: a panic in debug, and in release the busiest worktree
/// there has ever been drawn as the quietest.
#[test]
fn the_worktree_sum_saturates_rather_than_wrapping() {
    let now = base();
    let mut history = History::starting_at(now);

    // Two paths, each written far past what a `u32` sample can hold, in one
    // second. `wrote` floors a first sighting at one, so each takes two writes:
    // one to set the baseline and one to move it by the whole range.
    for path in ["src/a.rs", "src/b.rs"] {
        history.record_sized([(path, Some(0))], now);
        history.record_sized([(path, Some(u64::from(u32::MAX)))], now);
    }

    let newest = history.worktree_churn().0[HISTORY_SAMPLES - 1];
    assert_eq!(
        newest,
        u32::MAX,
        "the worktree sum wrapped instead of topping out, so the busiest second \
         this store has ever held draws as one of its quietest"
    );
}

/// `scale_of` at its edges, driven directly rather than through a rendered frame.
///
/// It is public API and every other gate reaches it through a screen, which is
/// how the empty case and the floor stayed unexercised.
#[test]
fn the_scale_rule_holds_at_its_edges() {
    assert_eq!(
        scale_of(std::iter::empty()),
        0,
        "nothing tracked is no scale"
    );
    assert_eq!(scale_of([0, 0, 0].into_iter()), 0, "all idle is no scale");

    // The floor: any non-empty input divides to at least one, or a single small
    // write would be measured against zero.
    assert_eq!(scale_of([1].into_iter()), 1);
    assert_eq!(
        scale_of([0, 0, 1].into_iter()),
        1,
        "the zeroes do not dilute it"
    );

    // 1.3 of the mean, floored, over the non-empty values only.
    assert_eq!(scale_of([10, 10, 10].into_iter()), 13);
    assert_eq!(scale_of([0, 10, 0, 10].into_iter()), 13);

    // **Above every input by design**, which is what stops a uniformly busy
    // worktree drawing as a solid block.
    assert!(scale_of([10, 10].into_iter()) > 10);

    // And it saturates rather than wrapping on a window full of ceilings.
    assert_eq!(scale_of([u32::MAX; 8].into_iter()), u32::MAX);
}

/// The multiple of the median a value may reach before it stops setting the
/// scale, restated rather than imported.
///
/// A gate reading the crate's own constant would agree with it by construction,
/// which is this suite's standing rule for every tuned number.
const OUTLIER: u32 = 10;

/// One loud value does not set the yardstick for the ones around it.
///
/// **[#256](https://github.com/breferrari/vigia/issues/256), reported from a live
/// pane**: a test run rewriting thousands of bytes sat in the same window as the
/// ordinary edits around it, and the mean it dragged up put every one of them on
/// the band's lowest level, which at the baseline row is not distinguishable from
/// the axis.
///
/// Asserted as an *equality against the window without the outlier in it*, which
/// is the strongest form available and the one a threshold cannot fake: the
/// figure is not merely lower, it is the figure the ordinary writes would have
/// set on their own.
#[test]
fn a_lone_outlier_does_not_set_the_yardstick() {
    let ordinary = [40u32; 12];
    let quiet = scale_of(ordinary.into_iter());

    let mut loud = ordinary.to_vec();
    loud.push(40 * 50);
    assert_eq!(
        scale_of(loud.iter().copied()),
        quiet,
        "one value fifty times the rest moved the yardstick the rest set"
    );

    // Non-vacuity, and it is the half that would catch a rule that excluded
    // everything: the loud value is genuinely in the window and genuinely large.
    assert!(
        loud.iter().copied().max().expect("a value") > quiet * 10,
        "the fixture's outlier is not an outlier, so nothing above is a gate"
    );
}

/// A window with nothing outlying in it is scaled exactly as it always was.
///
/// **The half of [#256](https://github.com/breferrari/vigia/issues/256) that is a
/// promise rather than a fix.** The cut is a no-op wherever the values are within
/// an order of magnitude of each other, which is by construction: nothing exceeds
/// the cut, so the arithmetic is the plain thirteen tenths it has always been.
/// Stated as a gate anyway, because "by construction" is what every silently
/// broken invariant was before it broke.
#[test]
fn a_window_with_no_outlier_scales_as_it_always_did() {
    /// The plain rule, written out: thirteen tenths of the mean of the non-empty
    /// values, with no cut in it at all.
    fn plain(values: &[u32]) -> u32 {
        let busy: Vec<u64> = values
            .iter()
            .map(|v| u64::from(*v))
            .filter(|v| *v > 0)
            .collect();
        if busy.is_empty() {
            return 0;
        }
        u32::try_from(busy.iter().sum::<u64>() * 13 / (busy.len() as u64 * 10)).expect("a scale")
    }

    // A four-to-one series, which is a legitimate dynamic range rather than a
    // tail: `masthead.rs`'s `QUARTERED` is this shape and is what binds the
    // constant's lower end.
    let quartered: Vec<u32> = (0..24).map(|at| if at < 12 { 16 } else { 4 }).collect();
    // A ramp of everything from one to the cut itself.
    let spread: Vec<u32> = (1..=OUTLIER).map(|step| step * 7).collect();

    for values in [vec![7u32; 30], quartered, spread, vec![3, 0, 9, 0, 5, 11]] {
        assert_eq!(
            scale_of(values.iter().copied()),
            plain(&values),
            "the cut moved a window with nothing outlying in it: {values:?}"
        );
    }
}

/// The cut is at the multiple itself, and the boundary is inclusive.
///
/// Both directions, because a threshold gate that only tests the far side passes
/// against a rule that never fires and against one that always does.
#[test]
fn the_cut_is_ten_times_the_median() {
    // An odd count so the median is a value rather than a choice between two,
    // and every other value equal to it so the median is unambiguous.
    let median = 30u32;
    let bulk = vec![median; 9];

    let mut at_the_cut = bulk.clone();
    at_the_cut.push(median * OUTLIER);
    let mut past_the_cut = bulk.clone();
    past_the_cut.push(median * OUTLIER + 1);

    assert!(
        scale_of(at_the_cut.iter().copied()) > scale_of(bulk.iter().copied()),
        "a value at exactly the cut was excluded, so the boundary is \
         exclusive where it should include"
    );
    assert_eq!(
        scale_of(past_the_cut.iter().copied()),
        scale_of(bulk.iter().copied()),
        "a value one past the cut still set the yardstick"
    );
}

/// The kept set is never empty, so the rule never divides by nothing.
///
/// **By construction rather than by a guard**: the median is at most ten times
/// itself, so it survives its own cut, so at least one value always does. The
/// small counts are where that would break first if the median were ever taken
/// off the end of the set rather than out of the middle.
#[test]
fn the_kept_set_is_never_empty() {
    for values in [
        vec![9u32],
        vec![1, 1_000_000],
        vec![1, 1, 1_000_000],
        vec![u32::MAX, 1],
        vec![0, 0, 7],
    ] {
        assert!(
            scale_of(values.iter().copied()) > 0,
            "the rule kept nothing and answered zero for {values:?}, which \
             every caller reads as \"no scale yet\""
        );
    }
}

/// When the loud writes are the majority, they are the ordinary write.
///
/// **The case the cut must not fire on**, and the one that says this is a robust
/// statistic rather than a ceiling. A reader watching an agent mid-burst is
/// looking at a worktree where the large writes *are* what is happening, and a
/// rule that excluded them would scale the band against the quiet between them
/// and draw the whole burst saturated.
#[test]
fn a_majority_burst_still_sets_the_yardstick() {
    let mut window = vec![2_000u32; 14];
    window.extend([40u32; 6]);

    let loud = scale_of(window.iter().copied());
    let quiet = scale_of([40u32; 6].into_iter());
    assert!(
        loud > quiet * 10,
        "a window that is mostly loud writes was scaled against the quiet \
         between them: {loud} against {quiet}"
    );
}

/// A coarser rung's yardstick is never below a finer rung's.
///
/// **A fact about the quantity, not about the estimator**: a drawn bucket that
/// sums more source buckets cannot be worth less than one that sums fewer, so the
/// figures are non-decreasing down [`SPARK_GROUPS`]. `SPEC.md` §11.1 states it as
/// *a rung is a change of resolution and not of height*, and until
/// [#256](https://github.com/breferrari/vigia/issues/256) nothing asserted it,
/// because a plain mean could not violate it.
///
/// The cut can, on a population too small to hold a bulk. **One tracked path with
/// one write is the case**, and it is not exotic: the kernel spreads a lone write
/// into a geometric ramp, the coarsest grouping has three buckets of it, and
/// three points of a ramp read as a bulk of two and an outlier. Measured on this
/// fixture the raw figures are 418, 837 and **302**, so coarsening the rung would
/// have made every bar taller.
#[test]
fn a_coarser_rung_is_never_measured_against_less() {
    let now = base();

    // One path and one write, which is the small population the clamp is for.
    let mut lone = History::starting_at(now);
    lone.record_sized([("src/lib.rs", Some(4_000))], now);
    lone.record_sized([("src/lib.rs", Some(28_000))], now);

    // And a populated worktree, where the estimator already answers in order and
    // the clamp changes nothing. Both, so this is not a gate about one fixture.
    let mut many = History::starting_at(now);
    for step in 0..8u32 {
        for file in 0..8u32 {
            let path = format!("src/f{file}.rs");
            let size = if file == 0 && step == 4 {
                90_000
            } else {
                4_000 + u64::from(step) * 300
            };
            many.record_sized(
                [(path.as_str(), Some(size))],
                now + HISTORY_SAMPLE * (step * 12 + file),
            );
        }
    }

    for (name, history) in [("one path", &lone), ("eight paths", &many)] {
        let scales = history.scales();
        assert!(
            scales.iter().max().copied().expect("a figure") > 0,
            "{name}: the fixture recorded nothing, so this compared nothing"
        );
        for pair in scales.windows(2) {
            assert!(
                pair[0] <= pair[1],
                "{name}: a coarser rung is measured against {} where the \
                 finer one is measured against {}, so widening the pane makes \
                 every bar shorter and narrowing it makes them taller: \
                 {scales:?}",
                pair[1],
                pair[0]
            );
        }
    }
}

/// A projection returns exactly the width it was asked for.
///
/// **The contract this gained in #232**, and the reason: the band asks for one
/// value per sub-column, which past a wide pane is more than the window holds
/// samples. It used to clamp and hand back a short `Vec`, and the band drew a
/// graph that stopped partway across the pane with bare axis after it.
#[test]
fn a_projection_returns_the_width_it_was_asked_for() {
    let mut samples = [0u32; HISTORY_SAMPLES];
    samples[0] = 5;
    samples[HISTORY_SAMPLES - 1] = 7;
    let churn = Churn(samples);

    for width in [
        1usize,
        7,
        HISTORY_SAMPLES - 1,
        HISTORY_SAMPLES,
        HISTORY_SAMPLES * 2 + 3,
    ] {
        let drawn = churn.projected(width);
        assert_eq!(
            drawn.len(),
            width,
            "a projection onto {width} was not {width} wide"
        );
    }

    // Below the sample count every sample lands in exactly one column, so the
    // total is conserved. Above it, values repeat instead, which is the honest
    // answer: the window holds what it holds.
    let exact: u32 = churn.projected(HISTORY_SAMPLES).iter().sum();
    assert_eq!(
        exact, 12,
        "a projection at the sample count lost or invented churn"
    );
    let narrow: u32 = churn.projected(10).iter().sum();
    assert_eq!(narrow, 12, "a narrower projection lost or invented churn");

    // The oldest value stays oldest however wide the ask, which is what stops a
    // wide pane drawing the window mirrored.
    let wide = churn.projected(HISTORY_SAMPLES * 2);
    assert_eq!(wide.first().copied(), Some(5));
    assert_eq!(wide.last().copied(), Some(7));
}

/// A deletion weighs what it removed, and the file after it weighs itself.
///
/// **`Track::wrote`'s docblock claimed this and only the shrink half was gated**
/// ([#232](https://github.com/breferrari/vigia/issues/232)). A path that vanishes
/// has a size of zero rather than an unknown one, and treating the two the same
/// left the baseline at whatever the file last held: the delete weighed the floor
/// and the *next* file at that path weighed the dead one's whole size. The
/// largest edit a reader can make drawn as the smallest, and a trivial one drawn
/// as a spike.
#[test]
fn deleting_a_file_weighs_what_it_removed() {
    let now = base();
    let mut history = History::starting_at(now);

    history.record_sized([("src/a.rs", Some(2_000))], now);
    let baseline = newest(&history, "src/a.rs");
    history.record_sized([("src/a.rs", Some(0))], now);
    let after_delete = newest(&history, "src/a.rs") - baseline;
    history.record_sized([("src/a.rs", Some(50))], now);
    let after_recreate = newest(&history, "src/a.rs") - baseline - after_delete;

    assert_eq!(
        after_delete, 2_000,
        "the deletion weighed {after_delete} rather than the two thousand bytes \
         it removed"
    );
    assert_eq!(
        after_recreate, 50,
        "the file that replaced it weighed {after_recreate} rather than its own \
         fifty bytes, so the baseline was still the dead file's"
    );
}

// ── #242: the glance elements draw a level ───────────────────────────────────
//
// `assets/preview.svg` draws each sparkline as a **wave**, monotone up then
// monotone down, and `SPEC.md` §5.1 rules that a published artifact answering an
// open question is the answer. A write is a point event, so the retained samples
// are zero almost everywhere and drawing them raw is a spike train. These gates
// are the picture's own property, asserted the way the picture states it.

/// A store holding one agent burst centred `ago` seconds back.
///
/// Written through `record_sized` rather than by poking samples, so the gates
/// below run against the store a reader actually gets. The sizes are the shape
/// of a save: a couple of small writes around one large one.
fn burst_at(history: &mut History, now: Instant, ago: u64, grown: u64) -> u64 {
    // **Cumulative sizes, because a sample weighs a difference.** `Track::wrote`
    // charges the change in a file's size, so handing the same four sizes twice
    // would make the second burst weigh nothing and handing four constants would
    // make it weigh whatever the gap between them happened to be. `grown` is what
    // the file already held, so each burst adds the same four deltas and two
    // bursts really are two of the same thing.
    let mut size = grown;
    for (offset, delta) in [(3u64, 1_000u64), (2, 3_500), (1, 4_500), (0, 9_000)] {
        size += delta;
        let at = ago.saturating_add(offset).min(HISTORY_WINDOW.as_secs() - 1);
        history.record_sized(
            [("src/engine/watch.rs", Some(size))],
            now - Duration::from_secs(at),
        );
    }
    size
}

/// Roll the window forward to `now` without touching the path under test.
///
/// The store ages on `record`, so a burst written at `now - 60` sits at the
/// newest sample until something later moves the window past it. A second path
/// is what does that here: it rolls every track and leaves this one's samples
/// where they were, which is exactly the ageing a real worktree would apply.
fn roll_to(history: &mut History, now: Instant) {
    history.record_sized([("src/other/untouched.rs", Some(1))], now);
}

/// The drawn buckets rise to a peak and fall away from it.
///
/// **The picture's property, not a preference.** Extracted from
/// `assets/preview.svg`, the eight sparkline bars of its first row are
/// `4 8 14 20 24 16 8 4`: strictly up, then strictly down. Raw samples cannot do
/// that from one burst, which is what this fails against.
#[test]
fn a_single_burst_draws_a_wave_rather_than_a_spike() {
    let now = base();
    let mut history = History::starting_at(now - HISTORY_WINDOW);
    burst_at(&mut history, now, 60, 0);
    roll_to(&mut history, now);

    let drawn = history
        .level("src/engine/watch.rs")
        .expect("the path is tracked");
    let peak = drawn
        .iter()
        .enumerate()
        .max_by_key(|(_, value)| **value)
        .map(|(at, _)| at)
        .expect("a peak");

    // Non-vacuity first: a wave needs more than one non-empty bucket, and a
    // spike train is exactly the case where it has one.
    // **And the far end of the window is empty.** Without this the gate passes
    // against a floor that lights every sample: a solid bar rises to a peak and
    // falls back to the bar, which is monotone in both directions and is the
    // defect this feature removes. Measured, that bug drew
    // `[10, 12, 58, 302, 1594, 8015, 5698, 1077, 204, 38, 10, 10]` and satisfied
    // every other assertion here.
    assert_eq!(
        (drawn[0], drawn[HISTORY_BUCKETS - 1]),
        (0, 0),
        "a burst in the middle of the window lit both far ends, so the level has          a floor under it rather than an axis: {drawn:?}"
    );

    let lit = drawn.iter().filter(|value| **value > 0).count();
    assert!(
        lit >= 4,
        "one burst lit {lit} of {HISTORY_BUCKETS} buckets, so the series is a \
         spike rather than a wave: {drawn:?}"
    );

    for pair in drawn[..=peak].windows(2) {
        assert!(
            pair[1] >= pair[0],
            "the buckets before the peak fall somewhere, so this is not a rise: {drawn:?}"
        );
    }
    for pair in drawn[peak..].windows(2) {
        assert!(
            pair[1] <= pair[0],
            "the buckets after the peak rise somewhere, so this is not a fall: {drawn:?}"
        );
    }
}

/// Two bursts thirty seconds apart still read as two.
///
/// **This is what defends the constant.** A level is a trade between shape and
/// resolution, and a kernel wide enough to make one burst pretty will merge two.
/// Measured across candidates on this gate's own fixture, the trough between two
/// bursts thirty seconds apart sits at **0.23** of the peak at six seconds, 0.39
/// at eight, 0.52 at ten and 0.62 at twelve, where they stop being two things.
/// Half is the line, and this gate is what fails if the constant is raised past
/// it.
///
/// **Re-derived at the finer source resolution**
/// ([#234](https://github.com/breferrari/vigia/issues/234)), because these
/// numbers are read through a bucket and the bucket halved. They moved in the
/// direction that matters and not the one that decides: six seconds went from
/// 0.36 to 0.23, so the shipped constant has half again the margin it was
/// credited with, while the *line* is where it was, between eight seconds and
/// ten. A finer bucket smears less, which is the whole of it.
#[test]
fn two_bursts_thirty_seconds_apart_still_read_as_two() {
    let now = base();
    let mut history = History::starting_at(now - HISTORY_WINDOW);
    let grown = burst_at(&mut history, now, 80, 0);
    burst_at(&mut history, now, 50, grown);
    roll_to(&mut history, now);

    let drawn = history
        .level("src/engine/watch.rs")
        .expect("the path is tracked");
    // Each burst's own peak, found in its own half. **Not one global maximum
    // over both**: a sample weighs the *difference* a write made to a file's
    // size, so two identical bursts do not weigh the same, and a gate keyed on
    // equal peaks would read one bucket twice and see no trough at all.
    let split = HISTORY_BUCKETS / 2;
    let argmax = |slice: &[u32]| {
        slice
            .iter()
            .enumerate()
            .max_by_key(|(_, value)| **value)
            .map(|(at, value)| (at, *value))
            .expect("a bucket")
    };
    let (first, left) = argmax(&drawn[..split]);
    let (second, right) = argmax(&drawn[split..]);
    let second = second + split;

    assert!(
        left > 0 && right > 0,
        "one of the two halves drew nothing, so the fixture is not two bursts \
         and this gate proves nothing: {drawn:?}"
    );
    assert!(
        second > first + 1,
        "the two bursts landed in adjacent buckets, so there is no trough \
         between them to measure: {drawn:?}"
    );

    let trough = *drawn[first + 1..second].iter().min().expect("a trough");
    let peak = left.min(right);

    assert!(
        u64::from(trough) * 2 <= u64::from(peak),
        "the trough between two bursts thirty seconds apart is {trough} against \
         the smaller peak of {peak}, over half, so they have merged into one and \
         the kernel is too wide: {drawn:?}"
    );
}

/// A burst at the newest sample reads at full height.
///
/// There are no samples after the newest one, so half the kernel falls outside
/// the window and the level would droop there. `levelled` divides by the weight
/// **actually available** at each sample, which is what stops it.
///
/// What was tried and rejected is the naive form of that, dividing by the whole
/// kernel's weight everywhere: it flattens the mid-window wave instead of lifting
/// the edges. An early reading of this recorded "edge normalisation rejected",
/// which was the wrong lesson from the right measurement.
#[test]
fn a_burst_at_the_newest_sample_reads_full_height() {
    let now = base();

    let mut fresh = History::starting_at(now - HISTORY_WINDOW);
    burst_at(&mut fresh, now, 3, 0);
    roll_to(&mut fresh, now);
    let newest = fresh.level("src/engine/watch.rs").expect("tracked");

    let mut older = History::starting_at(now - HISTORY_WINDOW);
    burst_at(&mut older, now, 60, 0);
    roll_to(&mut older, now);
    let middle = older.level("src/engine/watch.rs").expect("tracked");

    let (a, b) = (
        *newest.iter().max().expect("a peak"),
        *middle.iter().max().expect("a peak"),
    );
    assert!(
        u64::from(a) * 10 >= u64::from(b) * 7,
        "the same burst draws {a} at the window's edge against {b} in its middle, \
         so the newest write is being dimmed by the kernel running off the end"
    );
}

#[test]
fn an_empty_window_is_never_due() {
    // **The half of [#243](https://github.com/breferrari/vigia/issues/243) that
    // keeps I1.** The clock the shell runs to age this window is admissible only
    // because it stops, and what stops it is this answering `None`. Asserted on
    // the value rather than on anything downstream, because the loop's receive
    // branches on exactly this and a plausible-looking duration here is a poll
    // loop on an idle monitor.
    // `starting_at`, not `new`: `new` opens the window at its own
    // `Instant::now()`, so a second sample taken on the next line leaves
    // `opened < start` and every offset below is measured against a boundary
    // that has already moved.
    let start = base();
    let mut history = History::starting_at(start);

    assert_eq!(
        history.ages_in(start),
        None,
        "a store with nothing recorded asked to be woken"
    );

    // A write arms it, and the deadline is inside one sample: the boundary it
    // names is the next grid line, not a whole period from now.
    history.record_sized([("src/a.rs", Some(4_000u64))], start);
    let due = history.ages_in(start).expect("a live window asks to age");
    assert!(
        due > Duration::ZERO && due <= HISTORY_SAMPLE,
        "a live window asked to wait {due:?}, which is not inside one sample of \
         the grid it rolls on"
    );

    // **Overdue asks for zero rather than panicking**, which is the ordinary
    // case on the first wake after the process was busy elsewhere: the roll is
    // already late and the answer is to do it now.
    assert_eq!(
        history.ages_in(start + HISTORY_SAMPLE * 3),
        Some(Duration::ZERO),
        "a window three samples overdue did not ask to roll immediately"
    );
}

#[test]
fn a_drained_window_stops_asking_to_age() {
    // **The bound, and the reason the amendment to I1 is one sentence rather
    // than a licence to run a timer.** A burst arms the clock; the window empties
    // `HISTORY_WINDOW` after it, and the clock has to stop with it or the budget
    // is *some wakeups forever* rather than *at most `HISTORY_SAMPLES` after a
    // burst*.
    // `starting_at`, not `new`: `new` opens the window at its own
    // `Instant::now()`, so a second sample taken on the next line leaves
    // `opened < start` and every offset below is measured against a boundary
    // that has already moved.
    let start = base();
    let mut history = History::starting_at(start);
    history.record_sized([("src/a.rs", Some(4_000u64))], start);

    // One sample short of the window it is still live, which is what makes the
    // assertion below a boundary rather than an eventual truth.
    history.record_sized([], start + HISTORY_WINDOW - HISTORY_SAMPLE);
    assert!(
        history
            .ages_in(start + HISTORY_WINDOW - HISTORY_SAMPLE)
            .is_some(),
        "the window went quiet a sample early, so the graph stops moving before \
         it has finished draining"
    );

    history.record_sized([], start + HISTORY_WINDOW);
    assert_eq!(
        history.ages_in(start + HISTORY_WINDOW),
        None,
        "a drained window is still asking to be woken, so the clock outlives \
         everything it had to show"
    );

    // **And it stays stopped across an ageing wake, which is the property this
    // was written for.** Asking `ages_in` a second time does not test it:
    // the method takes `&self`, so with nothing mutating between the two calls
    // the answer cannot differ and the line could not fail. What can re-arm the
    // clock is a roll, so the roll has to happen first.
    history.record_sized([], start + HISTORY_WINDOW * 2);
    assert_eq!(
        history.ages_in(start + HISTORY_WINDOW * 2),
        None,
        "an ageing wake on a drained window armed the clock again, so a burst \
         two minutes gone still costs a wake every second, forever"
    );
}

#[test]
fn a_tick_that_moves_nothing_does_no_work() {
    // **The guard [#277](https://github.com/breferrari/vigia/issues/277) needed
    // once it put `record_sized` on the shell loop's timeout arm.** That arm also
    // fires every `STEP_REPEAT` while a scrollbar button is held, twenty times a
    // second, and nineteen of those cross no sample boundary. Without the guard
    // each one paid `repeak`, priced in its own docblock at 144µs mean and 191µs
    // worst at the path cap, to recompute an answer that cannot have changed.
    // Measured at the cap after it: a same-sample timeout is **20ns** against
    // **137µs** for one that crosses a boundary.
    //
    // **Asserted on `repeaks` rather than on the projection**, and the first
    // draft of this test got that wrong in the way that matters: `repeak` is
    // idempotent over unchanged tracks, so `scales` and the worktree series are
    // identical whether the walk ran or not. Reverting the guard to an
    // unconditional call left every assertion green, which made this a gate that
    // the skip is *safe* and never that it *happens*. The counter is the only
    // observable that separates the two.
    // `starting_at`, not `new`: `new` opens the window at its own
    // `Instant::now()`, so a second sample taken on the next line leaves
    // `opened < start` and every offset below is measured against a boundary
    // that has already moved. This test had the smallest margin of the three:
    // it asks about `start + HISTORY_SAMPLE / 2`, so 500ms of wall clock between
    // two statements was enough to cross a boundary and redden it.
    let start = base();
    let mut history = History::starting_at(start);
    history.record_sized([("src/a.rs", Some(4_000u64))], start);

    // Kept for the `assert_ne!` further down, which is the live half of this
    // pair: the deleted `assert_eq!` above it could not fail, and proving the
    // walk *does* happen when it should is a different claim from proving it
    // does not when it should not.
    let churn = history.worktree_churn();
    let walked = history.stats().repeaks;

    // Inside the same sample, naming nothing: the wake a held button produces
    // twenty times a second.
    history.record_sized([], start + HISTORY_SAMPLE / 2);
    assert_eq!(
        history.stats().repeaks,
        walked,
        "a timeout inside one sample walked every track, which is the 144µs a \
         held scrollbar button would pay for it nineteen times a second"
    );
    // **Read through `recency`, which is sample-granular, rather than through
    // `scales` and `worktree_churn`, which `repeak` caches.** The cached pair
    // cannot move while the counter above says no walk happened, so comparing
    // them here was a line that could not fail while its predecessor passed.
    //
    // `churn` was the first replacement and it is not enough either:
    // [`HISTORY_SAMPLES`] over [`HISTORY_BUCKETS`] is five samples to a bucket,
    // so a window that shifted by one and reported no steps stays inside the same
    // bucket four times in five and the projection does not move. `recency`
    // reads the newest sample itself, which is the cell such a shift zeroes.
    assert_eq!(
        history.recency("src/a.rs"),
        Recency::Pulse,
        "a timeout inside one sample moved the window anyway, so a path lost the \
         mark it had just earned and the skipped walk was hiding a real roll"
    );

    // **Both halves of the guard, or it is half a guard.** A tick that names a
    // path inside the same sample changed a track without moving the window, and
    // skipping the walk there would freeze the projection against live data.
    history.record_sized([("src/a.rs", Some(9_000u64))], start + HISTORY_SAMPLE / 2);
    assert_eq!(
        history.stats().repeaks,
        walked + 1,
        "a write inside the current sample skipped the walk, so the projection is \
         frozen against live data"
    );
    assert_ne!(
        history.worktree_churn(),
        churn,
        "a write inside the current sample did not reach the projection, so the \
         guard skipped a walk that had work to do"
    );

    // And crossing a boundary with nothing named still rolls.
    let rolled = history.worktree_churn();
    history.record_sized([], start + HISTORY_SAMPLE * 2);
    assert_ne!(
        history.worktree_churn(),
        rolled,
        "a timeout that crossed a sample boundary left the window where it was, \
         which is the freeze #243 exists to fix"
    );
}

#[test]
fn a_burst_lands_in_the_sample_the_roll_opened() {
    // **Order inside `record_sized`: the window rolls, and *then* the burst is
    // written.** Swap those and the write lands in the sample the roll is about
    // to shift out of, so it moves one cell left the instant it arrives and the
    // newest sample is zero: the path is `Live` on the frame it was written on,
    // and every drawn cell is one sample stale forever after.
    //
    // Gated here because nothing else reaches it. Reversing the two is caught
    // today by exactly one assertion, in
    // `a_path_that_ages_out_is_dropped_rather_than_kept_empty`, and only because
    // that fixture jumps a whole `HISTORY_WINDOW` and takes `roll`'s *clear*
    // branch, which wipes the just-written track. Its failure message then talks
    // about a path occupying a slot, which is not what happened. The ordinary
    // shift path is the production case and had no gate at all: the fixtures near
    // it assert totals, and a total is exactly what moving a sample sideways
    // preserves.
    let start = base();
    let mut history = History::starting_at(start);
    history.record(["src/a.rs"], start);

    // One boundary, so `roll` takes the shift branch rather than the clear.
    history.record(["src/a.rs"], start + HISTORY_SAMPLE);
    assert_eq!(
        history.recency("src/a.rs"),
        Recency::Pulse,
        "the write landed in the sample the roll then shifted out of, so a file \
         does not pulse on the frame it was written on"
    );
}

#[test]
fn a_write_after_the_whole_window_turned_over_still_accumulates() {
    // **The overnight case, and the branch that serves it is the one branch that
    // re-bases the window's origin.** `roll` clears every track when the whole
    // window has turned over rather than shifting each one past its own length,
    // and it has to move `opened` to `now` on the way out. Leave that line off
    // and the store still *looks* right for one call: the clear happens, the
    // window reads empty, and the next write lands. It is the call after that
    // one which never recovers, because `opened` is still pointing at whenever
    // the session began, so every later roll measures the same overnight gap and
    // clears the store again. The graph can then never hold more than the burst
    // being written at that instant, forever, on a monitor whose whole job is to
    // be left open for days.
    //
    // Nothing else here reaches it: every other gate over the drain asserts what
    // the window looks like *after* the turnover and stops, and the defect only
    // shows on the second roll past it.
    let start = base();
    let mut history = History::starting_at(start);
    history.record(["src/a.rs"], start);

    // Left open overnight. The window has turned over many times and holds
    // nothing, which is the state the branch exists for.
    let woke = start + HISTORY_WINDOW * 3;
    history.record_sized([], woke);
    assert_eq!(history.recency("src/a.rs"), Recency::Cold);
    assert_eq!(history.tracked(), 0, "the drained window kept a track");

    // A write after that turnover is an ordinary write and has to behave like
    // one.
    history.record(["src/b.rs"], woke);
    assert_eq!(history.recency("src/b.rs"), Recency::Pulse);

    // And it has to still be there one sample later. This is the assertion the
    // missing re-base fails: with `opened` left behind, this roll measures the
    // overnight gap a second time and clears the write above.
    history.record_sized([], woke + HISTORY_SAMPLE);
    assert_eq!(
        history.recency("src/b.rs"),
        Recency::Live,
        "a write made after the window drained was cleared by the next roll, so \
         the window re-measures the same overnight gap forever and can never \
         hold more than the instant being written"
    );
}

#[test]
fn the_pulse_ages_with_the_window_it_is_drawn_beside() {
    // **[#243](https://github.com/breferrari/vigia/issues/243) one field over.**
    // The mark was the burst ordinal alone, which only advances when a burst
    // names something, so an empty roll left it lit: a file written a hundred and
    // nineteen seconds ago drew at full brightness beside a band that had almost
    // drained, and then lost the mark all at once when its track was evicted.
    // Once the window ages on its own that is visible every second.
    let start = base();
    let mut history = History::starting_at(start);
    history.record(["src/a.rs"], start);
    assert_eq!(history.recency("src/a.rs"), Recency::Pulse);

    // One boundary is all it takes: `Track::shift` zeroes the newest sample, so
    // the mark expires by construction rather than by anything retiring it.
    history.record_sized([], start + HISTORY_SAMPLE);
    assert_eq!(
        history.recency("src/a.rs"),
        Recency::Live,
        "the pulse survived a roll, so a file that went quiet keeps claiming it \
         just wrote"
    );

    // **The rung above already carries "still tracked", so nothing asserts it
    // separately.** Two attempts did. The first asked `recency` again and
    // compared it to the same rung. The second asked `churn(..).is_some()`, on
    // the stated ground that "a `Live` that came from an eviction would read
    // identically", which is not true of this enum: `recency` returns `Cold` for
    // a path it cannot find and `Live` only for one it can, so `Live` is the
    // evidence and a second line restating it is a line that cannot fail.
    history.record_sized([], start + HISTORY_WINDOW * 2);
    assert_eq!(history.recency("src/a.rs"), Recency::Cold);
}
