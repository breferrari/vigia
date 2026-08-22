//! Glanceability history: I10.
//!
//! > Churn history is bounded by a fixed window and a fixed cap on tracked
//! > paths, independent of how many files the session changed. A path that ages
//! > out of the window is dropped entirely.
//!
//! `SPEC.md` §5 makes shape and colour the whole differentiator, and three of
//! the things it asks for are the same question asked at different resolutions:
//! *when did this file last change, and how busy has it been.* A churn
//! sparkline, the recency gradient that dims a settled row, and the
//! `●` pulse all read from here.
//!
//! ## Why this cannot live in the frame path
//!
//! [`Frame`](crate::Frame) drops a diff the moment its path stops being
//! changed, and `FrameStats.evicted` exists to prove it: that is how I3 is
//! argued, because the map is then bounded by the current diff rather than by
//! everything ever edited. History needs the opposite. *What was hot thirty
//! seconds ago* is the entire question a sparkline answers, so it has to
//! survive a file settling, and a store that emptied when a file went quiet
//! would show nothing worth glancing at.
//!
//! Surviving is not the same as growing without limit, which is what I10 is
//! for. Two rules bound it, and both are tested:
//!
//! * **By window.** A path with no sample left inside [`HISTORY_WINDOW`] is
//!   dropped entirely, not merely drawn as empty.
//! * **By cap.** At [`HISTORY_PATHS`] the least recently changed path is
//!   evicted to make room. A bulk operation touching ten thousand files must
//!   not grow this past the cap, which is the case the gate in
//!   `tests/history.rs` actually drives rather than approximating.
//!
//! ## The clock runs while the window holds something, and stops when it does not
//!
//! `SPEC.md` §5.1 asks the pulse to persist and decay, and the dimmed row to
//! fade as its last change ages. Both need a redraw to be *seen*, and this
//! module was built on the reading that **I1 forbids inventing a timer to get
//! one**, so the window was real time while the *sampling* was not:
//! [`History::record`] ran once per coalesced tick and nothing on screen changed
//! without an event.
//!
//! **[#243](https://github.com/breferrari/vigia/issues/243) reversed that on
//! 2026-08-22, and the paragraph it replaced is worth keeping in view because
//! its conclusion was the defect.** *A tree that has gone quiet holds its last
//! picture* is not what a monitor is supposed to do when the picture's own axis
//! is time: a frozen window keeps its newest sample at the right edge, so a
//! burst from ninety seconds ago draws as *just now*. The window ages on a clock
//! now, and I1's licence is what permits it rather than what forbids it, because
//! the clock stops: [`History::ages_in`] answers `None` once no track is left,
//! which is at most `HISTORY_WINDOW` after the last write. At most, because the
//! samples sit on a fixed grid anchored on the window's origin rather than on
//! the write, so a track can drain up to one `HISTORY_SAMPLE` sooner. That is
//! the safe direction: it stops the clock earlier, never later.
//!
//! The top rung of the ladder is still **not** a duration, and it gained the
//! half that makes it age. [`Recency::Pulse`] means *named by the most recent
//! tick, and not yet rolled past*: the ordinal marks **every** path in that
//! tick rather than one, which is `SPEC.md` §11.2 B2's ruling arriving from the
//! other side, and the newest sample being non-zero is what retires it. The
//! ordinal alone could not, because it only advances when a burst names
//! something, so a quiet window left every pulse lit.
//!
//! ## One mechanism, not three
//!
//! §5.1 rules that the dimmed row and the pulse are one
//! mechanism, because specifying them separately would produce two decay clocks
//! that disagree on screen. They are three rungs of [`Recency`], read from one
//! store through one lookup, and [`Recency::Cold`] is not a fourth rule: it is
//! what I10's own eviction leaves behind.

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// How far back a churn sparkline reaches.
///
/// Two minutes. Long enough that a file which went quiet is still visibly
/// warmer than one nobody has touched, short enough that the whole strip turns
/// over while a reader is still working on the same thing. It is also the
/// gradient's only boundary: a path with a sample in this window is
/// [`Recency::Live`] and one without is [`Recency::Cold`], so there is one
/// number here rather than two that could disagree.
pub const HISTORY_WINDOW: Duration = Duration::from_secs(120);

/// The resolution a sparkline is projected **from**, oldest bucket first.
///
/// **The source rather than the drawn count, since
/// [#234](https://github.com/breferrari/vigia/issues/234)**, which is what
/// `HEAT_BUCKETS` has been for the heat strip since
/// [#161](https://github.com/breferrari/vigia/issues/161). The shell's
/// `SPARK_RUNGS` halves this on the way down and sums adjacent buckets onto the
/// rung a pane affords, so a narrow row draws the whole window at a lower
/// resolution rather than the newest slice of it. `SPEC.md` §11.1 carries that
/// ruling and the one it amends.
///
/// **Twenty-four, and it is the only count available.** The rungs have to halve,
/// or a strip shedding one bucket at a time leaves widths where the picture is
/// neither the window nor an obvious fraction of it; and twelve has to stay on
/// the ladder, because twelve is what every pane wide enough for a sparkline and
/// narrower than the new top rung draws today. Of the divisions of
/// [`HISTORY_SAMPLES`] above twelve, only twenty-four gives `24 → 12 → 6`.
///
/// **What a finer source buys is columns rather than a finer period, which is
/// the opposite of what the row asking for it expected.** Measured over forty
/// seeded series in three work patterns, twelve buckets of five seconds draws
/// *fewer* distinct heights than twelve of ten: 5.25 against 5.75 on a steady
/// worktree, 6.20 against 6.35 on a bursty one. Twenty-four of five draws 6.72
/// and 7.40. So [#223](https://github.com/breferrari/vigia/issues/223)'s
/// "distinct heights peak near four to five seconds" was taken with the column
/// count moving with the period, and it is the columns the shape follows.
///
/// **This is what one element *draws from*, and since
/// [#198](https://github.com/breferrari/vigia/issues/198) it is no longer what
/// the store *samples*.** The two were the same number for two phases, and the
/// reason was a sampling rate chosen from one element's column count.
/// [`HISTORY_SAMPLES`] is the sampling rate, and [`History::churn`] projects one
/// onto the other.
///
/// **Still not an I10 change.** That row bounds the window and the path cap, and
/// how finely the window is divided sits underneath both. The store's sample
/// rate is untouched at 120, and twenty-four divides it exactly.
pub const HISTORY_BUCKETS: usize = 24;

// Eight until 2026-08-18, then twelve
// ([#161](https://github.com/breferrari/vigia/issues/161)), then twenty-four
// ([#234](https://github.com/breferrari/vigia/issues/234)), and the three steps
// were argued from three different things. Eight was one element's column count.
// Twelve was a period: a fifteen-second bucket is coarse enough that a steady
// worktree drew five and a half of the ramp's nine rungs, which is a block rather
// than a shape, and ten seconds took it to 6.5 without crossing into scatter.
// Twenty-four is neither, because a *rung* has no single period: it is the widest
// division the ladder can halve twice through twelve.
//
// **Twelve rather than fifteen was ruled on the band's floor, and that floor was
// retired the same day by #232.** What outlived it is the halving requirement,
// which is why fifteen is still not available and twenty-four is.

/// Source buckets one **drawn** bucket may cover, finest first.
///
/// **The halving ladder the shell narrows on, seen from the store's side**
/// ([#234](https://github.com/breferrari/vigia/issues/234)). A row draws
/// `HISTORY_BUCKETS / group` buckets, each the sum of `group` source ones, so
/// `render::SPARK_RUNGS` is **computed from this table** rather than written out
/// beside it, so the two cannot disagree. That its order is strictly ascending is
/// asserted in **both** crates, at each reliance rather than once: a `const` block
/// beside `SPARK_RUNGS` says a reordering would hand the widest pane the
/// narrowest rung, and the block below says the figures [`History::scales`]
/// answers with are claimed non-decreasing down this table, which is a claim
/// about its order.
///
/// **It lives here because what it decides is a *scale*, not a width.** A drawn
/// bucket summing four source buckets has to be measured against what four
/// source buckets are worth, and [`scale_of`] is not linear in the grouping: it
/// averages the **non-empty** values, and grouping merges empties into their
/// neighbours. Multiplying one figure by the group is therefore an estimate that
/// is exact on a busy worktree and wrong by up to the group on a quiet one, which
/// is the state this element exists to report. Measured before it was believed:
/// on a sparse fixture the estimate moved 59 of 2400 cells and the worst moved by
/// **four rungs of the eight-level ramp**. So [`History`] computes one figure per
/// grouping, in the walk it already makes.
pub const SPARK_GROUPS: [usize; 3] = [1, 2, 4];

const _: () = {
    let mut at = 0;
    while at < SPARK_GROUPS.len() {
        assert!(
            HISTORY_BUCKETS % SPARK_GROUPS[at] == 0,
            "a sparkline grouping does not divide the source resolution, so a \
             drawn bucket would cover less time than its neighbours"
        );
        // **Ascending, which `History::repeak`'s running maximum reads as an
        // order.** That step keeps a coarser rung's yardstick from falling below
        // a finer one's, and it walks this array in order to know which is
        // coarser. Listed out of order it would clamp the wrong way round and
        // silently invert the promise it exists to keep.
        assert!(
            at == 0 || SPARK_GROUPS[at - 1] < SPARK_GROUPS[at],
            "the sparkline groupings are not listed finest first"
        );
        at += 1;
    }
};

/// How much time one **source** bucket covers, which is the finest a rung draws.
///
/// **A rung's own period is this times the buckets it groups**, since
/// [#234](https://github.com/breferrari/vigia/issues/234): five seconds at the
/// widest rung, ten at the settled one, twenty at the narrowest. Every rung
/// covers the whole window, which is what makes this a resolution rather than a
/// span.
pub const HISTORY_BUCKET: Duration =
    Duration::from_nanos(HISTORY_WINDOW.as_nanos() as u64 / HISTORY_BUCKETS as u64);

