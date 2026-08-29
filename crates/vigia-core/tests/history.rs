//! I10, as an invariant over the public store rather than over its arithmetic.

use std::time::Duration;
use std::time::Instant;

use vigia_core::{
    Churn, HISTORY_BUCKET, HISTORY_BUCKETS, HISTORY_PATHS, HISTORY_SAMPLE, HISTORY_SAMPLES,
    HISTORY_WINDOW, History, PULSE_SAMPLES, Recency, SPARK_GROUPS, scale_of,
};

/// Paths a bulk operation invents, well past the cap.
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

    // Every figure, not only the finest, and the band's series with them.
    assert_eq!(
        history.scales(),
        [0; SPARK_GROUPS.len()],
        "a rung's figure outlived its samples"
    );
    assert_eq!(
        history.worktree_churn(),
        Churn::default(),
        "the band's series survived a whole window of silence, so an emptied \
         store still draws the graph it had"
    );
}

/// The same claim, reached one bucket at a time.
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

/// A path that ages out is dropped, not drawn empty.
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
fn total(drawn: &[u32; HISTORY_BUCKETS]) -> u32 {
    drawn.iter().copied().sum()
}

/// A drawn column is the sum of the samples under it, not one of them.
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
    // The period the spec names, gated where it is computed.
    assert_eq!(HISTORY_BUCKET, Duration::from_secs(5));

    // And it tiles the window, which is what makes a drawn bucket mean the same
    // amount of time wherever it sits.
    assert_eq!(HISTORY_BUCKET * HISTORY_BUCKETS as u32, HISTORY_WINDOW);

    // The band half of this gate is gone with the constants it read
    // ([#232](https://github.com/breferrari/vigia/issues/232)).
}

/// The newest drawn bucket of a path, which is where a write just landed.
fn newest(history: &History, path: &str) -> u32 {
    *history
        .churn(path)
        .expect("the path is tracked")
        .last()
        .expect("a window has buckets")
}

/// A write weighs the bytes it moved, not the fact that it happened.
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
#[test]
fn a_write_that_shrinks_a_file_weighs_what_it_removed() {
    let now = base();
    let mut history = History::starting_at(now);

    history.record_sized([("src/a.rs", Some(5_000))], now);
    history.record_sized([("src/a.rs", Some(1_000))], now);

    assert_eq!(newest(&history, "src/a.rs"), 1 + 4_000);
}

/// A size that could not be read still counts the write.
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

    // Above every input by design, which is what stops a uniformly busy
    // worktree drawing as a solid block.
    assert!(scale_of([10, 10].into_iter()) > 10);

    // And it saturates rather than wrapping on a window full of ceilings.
    assert_eq!(scale_of([u32::MAX; 8].into_iter()), u32::MAX);
}

/// The multiple of the median a value may reach before it stops setting the
/// scale, restated rather than imported.
const OUTLIER: u32 = 10;

/// One loud value does not set the yardstick for the ones around it.
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

/// What the cut keeps, pinned by the figure it produces rather than by a floor.
#[test]
fn the_cut_keeps_the_bulk_and_drops_the_outlier() {
    // Three values, median 1, cut 10.
    assert_eq!(
        scale_of([1u32, 1, 1_000_000].into_iter()),
        1,
        "the outlier set the yardstick for the two values beside it"
    );

    // And one whose answer is mid-range, because the assertion above lands on the
    // arithmetic floor.
    let bulk = [1u32, 40, 40, 40, 40, 40, 40, 40, 2_000];
    assert_eq!(
        scale_of(bulk.into_iter()),
        45,
        "the cut is not taken at the median: a mean pivot answers 329 here and a \
         minimum pivot answers 1"
    );

    // A population of two never cuts, and that is the median's rounding rather than an
    // accident.
    assert!(
        scale_of([1u32, 1_000_000].into_iter()) > 500_000,
        "a population of two cut something, where the larger value is the median"
    );

    // A single value is its own median and its own mean.
    assert_eq!(
        scale_of([9u32].into_iter()),
        11,
        "one value, thirteen tenths"
    );

    // The empties are not in the population at all, so they cannot become the
    // median and drag the cut down to nothing.
    assert_eq!(
        scale_of([0u32, 0, 7].into_iter()),
        scale_of([7u32].into_iter()),
        "the empties reached the median"
    );
}