// **`GRAPH_COLUMNS` and `GRAPH_PERIOD` were here and are retired**
// ([#232](https://github.com/breferrari/vigia/issues/232)). They fixed the
// band's period at fifteen columns of eight seconds, which
// [#223](https://github.com/breferrari/vigia/issues/223) tuned over forty seeded
// series. That row diagnosed the right defect, a save drawing a one-column
// hairline between two blanks, and reached for the wrong fix: the answer to the
// same defect on the same shape of signal is the **axis**, and with a
// floor under it a narrow column is a spike rather than a mark in a void. The
// band draws one value per sub-column now, so its period is a property of the
// pane and there is no constant to name.
//
// The measurement is not lost, only re-aimed: what it found is that the shape
// lives in the *distinct heights*, and those collapse when columns coarsen.
// Coarsening was the thing being tuned, and it is gone.
//
// **What that left open is closed by [#234](https://github.com/breferrari/vigia/issues/234),
// and the replacement is a bound rather than an absence.** `GRAPH_PERIOD` was
// what stopped a drawn bucket going finer than a band column; with it gone the
// two elements are still read side by side over one store, so what has to agree
// is the **window** they cover. The band covers all of it at every width, and
// since #234 so does the sparkline at every rung. `SPEC.md` §11.1 states it and
// `crates/vigia/tests/masthead.rs` gates the comparison the retired constant
// used to make.

/// Samples the store keeps per path, oldest first.
///
/// **The resolution the store records at, which is not the resolution any one
/// element draws at** ([#198](https://github.com/breferrari/vigia/issues/198)).
/// It was [`HISTORY_BUCKETS`] until then, so one element's column count decided
/// what the whole store could ever answer, and a worktree-wide graph across a wide
/// pane had eight points to draw whatever glyph drew them.
///
/// **Not an I10 change.** That row bounds 256 paths and a 120-second window: the
/// path cap and the window are the invariant, and how finely the window is divided
/// sits underneath it. Nothing here changes how long a sample lives or how many
/// paths are kept.
///
/// **One hundred and twenty, so a sample is exactly one second** and exactly
/// five of them make one source bucket. Both divisions are exact, which is what
/// keeps [`HISTORY_BUCKET`] an honest five seconds rather than a rounding of
/// one: a projection that did not divide evenly would make some drawn columns
/// cover more time than others, and a sparkline compared down a list would be
/// comparing unequal windows.
///
/// The cost is `paths * samples` `u16`s, which at the cap is a hundred and twenty kibibytes, and
/// a [`History::record`] walk fifteen times longer than it was. Both are measured
/// in the issue rather than asserted to be small.
///
/// **Public since [#158](https://github.com/breferrari/vigia/issues/158)**, which
/// is the second element and the one this resolution was raised for. It was kept
/// private while [`History::churn`] was the only reader, on the ground that
/// private is the reversible direction under semver and `pub` is not; the graph
/// draws the whole series across a whole pane, so it needs the length to project
/// against.
pub const HISTORY_SAMPLES: usize = 120;

/// How much time one **sample** covers, which is the grid the window rolls on.
///
/// **Public since [#243](https://github.com/breferrari/vigia/issues/243)**,
/// because it stopped being an internal grid and became a term in I1's budget:
/// the ageing clock wakes at most once per sample, and it is the *derivation*
/// that ruling rests on, since a sample is the finest interval at which any drawn
/// cell can change.
///
/// **The reason is one home for that derivation rather than reach.**
/// `HISTORY_WINDOW / HISTORY_SAMPLES` is literally this definition and both have
/// been public all along, so the export buys no new ability to name the rate;
/// what it buys is that the gates asserting the budget name the same term the
/// budget is written in, instead of restating its arithmetic.
pub const HISTORY_SAMPLE: Duration =
    Duration::from_nanos(HISTORY_WINDOW.as_nanos() as u64 / HISTORY_SAMPLES as u64);

/// How many samples one source bucket is the sum of.
///
/// **Exact, and asserted at compile time rather than by a test**, because a test
/// is the wrong instrument here twice over: the constants are private, so the
/// integration crate that would hold them cannot name them, and an inexact
/// division is not a behaviour to observe but a shape that must never build.
/// [`Track::drawn`] slices by this, so an inexact one would panic on the last
/// group rather than quietly dropping it, and the assertion below means neither
/// can happen.
///
/// The window's own tiling is asserted beside it for the same reason `roll`
/// needs it: `opened` advances by whole [`HISTORY_SAMPLE`]s, so a window that is
/// not a whole number of samples leaves a remainder no sample covers, which is
/// the argument `tests/history.rs` already makes one grid up for
/// [`HISTORY_BUCKET`].
const SAMPLES_PER_BUCKET: usize = HISTORY_SAMPLES / HISTORY_BUCKETS;

const _: () = {
    assert!(
        HISTORY_SAMPLES % HISTORY_BUCKETS == 0,
        "the samples do not divide into the source buckets, so a drawn column \
         would cover more time than its neighbours"
    );
    assert!(
        HISTORY_WINDOW.as_nanos() % HISTORY_SAMPLES as u128 == 0,
        "the samples do not tile the window, so a write can land in no sample"
    );
};

/// The most paths history will track at once.
///
/// The cap is the half of I10 that a window cannot provide: a bulk operation
/// puts ten thousand paths inside the window at the same instant, and without a
/// cap the store would be bounded by what the session did rather than by
/// anything fixed.
///
/// 256 is comfortably above any changed-file count a reader can look at (a
/// forty-row pane shows a few dozen) and small enough that the whole store is
/// tens of kilobytes against the ~26 MiB the soak measures. It is a bound, not
/// a tuning parameter: nothing is drawn from a path the reader cannot reach.
pub const HISTORY_PATHS: usize = 256;

/// How recently a path changed, as three rungs of one ladder.
///
/// Ordered by intensity rather than alphabetically, so the enum reads the way
/// the screen does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Recency {
    /// Named by the most recent tick **and** holding ink in the newest sample.
    /// Drawn brightest, and the only rung that carries the `●` mark.
    ///
    /// Both halves, since [#243](https://github.com/breferrari/vigia/issues/243).
    /// The ordinal alone only advances when a burst names something, so on a
    /// quiet worktree it left the mark lit: a file written two minutes ago drew
    /// at full brightness beside a band that had almost drained.
    ///
    /// Still not a duration, and the reason is no longer the one recorded here.
    /// This said a rung measured in seconds "would have to age while the event
    /// loop is blocked, and being able to see that happen needs a redraw I1
    /// forbids scheduling", which #243 reversed: I1's licence does reach that
    /// redraw, and the window ages on its own. What is left is the coherence
    /// argument. This rung means *there is ink in the newest sample*, so it is
    /// bounded by that sample and expires on the grid the elements beside it are
    /// drawn on. A duration of its own would be a second clock disagreeing with
    /// them about how long ago now was.
    Pulse,
    /// Changed inside [`HISTORY_WINDOW`], but not in the newest tick.
    Live,
    /// Nothing inside the window, so nothing is tracked for it at all.
    ///
    /// The ordinary state for a file that was edited before `vigia` started
    /// watching, which is the mockup's dimmed `Cargo.toml`.
    Cold,
}

/// What a [`History`] has done since it was created.
///
/// Cumulative, like [`FrameStats`](crate::FrameStats), so a test describes one
/// step by subtracting two readings.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HistoryStats {
    /// Path samples taken, counting a path once per tick that named it.
    pub recorded: u64,
    /// Paths dropped to stay under [`HISTORY_PATHS`].
    ///
    /// The number that separates a bound which *held* from one that was never
    /// approached. A gate asserting the cap without asserting this passes
    /// against a store nothing ever filled.
    pub evicted_by_cap: u64,
    /// Paths dropped because nothing was left inside [`HISTORY_WINDOW`].
    pub evicted_by_window: u64,
    /// Projections walked, which is the work
    /// [#243](https://github.com/breferrari/vigia/issues/243)'s clock made worth
    /// counting.
    ///
    /// **A counter because the saving is otherwise unobservable**, which is the
    /// same reason `evicted_by_cap` two fields up exists. `History::repeak` is
    /// idempotent over unchanged tracks, so skipping it and running it leave the
    /// store byte-identical: a gate over the *state* passes whether the guard is
    /// there or not, and the guard is what keeps a held scrollbar button from
    /// paying that walk twenty times a second. This is the only thing that can
    /// tell the two apart.
    pub repeaks: u64,
}

/// Every tracked path's churn added together, oldest sample first.
///
/// **A newtype rather than a bare array, and the reason is `Default`.** Std only
/// implements it for arrays up to thirty-two long, and [`HISTORY_SAMPLES`] is
/// longer, so a bare array would take `Default` away from every type that holds
/// one. That reaches further than it looks: the shell's `View` derives it, and
/// most of the fixtures in its suite are struct-update literals that would all
/// have to name a field they do not care about.
///
/// It also gives the series somewhere to say what it is. A bare `[u32; N]` on a
/// struct is a length and nothing else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Churn(pub [u32; HISTORY_SAMPLES]);

/// How far a write's weight is spread when the series is read as a **level**.
///
/// **The picture is the ruling and this is only its constant.**
/// `assets/preview.svg` draws every sparkline as a wave: its first row is
/// `4 8 14 20 24 16 8 4`, monotone up then monotone down, and `SPEC.md` §5.1
/// rules that a published artifact answering an open question is the answer. A
/// write is a **point event**, so the retained samples are zero almost
/// everywhere and drawing them raw is a spike train rather than a wave. Reported
/// from a live pane as *"spikes on a flat line"*
/// ([#242](https://github.com/breferrari/vigia/issues/242)).
///
/// **A causal decay is the obvious first idea and it is the wrong shape.** An
/// impulse rises instantly and falls slowly, which is a sawtooth: measured, it
/// draws `█▇▃▁` where the picture wants `▁▂▃▅▆█▅▂`. The picture rises *and*
/// falls, so the kernel has to look both ways, which is what reading a point
/// process as a density means.
///
/// **Six seconds, and the number is a trade rather than a taste.** A wider
/// kernel draws a prettier single burst and merges two nearby ones. Measured
/// against the aggregation these values are actually read through, a sum of five
/// smoothed samples per source bucket, two bursts thirty seconds apart leave a
/// trough at **0.23** of the peak here, 0.39 at eight seconds, 0.52 at ten and
/// 0.62 at twelve, where they stop being two things. Half is the line, and
/// `tests/history.rs::two_bursts_thirty_seconds_apart_still_read_as_two` is what
/// fails if this is raised past it.
///
/// **Re-derived when the source halved**
/// ([#234](https://github.com/breferrari/vigia/issues/234)) rather than carried
/// over, because every one of those numbers is read through a bucket. This
/// docblock and the gate's also used to quote *different* figures for eight
/// seconds, 0.54 against 0.40, having been taken from two fixtures; they are one
/// measurement on one fixture now. The constant did not move and its margin
/// improved: a finer bucket smears less, so six seconds is 0.23 where it was
/// 0.36.
///
/// **Derived twice, and the first derivation was wrong in a way worth keeping.**
/// Modelled against a *maximum* per bucket it read as eight seconds; the drawn
/// bucket is a **sum**, which smears further, so the same criterion lands two
/// seconds lower. A constant tuned against the wrong aggregation is a constant
/// tuned against nothing.
pub const HISTORY_LEVEL: Duration = Duration::from_secs(6);