/// When the loud writes are the majority, they are the ordinary write.
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
#[test]
fn a_coarser_rung_is_never_measured_against_less() {
    let now = base();

    // One path and one write, which is the small population where a robust
    // statistic has the least to be robust about.
    let mut lone = History::starting_at(now);
    lone.record_sized([("src/lib.rs", Some(4_000))], now);
    lone.record_sized([("src/lib.rs", Some(28_000))], now);

    // And a populated worktree, where the population is large enough that the cut
    // would answer in order however it were taken. Both, so this is not a gate
    // about one fixture.
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

    // Below the sample count every sample's weight lands inside the columns that
    // cover it, so the total is conserved whether or not the width divides the
    // window.
    let exact: u32 = churn.projected(HISTORY_SAMPLES).iter().sum();
    assert_eq!(
        exact, 12,
        "a projection at the sample count lost or invented churn"
    );
    let narrow: u32 = churn.projected(10).iter().sum();
    assert_eq!(narrow, 12, "a narrower projection lost or invented churn");

    // The oldest value stays oldest however wide the ask, which is what stops a
    // wide pane drawing the window mirrored. Above the sample count a column is
    // a fraction of a sample rather than a whole repeated one, so what carries
    // the claim is which end the weight is at, not what it weighs there.
    let wide = churn.projected(HISTORY_SAMPLES * 2);
    let (head, tail) = (
        wide.first().copied().expect("a column"),
        wide.last().copied().expect("a column"),
    );
    assert!(
        head > 0 && tail > 0,
        "a wide projection lost both ends: {wide:?}"
    );
    assert!(
        head < tail,
        "the oldest sample weighs 5 and the newest 7, and they came out {head} \
         and {tail}, so a wide pane draws the window mirrored: {wide:?}"
    );
    assert!(
        wide[1..wide.len() - 1].iter().all(|value| *value <= tail),
        "a wide projection put weight where the window has none: {wide:?}"
    );
}

/// A deletion weighs what it removed, and the file after it weighs itself.
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

/// A store holding one agent burst centred `ago` seconds back.
fn burst_at(history: &mut History, now: Instant, ago: u64, grown: u64) -> u64 {
    // Cumulative sizes, because a sample weighs a difference.
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
fn roll_to(history: &mut History, now: Instant) {
    history.record_sized([("src/other/untouched.rs", Some(1))], now);
}

/// The drawn buckets rise to a peak and fall away from it.
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

    // Non-vacuity first: a wave needs more than one non-empty bucket, and a spike train
    // is exactly the case where it has one.
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
    // Each burst's own peak, found in its own half.
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

    // Overdue asks for zero rather than panicking, which is the ordinary
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
    // The bound, and the reason the amendment to I1 is one sentence rather than a
    // licence to run a timer.
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

    // And it stays stopped across an ageing wake, which is the property this was
    // written for.
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
        "a timeout inside one sample walked every track, which is the 150µs a \
         held scrollbar button would pay for it nineteen times a second"
    );
    // Read through `recency`, which is sample-granular, rather than through `scales`
    // and `worktree_churn`, which `repeak` caches.
    assert_eq!(
        history.recency("src/a.rs"),
        Recency::Pulse,
        "a timeout inside one sample moved the window anyway, so a path lost the \
         mark it had just earned and the skipped walk was hiding a real roll"
    );

    // Both halves of the guard, or it is half a guard. A tick that names a
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
    // Order inside `record_sized`: the window rolls, and *then* the burst is written.
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
    // The overnight case, and the branch that serves it is the one branch that re-bases
    // the window's origin.
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
        history.tracked(),
        1,
        "a write made after the window drained was cleared by the next roll, so \
         the window re-measures the same overnight gap forever and can never \
         hold more than the instant being written"
    );
    assert_ne!(history.recency("src/b.rs"), Recency::Cold);

    // And it ages the ordinary way from there, which is what says the re-based
    // window is a real window rather than one frozen at the turnover.
    history.record_sized([], woke + HISTORY_SAMPLE * PULSE_SAMPLES as u32);
    assert_eq!(history.recency("src/b.rs"), Recency::Live);
}

#[test]
fn the_pulse_ages_with_the_window_it_is_drawn_beside() {
    let start = base();
    let mut history = History::starting_at(start);
    history.record(["src/a.rs"], start);
    assert_eq!(history.recency("src/a.rs"), Recency::Pulse);

    // [`PULSE_SAMPLES`] boundaries, and the mark expires by construction rather
    // than by anything retiring it: `Track::shift` walks the write out of the
    // newest end of the track and the mark goes with it.
    for step in 1..PULSE_SAMPLES as u32 {
        history.record_sized([], start + HISTORY_SAMPLE * step);
        assert_eq!(
            history.recency("src/a.rs"),
            Recency::Pulse,
            "the mark went early, so a write landing late in a sample is one the \
             reader never catches"
        );
    }
    history.record_sized([], start + HISTORY_SAMPLE * PULSE_SAMPLES as u32);
    assert_eq!(
        history.recency("src/a.rs"),
        Recency::Live,
        "the pulse survived its window, so a file that went quiet keeps claiming \
         it just wrote"
    );

    // The rung above already carries "still tracked", so nothing asserts it separately.
    history.record_sized([], start + HISTORY_WINDOW * 2);
    assert_eq!(history.recency("src/a.rs"), Recency::Cold);
}