/// [`HISTORY_LEVEL`] in samples, which is what the filter actually steps in.
const LEVEL_SAMPLES: f64 = HISTORY_LEVEL.as_nanos() as f64 / HISTORY_SAMPLE.as_nanos() as f64;

/// Sum a retained series into the source buckets a sparkline is drawn from.
///
/// Shared by [`Track::drawn`] and [`Track::levelled`] so the two differ only in
/// the series handed to them. Reconstructing a whole `Track` to reuse the loop
/// was the first shape and copied fields the sum never reads.
///
/// Sliced rather than zipped against `chunks`, which **truncates**: a division
/// that stopped being exact would silently drop the last group, and the last
/// group is the newest, so every fresh write would vanish from the screen with
/// nothing failing. Slicing panics instead, and the `const` assertion beside
/// [`SAMPLES_PER_BUCKET`] means it cannot.
fn bucketed(samples: &[u32; HISTORY_SAMPLES]) -> [u32; HISTORY_BUCKETS] {
    std::array::from_fn(|bucket| {
        samples[bucket * SAMPLES_PER_BUCKET..][..SAMPLES_PER_BUCKET]
            .iter()
            .copied()
            .fold(0, u32::saturating_add)
    })
}

/// Read a retained series as a level rather than as the events that made it.
///
/// A two-sided exponential kernel, `x[j] * a^|i-j|`, normalised so the total it
/// carries is the total it was given: the shape changes and the quantity does
/// not.
///
/// **Two passes rather than a convolution, and that is what makes it
/// affordable.** Written out, the kernel is `O(n·k)` and this runs on the frame
/// path for the band and once per drawn row for the sparklines. The same kernel
/// is a forward pass plus a backward pass, `O(n)`, because an exponential is the
/// one shape that is its own running sum. The centre sample is in both passes,
/// so it is subtracted once.
fn levelled(samples: &[u32; HISTORY_SAMPLES]) -> [u32; HISTORY_SAMPLES] {
    let decay = (-1.0 / LEVEL_SAMPLES).exp();

    // Two passes over the samples, and two more over a flat series of ones. The
    // second pair is the **weight actually available** at each position, and
    // dividing by it is what makes this a weighted average rather than a sum.
    //
    // **Without it a uniform window is not drawn uniform.** The kernel runs off
    // both ends of a fixed window, so the first and last samples see half of it
    // and droop: measured, a busy two minutes drew `▂▂▂▄▄▄…▄▄▄▂▂▂` where every
    // second of it was equally busy. The oldest end drooping is a curiosity; the
    // **newest** end drooping means the present under-reports itself, on the one
    // element a reader looks at to see whether anything is happening now.
    //
    // An earlier attempt divided by the whole kernel's weight everywhere, which
    // is the same thing done wrong: it flattened the interior instead of lifting
    // the edges, and that is why the first reading of this rejected the idea.
    let mut smoothed = [0.0f64; HISTORY_SAMPLES];
    let mut weight = [0.0f64; HISTORY_SAMPLES];

    let (mut value, mut mass) = (0.0, 0.0);
    for at in 0..HISTORY_SAMPLES {
        value = f64::from(samples[at]) + value * decay;
        mass = 1.0 + mass * decay;
        smoothed[at] = value;
        weight[at] = mass;
    }

    let mut out = [0u32; HISTORY_SAMPLES];
    let (mut value, mut mass) = (0.0, 0.0);
    for at in (0..HISTORY_SAMPLES).rev() {
        value = f64::from(samples[at]) + value * decay;
        mass = 1.0 + mass * decay;
        // The centre sample is in both passes, so it is counted once.
        let total = smoothed[at] + value - f64::from(samples[at]);
        let share = weight[at] + mass - 1.0;
        let level = if share > 0.0 { total / share } else { 0.0 };
        // **One is the floor where a write actually landed, and nowhere else.**
        //
        // `Track::wrote` gives every write at least one so that a store fed no
        // sizes behaves as it always did. A level divides by the kernel's own
        // weight, so that one becomes a fraction and rounds to nothing: a file
        // with a single recorded tick drew no bucket at all.
        //
        // **Gated on the sample rather than on the level, and the difference is
        // the whole element.** `exp(-1/6)` needs about four thousand steps to
        // underflow and the window is a hundred and twenty, so after any write at
        // all *every* sample is positive by a hair. A floor keyed on that lifts
        // the entire window to one: measured, a single burst sixty seconds back
        // drew `[10, 12, 58, 302, 1594, 8015, 5698, 1077, 204, 38, 10, 10]`, a
        // solid bar across a quiet worktree, which is the exact defect this
        // feature exists to remove, reintroduced by its own guard. Keyed on the
        // raw sample, the floor protects the write that was made and leaves the
        // tail to round away, so a quiet stretch is an axis again.
        let rounded = level.round().clamp(0.0, f64::from(u32::MAX));
        out[at] = if rounded < 1.0 && samples[at] > 0 {
            1
        } else {
            rounded as u32
        };
    }
    out
}

impl Default for Churn {
    fn default() -> Self {
        Self([0; HISTORY_SAMPLES])
    }
}

impl Churn {
    /// The series re-projected onto `width` columns, oldest first.
    ///
    /// **A projection re-projects; it does not drop items**, which is `heat_at`'s
    /// ruling in the shell and [`Track::drawn`]'s one crate over. Every column is
    /// the sum of the samples under it, so a narrow pane shows the same total
    /// churn at a lower resolution rather than a suffix of the window.
    ///
    /// The remainder is spread rather than dropped: with a width that does not
    /// divide [`HISTORY_SAMPLES`], the earlier columns take one extra sample
    /// each. Every sample lands in exactly one column, which is the property that
    /// matters at or below [`HISTORY_SAMPLES`], which is the range every caller
    /// but the churn band asks for.
    ///
    /// **Exactly `width` values, whatever `width` is.** This used to clamp to
    /// [`HISTORY_SAMPLES`] and hand back a short `Vec`, which was fine while
    /// every caller drew one column per cell and became a trap when the churn
    /// band started asking for one per braille sub-column: a pane wide enough
    /// wanted more columns than the window holds samples, got fewer, and drew a
    /// graph that stopped partway across and left bare axis after it. The caller
    /// then re-derived the mapping to work around it, so this formula existed
    /// twice, once forwards and once inverted.
    ///
    /// Past that point a value covers **less** than one sample and neighbouring
    /// columns repeat, which is the honest answer: the window holds what it holds
    /// and a wider pane cannot invent resolution. `.max(from + 1)` is what makes
    /// the span non-empty there, and it changes nothing at or below
    /// [`HISTORY_SAMPLES`], where the spans are already at least one wide.
    ///
    /// Saturating for [`Track::drawn`]'s reason: a column already at the top of
    /// the ramp must not wrap to the bottom of it.
    pub fn projected(&self, width: usize) -> Vec<u32> {
        if width == 0 {
            return Vec::new();
        }
        (0..width)
            .map(|column| {
                let from = column * HISTORY_SAMPLES / width;
                let to = ((column + 1) * HISTORY_SAMPLES / width).max(from + 1);
                self.0[from..to]
                    .iter()
                    .copied()
                    .fold(0u32, u32::saturating_add)
            })
            .collect()
    }

    /// The series read as a **level**, re-projected onto `width` columns.
    ///
    /// [`Churn::projected`] answers *what was written when*, which is what the
    /// store retains and what every gate over it asserts. This answers *how busy
    /// the worktree was around then*, which is what the picture draws, and it is
    /// a separate method for exactly that reason: a level is a presentation of
    /// the events rather than a correction to them, and folding it into the
    /// projection would change what a drawn column means underneath ten gates
    /// that are right.
    ///
    /// See [`levelled`] for the kernel and [`HISTORY_LEVEL`] for its constant.
    pub fn levels(&self, width: usize) -> Vec<u32> {
        Churn(levelled(&self.0)).projected(width)
    }
}

/// What a churn height is measured against: above the ordinary write, not at the
/// largest one, and not dragged up by a burst either.
///
/// **Not a maximum, and the reason is what a maximum does to everything else.**
/// One `cargo build` rewriting a lock file is two orders of magnitude above an
/// ordinary save, and against that denominator every edit a reader makes for the
/// next two minutes draws one level high: a graph that goes blank because
/// something interesting happened
/// ([#232](https://github.com/breferrari/vigia/issues/232)). Scaling against the
/// ordinary case and letting the outlier saturate keeps the shape a reader is
/// watching for.
///
/// **The mean is over the non-empty values only.** A worktree is idle most of the
/// time, so counting the zeroes would make the denominator a measure of how long
/// the reader has been away rather than of how large a write is.
///
/// **And it is over the non-outlying values only**, which is
/// [#256](https://github.com/breferrari/vigia/issues/256) and is what
/// [`SCALE_OUTLIER`] decides. A mean is not robust, so on a heavy-tailed window
/// the outlier this rule exists to let saturate was setting the denominator from
/// inside it.
///
/// **Shared by both glance elements, which is why it lives here.** The churn band
/// and the per-file sparkline divide by different quantities, worktree-wide and
/// per-file, and that asymmetry is `SPEC.md` §11.1's ruling. What they must not
/// disagree about is the *rule*, and it existed in the renderer for one of them
/// before this: the band was fixed where the symptom was reported and the
/// sparkline kept dividing by a maximum over the same byte samples.
///
/// **This entry point materialises the non-empty values, and it used to be
/// written so it need not.** A median cannot be taken from a running pair, so the
/// iterator is collected here. The caller that sentence was written for is
/// [`History::repeak`], which walks every bucket of every tracked path on the
/// frame path; it no longer comes through here at all, but through
/// [`scale_of_busy`] over a scratch it keeps between ticks. What is left on this
/// path is one drawn series, which the band already holds as a `Vec` anyway.
///
/// Zero when nothing is in the window at all, which every caller reads as "no
/// scale yet".
pub fn scale_of(values: impl Iterator<Item = u32>) -> u32 {
    let mut busy: Vec<u32> = values.filter(|value| *value > 0).collect();
    scale_of_busy(&mut busy)
}

/// How many times the median a value may be before it stops setting the scale.
///
/// **The estimator was not robust, and one loud write was enough**
/// ([#256](https://github.com/breferrari/vigia/issues/256), reported from a live
/// pane). A test run rewriting 3,400 bytes sits in the same window as the
/// 180-byte edits around it; the mean that sets the denominator is dragged up by
/// the burst, and every ordinary edit lands on the lowest of the band's sixteen
/// levels, which at the baseline row is `▁` and is not distinguishable from the
/// axis at a glance. Measured at 80 columns on a trace shaped like the report:
/// **36 of 76 columns pinned there, 7 distinct heights**, against 0 and 13 with
/// this cut.
///
/// **A fixed window is what makes a robust statistic necessary rather than
/// optional.** An estimator that follows the present can let a burst age out of
/// its own reckoning. I10 fixes this window at two minutes and the sparkline's
/// denominator spans every tracked path so rows stay comparable, so there is no
/// present here to follow: the whole window is in the estimate at once, and the
/// only way to keep a minority of loud values out of it is to exclude them.
///
/// **Ten, and the interval it sits in the middle of was measured** rather than
/// chosen. Sweeping this constant from 2 to 40 at five pane widths over ten
/// fixtures gives a safe band of `[8, 12]`, and both ends bind on a named
/// fixture:
///
/// - Below **8**, a window with a legitimate dynamic range stops drawing what it
///   drew before. `masthead.rs`'s `QUARTERED`, a deliberate 4:1 series, is the
///   binding one and moves at 7 at 60 and 109 columns.
/// - Above **12**, the floor comes back on a burst filling a third of the window.
///   A burst of forty samples over ordinary edits is the binding one and the
///   floor returns at 13 at 40 and 80 columns.
///
/// So ten has two of margin either side, and the endpoints are here so the next
/// reader can re-derive them rather than trust them.
///
/// **A cut against the median rather than against the mean, and that is the load
/// bearing half.** The mean is itself corrupted by the tail, so a multiple of it
/// is a threshold that rises with the thing it is meant to exclude: measured, a
/// mean-relative cut fixes a burst of a quarter of the window and fails on one of
/// a third. The median is unmoved by a minority of loud values, which is the
/// property being bought.
const SCALE_OUTLIER: u64 = 10;

/// [`scale_of`] over a scratch of the **non-empty** values, which it reorders.
///
/// **Split out rather than written twice**, because [`History::repeak`] computes
/// one of these per [`SPARK_GROUPS`] entry over buckets it has already gathered,
/// and a second copy of the rule is a rule that can move in one place and not the
/// other. It took `(sum, busy)` until
/// [#256](https://github.com/breferrari/vigia/issues/256); a median cannot be
/// taken from a running pair, so what crosses the boundary is now the values.
///
/// `select_nth_unstable` rather than a sort: the median is the only order
/// statistic wanted and this is `O(n)` where a sort is `O(n log n)`, on a walk
/// that runs on the frame path.
///
/// **The kept set is never empty, by construction rather than by a guard.** The
/// median is at most [`SCALE_OUTLIER`] times itself for any multiple of one or
/// more, so it survives its own cut, so at least one value always does. A guard
/// here would be dead code that reads like a safety net.
fn scale_of_busy(busy: &mut [u32]) -> u32 {
    let Some(cut) = outlier_cut(busy) else {
        return 0;
    };
    let (sum, kept) = busy
        .iter()
        .map(|value| u64::from(*value))
        .filter(|value| *value <= cut)
        .fold((0u64, 0u64), |(sum, kept), value| (sum + value, kept + 1));
    scale_from(sum, kept)
}

/// The value a member of `busy` may not exceed and still set the scale, or
/// `None` when there is nothing to measure.
///
/// **Taken once over the whole population and then applied, rather than
/// re-derived wherever the values are summed.** That is the difference between
/// this and the first shape of
/// [#256](https://github.com/breferrari/vigia/issues/256), and it is what keeps
/// [`History::repeak`]'s figures in order: see the paragraph there about a
/// coarser rung. Reorders `busy`.
///
/// `select_nth_unstable` rather than a sort: the median is the only order
/// statistic wanted and this is `O(n)` where a sort is `O(n log n)`, on a walk
/// that runs on the frame path.
///
/// **Every population always keeps at least one member, by construction rather
/// than by a guard.** The median is at most [`SCALE_OUTLIER`] times itself for
/// any multiple of one or more, so it survives its own cut. A guard against an
/// empty kept set would be dead code that reads like a safety net.
fn outlier_cut(busy: &mut [u32]) -> Option<u64> {
    if busy.is_empty() {
        return None;
    }
    let mid = busy.len() / 2;
    let (_, median, _) = busy.select_nth_unstable(mid);
    Some(u64::from(*median) * SCALE_OUTLIER)
}

/// Thirteen tenths of the mean of what the cut kept.
///
/// **Split out rather than written twice**, because [`History::repeak`] reaches
/// the same rule over a series it is already walking for another reason and must
/// not carry a second copy of the factor.
fn scale_from(sum: u64, kept: u64) -> u32 {
    if kept == 0 {
        return 0;
    }
    // Thirteen tenths: above the mean, so an ordinary write does not sit at the
    // ceiling, and close enough to it that an ordinary write is still legible as
    // a shape rather than as a stub.
    u32::try_from(sum * 13 / (kept * 10)).unwrap_or(u32::MAX)
}

/// One path's churn, and when it last moved.
///
/// **`Clone` and not `Copy` since [#198](https://github.com/breferrari/vigia/issues/198)**,
/// which took this from twenty-four bytes to two hundred and forty-eight.
/// Nothing copies one today, and that is exactly why the trait comes off now: a
/// `Copy` of this size makes the next accidental by-value use invisible at the
/// call site and an order of magnitude more expensive than it reads.
#[derive(Debug, Clone)]
struct Track {
    /// Oldest sample first, so the array projects left to right as written.
    samples: [u32; HISTORY_SAMPLES],
    /// Ordinal of the last tick that named this path.
    ///
    /// A counter rather than an [`Instant`], because the only question asked of
    /// it is "was this the newest tick", and comparing ordinals cannot drift
    /// the way two clocks can.
    tick: u64,
    /// Bytes this path held when it was last weighed, if it ever has been.
    ///
    /// **What turns a count of writes into a measure of change**
    /// ([#232](https://github.com/breferrari/vigia/issues/232)). A sample used to
    /// be how many files moved, so a worktree where one file is saved repeatedly
    /// put exactly one in every sample, made itself the peak, and drew every
    /// active column at full height. The element said *when* and could not say
    /// *how much*, which is the opposite of the "change density over time"
    /// `SPEC.md` §5.1 names it for.
    ///
    /// `None` until the first weighed write, which is why that write still counts
    /// one: a delta needs two observations and the first one is not a change, it
    /// is a baseline. Charging a file's whole size on first sight would spike the
    /// peak on the first save of a session and flatten the two minutes after it.
    bytes: Option<u64>,
}

impl Track {
    fn new(tick: u64) -> Self {
        Self {
            samples: [0; HISTORY_SAMPLES],
            tick,
            bytes: None,
        }
    }

    /// Slide `steps` samples into the past, filling the newest end with zeroes.
    fn shift(&mut self, steps: usize) {
        if steps >= HISTORY_SAMPLES {
            self.samples = [0; HISTORY_SAMPLES];
            return;
        }
        self.samples.rotate_left(steps);
        self.samples[HISTORY_SAMPLES - steps..].fill(0);
    }

    /// Whether nothing at all is left inside the window for this path.
    ///
    /// **Walked newest-first**, which is not a style choice: `samples` is
    /// oldest-first and a surviving track's writes are at the newest end, so a
    /// forward scan reads almost the whole array before it can return `false`
    /// for exactly the tracks that are kept. Reversed, the common case stops
    /// within a few reads and the genuinely empty case is identical.
    fn empty(&self) -> bool {
        self.samples.iter().rev().all(|&count| count == 0)
    }

    /// The samples summed into the source buckets a sparkline is drawn from,
    /// oldest first.
    ///
    /// **A projection re-projects; it does not drop items.** That is `heat_at`'s
    /// ruling in the shell, and it is the whole of why raising the sampling rate
    /// leaves the sparkline where it was: every column is the sum of the
    /// [`SAMPLES_PER_BUCKET`] samples covering exactly the seconds it always
    /// covered, so the same writes land in the same column.
    ///
    /// **This is one projection of two since
    /// [#234](https://github.com/breferrari/vigia/issues/234).** The shell groups
    /// these again onto the rung a pane affords, by the same rule and for the
    /// same reason, so what a reader sees is the whole window at whatever
    /// resolution the row can hold.
    ///
    /// Saturating, for [`Track::bump`]'s reason one level up: a column summing
    /// past its type is already at the top of the ramp, and wrapping would draw the
    /// busiest file in the worktree as the quietest.
    fn drawn(&self) -> [u32; HISTORY_BUCKETS] {
        // Sliced rather than zipped against `chunks`, which **truncates**: a
        // division that stopped being exact would silently drop the last group,
        // and the last group is the newest, so every fresh write would vanish
        // from the screen with nothing failing. Slicing panics instead, and the
        // `const` assertion beside `SAMPLES_PER_BUCKET` means it cannot.
        bucketed(&self.samples)
    }

    /// [`Track::drawn`] read as a **level** rather than as the writes that made
    /// it. See [`levelled`] for the kernel and [`HISTORY_LEVEL`] for its
    /// constant.
    ///
    /// Its own function rather than a flag on `drawn`, because the two answer
    /// different questions and every gate over `drawn` asserts the one it has.
    fn levelled(&self) -> [u32; HISTORY_BUCKETS] {
        bucketed(&levelled(&self.samples))
    }