/// The pulse must last long enough to be seen, whenever the write lands.
#[test]
fn a_pulse_lasts_long_enough_to_be_seen_wherever_in_the_sample_it_landed() {
    // Every corner of the grid, including the two that measured 10ms and 5ms.
    let offsets = [0, 1, 250, 500, 750, 900, 990, 999];
    for offset in offsets {
        let start = base();
        let wrote = start + Duration::from_millis(offset);
        let mut history = History::starting_at(start);
        history.record(["src/a.rs"], wrote);
        assert_eq!(
            history.recency("src/a.rs"),
            Recency::Pulse,
            "a write pulses on the frame it caused (+{offset}ms)"
        );

        // Roll the way `Shell::draw` does, in fine steps, and find the moment the
        // mark goes. Nothing else in the store may be touched: a second write
        // would take the pulse for itself, which is a different rule.
        let mut alive = None;
        for step in 1..4_000u32 {
            let now = wrote + Duration::from_millis(5 * u64::from(step));
            history.record_sized([], now);
            if history.recency("src/a.rs") != Recency::Pulse {
                alive = Some(now.duration_since(wrote));
                break;
            }
        }
        let alive = alive.expect("the mark expires rather than freezing");

        assert!(
            alive >= HISTORY_SAMPLE,
            "a write at +{offset}ms into the sample pulsed for only {alive:?}. \
             The mark is the whole of B2 and one this short is one a reader \
             never catches"
        );
        // The ceiling is written out rather than computed from [`PULSE_SAMPLES`], and
        // that is the whole difference between a gate and a tautology.
        assert!(
            alive <= Duration::from_secs(2),
            "a write at +{offset}ms into the sample pulsed for {alive:?}, which \
             is past the two seconds the ruling allows. A mark that outlives \
             what it describes is the frozen clock #243 removed"
        );
    }
}

/// And a newer burst still takes the mark from an older one immediately.
#[test]
fn a_newer_burst_takes_the_pulse_from_an_older_one_inside_the_same_sample() {
    let start = base();
    let mut history = History::starting_at(start);

    history.record(["src/a.rs"], start + Duration::from_millis(100));
    history.record(["src/b.rs"], start + Duration::from_millis(200));

    assert_eq!(
        history.recency("src/b.rs"),
        Recency::Pulse,
        "the newer burst"
    );
    assert_eq!(
        history.recency("src/a.rs"),
        Recency::Live,
        "the older one is still in the window and is no longer the newest"
    );

    // And across a boundary, where both writes are now in older samples: the
    // ordinal is what still separates them.
    history.record_sized([], start + HISTORY_SAMPLE + Duration::from_millis(300));
    assert_eq!(history.recency("src/b.rs"), Recency::Pulse);
    assert_eq!(history.recency("src/a.rs"), Recency::Live);
}

/// `SPEC.md` §11.1's `●`: the file the newest burst named, until another arrives.
#[test]
fn the_newest_mark_stays_on_the_last_written_file_until_another_is_written() {
    let start = base();
    let mut history = History::starting_at(start);
    let wrote = start + Duration::from_millis(1);
    history.record(["src/a.rs"], wrote);
    assert!(
        history.newest("src/a.rs"),
        "a write does not carry the mark at all"
    );

    // Ten seconds of quiet, rolled the way `Shell::draw` rolls it.
    let mut now = wrote;
    for _ in 0..2_000u32 {
        now += Duration::from_millis(5);
        history.record_sized([], now);
    }
    assert!(
        now.duration_since(wrote) >= Duration::from_secs(10),
        "the quiet was shorter than the ten seconds this gate is about"
    );
    assert!(
        history.newest("src/a.rs"),
        "the mark left the last written file after {:?} of quiet, and nothing had \
         been written since",
        now.duration_since(wrote)
    );

    // And the ink is gone by then, which is what says the two are separate.
    // A build that answered the mark out of `recency` would fail here rather than
    // above, and a build that never expired the ink would fail here too.
    assert_ne!(
        history.recency("src/a.rs"),
        Recency::Pulse,
        "the row is still drawn at full brightness after ten seconds of quiet, so \
         the mark and the ink have been rejoined"
    );

    // A second write takes the mark and the first file loses it.
    let later = now + Duration::from_millis(5);
    history.record(["src/b.rs"], later);
    assert!(
        history.newest("src/b.rs"),
        "a later write did not take the mark"
    );
    assert!(
        !history.newest("src/a.rs"),
        "the earlier file kept the mark after a later write took it"
    );
}