    /// Add this write's weight to the newest sample.
    ///
    /// **One is the floor and not the unit**, which is the whole of
    /// [#232](https://github.com/breferrari/vigia/issues/232). A write always
    /// counts at least one, so a store fed no sizes behaves exactly as it did
    /// before and every gate written against that still holds. A write whose
    /// size can be measured counts what it moved instead, so a paste draws taller
    /// than a typo and the graph acquires the shape it was named for.
    ///
    /// **A `u32` sample rather than a `u16` one, and the width is load bearing
    /// now that the unit is bytes.** Sixteen bits was ample while a sample
    /// counted writes: sixty-five thousand saves inside one second is not a
    /// thing. It is one ordinary save of a lockfile once the unit is bytes, and
    /// a sample that pegged there would put the peak at its ceiling for the
    /// whole two-minute window and scale every other row to nothing, which is
    /// the exact flattening [#232](https://github.com/breferrari/vigia/issues/232)
    /// exists to remove, re-entered from the top. Four gigabytes in one second is
    /// a bound a disk cannot reach.
    ///
    /// Saturating rather than wrapping even so, because the ceiling still exists
    /// and wrapping past it would draw the busiest file in the worktree as the
    /// quietest. [`Track::drawn`] saturates again when it sums a column, for the
    /// same reason one level up.
    fn bump(&mut self, weight: u32) {
        let newest = &mut self.samples[HISTORY_SAMPLES - 1];
        *newest = newest.saturating_add(weight.max(1));
    }

    /// What this write weighs, and remember the size it was weighed against.
    ///
    /// **The absolute difference, so a deletion weighs what it removed.** A file
    /// that shrinks by five thousand bytes moved as much as one that grew by
    /// five thousand, and signing it would draw the larger of the two edits a
    /// reader can make as the quieter one.
    ///
    /// `None` for a size that could not be read, and for the first write of a
    /// path: both mean there is no delta to take, and both fall back to
    /// [`Track::bump`]'s floor rather than inventing a magnitude.
    ///
    /// **One call rather than a weigh-then-bump pair, because the two were never
    /// valid apart.** Weighing without bumping advances the baseline and eats the
    /// write, which is a silent loss; bumping without weighing leaves the
    /// baseline at its first sighting forever, so every later write measures
    /// against a size the file has not had for minutes. Both call sites did the
    /// same two lines in the same order, which is a protocol rather than an API.
    fn wrote(&mut self, bytes: Option<u64>) {
        let weight = match (bytes, self.bytes) {
            (Some(now), Some(before)) => {
                self.bytes = Some(now);
                u32::try_from(now.abs_diff(before)).unwrap_or(u32::MAX)
            }
            // A first sighting records the baseline and weighs nothing, so
            // `bump`'s floor is the only place the minimum is stated.
            (Some(now), None) => {
                self.bytes = Some(now);
                0
            }
            (None, _) => 0,
        };
        self.bump(weight);
    }
}

/// Per-path churn over a bounded window: I10.
///
/// Created by [`History::new`], fed one coalesced tick at a time by
/// [`History::record`], and read per drawn row by [`History::churn`] and
/// [`History::recency`].
///
/// ```
/// use std::time::Instant;
/// use vigia_core::{History, Recency};
///
/// let mut history = History::new();
/// let now = Instant::now();
/// history.record(["src/lib.rs", "Cargo.toml"], now);
///
/// // Both were named by the newest tick, so both pulse. Follow mode moves to
/// // one of them; the pulse is what says the other moved too.
/// assert_eq!(history.recency("src/lib.rs"), Recency::Pulse);
/// assert_eq!(history.recency("Cargo.toml"), Recency::Pulse);
/// assert_eq!(history.recency("README.md"), Recency::Cold);
///
/// history.record(["src/lib.rs"], now);
/// assert_eq!(history.recency("src/lib.rs"), Recency::Pulse);
/// assert_eq!(history.recency("Cargo.toml"), Recency::Live);
/// ```
#[derive(Debug)]
pub struct History {
    tracks: HashMap<String, Track>,
    /// Ticks that named at least one path.
    ///
    /// Only those, because staging rewrites the index without changing a byte
    /// on disk, and letting it advance the counter would clear the pulse off
    /// the file the reader was watching for a reason they cannot see.
    tick: u64,
    /// When the newest **sample** opened, which is the grid the window rolls on.
    opened: Instant,
    /// What a drawn bucket's height is divided by, one figure per
    /// [`SPARK_GROUPS`] entry. See [`scale_of`].
    scales: [u32; SPARK_GROUPS.len()],
    /// Every tracked path's levelled **source** buckets, oldest path first, held
    /// between ticks so [`Self::repeak`] allocates nothing.
    ///
    /// **A denominator stopped being expressible as a running pair**
    /// ([#256](https://github.com/breferrari/vigia/issues/256)): the rule takes a
    /// median before it takes a mean, and a median needs the values. This is
    /// where they go.
    ///
    /// **Source buckets rather than grouped ones, which is what keeps the
    /// figures in order.** See the paragraph in [`Self::repeak`] about a coarser
    /// rung: deciding what is outlying once, at the finest resolution, leaves
    /// every rung summing one kept series, and that is the whole of why the
    /// figures come out non-decreasing without anything asserting that they do.
    ///
    /// **Reused rather than reallocated, and the capacity is the point.** At the
    /// [`HISTORY_PATHS`] cap this holds [`HISTORY_BUCKETS`] `u32` per track,
    /// which is twenty-four kilobytes, allocated once and `clear`ed on every
    /// walk. I3 bounds RSS *drift*, and a fixed buffer of that size is the same
    /// trade [#161](https://github.com/breferrari/vigia/issues/161) made when the
    /// store went from 4KiB to 60KiB.
    scratch: Vec<u32>,
    /// The non-empty members of [`Self::scratch`], which is the population the
    /// median is taken over.
    ///
    /// **Separate because the two want opposite things.** `scratch` must stay in
    /// path order so it can be walked one track's buckets at a time, and taking a
    /// median reorders whatever it is given. Copying the non-empty values out is
    /// what lets both hold: this one is reordered freely and read once.
    ///
    /// **The zeroes are left behind for the reason they always were**: a worktree
    /// is idle most of the time, so a median over them would measure how long the
    /// reader has been away rather than how large a write is.
    busy: Vec<u32>,
    /// Every tracked path added together, kept current by the walk that finds
    /// the peak. See [`History::worktree_churn`].
    worktree: Churn,
    stats: HistoryStats,
}

impl History {
    /// An empty history, with its first sample opening now.
    pub fn new() -> Self {
        Self::starting_at(Instant::now())
    }

    /// An empty history whose first sample opened at `now`.
    ///
    /// Separate from [`History::new`] so a test can drive the window without
    /// sleeping through two minutes of it. `Instant` cannot be constructed from
    /// nothing, so the base has to come from somewhere real either way.
    pub fn starting_at(now: Instant) -> Self {
        Self {
            tracks: HashMap::new(),
            tick: 0,
            opened: now,
            scales: [0; SPARK_GROUPS.len()],
            scratch: Vec::new(),
            busy: Vec::new(),
            worktree: Churn::default(),
            stats: HistoryStats::default(),
        }
    }

    /// Take one coalesced tick's worth of samples.
    ///
    /// Rolls the window forward to `now` first, so a sample lands in the bucket
    /// its wall-clock time belongs to rather than in whichever one was open when
    /// the last tick arrived.
    ///
    /// **An empty `paths` still rolls the window and still does not move the
    /// pulse.** That is the index-write case: staging is a real change and
    /// produces a real tick, because the index is the left-hand side of every
    /// diff drawn, but nothing on disk moved, so the file the reader is watching
    /// keeps its label.
    ///
    /// That is also **why there is no separate `expire`**, which the plan for
    /// [#38](https://github.com/breferrari/vigia/issues/38) named as a second
    /// public method. A public `expire` would have been exactly `record` with no
    /// paths, and two entry points into one rule are two things a caller can get
    /// out of step. Ageing with no paths is therefore spelled `record_sized([])`
    /// and goes through the same door as everything else.
    ///
    /// **Called on a tick *and* on the ageing wake, which was not true before
    /// [#243](https://github.com/breferrari/vigia/issues/243).** This said "once
    /// per tick and never on a timer", and that was the whole of the freeze: with
    /// a tick as the only clock, a worktree that went quiet stopped ageing and
    /// the window kept a ninety-second-old burst pinned at the right edge,
    /// drawing it as *just now*. The window is still real time and the store
    /// still owns no clock of its own; what changed is that the shell now has a
    /// deadline to call this on, bounded by [`History::ages_in`] returning `None`
    /// once the window empties. I1's amended row carries why that is licensed.
    ///
    /// # Cost
    ///
    /// A path already tracked is a hash lookup. A new one at the cap costs a
    /// scan for the least recently changed, which is bounded by
    /// [`HISTORY_PATHS`] and therefore constant rather than proportional to the
    /// session.
    pub fn record<'p>(&mut self, paths: impl IntoIterator<Item = &'p str>, now: Instant) {
        self.record_sized(paths.into_iter().map(|path| (path, None)), now);
    }

    /// [`History::record`], with what each written path now holds on disk.
    ///
    /// **This is the one the shell calls, and the difference is what the graph
    /// can say** ([#232](https://github.com/breferrari/vigia/issues/232)). A
    /// sample fed through [`History::record`] counts *files written*, so a
    /// worktree where one file is saved repeatedly puts exactly one in every
    /// sample, makes itself the peak, and draws every active column at full
    /// height: the band and the sparkline both say *when* and neither can say
    /// *how much*. Fed a size, a sample counts the bytes that moved, and both
    /// elements acquire the shape §5.1 names them for.
    ///
    /// `None` for a path whose size could not be read, which is a file that
    /// vanished between the watch naming it and this call. It weighs the floor
    /// rather than nothing, because it was still written.
    ///
    /// **The size is the caller's to take, deliberately.** This crate has no
    /// working directory and the shell does, and putting the `stat` at the call
    /// site is what keeps its cost inside the frame the budget gates measure
    /// rather than hidden behind a store that looks pure.
    ///
    /// # Cost
    ///
    /// A path already tracked is a hash lookup. A new one at the cap costs a
    /// scan for the least recently changed, which is bounded by
    /// [`HISTORY_PATHS`] and therefore constant rather than proportional to the
    /// session.
    pub fn record_sized<'p>(
        &mut self,
        paths: impl IntoIterator<Item = (&'p str, Option<u64>)>,
        now: Instant,
    ) {
        let rolled = self.roll(now);

        let mut named = false;
        for (path, bytes) in paths {
            if !named {
                // Bumped once for the whole tick, before the first sample, so
                // every path in one burst shares an ordinal and therefore
                // pulses together.
                self.tick += 1;
                named = true;
            }
            self.stats.recorded += 1;

            if let Some(track) = self.tracks.get_mut(path) {
                track.tick = self.tick;
                track.wrote(bytes);
                continue;
            }

            if self.tracks.len() >= HISTORY_PATHS {
                self.evict_one();
            }
            let mut track = Track::new(self.tick);
            // The first write of a path has no earlier size to differ from, so it
            // weighs the floor and leaves the baseline behind for the next one.
            track.wrote(bytes);
            self.tracks.insert(path.to_owned(), track);
        }

        // **Skipped when nothing moved and nothing was written**, which is new
        // with [#277](https://github.com/breferrari/vigia/issues/277) and is not
        // an optimisation looking for a problem: that row put this call on the
        // shell loop's *timeout* arm, which also fires every `STEP_REPEAT` while
        // a scrollbar button is held. At 50ms a held button drives twenty
        // timeouts a second and nineteen of them cross no sample boundary, so
        // without this guard each one pays the walk priced at **144µs mean and
        // 191µs worst** in [`Self::repeak`]'s own docblock, for an answer that
        // cannot have changed.
        //
        // Both terms are load bearing. A tick that named a path inside the
        // current sample changed a track without moving the window, and a roll
        // that moved the window changed every track's shape without naming one.
        if rolled > 0 || named {
            self.stats.repeaks += 1;
            self.repeak();
        }
    }

    /// This path's buckets, oldest first, or `None` when nothing is tracked.
    ///
    /// Copied rather than borrowed. It is [`HISTORY_BUCKETS`] `u32`s, which is
    /// comparable to the reference on every target, and returning it by value is
    /// what lets a caller hold one while asking about the next path.
    ///
    /// **Projected rather than stored since
    /// [#198](https://github.com/breferrari/vigia/issues/198)**, which is what
    /// keeps the sentence above true: the store holds [`HISTORY_SAMPLES`] of them
    /// and this sums each drawn column's worth. Handing back the samples instead
    /// would have made the array larger than a reference on every target and
    /// pushed the projection out to every caller, where each would have had to
    /// agree about it.
    pub fn churn(&self, path: &str) -> Option<[u32; HISTORY_BUCKETS]> {
        self.tracks.get(path).map(Track::drawn)
    }

    /// This path's churn read as a **level** rather than as the writes that made
    /// it, in the same source buckets [`History::churn`] returns.
    ///
    /// The sparkline's half of [#242](https://github.com/breferrari/vigia/issues/242),
    /// and it shares one kernel with the worktree band so the two cannot
    /// disagree, which is [#234](https://github.com/breferrari/vigia/issues/234)'s
    /// constraint met by construction rather than by discipline.
    pub fn level(&self, path: &str) -> Option<[u32; HISTORY_BUCKETS]> {
        self.tracks.get(path).map(Track::levelled)
    }

    /// Which rung of the recency ladder this path is on.
    ///
    /// **The newest burst, and only until the window rolls past it**
    /// ([#243](https://github.com/breferrari/vigia/issues/243)). Both halves are
    /// load bearing and the first was here alone.
    ///
    /// `track.tick == self.tick` is the newest *burst*, which is what
    /// distinguishes two saves a second apart: §11.2 B2 has follow move to the
    /// write that landed last, and the pulse is what says the rest of that batch
    /// moved with it. The ordinal only advances when a burst names something, so
    /// on its own it never expired: an empty roll left every pulse where it was,
    /// and a file written a hundred and nineteen seconds ago kept its mark at
    /// full brightness while the band beside it drained, then lost it all at once
    /// when the track was evicted. That is this row's own freeze, one field over,
    /// and it became visible the moment the window started moving on its own.
    ///
    /// The newest sample is what expires it. [`Track::shift`] zeroes that sample
    /// on the first roll that crosses a boundary, so the mark ages by
    /// construction rather than by anything remembering to retire it. Dropping
    /// the ordinal for it alone is wrong and a gate says so: two separate saves
    /// inside one second are one sample and two bursts, and only the later one
    /// pulses.
    pub fn recency(&self, path: &str) -> Recency {
        match self.tracks.get(path) {
            // `self.tick` is zero until something is recorded and no track can
            // exist before then, so this never reads a pulse out of an empty
            // store. `Track::bump` floors a write at one, so a non-zero newest
            // sample is exactly "written since the last boundary".
            Some(track) if track.tick == self.tick && track.samples[HISTORY_SAMPLES - 1] > 0 => {
                Recency::Pulse
            }
            Some(_) => Recency::Live,
            None => Recency::Cold,
        }
    }

    /// What a **source** bucket's height is divided by, across every tracked path.
    ///
    /// **A caller drawing a coarser rung scales this with it**, since
    /// [#234](https://github.com/breferrari/vigia/issues/234): a drawn column
    /// summing `n` source buckets is measured against `n` times this figure, so a
    /// wider bucket is compared with a proportionally wider yardstick rather than
    /// saturating against a narrow one. That keeps a rung a change of resolution
    /// and not of height, which is what makes the ladder invisible to a reader
    /// who never resizes.
    ///
    /// Rows share one denominator rather than each being drawn against its own
    /// maximum, because the question a reader asks across a file list is which
    /// file is busiest, and per-file scaling draws every file at full height the
    /// moment it is the busiest thing it has ever been.
    ///
    /// **It is [`scale_of`]'s figure rather than the largest bucket, since
    /// [#232](https://github.com/breferrari/vigia/issues/232).** A sample weighs
    /// the bytes a write moved now, so the largest bucket in a two-minute window
    /// is whatever build last rewrote a lock file, and dividing by that draws
    /// every edit a reader makes for the next two minutes one level high. The
    /// band was given this rule when the symptom was reported from a live pane;
    /// the sparkline divides by the same store and needed it too.
    ///
    /// **Zero when nothing is tracked, which is a scale a caller must not divide
    /// by.** It used to say the caller must treat it as "draw nothing", and that
    /// is no longer what the shell does: since
    /// [#78](https://github.com/breferrari/vigia/issues/78) an empty bucket draws
    /// a track, so zero means every bucket is empty and every one of them is
    /// still drawn. The constraint this states is arithmetic and belongs here;
    /// what to draw is the shell's and belongs in `SPEC.md` §5.1.
    pub fn scale(&self) -> u32 {
        self.scales[0]
    }

    /// Every figure [`SPARK_GROUPS`] names, in that order.
    ///
    /// **What a caller drawing a coarser rung needs**, since
    /// [#234](https://github.com/breferrari/vigia/issues/234): a drawn bucket is
    /// the sum of `group` source ones, and the figure it is measured against is
    /// this one rather than [`History::scale`] multiplied, for the reason
    /// [`SPARK_GROUPS`] states with the measurement behind it.
    ///
    /// Handed back whole rather than one at a time, so the shell holds a
    /// consistent set from one tick rather than three reads that could straddle
    /// two.
    pub fn scales(&self) -> [u32; SPARK_GROUPS.len()] {
        self.scales
    }

    /// Paths currently tracked. Never more than [`HISTORY_PATHS`].
    ///
    /// This is the number I10 is a claim about: it follows the window and the
    /// cap, never the number of files the session has touched.
    pub fn tracked(&self) -> usize {
        self.tracks.len()
    }

    /// Counters for what this history has done.
    pub fn stats(&self) -> HistoryStats {
        self.stats
    }

    /// Advance the window to `now`, dropping whatever fell out of it.
    /// Returns how many whole samples the window moved, which is what lets
    /// [`Self::record_sized`] skip [`Self::repeak`] over state that did not
    /// change ([#277](https://github.com/breferrari/vigia/issues/277)).
    fn roll(&mut self, now: Instant) -> usize {
        let elapsed = now.saturating_duration_since(self.opened);
        // Saturating into `usize` before the comparison below, so an instant far
        // in the future cannot overflow the multiplication that moves `opened`.
        let steps = usize::try_from(elapsed.as_nanos() / HISTORY_SAMPLE.as_nanos())
            .unwrap_or(HISTORY_SAMPLES);
        if steps == 0 {
            return 0;
        }

        if steps >= HISTORY_SAMPLES {
            // The whole window has turned over, so nothing tracked can have a
            // sample left in it. Clearing beats shifting every track by more
            // samples than it has, and it is the state a monitor left open
            // overnight wakes up in.
            self.stats.evicted_by_window += self.tracks.len() as u64;
            self.tracks.clear();
            self.opened = now;
            // **`repeak` owns both derived fields, so neither is zeroed here.**
            // This zeroed `scales` and left `worktree` alone, which was harmless
            // only because the caller repeaked unconditionally. Since
            // [#277](https://github.com/breferrari/vigia/issues/277) it does not,
            // and a branch that clears one of two derived fields is one edit from
            // a window that reads as full ink forever while `ages_in` says there
            // is nothing left to age. The caller repeaks whenever this returns
            // non-zero, and this branch always does.
            return steps;
        }

        // Advanced by whole samples rather than set to `now`, so the boundaries
        // stay on a fixed grid and a burst of ticks inside one sample cannot
        // walk it forward a fraction at a time.
        self.opened += HISTORY_SAMPLE * steps as u32;

        let before = self.tracks.len();
        self.tracks.retain(|_, track| {
            track.shift(steps);
            !track.empty()
        });
        self.stats.evicted_by_window += (before - self.tracks.len()) as u64;
        // **No repeak here**, deliberately: `record_sized` is this function's
        // only caller and repeaks after it returns whenever this reported a
        // non-zero step, so a second full projection of every track would be pure
        // duplicate work. That was survivable while a track held eight samples
        // and is a quarter of the tick's cost now that it holds a hundred and
        // twenty. (It repeaked *unconditionally* until
        // [#277](https://github.com/breferrari/vigia/issues/277) gave the caller
        // this function's step count to skip on.)
        steps
    }

    /// Drop the least recently changed path to make room for a new one.
    ///
    /// Least recently *changed* rather than least recently inserted: a path that
    /// keeps moving is the one a reader is watching, and evicting by age of
    /// arrival would throw it away in favour of something written once and
    /// forgotten.
    fn evict_one(&mut self) {
        // Compared rather than keyed, because a key has to be **owned**: the
        // keyed form cloned every path it looked at, so one eviction allocated
        // two hundred and fifty-six strings and a burst that filled the cap
        // allocated them again per victim. The ordering is identical, oldest
        // tick first and the path breaking ties so the choice is deterministic.
        let victim = self
            .tracks
            .iter()
            .min_by(|a, b| a.1.tick.cmp(&b.1.tick).then_with(|| a.0.cmp(b.0)))
            .map(|(path, _)| path.clone());
        if let Some(path) = victim {
            self.tracks.remove(&path);
            self.stats.evicted_by_cap += 1;
        }
    }

    /// How long until the window's next sample boundary, or `None` when it holds
    /// nothing to age.
    ///
    /// **The whole of [#243](https://github.com/breferrari/vigia/issues/243)'s
    /// budget, and `None` is the load-bearing half.** The shell's loop waits
    /// untimed when every deadline it can see is `None`, so returning a duration
    /// here for an empty window would put an idle monitor on a poll loop while
    /// looking entirely reasonable. That is the same shape `Held::wait` is
    /// written in, and for the same reason: the answer is a value a test can
    /// read rather than a behaviour it has to observe.
    ///
    /// **A window with no tracks holds nothing**, which is exactly when it is
    /// safe to stop: [`Self::repeak`] rebuilds the worktree series from the
    /// tracks, so an empty map is an all-zero series, and [`Self::roll`] clears
    /// the map once the whole window has turned over. That bounds the clock at
    /// [`HISTORY_SAMPLES`] wakes after any burst, and it is an **upper** bound
    /// rather than an exact one: samples fall on a fixed grid anchored on
    /// `opened`, not on the write, so a track can drain up to one
    /// [`HISTORY_SAMPLE`] sooner than `HISTORY_WINDOW` after the last write.
    /// Draining early only stops the clock early, which is the safe direction for
    /// this invariant, and every gate here happens to write exactly on a boundary
    /// so none of them can see the difference.
    ///
    /// That bound is why I1's *0 wakeups while idle* survives the amendment: the
    /// state a monitor left open overnight is in is an empty window.
    ///
    /// Saturating, so a boundary already passed asks for zero rather than
    /// panicking. That happens whenever the process was not woken for longer
    /// than a sample, which is the ordinary case on the first ageing wake after
    /// a quiet stretch, and asking for zero is right: the roll is overdue.
    pub fn ages_in(&self, now: Instant) -> Option<Duration> {
        if self.tracks.is_empty() {
            return None;
        }
        Some((self.opened + HISTORY_SAMPLE).saturating_duration_since(now))
    }

    /// Every tracked path's churn added together, oldest sample first.
    ///
    /// **The worktree's own series, which is what `SPEC.md` §5.3 calls the
    /// worktree churn graph** ([#158](https://github.com/breferrari/vigia/issues/158)).
    /// It invents nothing: it is arithmetic over state I10 already bounds, so it
    /// needs no wake, no write and no clock, and it is drawn on frames that were
    /// going to happen anyway.
    ///
    /// **Maintained rather than computed on demand, and it is free**, because
    /// [`History::repeak`] already walks every sample of every track on every
    /// [`History::record`]. Summing in that same pass costs one add per element
    /// and the frame path pays nothing at all: a caller asking for this is a
    /// field read.
    ///
    /// `u32` because the sum is over paths as well as time, and **saturating**
    /// rather than merely wide since [#232](https://github.com/breferrari/vigia/issues/232):
    /// this said 256 paths of `u16` could not reach a `u32`'s ceiling, which was
    /// true until a sample became a `u32` of bytes that saturates at that ceiling
    /// on its own. [`History::repeak`] adds them with `saturating_add` for exactly
    /// that reason.
    pub fn worktree_churn(&self) -> Churn {
        self.worktree
    }

    /// Recompute the busiest source bucket and the worktree series, in one walk.
    ///
    /// **Two results from one pass, which is why the series is free.** This
    /// already had to touch every sample of every track to find the peak, so
    /// [`History::worktree_churn`]'s sum rides along at one add per element
    /// rather than costing a walk of its own. Splitting them would double the
    /// most expensive thing a tick does.
    ///
    /// The peak is over the **projection** rather than the raw samples, which is
    /// the half of [#198](https://github.com/breferrari/vigia/issues/198) that
    /// would have moved the sparkline if it were got wrong: heights are scaled
    /// against it, so a denominator measured one sample at a time would be
    /// smaller than the columns it divides and every bar on screen would top
    /// out.
    fn repeak(&mut self) {
        let mut worktree = [0u32; HISTORY_SAMPLES];
        for track in self.tracks.values() {
            for (total, &count) in worktree.iter_mut().zip(track.samples.iter()) {
                // **Saturating, like every other add on this path.** It was a
                // plain `+=` while a sample was a `u16` and the sum a `u32`, where
                // 256 paths of `u16::MAX` could not reach the ceiling. A sample is
                // a `u32` of bytes now and [`Track::bump`] saturates at its own
                // ceiling, so two large writes in one second would panic in debug
                // and wrap in release, drawing the busiest worktree there has ever
                // been as the quietest. That is the exact failure `bump` and
                // `drawn` already saturate against, reached through the widening
                // rather than through a count.
                *total = total.saturating_add(count);
            }
        }
        // **Collected rather than streamed, since
        // [#256](https://github.com/breferrari/vigia/issues/256).** This walked
        // the buckets through an iterator so nothing had to hold them, and the
        // rule now takes a median, which no running pair can answer.
        // [`Self::scratch`] is what keeps that from costing an allocation a tick.
        // **Measured rather than assumed, since this is the frame path.** At the
        // full 256-path cap over a populated window, the tick that recomputes
        // this costs **144µs mean and 191µs worst**, against I9's 16ms: under one
        // percent. `levelled` is two O(n) passes over 120 samples per track
        // rather than the O(n·k) convolution its shape suggests, which is why.
        //
        // **Levelled, because that is what the sparkline draws**
        // ([#242](https://github.com/breferrari/vigia/issues/242)). A denominator
        // taken over the raw buckets while the bars are drawn from the levelled
        // ones is two different quantities in one division, and every bar on
        // screen would be wrong by whatever the kernel moved.
        //
        // **One figure per [`SPARK_GROUPS`] entry, from the one walk**, since
        // [#234](https://github.com/breferrari/vigia/issues/234). The shell's
        // `spark_of` groups these same buckets the same way to get the numerator
        // this is the denominator for, and the two are deliberately not one
        // function: that one materialises the groups so a drawn row allocates
        // nothing, this one gathers them into a buffer it keeps.
        // `tests/rows.rs::a_recorded_tick_reaches_the_drawn_sparkline` pins the
        // figures this answers with, so a grouping that moved on one side and not
        // the other reddens. The kernel is what costs here and it runs once per
        // track either way; what the ladder adds is two more folds over
        // twenty-four `u32`s that are already in cache. Scaling a single figure by
        // the grouping instead was measured and rejected, and that constant's
        // docblock carries the number.
        //
        // **Gathered rather than accumulated, since
        // [#256](https://github.com/breferrari/vigia/issues/256).** This kept a
        // running `(sum, busy)` per grouping while the rule was a plain mean, and
        // the rule now takes a median first, which no running pair can answer.
        // [`Self::scratch`] is what keeps that from being an allocation per tick:
        // it is cleared and refilled, so the cost of the change is the gather
        // rather than the memory. The kernel still runs once per track, which is
        // the term that costs.
        self.scratch.clear();
        self.scratch
            .reserve(self.tracks.len().saturating_mul(HISTORY_BUCKETS));
        for track in self.tracks.values() {
            self.scratch.extend_from_slice(&track.levelled());
        }

        // **What is outlying is decided once, at the source resolution, and then
        // every rung sums one kept series.** That ordering is not a detail: it is
        // what makes a coarser rung's figure provably no smaller than a finer
        // rung's, which `SPEC.md` §11.1 requires as *a rung is a change of
        // resolution and not of height*.
        //
        // The proof is one line and it is worth stating, because the first shape
        // of this row broke it and then asserted the conclusion back. Grouping
        // puts every source bucket in exactly one group, so the kept **sum** is
        // the same whichever rung is drawn; merging buckets can only reduce the
        // number of non-empty ones; so `13/10 * sum / busy` is non-decreasing as
        // the rung coarsens, by arithmetic. Take the cut per grouping instead and
        // each rung keeps a *different* sum, the shared term is gone and the
        // ordering goes with it: measured on `tests/rows.rs`'s one-path fixture,
        // that shape produced 418, 837 and **302**, so coarsening the rung drew
        // every bar taller. This one produces 418, 837 and 1116.
        //
        self.busy.clear();
        self.busy.reserve(self.scratch.len());
        self.busy
            .extend(self.scratch.iter().copied().filter(|bucket| *bucket > 0));
        let Some(cut) = outlier_cut(&mut self.busy) else {
            // Nothing tracked, or a window that holds only empties. Every figure
            // is zero, which every caller reads as "no scale yet".
            self.scales = [0; SPARK_GROUPS.len()];
            self.worktree = Churn(worktree);
            return;
        };
        let mut parts = [(0u64, 0u64); SPARK_GROUPS.len()];
        for buckets in self.scratch.chunks(HISTORY_BUCKETS) {
            for (part, group) in parts.iter_mut().zip(SPARK_GROUPS) {
                for chunk in buckets.chunks(group) {
                    // The kept series, summed at this rung. A source bucket past
                    // the cut contributes nothing here and still draws at its own
                    // height: this decides the yardstick, never the bar.
                    let total = chunk
                        .iter()
                        .copied()
                        .filter(|bucket| u64::from(*bucket) <= cut)
                        .fold(0u32, u32::saturating_add);
                    if total > 0 {
                        part.0 += u64::from(total);
                        part.1 += 1;
                    }
                }
            }
        }
        self.scales = std::array::from_fn(|at| scale_from(parts[at].0, parts[at].1));
        self.worktree = Churn(worktree);
    }
}