/// Every file one burst names carries the mark, which is what *newest* means.
#[test]
fn every_file_a_burst_names_carries_the_newest_mark() {
    let start = base();
    let mut history = History::starting_at(start);
    let wrote = start + Duration::from_millis(1);
    history.record(["src/a.rs", "src/b.rs", "src/c.rs"], wrote);

    for path in ["src/a.rs", "src/b.rs", "src/c.rs"] {
        assert!(
            history.newest(path),
            "{path} was in the burst and carries no mark"
        );
    }
    assert!(
        !history.newest("src/never.rs"),
        "a path nothing is tracked for carries the mark"
    );
}

/// And the mark's real bound is I10's, which is not eternity.
#[test]
fn the_newest_mark_goes_when_the_window_it_lives_in_does() {
    let start = base();
    let mut history = History::starting_at(start);
    let wrote = start + Duration::from_millis(1);
    history.record(["src/a.rs"], wrote);
    assert!(
        history.newest("src/a.rs"),
        "the write carries no mark to lose"
    );

    // Past the window, rolled the way the shell rolls it and never writing again.
    let mut now = wrote;
    while now.duration_since(wrote) <= HISTORY_WINDOW + Duration::from_secs(2) {
        now += Duration::from_millis(50);
        history.record_sized([], now);
    }
    assert!(
        !history.newest("src/a.rs"),
        "the mark outlived the window it is drawn from, which is the frozen clock \
         a bounded history exists to refuse"
    );
}

/// One write in an empty window, as a series rather than through a store.
fn lone_write(bytes: u32) -> Churn {
    let mut samples = [0u32; HISTORY_SAMPLES];
    samples[HISTORY_SAMPLES / 2] = bytes;
    Churn(samples)
}

/// Samples of a level that carry anything at all.
fn lit(churn: &Churn) -> usize {
    churn
        .levels(HISTORY_SAMPLES)
        .iter()
        .filter(|level| **level > 0)
        .count()
}

/// A level says *when* a write happened. Its width must not say *how much*.
#[test]
fn a_levels_reach_is_the_kernels_rather_than_the_writes() {
    // Named rather than counted, and spanning six orders of magnitude: a kernel
    // whose reach follows the write's size holds this at any single size and
    // fails across them, so one size proves nothing.
    let sizes = [1u32, 100, 9_000, 127_000, 5_000_000];
    let widths: Vec<usize> = sizes.iter().map(|bytes| lit(&lone_write(*bytes))).collect();

    // At every width a pane produces, not only at the sample count, where the
    // projection is the identity and cannot lose the small end of a level. Both
    // rungs of the glyph ladder, so the dense one's doubled sub-columns are here.
    for width in [46usize, 60, 80, 109, 134, 218, 268] {
        let drawn: Vec<usize> = sizes
            .iter()
            .map(|bytes| {
                lone_write(*bytes)
                    .levels(width)
                    .iter()
                    .filter(|level| **level > 0)
                    .count()
            })
            .collect();
        assert!(
            drawn.iter().all(|lit| *lit == drawn[0]),
            "projected onto {width} columns the same kernel drew {drawn:?} for {sizes:?} bytes, so the write's size is back in its drawn width"
        );
    }

    let first = widths[0];
    assert!(
        first > 1,
        "a write of one byte, which is what `bump`'s floor weighs, lit {first} \
         sample of the window, so the level is a mark rather than a level"
    );
    assert!(
        widths.iter().all(|width| *width == first),
        "the same kernel drew {widths:?} samples for {sizes:?} bytes, so a \
         level's width reports the size of the write rather than when it landed"
    );

    // And it reaches equally both ways, which the count above cannot see: a
    // kernel bounded on one side draws the same number of samples for every
    // magnitude too, and leans every one of them to one side of the write.
    let at = HISTORY_SAMPLES / 2;
    let levels = lone_write(9_000).levels(HISTORY_SAMPLES);
    let span: Vec<usize> = levels
        .iter()
        .enumerate()
        .filter(|(_, level)| **level > 0)
        .map(|(sample, _)| sample)
        .collect();
    let (back, forward) = (at - span[0], span[span.len() - 1] - at);
    assert_eq!(
        back, forward,
        "the level reaches {back} back from the write and {forward} forward, so the kernel is bounded on one side: {levels:?}"
    );
}