impl Default for History {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    //! The window, the cap and the ladder, tested as the arithmetic they are.
    //!
    //! Every one of these drives the clock by handing [`History::record`] an
    //! instant rather than by sleeping. That is not only for speed: the window
    //! is two minutes long and the cap needs a bulk write, so a suite that
    //! waited for real time could not gate either of them at all.

    use super::*;

    fn base() -> Instant {
        // Far enough ahead that a test can step backwards inside a bucket
        // without underflowing a monotonic clock.
        Instant::now() + HISTORY_WINDOW
    }

    #[test]
    fn an_untracked_path_is_cold_rather_than_absent() {
        let history = History::starting_at(base());
        assert_eq!(history.recency("src/lib.rs"), Recency::Cold);
        assert_eq!(history.churn("src/lib.rs"), None);
        assert_eq!(history.scale(), 0);
    }

    #[test]
    fn a_path_named_by_the_newest_tick_pulses_and_the_rest_do_not() {
        let now = base();
        let mut history = History::starting_at(now);
        history.record(["a"], now);
        history.record(["b"], now);

        assert_eq!(history.recency("b"), Recency::Pulse);
        assert_eq!(history.recency("a"), Recency::Live);
    }

    /// `SPEC.md` §11.2 B2: follow moves to the write that landed last, and the
    /// pulse is what says the others in the batch moved too. One tick, one
    /// ordinal, so a twelve-file save lights all twelve.
    #[test]
    fn every_path_in_one_tick_pulses_together() {
        let now = base();
        let mut history = History::starting_at(now);
        history.record(["a", "b", "c"], now);

        for path in ["a", "b", "c"] {
            assert_eq!(history.recency(path), Recency::Pulse, "{path}");
        }
    }