/// The axis is the half of the band a flooded window cannot draw.
#[test]
fn a_large_burst_leaves_both_ends_of_the_window_on_the_axis() {
    // Sizes a formatter, a lockfile rewrite or a generated file reaches on an
    // ordinary afternoon, which is where §11.1's floor has the most to lose:
    // a level reaching the window's ends leaves no quiet stretch to draw.
    for bytes in [127_000u32, 500_000, 5_000_000] {
        let levels = lone_write(bytes).levels(HISTORY_SAMPLES);
        assert_eq!(
            (levels[0], levels[HISTORY_SAMPLES - 1]),
            (0, 0),
            "a single write of {bytes} bytes in the middle of the window lit \
             both far ends, so a quiet stretch has no axis to draw: {levels:?}"
        );
    }
}

/// The ordinary case: a write whose size did not move weighs `bump`'s floor.
#[test]
fn a_floor_weight_write_draws_a_shape_rather_than_a_mark() {
    let now = base();
    let mut history = History::starting_at(now);
    // Twice at the same size, so the second write's weight is the floor and
    // nothing else. This is what every path's first write in the window weighs
    // too, which is why it is the ordinary case rather than an edge.
    history.record_sized([("src/a.rs", Some(4_096))], now);
    history.record_sized([("src/a.rs", Some(4_096))], now + HISTORY_SAMPLE);

    let levels = history.worktree_churn().levels(HISTORY_SAMPLES);
    let mut distinct: Vec<u32> = levels.iter().copied().filter(|level| *level > 0).collect();
    distinct.sort_unstable();
    distinct.dedup();

    assert!(
        distinct.len() > 1,
        "a floor-weight write levelled to {} distinct value(s), so every \
         non-empty column is the same height, the yardstick equals it, and the \
         band has only the axis and the ceiling to draw: {levels:?}",
        distinct.len()
    );
}

/// Every drawn column has to cover the same slice of the window.
#[test]
fn a_projection_covers_the_same_span_in_every_column() {
    // A signal that never moves. Anything but a level projection here is the
    // arithmetic rather than the data.
    let flat = Churn([600; HISTORY_SAMPLES]);

    // Named individually: none of them divides `HISTORY_SAMPLES`, which is the
    // only case where whole-sample columns come out uneven, and a width that
    // divides it would pass against the shape this gate exists to refuse.
    for width in [46usize, 80, 109, 134] {
        assert!(
            HISTORY_SAMPLES % width != 0,
            "{width} divides the window, so it cannot tell an even projection \
             from an uneven one"
        );
        let drawn = flat.projected(width);
        let (low, high) = (
            drawn.iter().min().copied().expect("a column"),
            drawn.iter().max().copied().expect("a column"),
        );
        assert_eq!(
            low, high,
            "a series that never moved projected onto {width} columns between \
             {low} and {high}, so neighbouring columns carry different amounts \
             of time and a steady worktree draws as a comb: {drawn:?}"
        );
    }
}

/// A level is carried in a fraction of a byte, so it saturates far below what a
/// sample can hold. Pinned rather than described, because the threshold moves
/// with the unit and with the kernel's own decay, and nothing else would notice.
#[test]
fn a_level_saturates_rather_than_wrapping_and_keeps_its_reach() {
    // Bracketing the measured threshold at the position tested, so the pair
    // reddens whether the unit rises or falls.
    let under = lone_write(200_000_000);
    assert!(
        under
            .levels(HISTORY_SAMPLES)
            .iter()
            .all(|level| *level < u32::MAX),
        "two hundred megabytes in one sample already saturates, so the store represents less of a burst than it did"
    );

    // Past it, the level pins at the ceiling rather than wrapping to nothing,
    // and the reach is still the kernel's.
    let over = lone_write(210_000_000);
    let levels = over.levels(HISTORY_SAMPLES);
    assert!(
        levels.contains(&u32::MAX),
        "the largest sample a store can hold did not reach the ceiling, so the arithmetic wrapped somewhere: {levels:?}"
    );
    assert_eq!(
        lit(&over),
        lit(&lone_write(9_000)),
        "a saturating write drew a different width from an ordinary one, so saturation costs the reach as well as the magnitude"
    );
}