    /// Staging rewrites the index and produces a real tick that names no file.
    /// It must not clear the pulse off whatever the reader was watching.
    #[test]
    fn a_tick_naming_no_path_leaves_the_pulse_where_it_was() {
        let now = base();
        let mut history = History::starting_at(now);
        history.record(["a"], now);
        history.record(std::iter::empty(), now);

        assert_eq!(history.recency("a"), Recency::Pulse);
    }

    #[test]
    fn a_bucket_rolls_forward_rather_than_accumulating_for_ever() {
        let now = base();
        let mut history = History::starting_at(now);
        history.record(["a"], now);
        assert_eq!(history.churn("a").unwrap()[HISTORY_BUCKETS - 1], 1);

        // One bucket on: the old sample slides left, the newest is empty again.
        history.record(std::iter::empty(), now + HISTORY_BUCKET);
        let buckets = history.churn("a").unwrap();
        assert_eq!(buckets[HISTORY_BUCKETS - 2], 1);
        assert_eq!(buckets[HISTORY_BUCKETS - 1], 0);
    }

    #[test]
    fn a_path_with_nothing_left_in_the_window_is_dropped_entirely() {
        let now = base();
        let mut history = History::starting_at(now);
        history.record(["a"], now);
        assert_eq!(history.tracked(), 1);

        history.record(std::iter::empty(), now + HISTORY_WINDOW);

        assert_eq!(history.tracked(), 0, "the path is dropped, not drawn empty");
        assert_eq!(history.recency("a"), Recency::Cold);
        assert_eq!(history.churn("a"), None);
        assert_eq!(history.stats().evicted_by_window, 1);
    }

    /// The boundary in both directions, because a window that is a bucket too
    /// generous still passes the test above.
    #[test]
    fn a_sample_survives_until_the_window_has_wholly_passed() {
        let now = base();
        let mut history = History::starting_at(now);
        history.record(["a"], now);

        history.record(std::iter::empty(), now + HISTORY_WINDOW - HISTORY_BUCKET);
        assert_eq!(history.tracked(), 1, "one bucket short of the window");

        history.record(std::iter::empty(), now + HISTORY_WINDOW);
        assert_eq!(history.tracked(), 0);
    }

    #[test]
    fn the_cap_evicts_the_least_recently_changed_path() {
        let now = base();
        let mut history = History::starting_at(now);

        // Fill it exactly, each path in its own tick so the ordinals differ.
        for n in 0..HISTORY_PATHS {
            history.record([format!("f{n}").as_str()], now);
        }
        assert_eq!(history.tracked(), HISTORY_PATHS);
        assert_eq!(history.stats().evicted_by_cap, 0);

        // Touch the oldest, so it is no longer the least recently changed.
        history.record(["f0"], now);
        history.record(["new"], now);

        assert_eq!(history.tracked(), HISTORY_PATHS);
        assert_eq!(history.stats().evicted_by_cap, 1);
        assert_ne!(history.recency("f0"), Recency::Cold, "it was touched");
        assert_eq!(history.recency("f1"), Recency::Cold, "it was the oldest");
    }

    /// One denominator across every path, and it is not the largest bucket.
    ///
    /// **The shared half is unchanged and the rule under it moved**
    /// ([#232](https://github.com/breferrari/vigia/issues/232)). Rows still divide
    /// by one figure, because the question a reader asks across a file list is
    /// which file is busiest. What that figure is stopped being a maximum once a
    /// sample weighed bytes: see [`scale_of`].
    ///
    /// Two buckets of 2 and 1 give a non-zero mean of 1.5, and 1.3 of that is 1
    /// after the integer division. The maximum would be 2, which is what this
    /// asserted before and what would have left every ordinary write drawing
    /// against whatever build last rewrote a lock file.
    #[test]
    fn one_scale_serves_every_path_and_it_is_not_the_largest_bucket() {
        let now = base();
        let mut history = History::starting_at(now);
        history.record(["a"], now);
        history.record(["a"], now);
        history.record(["b"], now);

        let ordinary = history.scale();
        assert!(ordinary > 0, "an ordinary window produced no scale at all");

        // **The claim, asserted by moving the outlier rather than by comparing
        // against one bucket.** This used to read `scale < busiest`, which was a
        // true consequence of a mean-based scale while the buckets were spiky and
        // stopped being one when [#242](https://github.com/breferrari/vigia/issues/242)
        // made them a level: a smooth series has few outliers, so a mean is
        // representative and thirteen tenths of it sits *above* a typical bucket
        // rather than below the tallest. The property that actually matters never
        // depended on which side it landed: one enormous write must not drag the
        // denominator with it, because against a maximum every ordinary edit for
        // the next two minutes draws one level high.
        let mut spiked = History::starting_at(now);
        spiked.record(["a"], now);
        spiked.record(["a"], now);
        spiked.record(["b"], now);
        // Twice, because a first write has no earlier size to differ from and
        // weighs the floor: the outlier is the *delta*, not the size.
        spiked.record_sized([("c", Some(1_000))], now);
        spiked.record_sized([("c", Some(50_000_000))], now);

        let busiest = spiked
            .level("c")
            .expect("c is tracked")
            .into_iter()
            .max()
            .expect("a window has buckets");

        assert!(
            busiest > spiked.scale() * 4,
            "the outlier's own bucket is {busiest} against a scale of {}, so the              fixture has no outlier to speak of and this proves nothing",
            spiked.scale()
        );
        assert_eq!(
            spiked.churn("b").unwrap()[HISTORY_BUCKETS - 1],
            1,
            "the small path stopped being recorded, so the fixture changed rather              than the scale"
        );
    }

    /// A path written more than `u16::MAX` times in one sample is already at the
    /// top of the ramp; wrapping would draw the busiest file as the quietest.
    ///
    /// **Both halves, since #198 gave the projection its own saturating add.** A
    /// sample that saturates and a drawn column that sums fifteen of them are two
    /// places the same wrap could happen, and the second is the newer one.
    #[test]
    fn a_bucket_saturates_rather_than_wrapping() {
        let now = base();
        let mut history = History::starting_at(now);
        let mut track = Track::new(1);
        track.samples[HISTORY_SAMPLES - 1] = u32::MAX;
        // Both ends of the weight, because #232 gave a sample one: the floor a
        // sizeless write takes, and a full-range one from a large edit. Neither
        // may wrap.
        track.bump(1);
        assert_eq!(track.samples[HISTORY_SAMPLES - 1], u32::MAX);
        track.bump(u32::MAX);
        assert_eq!(track.samples[HISTORY_SAMPLES - 1], u32::MAX);
        track.samples[HISTORY_SAMPLES - 2] = 9;
        assert_eq!(
            track.drawn()[HISTORY_BUCKETS - 1],
            u32::MAX,
            "a drawn column summing past its type wrapped instead of topping out"
        );

        history.record(["a"], now);
        assert!(history.scale() > 0);
    }

    /// A monitor left open overnight wakes up with nothing in the window, and
    /// the shift path must not be asked to move a track further than it has
    /// buckets.
    #[test]
    fn a_gap_longer_than_the_whole_window_clears_the_store() {
        let now = base();
        let mut history = History::starting_at(now);
        history.record(["a", "b"], now);

        history.record(std::iter::empty(), now + HISTORY_WINDOW * 100);

        assert_eq!(history.tracked(), 0);
        assert_eq!(history.scale(), 0);
        assert_eq!(history.stats().evicted_by_window, 2);
    }

    /// Bucket boundaries stay on a fixed grid. Ticks arriving repeatedly just
    /// inside one sample must not walk the grid forward a fraction at a time,
    /// which would make the window drift longer than it is specified to be.
    #[test]
    fn ticks_inside_one_bucket_do_not_move_the_boundary() {
        let now = base();
        let mut history = History::starting_at(now);
        history.record(["a"], now);

        let nearly = HISTORY_BUCKET - Duration::from_millis(1);
        for _ in 0..8 {
            history.record(std::iter::empty(), now + nearly);
        }
        assert_eq!(
            history.churn("a").unwrap()[HISTORY_BUCKETS - 1],
            1,
            "still in the bucket it was recorded in"
        );

        history.record(std::iter::empty(), now + HISTORY_BUCKET);
        assert_eq!(history.churn("a").unwrap()[HISTORY_BUCKETS - 2], 1);
    }
}
