//! I3, gated over a soak.
//!
//! > **Flat resources over days.** No unbounded growth in RSS, file handles, or
//! > temp files. RSS drift < 5% over 24h; zero temp files retained.
//!
//! The one thing this file is not allowed to be is a measurement of the engine.
//! I3 is a claim about the process a reader leaves open beside an agent, so the
//! harness is `vigia::run` with the terminal taken out: a real `notify` watch
//! thread, real coalescing, one [`vigia_core::Frame::advance`] per tick, follow
//! and scroll through [`vigia::App`], `View::collect` driving the
//! [`vigia_core::Highlighter`], and [`vigia::render`] into a real buffer.
//!
//! `SPEC.md` §7 carries what it leaves out and why.
//!
//! ## Why this file is two processes
//!
//! "Zero temp files retained" cannot be asserted against a temp directory the
//! rest of the machine is also writing to, so the run needs one of its own, and
//! pointing a process at one means setting `TMPDIR`, `TMP` and `TEMP` **before**
//! it starts. So the parent test builds a private directory and re-executes the
//! test binary into it; the child is the soak. That also makes the gate exact
//! rather than approximate: after the child has exited, anything left in there
//! was put there by the libraries under test.
//!
//! ## Reading it
//!
//! The child prints its whole report, and the parent puts that report in its
//! panic message, so a failure anywhere carries the numbers rather than a
//! boolean. `VIGIA_SOAK_SECS` sets the window; everything else has a default
//! that keeps `cargo test` to a few seconds.

#[path = "../../vigia-core/tests/support/mod.rs"]
mod support;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use vigia::{Action, App, Body, Row, Theme, View, body_layout, diff_height, render};
use vigia_core::{
    FrameStats, HISTORY_PATHS, HISTORY_WINDOW, HighlightStats, Highlighter, History, HistoryStats,
    RETAINED_HUNKS, WatchOptions, Worktree,
};

use support::{Scratch, generated};

/// Set by the parent, and the child's proof that it was not run by hand.
const CHILD: &str = "VIGIA_SOAK_CHILD";
/// The private temp directory, named so the child can check it was given it.
const PRIVATE: &str = "VIGIA_SOAK_TEMP";
/// Where the fixture goes, which is never inside [`PRIVATE`].
const WORKTREE: &str = "VIGIA_SOAK_WORKTREE";
/// How long to soak for. The one knob the scheduled run changes.
const SECS: &str = "VIGIA_SOAK_SECS";
/// Fixture size.
const FILES: &str = "VIGIA_SOAK_FILES";
const LINES: &str = "VIGIA_SOAK_LINES";

/// The child test's name, as libtest filters it.
const CHILD_TEST: &str = "soak_child";

/// Window used when nothing asks for another.
///
/// Short on purpose: this runs inside every `cargo test`, where its job is the
/// structural half. The drift gate needs [`GATED_WINDOW`] and says so rather
/// than pretending fifteen seconds proved something.
const DEFAULT_SECS: u64 = 15;

/// Fixture the default window can actually exercise.
///
/// The budget gates use 100 x 500, and the scheduled soak does too. Building
/// that fixture costs seconds, which is most of a fifteen-second window, so the
/// per-commit run takes a smaller one and the environment carries the rest.
///
/// **Twenty was too few once the body grew a second region, and the failure was
/// on the assertion rather than on the claim.** `SPEC.md` §11.1's pinned list
/// diffs every row it draws, so the working set is the diff viewport *plus* the
/// list. On macOS that took tracked diffs to 23 against 26 files ever present,
/// and the situation guard below asks for twice as many paths as the high-water
/// mark: with a working set that size, a twenty-file fixture cannot churn
/// enough paths for "bounded by the diff" to look different from "bounded by
/// the session", however correct the bound is. I3 itself was fine on that run,
/// with drift at 1.30% of a 5% budget and fifteen diffs evicted.
///
/// The lever is **churn**, not fixture size, and `CREATE_EVERY` below carries
/// it. Both raise the guard's numerator and leave its denominator alone, since
/// the high-water mark is bounded by the screen. Churn is the one that does not
/// also move RSS: doubling the fixture took the reported drift from 1.30% to
/// between 5% and 11% over three local runs, and a fixture that makes a printed
/// budget look blown is not worth the paths it buys.
const DEFAULT_FILES: usize = 20;
const DEFAULT_LINES: usize = 200;

/// Samples across the window, whatever its length.
///
/// 288 is exactly `SPEC.md`'s "every 5 min" at 24h, and holding the *count*
/// rather than the interval is what makes a four-hour run and a one-day run
/// produce the same statistic from the same code. The floor stops a short run
/// from sampling faster than the platform can answer: on Windows a sample is a
/// `tasklist` process.
const MAX_SAMPLES: usize = 288;
const MIN_SAMPLES: usize = 12;

/// Below this window the drift gate reports its numbers and does not assert.
///
/// `SPEC.md` §7: a drift gate over a window shorter than its own warmup is
/// measuring warmup. Ten minutes leaves a minute of warmup and about two and a
/// quarter minutes at each end of the comparison: 288 samples at 2.08s, at
/// least 29 discarded, and a quarter of what is left at each end.
const GATED_WINDOW: Duration = Duration::from_secs(600);

/// File descriptors a sample may exceed the baseline by.
///
/// Generous because a *leak* is per-frame and reaches thousands within seconds,
/// while a transient open is a handful: the gap between the two failure modes
/// is wide enough that the threshold does not have to be precise. Linux only,
/// where `/proc/self/fd` makes it free; elsewhere it reports unavailable rather
/// than passing quietly.
const FD_HEADROOM: usize = 16;

/// Fewest samples the baseline is ever taken after, as a fraction of the run.
///
/// Every process climbs to an allocator plateau before it is flat, and that
/// climb is not a leak. Measuring from the first sample would read it as one,
/// and the threshold needed to tolerate it would be wide enough to wave a real
/// leak through.
///
/// **A floor since [#126](https://github.com/breferrari/vigia/issues/126), where
/// it used to be the whole rule, and the day-long window is what changed it.** A
/// fraction of the window is window-independent, which is what it was chosen
/// for, and it is *not* workload-independent: RSS reaches its plateau in about
/// thirty seconds over the default fixture and in about **eight hours** over the
/// 100 x 500 one, so a tenth of a 24-hour run discards 2.4 hours of a climb
/// that takes eight. The first full-window soak failed at **13.26%**
/// against a 5% budget with its own back half flat to 0.02 MiB/h. [`warmup`]
/// measures the plateau instead and this is the least it may ever discard.
///
/// This is the RSS baseline's floor and nothing else. Descriptors have their
/// own, [`FD_WARMUP_FRACTION`], which is the same number for a different reason.
const WARMUP_FRACTION: f64 = 0.10;

/// Samples [`Report::gate_descriptors`] discards before its baseline.
///
/// **Equal to [`WARMUP_FRACTION`] and deliberately not the same constant**, and
/// what separates them is that only one of the two is a floor. RSS climbs for
/// hours at some fixtures, so its prefix is a lower bound under a measured
/// plateau; descriptors are a small bounded integer that reaches its level in
/// seconds, so there is no ramp for a detector to find and this fraction is the
/// whole rule. Retuning the RSS floor is a change to where a 5% budget is
/// measured from, and it must not silently move a handle-leak baseline as well:
/// one number serving two rules is a number that is right for at most one of
/// them the first time either moves.
///
/// The gate it feeds takes a **maximum** over the prefix rather than a median,
/// which is why it can afford a coarse prefix: [`FD_HEADROOM`] is wide enough
/// that the exact cut does not decide the verdict.
const FD_WARMUP_FRACTION: f64 = 0.10;

/// How close a rolling quarter median sits to the settled level before the
/// series counts as plateaued.
///
/// A fifth of [`DRIFT_BUDGET`], and **the direction the error falls in is what
/// makes the number safe rather than tuned**. Too tight and no plateau is found,
/// [`warmup`] falls back to [`WARMUP_FRACTION`], and the gate is exactly the one
/// that shipped before it. Too loose and the plateau is called early, which
/// weakens the gate. So the conservative direction is down, and 1% is on a
/// smooth part of the curve rather than a cliff: over the recorded 288-sample
/// day (`statistic::DAY_LONG`) the walk reaches sample 135 at a band of 0.5%,
/// 101 at 0.75%, 97 at 1%, 88 at 1.25%, 86 at 1.5% and 76 at 2%. Those are the
/// walk's positions and not all of them become baselines: at 0.5% the lag
/// carries 135 past the ceiling and the floor stands, which is the tightening
/// direction behaving as this doc says it does.
const PLATEAU_BAND: f64 = 0.01;

/// The share of a run's whole climb that has to happen in its first quarter
/// before the prefix counts as a warmup: see [`rose_from_the_start`].
///
/// A sixteenth, chosen against a measured separation rather than picked: the
/// recorded day's share is **24.2%**, about four times this, and the one-page
/// attack that defeated a bare "did it rise" test is **0.013%**, about five
/// hundred times under it. Anything between roughly a thousandth and a quarter
/// separates those two, so the exact value is not load bearing and the margin
/// on both sides is.
const RISE_SHARE: u64 = 16;

/// Most of a run the warmup may ever consume.
///
/// Past it [`warmup`] reports [`Baseline::Unsettled`] and hands the gate the
/// whole window from the floor, so a process that has not settled by this much
/// of its own run is measured rather than trusted. **A leak looks exactly like
/// this**, which is the point: the alternative — believing a late-arriving
/// plateau — is a detector that adapts to a slow leak and hides it.
///
/// **Three fifths and not a half, because a half is where the one real workload
/// on record actually sits.** The recorded day's rolling median comes inside the
/// band at sample 97 of 288, and the walk lags the true settle point by half its
/// own width, so that day genuinely warms up for about **46%** of itself. At a
/// half the clearance is eleven samples, and an audit round showed that adding
/// less noise than the series' own jitter crossed it on **93 of 300 seeds**: a
/// gate whose only real fixture is that close to its own cliff fails on the
/// weather. At three fifths the same day clears by 39 samples, which
/// `statistic::the_recorded_day_clears_the_ceiling_under_its_own_jitter` holds
/// as a number rather than as a hope.
///
/// It is a real loosening and worth saying so: it permits a warmup of three
/// fifths of a run, leaving two fifths for the comparison. That is still 115
/// samples at the full count and four quarters of 28, and the alternative is a
/// constant tuned so tightly to one measurement that the measurement itself only
/// just passes.
const WARMUP_CEILING: f64 = 0.60;

/// I3's budget: RSS drift over the window.
///
/// `VIGIA_BUDGET_SLACK` deliberately does not reach it. That multiplier exists
/// because a hosted runner's *wall clock* is not a property of this code
/// (`SPEC.md` §7), and this is a ratio of a process against itself: a slower
/// machine takes fewer frames in the window, it does not leak more per frame.
const DRIFT_BUDGET: f64 = 0.05;

/// Samples each end of the comparison needs before there is a verdict at all.
///
/// Two, so a single unlucky sample cannot be a median. Below it [`drift`]
/// reports nothing rather than a number it cannot stand behind: `SPEC.md` §7
/// makes refusing the point rather than the fallback.
const MIN_END: usize = 2;

/// Bytes in a mebibyte, and seconds in an hour.
///
/// Named because this file quotes RSS in MiB everywhere and its report is read
/// beside the shell's own `19MiB` cell: several sites agreeing on one divisor
/// by eye is how the two come to disagree.
const MIB: f64 = 1024.0 * 1024.0;
const SECS_PER_HOUR: f64 = 3600.0;

/// A byte count as the MiB every number in the report is quoted in.
fn mib(bytes: u64) -> f64 {
    bytes as f64 / MIB
}

/// An elapsed time, in whichever unit puts a significant figure on the page.
///
/// `{:.2}h` alone renders every span under **eighteen seconds** as `0.00h`,
/// which covers the whole band [`Drift::span`] exists to explain: `+903 MiB/h
/// over 0.00h` reads as a division by zero rather than as a very short lever
/// arm.
fn span(elapsed: Duration) -> String {
    let seconds = elapsed.as_secs_f64();
    if seconds < SECS_PER_HOUR {
        format!("{seconds:.1}s")
    } else {
        format!("{:.2}h", seconds / SECS_PER_HOUR)
    }
}

/// What a series of RSS samples did after it warmed up.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Drift {
    /// Medians of a quarter's worth of samples at four positions, in order.
    ///
    /// The **ends are the gate** and the middle two are diagnostic, which is
    /// why they are not sliced the obvious way. `quarters[0]` and `quarters[3]`
    /// are exactly [`Drift::baseline`] and [`Drift::settled`], so taking the
    /// four at `len * k / 4` would put `quarters[3]`'s left edge a sample
    /// earlier on any series whose length is not a multiple of four — 259
    /// post-warmup samples is what a 288-sample run leaves at the floor, and
    /// 191 is what the recorded day leaves at its measured plateau — and
    /// the median I3 asserts on would then be taken over a different window.
    /// So the middle pair is measured a quarter's width *inward from each end*
    /// instead, leaving a gap of up to three samples in the centre that decides
    /// nothing, which is what §7's rule about the two ends leaves free.
    ///
    /// Four rather than two because a *shape* is what separates a plateau from
    /// a trend: §10's four rise monotonically, 25.58, 25.86, 25.90 and 26.14
    /// MiB, and no pair of endpoints can show that.
    quarters: [u64; 4],
    /// `(settled - baseline) / baseline`, and never negative: a process that
    /// gave memory back has not drifted, and reporting that as a signed number
    /// invites a threshold that passes for the wrong reason.
    ratio: f64,
    /// Samples discarded before [`Drift::baseline`]. See [`warmup`].
    warm: usize,
    /// Which rule discarded them, which is the half a sample count cannot
    /// carry: five of the six discard exactly the floor and mean five
    /// different things. [`warmup`] chooses all but [`Baseline::TooShort`] and
    /// [`Baseline::Trough`], which [`drift`] owns because each needs something
    /// `warmup` cannot see: the slicing's arity, and the floor's own answer.
    baseline_rule: Baseline,
    /// Elapsed time of the first sample the gate looks at.
    ///
    /// Printed rather than derived, for the reason [`Drift::span`] gives about
    /// the gradient: a warmup quoted in *samples* is only a duration while the
    /// cadence is known, and the whole point of a measured warmup is that it is
    /// a property of the workload rather than of the window.
    at: Duration,
    /// The same ratio taken from [`WARMUP_FRACTION`] instead of the measured
    /// baseline. **Reported, never gated.**
    ///
    /// What the measured baseline absorbed, on the page rather than hidden.
    /// [`WARMUP_CEILING`]'s doc names the failure this closes: a detector that
    /// adapts to a slow leak would hide it, and the honest answer to that is not
    /// a cleverer detector but printing the number the detector moved. Equal to
    /// [`Drift::ratio`] whenever the baseline came from the floor, which is
    /// every run shorter than its own warmup and every leak.
    from_floor: f64,
    /// Least-squares gradient over the post-warmup series, in MiB per hour.
    ///
    /// **Signed, where [`Drift::ratio`] deliberately is not, and the asymmetry
    /// is the point.** `ratio` is the gate, so it must not pass for the wrong
    /// reason and a shrinking process reports zero. This is a *diagnostic*, and
    /// its whole job is the sign: `SPEC.md` §10's open question is that one
    /// hour of this code slopes **+0.92 MiB/h** and fifteen minutes of the same
    /// code slopes **−0.70 MiB/h**, both about one percent of the +81.3 MiB/h
    /// an injected 1 KiB-per-frame leak produced. A statistic that clamped the
    /// negative away could not state that disagreement, let alone settle it.
    ///
    /// Never gated. A threshold on this would be a budget `SPEC.md` does not
    /// name, and §10's rule for drift that crosses on variation rather than on
    /// a leak is a measured warmup or a measured budget, never a wider one.
    slope: f64,
    /// Elapsed time the gradient was fitted across, which is its lever arm.
    ///
    /// **A gradient without this is not comparable to another gradient, and
    /// comparing them is exactly what §10 asks a reader to do.** For one fixed
    /// amount of RSS wander the reported MiB/h goes as the *reciprocal* of the
    /// window, because the same rise is divided by a longer and longer base.
    /// Reference machine, by window, and the provenance is not uniform so
    /// `SPEC.md` §7 keeps the split rather than flattening it. From this
    /// instrument: **+364.71 MiB/h** at 15s, **+7.89** and **+2.74** at 600s,
    /// the first three over the default 20 x 200 fixture, and **+1.94** at 900s
    /// over 100 x 500. From hand arithmetic off the printed series, on code
    /// that predates the retained span map and the grammar warmer: **−0.70** at
    /// 900s and **+0.92** at 3600s. Nothing is wrong with any of them; the
    /// 15-second one is eleven seconds of lever arm and says so once this is
    /// printed beside it.
    ///
    /// So the statistic only becomes readable somewhere around the window I3's
    /// budget names, which is an argument for running that window rather than
    /// against the statistic. Printing the span is what stops the shorter runs
    /// from looking like the same kind of number.
    span: Duration,
}

impl Drift {
    /// Median of the first quarter after the warmup.
    fn baseline(&self) -> u64 {
        self.quarters[0]
    }

    /// Median of the last quarter.
    fn settled(&self) -> u64 {
        self.quarters[3]
    }
}

/// The nearest-rank median: a value that actually occurred, never an
/// interpolation between two that did.
///
/// The same convention [`vigia_core::Samples::percentile`] uses, and it has to
/// be the same one, or two numbers in the same report would mean different
/// things.
fn median(values: &[u64]) -> Option<u64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let rank = (0.5 * sorted.len() as f64).ceil() as usize;
    Some(sorted[rank.saturating_sub(1).min(sorted.len() - 1)])
}

/// The four quarter medians of an already-warm series, or `None` when it is too
/// short to have two ends.
///
/// Split out of [`drift`] because the gate needs the same slicing twice: once at
/// the measured baseline, which is the number it asserts on, and once at
/// [`WARMUP_FRACTION`] for [`Drift::from_floor`], which is only ever printed.
/// Two spellings of one cut would be two answers to one question.
fn quarter_medians(values: &[u64]) -> Option<[u64; 4]> {
    // A quarter at each end, so the two never overlap however short the series
    // is and the middle half is free to wander without deciding anything.
    let quarter = values.len() / 4;
    if quarter < MIN_END {
        return None;
    }
    let end = values.len();
    // Inward from each end rather than `end * k / 4`: see [`Drift::quarters`].
    // The two never overlap, because `quarter` is a floor of a quarter, so
    // `4 * quarter <= end` and therefore `2 * quarter <= end - 2 * quarter`.
    Some([
        median(&values[..quarter])?,
        median(&values[quarter..quarter * 2])?,
        median(&values[end - quarter * 2..end - quarter])?,
        median(&values[end - quarter..])?,
    ])
}

/// The gate's own statistic over four quarter medians: see [`Drift::ratio`].
///
/// The caller has checked `quarters[0]` is not zero; a baseline of zero is a
/// platform that never answered rather than a process that used nothing.
fn ends_ratio(quarters: [u64; 4]) -> f64 {
    quarters[3].saturating_sub(quarters[0]) as f64 / quarters[0] as f64
}

/// The least a run may discard before its baseline: see [`WARMUP_FRACTION`].
fn warmup_floor(len: usize) -> usize {
    (len as f64 * WARMUP_FRACTION).ceil() as usize
}

/// Which rule placed the baseline, so a report and a failure can say.
///
/// **Six outcomes where a sample count carries one**, and five of them discard
/// exactly [`WARMUP_FRACTION`]'s floor for five different reasons. A reader who
/// is told only "29 samples" cannot tell a process that was already flat from
/// one that never settled, from one whose rise started too late to be a warmup,
/// from one where moving the baseline made the drift worse. Those want different
/// responses, so they get different names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Baseline {
    /// Already within [`PLATEAU_BAND`] of its settled level by the floor, so the
    /// floor is what was discarded. Every window short enough to plateau in its
    /// first tenth is this, which is every run this repository takes per commit.
    Floor,
    /// A plateau was found past the floor, and the climb up to it is what was
    /// discarded. The day-long run over the 100 x 500 fixture is this.
    Measured,
    /// Nothing settled inside [`WARMUP_CEILING`], so the floor is used and the
    /// gate measures the whole climb.
    ///
    /// **A leak looks exactly like this**, and so does a window shorter than its
    /// own warmup, and both should fire. Telling the two apart is the reader's
    /// job from the quarters and the gradient printed beside the verdict, which
    /// is the division `SPEC.md` §7 already draws between what is gated and what
    /// is reported.
    Unsettled,
    /// The measured plateau would have left the gate without two ends, so the
    /// floor stands.
    ///
    /// **Distinct from [`Baseline::Unsettled`] because the cause is arithmetic
    /// rather than the workload**, and merging the two produced a false message
    /// on the most-run path in this repository. [`MIN_SAMPLES`] is 12, which is
    /// what the fifteen-second per-commit soak takes: `4 * MIN_END` leaves 4
    /// where [`WARMUP_CEILING`] allows 6, so a plateau found 42% of the way in
    /// is inside the ceiling and still too late to slice. Reported as
    /// `Unsettled` it printed "no plateau early enough to believe" and would
    /// have failed saying "nothing settled inside half the window", both of
    /// which are untrue of that run: something settled, and the run is simply
    /// too short to take a baseline there.
    TooShort,
    /// A plateau was found, but the series was already flat when the run began,
    /// so what precedes the plateau is a rise the process made *after* settling.
    ///
    /// **This is the leak-laundering case, and it is the reason the detector
    /// looks at the start of the run and not only at the end of the climb.**
    /// Warmup is a prefix a process is in *because it just started*: allocator
    /// arenas filling, caches reaching their working set. "flat, then a rise,
    /// then flat" has the same tail as "warmup, then flat" and is not the same
    /// thing at all: it is a process that took more memory and kept it.
    ///
    /// Measured, on the shipped rule before this variant existed: 100 MiB flat
    /// for 86 samples of 288, a linear rise to 120 MiB over the next 86, then
    /// 120 MiB flat. The baseline landed at 132 and the gate reported **4.24%**
    /// against a 5% budget, where the whole window reports 20.00%. A rise of any
    /// width above half the rolling window is absorbed whole that way, and
    /// `SPEC.md` §7's "a step is not absorbed" only ever covered the
    /// instantaneous case.
    RoseLate,
    /// Moving the baseline forward made the drift *worse*, so the floor stands.
    ///
    /// **A measured baseline may only ever reduce the measured drift**, and that
    /// is not a preference but what the operation means: dropping a warmup climb
    /// out of the comparison raises the first quarter's median and lowers the
    /// ratio. A baseline that raises the ratio did not drop a climb; it parked
    /// itself inside a trough, and the dip then reads as drift the window does
    /// not have.
    ///
    /// Measured, before this clamp existed: a process flat at 100 MiB with a dip
    /// to 95 MiB across samples 104 to 140 of 288 reported **5.26%** and failed,
    /// on a series whose first and last samples are the same number and whose
    /// whole-window ratio is 0.00%. A quiet stretch shrinking RSS is ordinary
    /// here, because `SPEC.md` §7 bounds the caches by the current diff.
    Trough,
}

/// Whether the run was still climbing when it began, which is what makes a
/// prefix a warmup rather than a leak.
///
/// **The detector's other half, and the one that is about the start of the run
/// rather than the end of the climb.** A plateau at the end says only that the
/// series stopped moving; it cannot tell "warmup, then flat" from "flat, then a
/// rise, then flat", and the second is a process that took more memory and kept
/// it. Warmup is a prefix a process is in *because it just started*, so the
/// discarded region has to begin at the start.
///
/// Compares the first eighth of the series against the second, which is the
/// coarsest cut that can see it: both sit inside the first quarter, so a genuine
/// warmup is still climbing across them, and a run that was flat at its start
/// shows no rise at all.
///
/// **The rise is measured against the climb it is part of, and a bare "did it go
/// up at all" is what two earlier versions got wrong in opposite directions.**
/// A fixed band of [`PLATEAU_BAND`] rejected this module's own ramp fixtures,
/// which rise only 0.70% across the cut: a genuine warmup may be slow. Strictly
/// greater then admitted **one 4 KiB page**, which is how an audit round got a
/// 29.5% leak to pass at 4.77%: prepend a single-page step to a flat head and
/// the whole prefix reads as a climb. Neither absolute test can work, because
/// the same rise is decisive on a series that climbs 6 MiB and meaningless on
/// one that climbs 30.
///
/// So the question is what **share** of the climb happened in the first quarter,
/// and [`RISE_SHARE`] is the fraction it must be. Measured, and the separation
/// is four orders of magnitude wide: the recorded day rises 1.441 MiB across the
/// cut against a 5.945 MiB climb, a share of **24.2%**, and the one-page attack
/// rises 0.0039 MiB against 29.5 MiB, a share of **0.013%**.
///
/// It is still a threshold and it still has a boundary, which is worth stating
/// rather than hiding: a rise that begins near it flips between this and the
/// floor. That is tolerable here and would not be anywhere else, because the two
/// sides are not pass and fail. Refusing gives [`Baseline::RoseLate`] and the
/// floor, which is the gate that shipped before any of this and always the safer
/// answer, so the boundary separates "measured" from "conservative" rather than
/// "green" from "red".
fn rose_from_the_start(values: &[u64], quarter: usize, level: u64) -> bool {
    let eighth = quarter / 2;
    let (Some(first), Some(second)) = (median(&values[..eighth]), median(&values[eighth..quarter]))
    else {
        // Unreachable: [`quarter_medians`] answered `Some` before this is
        // called, so `quarter >= MIN_END` and `eighth >= 1`, and neither slice
        // is empty. `false` rather than `true` all the same, because an
        // unreachable branch should still fail towards the floor.
        return false;
    };
    second.saturating_sub(first) >= level.saturating_sub(first) / RISE_SHARE
}

/// Samples to discard before the baseline, and the rule that chose them.
///
/// **The plateau is measured from the series rather than assumed from the
/// window**, which is [#126](https://github.com/breferrari/vigia/issues/126) and
/// the reason is in [`WARMUP_FRACTION`]: a tenth of the window is the right
/// prefix at a fixture that plateaus in thirty seconds and discards 2.4 hours
/// of one that takes eight.
///
/// The **settled level** is the median of the last quarter of the whole series,
/// taken over the whole series on purpose: a reference computed after the warmup
/// would move with the answer it is being used to find. A rolling window of that
/// same quarter width then walks back from the end while its median stays within
/// [`PLATEAU_BAND`] of the level, and where it stops is the baseline.
///
/// **A quarter wide, because that is the width whose median the gate already
/// trusts**, and narrower does not work: over the recorded day
/// (`statistic::DAY_LONG`) disjoint blocks of eighteen samples leave the settled
/// region stepping by up to 2.30% against a 9.72% step across the ramp, which is
/// not a margin. At a quarter's width the settled region sits inside 1% of its
/// level and the ramp is nowhere near it.
///
/// **Only a rise the run began inside is absorbed**, which is the property that
/// makes this a warmup detector rather than a leak launderer, and the walk alone
/// does not give it. A plateau at the end says the series stopped moving and
/// says nothing about what it did at the start, so "flat, then a rise, then
/// flat" and "warmup, then flat" reach this point indistinguishable.
/// [`rose_from_the_start`] is what tells them apart, and it is why every
/// synthetic step in this module's tests reports [`Baseline::RoseLate`]: a step
/// is flat before it, so there is no warmup to measure.
///
/// A rolling *median* also lags a step by half its own width, which keeps the
/// pre-step samples inside the gated region even where a baseline is measured.
/// That is a second line rather than the first: it holds for a step and fails
/// for a rise wider than `quarter / 2`, which is a twentieth of a day-long run,
/// and a version of this function that leaned on it alone passed a 20% leak at
/// 4.24%.
fn warmup(values: &[u64]) -> (usize, Baseline) {
    let floor = warmup_floor(values.len());
    let quarter = values.len() / 4;
    // **The level comes through the gate's own cut rather than a second
    // spelling of it.** `quarters[3]` *is* the median of the last quarter, so
    // taking it here makes "the same width the gate trusts" structural instead
    // of a claim two expressions have to keep agreeing on, which is the
    // argument [`quarter_medians`] already makes for itself. `None` is a series
    // too short to have two ends, and there is nothing to measure against.
    //
    // **A zero level is not guarded here, and that is deliberate.** It reads
    // like an oversight, so: a filter for it was written, and a mutation showed
    // it could never fire. At `level == 0` the division below is an infinity or
    // a `NaN`, both of which compare false, so `settled` is false at every
    // position, the walk stops where it started, and the ceiling refuses. The
    // outcome is the floor either way. Guarding it would be a branch no
    // mutation can kill, which is what `mib_per_hour`'s doc deletes one for.
    // The refusal a zero series actually needs belongs to [`drift`], which
    // checks *both* ends because clamping a fall to zero makes a vanished
    // series read as the flattest one ever measured.
    let Some(level) = quarter_medians(values).map(|whole| whole[3]) else {
        return (floor, Baseline::Floor);
    };

    let settled = |at: usize| {
        median(&values[at..at + quarter])
            .is_some_and(|m| m.abs_diff(level) as f64 / (level as f64) < PLATEAU_BAND)
    };
    let mut at = values.len() - quarter;
    while at > 0 && settled(at - 1) {
        at -= 1;
    }

    // **Only the workload rule lives here.** An earlier version also clamped by
    // `len - 4 * MIN_END`, which is [`quarter_medians`]' arity rather than
    // anything about the process, and merging the two meant one outcome name
    // for two causes: at [`MIN_SAMPLES`] the arity bound is the tighter of the
    // pair, so the per-commit soak reported a plateau it *had* found as
    // "nothing settled inside half the window". The arity belongs beside the
    // slicing, and [`drift`] holds it as [`Baseline::TooShort`].
    //
    // **The lag is not slack.** A rolling median of width `quarter` only comes
    // inside the band once about half of it is past the true settle point, so
    // `at` names a position half a window *earlier* than where the series
    // actually flattened. Comparing `at` alone against the ceiling therefore
    // bounds the warmup at 64% of the run while `WARMUP_CEILING` says 50%:
    // measured, a climb ending at 63% was still `Measured`, against an
    // analytic bound of 62.5%.
    // A budget that holds at a different number from the one it documents is
    // the shape `SPEC.md` §7 exists to refuse, so the lag is added back.
    let ceiling = (values.len() as f64 * WARMUP_CEILING) as usize;
    if at <= floor {
        (floor, Baseline::Floor)
    // **Was it a warmup at all, before asking whether it was too long a one.**
    // The order is the meaning: a run that was already flat when it started has
    // no warmup to measure however early the rise afterwards finished, so asking
    // about its length first would answer the wrong question and report a leak
    // as a window too short for its own warmup.
    } else if !rose_from_the_start(values, quarter, level) {
        (floor, Baseline::RoseLate)
    } else if at + quarter / 2 > ceiling {
        (floor, Baseline::Unsettled)
    } else {
        (at, Baseline::Measured)
    }
}

/// What `rss` drifted by, or `None` when the series cannot answer.
///
/// Medians at both ends rather than first-against-last, because RSS jitters by
/// a page or two between samples and a leak is a *trend*: one unlucky sample at
/// either end would otherwise decide a 5% budget.
///
/// Pure, and tested directly, for the reason `SPEC.md` §7 gives about the
/// racily-clean guard: the rule governs a 24-hour window and no test can wait
/// one, so a test that drives it with a series it built is the only gate that
/// can exist. A leak in the harness that never leaked, and a plateau that never
/// plateaued, are both reachable here in microseconds.
fn drift(samples: &[(Duration, u64)]) -> Option<Drift> {
    // Projected once, so every window below is a slice of one buffer rather
    // than a throwaway copy, and so `median` keeps the `&[u64]` signature its
    // own tests call it through.
    let values: Vec<u64> = samples.iter().map(|&(_, rss)| rss).collect();
    let floor = warmup_floor(values.len());
    let (measured_warm, measured_rule) = warmup(&values);

    // **A measured baseline may not take the verdict away.** The floor answered
    // before #126 and has to keep answering, so where the plateau would leave
    // too few samples to have two ends, the floor stands and says so. The retry
    // is here rather than inside [`warmup`] because the bound it needs is
    // [`quarter_medians`]' arity, which is a fact about the slicing beside it
    // and not about the process: the version that clamped it inside `warmup`
    // labelled a plateau it had genuinely found as one it had not.
    let (warm, baseline_rule, quarters) = match quarter_medians(&values[measured_warm..]) {
        Some(quarters) => (measured_warm, measured_rule, quarters),
        None => (
            floor,
            Baseline::TooShort,
            quarter_medians(&values[floor..])?,
        ),
    };
    // A zero at either end means the platform stopped reporting RSS, and
    // neither end may be one. A zero *baseline* makes the ratio an infinity that
    // passes or fails by luck. A zero *settled* figure is worse, because it
    // passes quietly: `ends_ratio` clamps a fall to zero, so a run whose reads
    // vanish half way through reports 0.00% drift and looks like the flattest
    // process ever measured. Only the first was guarded until a mutation showed
    // [`warmup`]'s own `level > 0` filter could never fire, which is the same
    // hole one function further in.
    if quarters[0] == 0 || quarters[3] == 0 {
        return None;
    }

    // The floor's own answer, which is both the printed comparison and the
    // clamp below. Taken through the same cut, so the two cannot describe
    // different windows.
    let floor_ends = quarter_medians(&values[floor..]).filter(|ends| ends[0] != 0);
    let from_floor = floor_ends.map_or_else(|| ends_ratio(quarters), ends_ratio);

    // **A measured baseline may only ever reduce the measured drift**, which is
    // what the operation means rather than a preference: dropping a warmup climb
    // out of the comparison raises the first quarter's median and lowers the
    // ratio. One that *raises* it did not drop a climb, it parked itself inside
    // a trough, and the dip then reads as drift the window does not have. See
    // [`Baseline::Trough`] for the series that measured 5.26% against a 0.00%
    // window before this clamp existed.
    let (warm, baseline_rule, quarters, ratio) = match floor_ends {
        Some(ends) if ends_ratio(ends) < ends_ratio(quarters) => {
            (floor, Baseline::Trough, ends, ends_ratio(ends))
        }
        _ => (warm, baseline_rule, quarters, ends_ratio(quarters)),
    };
    // Indexed rather than guarded: `quarter_medians` answered `Some` at `warm`
    // in both arms above, so this slice holds at least `4 * MIN_END` samples. A
    // `get(..)?` here is a branch no mutation can kill, which is what
    // `mib_per_hour`'s own doc deletes one for.
    let rest = &samples[warm..];

    Some(Drift {
        quarters,
        ratio,
        warm,
        baseline_rule,
        // The first sample the gate looks at, not the last one it discarded:
        // the report says where the measurement *starts*. Indexed rather than
        // guarded, because `quarter_medians` above returned `Some`, which means
        // `rest` holds at least `4 * MIN_END` samples: a fallback here would be
        // a branch no mutation could kill, which is what `mib_per_hour`'s own
        // doc deletes one for.
        at: rest[0].0,
        from_floor,
        // Over `rest`, not over `samples`: the warmup climb is not drift, and a
        // gradient fitted through it would report the allocator plateau as a
        // trend for exactly the reason the medians discard it.
        slope: mib_per_hour(rest),
        // The lever arm the line above was divided by. Same slice, so the two
        // cannot describe different windows.
        span: match (rest.first(), rest.last()) {
            (Some((first, _)), Some((last, _))) => last.saturating_sub(*first),
            _ => Duration::ZERO,
        },
    })
}

/// The least-squares gradient of `samples`, in MiB per hour.
///
/// Ordinary linear regression of RSS against **elapsed time**, so the answer is
/// in the units `SPEC.md` §10 states its open question in and is comparable
/// between runs of different lengths. Fitting against the sample *index*
/// instead would give a slope per sample, which is only a slope per hour while
/// the loop keeps its cadence, and a day-long run that stalled would report a
/// climb it did not have.
///
/// Least squares rather than first-against-last for the reason the medians
/// exist: RSS jitters by a page between samples, and a fit uses all of them.
///
/// **Zero when there is no line to fit** — no samples, one sample, or every
/// sample at the same instant — because dividing by a zero variance gives an
/// infinity that reads as a catastrophic leak. One guard covers all three, and
/// it is the one below rather than a length check in front: with fewer than two
/// samples the loop contributes nothing and `variance` is still zero, so a
/// leading `samples.len() < 2` was a branch no mutation could kill. Verified by
/// deleting it and watching the suite stay green, which is what
/// [`statistic::a_series_with_nothing_to_fit_through_reports_no_gradient`] then
/// had to be able to say about the survivor.
///
/// The empty case does compute `0.0 / 0.0` for the means. That `NaN` is never
/// read: the loop does not run, so the return below is reached first.
fn mib_per_hour(samples: &[(Duration, u64)]) -> f64 {
    let count = samples.len() as f64;
    let mean_at = samples.iter().map(|(at, _)| at.as_secs_f64()).sum::<f64>() / count;
    let mean_rss = samples.iter().map(|&(_, rss)| rss as f64).sum::<f64>() / count;

    let (mut covariance, mut variance) = (0.0, 0.0);
    for &(at, rss) in samples {
        let offset = at.as_secs_f64() - mean_at;
        covariance += offset * (rss as f64 - mean_rss);
        variance += offset * offset;
    }
    if variance == 0.0 {
        return 0.0;
    }

    // Bytes per second into MiB per hour, which is what the report prints.
    covariance / variance * SECS_PER_HOUR / MIB
}

/// Resident set size of this process, or `None` where the platform has no way
/// to say.
///
/// **The shipped reader, not a second one.** This file carried its own three
/// `#[cfg]` bodies until [#41](https://github.com/breferrari/vigia/issues/41)
/// put a memory cell on the status bar, and then there were two answers to one
/// question about one process. `vigia::memory` is the only one now, and that is
/// what makes the number on screen and the number in this report comparable:
/// `tasklist` and `GetProcessMemoryInfo` disagree by a few percent on the same
/// process because they sample at different instants, so a series mixing sources
/// reads as drift.
///
/// The subprocess readers went with them. They were right for a soak and wrong
/// for a frame — 288 samples across an hour against sixty a second — and what
/// changed is that the cheap answer turned out to exist on every tier-1 target
/// through a crate `gix` already puts in the graph. `crates/vigia/src/memory.rs`
/// carries the measurement that decided it.
fn rss_bytes() -> Option<u64> {
    vigia::memory::resident()
}

/// Descriptors this process holds open, where that is free to ask.
///
/// I3's prose names file handles alongside RSS, and on Linux the answer is a
/// directory listing. Everywhere else it reports unavailable, which the report
/// prints: a metric that silently returns "fine" on two platforms out of three
/// would read as coverage it does not have.
#[cfg(target_os = "linux")]
fn open_files() -> Option<usize> {
    Some(std::fs::read_dir("/proc/self/fd").ok()?.count())
}

#[cfg(not(target_os = "linux"))]
fn open_files() -> Option<usize> {
    None
}

/// How many samples a window of this length gets.
///
/// The count is what is fixed, not the interval: see [`MAX_SAMPLES`].
fn samples_for(window: Duration) -> usize {
    (window.as_secs() as usize / 2).clamp(MIN_SAMPLES, MAX_SAMPLES)
}

fn env_var<T: std::str::FromStr>(name: &str, fallback: T) -> T {
    std::env::var(name)
        .ok()
        .and_then(|raw| raw.parse().ok())
        .unwrap_or(fallback)
}

/// One reading of everything I3 bounds.
struct Sample {
    at: Duration,
    /// Zero means the platform declined, and the gates treat that as a failure
    /// rather than as a flat curve.
    rss: u64,
    fds: Option<usize>,
    /// Diffs [`vigia_core::Frame`] is holding between frames.
    tracked_diffs: usize,
    /// Heights the same [`vigia_core::Frame`] is holding between frames.
    ///
    /// **A fourth retained cache, and it needs its own reading rather than the
    /// diffs' one.** The two populations are not the same: a diff is kept only
    /// for a file something has drawn, where a span is kept for every changed
    /// file the moment anything totals the diff. So `tracked_diffs` is small
    /// while this is the whole changed set, and a bound asserted over the first
    /// says nothing at all about the second.
    ///
    /// It exists because [#101](https://github.com/breferrari/vigia/issues/101)
    /// stopped clearing this map on every tick, which is what previously made it
    /// unable to grow. What bounds it now is the migration in `Frame::advance`,
    /// and this is where a soak can see that hold over hours of churn rather
    /// than over one fixture.
    tracked_spans: usize,
    /// Hunk parses [`vigia_core::Highlighter`] is holding between frames.
    tracked_hunks: usize,
    /// Paths [`vigia_core::History`] is holding, which is I10's own number.
    tracked_history: usize,
    /// Changed files in the whole worktree at that moment.
    files: usize,
    /// Body height of the last frame drawn, which is what bounds the hunk
    /// cache.
    body: usize,
}

/// Everything one soak produced.
struct Report {
    window: Duration,
    samples: Vec<Sample>,
    frames: u64,
    full_frames: u64,
    ticks: u64,
    /// Rounds the writer completed.
    rounds: u64,
    /// Files the writer created, which is how many paths this run invented.
    created: u64,
    fixture_files: usize,
    /// Frames whose walk, scroll or collect failed. A file that vanishes
    /// between status naming it and the diff reading it is ordinary here, for
    /// the reason `SPEC.md` §2 gives: the workload deletes files on purpose.
    failed: u64,
    last_error: Option<String>,
    /// The frame that came closest to breaking the viewport bound, as
    /// `(parses held, parses the screen could have asked for)`.
    ///
    /// The sharp version of "bounded by the viewport", and it has to be
    /// per-frame rather than per-sample: it is a comparison against *this*
    /// screen's hunks, which is a number that exists for one frame only.
    /// Against the body height instead, the bound is loose by construction, and
    /// no workload can close the gap: a hunk is a header plus at least one line
    /// plus up to `2 * CONTEXT` context rows, so a forty-row body cannot show
    /// more than about five of them however the diff is shaped.
    closest_hunk_bound: Option<(usize, usize)>,
    frame: FrameStats,
    highlight: HighlightStats,
    history: HistoryStats,
}

impl Report {
    /// Distinct paths that were ever part of the diff.
    ///
    /// Counted rather than collected. A `HashSet` of every path would be
    /// harness memory growing with the run, inside the process whose memory is
    /// the measurement, which is a leak this test would then blame on the
    /// product.
    fn paths(&self) -> u64 {
        self.fixture_files as u64 + self.created
    }

    fn max_tracked_diffs(&self) -> usize {
        self.samples
            .iter()
            .map(|s| s.tracked_diffs)
            .max()
            .unwrap_or(0)
    }

    fn max_tracked_spans(&self) -> usize {
        self.samples
            .iter()
            .map(|s| s.tracked_spans)
            .max()
            .unwrap_or(0)
    }

    fn max_tracked_hunks(&self) -> usize {
        self.samples
            .iter()
            .map(|s| s.tracked_hunks)
            .max()
            .unwrap_or(0)
    }

    fn max_tracked_history(&self) -> usize {
        self.samples
            .iter()
            .map(|s| s.tracked_history)
            .max()
            .unwrap_or(0)
    }

    /// The RSS series against the elapsed time each sample was taken at.
    ///
    /// What [`drift`] needs, and it has to be the real elapsed [`Duration`]
    /// rather than the sample index: the index is only a clock while the loop
    /// keeps up, and a gradient in MiB *per hour* read off a stalled day-long
    /// run would be wrong in exactly the direction that looks like a leak.
    ///
    /// The only accessor for it. A bare `Vec<u64>` sibling went with the move,
    /// because the three remaining places that want values alone — the min and
    /// max in [`Report::print`], the peak in [`Report::gate_drift`], and the
    /// printed curve — read `samples` directly instead.
    ///
    /// All three are report-time and so is this: nothing here runs while the
    /// process is still being measured, which is the rule [`Report::paths`]
    /// states and the only reason any of it may allocate at all. The curve is
    /// much the largest allocation of the four, a `String` per sample, and is
    /// equally harmless for the same reason.
    fn series(&self) -> Vec<(Duration, u64)> {
        self.samples.iter().map(|s| (s.at, s.rss)).collect()
    }

    fn print(&self) {
        let mb = mib;
        println!(
            "soak: window {:?}, {} samples, {} frames ({} full), {} ticks, \
             {} write rounds, {} files created",
            self.window,
            self.samples.len(),
            self.frames,
            self.full_frames,
            self.ticks,
            self.rounds,
            self.created
        );
        match drift(&self.series()) {
            Some(drift) => {
                println!(
                    "soak: rss baseline {:.1} MiB, settled {:.1} MiB, drift {:.2}% \
                     (budget {:.0}%), min {:.1}, max {:.1}",
                    mb(drift.baseline()),
                    mb(drift.settled()),
                    drift.ratio * 100.0,
                    DRIFT_BUDGET * 100.0,
                    mb(self.samples.iter().map(|s| s.rss).min().unwrap_or(0)),
                    mb(self.samples.iter().map(|s| s.rss).max().unwrap_or(0)),
                );
                // Where the baseline was taken and by which rule, and what the
                // window fraction would have said instead.
                //
                // **A gate whose baseline moves has to say where it put it**,
                // and the second half of this line is the answer to
                // `WARMUP_CEILING`'s own objection: a detector that adapts to a
                // slow leak would hide it, so the figure it moved is printed
                // beside the one it produced rather than left to be recovered
                // from the series at the bottom of this report. On every run
                // whose baseline came from the floor the two are equal, which
                // is most of them and is not worth a branch to suppress.
                println!(
                    "soak: rss warmup {} of {} samples ({} in), {}; \
                     {:.2}% from the {:.0}% floor instead (reported, not gated)",
                    drift.warm,
                    self.samples.len(),
                    span(drift.at),
                    match drift.baseline_rule {
                        Baseline::Floor => "already settled by the floor",
                        Baseline::Measured => "plateau measured from the series",
                        Baseline::Unsettled =>
                            "nothing settled inside half the window, so the \
                             floor stands and the gate measures the whole climb",
                        Baseline::TooShort =>
                            "a plateau was found too late in too short a run to \
                             take a baseline at, so the floor stands",
                        Baseline::RoseLate =>
                            "the run was already flat when it began, so the rise \
                             after it is not a warmup and the floor stands",
                        Baseline::Trough =>
                            "a measured baseline landed in a dip and read worse \
                             than the whole window, so the floor stands",
                    },
                    drift.from_floor * 100.0,
                    WARMUP_FRACTION * 100.0,
                );
                // The shape, and the gradient through it. `SPEC.md` §10's open
                // question about I3 is a *sign* disagreement between two runs,
                // and it was settled by hand off the series below the last time
                // anyone asked. Printing both is what makes the next long run
                // answer it from its own report.
                //
                // **The span goes with the gradient, always.** See
                // [`Drift::span`]: the same RSS wander reads +364 MiB/h over a
                // fifteen-second window and +0.92 over an hour, so a bare MiB/h
                // invites a comparison between runs that the number does not
                // support. Below the gated window it also says so outright,
                // because that is the range where the figure is almost entirely
                // lever-arm and a reader has no other cue on this line.
                println!(
                    "soak: rss quarters {:.2}, {:.2}, {:.2}, {:.2} MiB, \
                     slope {:+.2} MiB/h over {} (reported, not gated){}",
                    mb(drift.quarters[0]),
                    mb(drift.quarters[1]),
                    mb(drift.quarters[2]),
                    mb(drift.quarters[3]),
                    drift.slope,
                    span(drift.span),
                    if self.window < GATED_WINDOW {
                        ", over too short a span to compare against another run"
                    } else {
                        ""
                    },
                );
            }
            None => println!(
                "soak: rss has no verdict from {} samples",
                self.samples.len()
            ),
        }
        println!(
            "soak: tracked diffs max {} and heights max {} of {} files max; \
             tracked hunks max {} of body {}, closest to its screen's own bound \
             {:?}; paths ever changed {}",
            self.max_tracked_diffs(),
            self.max_tracked_spans(),
            self.samples.iter().map(|s| s.files).max().unwrap_or(0),
            self.max_tracked_hunks(),
            self.samples.iter().map(|s| s.body).max().unwrap_or(0),
            self.closest_hunk_bound,
            self.paths()
        );
        println!(
            "soak: tracked history max {} of cap {}; recorded {}, evicted {} by \
             cap and {} by window",
            self.max_tracked_history(),
            HISTORY_PATHS,
            self.history.recorded,
            self.history.evicted_by_cap,
            self.history.evicted_by_window
        );
        println!(
            "soak: frame computed {}, reused {}, evicted {}, probes {}, {:.1} MiB read",
            self.frame.computed,
            self.frame.reused,
            self.frame.evicted,
            self.frame.probes,
            mb(self.frame.bytes)
        );
        println!(
            "soak: highlight parsed {}, reused {}, evicted {}, {} lines",
            self.highlight.parsed,
            self.highlight.reused,
            self.highlight.evicted,
            self.highlight.lines
        );
        match self.samples.first().and_then(|sample| sample.fds) {
            Some(first) => println!(
                "soak: descriptors {first} at the first sample, {} at the most",
                self.samples.iter().filter_map(|s| s.fds).max().unwrap_or(0)
            ),
            None => println!("soak: descriptors unavailable on this platform"),
        }
        println!(
            "soak: failed frames {} of {}{}",
            self.failed,
            self.frames,
            match &self.last_error {
                Some(e) => format!(", last: {e}"),
                None => String::new(),
            }
        );
        // The curve itself, so a reader can see the shape rather than trust the
        // statistic that read it. One line, KiB, in sample order.
        let curve: Vec<String> = self
            .samples
            .iter()
            .map(|s| (s.rss / 1024).to_string())
            .collect();
        println!("soak: rss KiB = {}", curve.join(","));
    }
}

/// The synthetic edits, as an agent in the other pane would make them.
///
/// Deterministic: a counter rather than a random source, so a failing run can
/// be repeated. Every phase is here because it moves something I3 bounds, and
/// the periods are deliberately not multiples of each other, so the phases
/// interleave instead of arriving together.
fn workload(
    scratch: &Scratch,
    files: usize,
    lines: usize,
    stop: &AtomicBool,
    rounds: &AtomicU64,
    created: &AtomicU64,
) {
    // The file the viewport spends its life on, made of many small hunks
    // rather than one enormous one. `fill_large_diff` rewrites every line, so
    // each of its files is a single thousand-row hunk, and a screenful of that
    // holds one or two: the highlight cache would then be bounded by the
    // viewport and never asked to hold more than a couple of entries. Restored
    // after every bulk rewrite below, which flattens it again.
    scratch.write("src/mod_0.rs", sparse(lines, SPARSE_EVERY));

    let mut round = 0u64;
    let mut made = 0u64;
    while !stop.load(Ordering::Relaxed) {
        let at = round as usize;

        // The hot file: one line, before every frame, which is the shape I9 is
        // written against and the one that keeps a hunk changing under the
        // highlighter.
        scratch.edit_line(
            "src/mod_0.rs",
            0,
            &format!("fn hot_{at}() {{ let value = {at}; }}"),
        );

        // A file the viewport is not on, so the frame path has something to
        // revalidate rather than recompute.
        //
        // Every fourth round rather than every round, and the arithmetic is the
        // whole of it: with one cold edit per round the rotation returns to each
        // file inside the two-second settle margin, so **nothing is ever
        // provably unchanged** and the reuse path this soak is supposed to
        // exercise is never taken. Measured before the divisor existed: 380
        // diffs computed and **zero** reused across a whole run.
        if files > 1 && at % COLD_EVERY == 0 {
            let cold = 1 + (at / COLD_EVERY) % (files - 1);
            scratch.edit_line(
                &format!("src/mod_{cold}.rs"),
                1,
                &format!("fn cold_{at}() {{ let value = {at}; }}"),
            );
        }

        // A new path appears, and one from a few rounds ago goes away. This is
        // what makes the *set* of changed paths churn while its size does not,
        // which is the difference between "bounded by the current diff" and
        // "bounded by the session".
        if at % CREATE_EVERY == 0 {
            scratch.write(
                &format!("scratch/new_{made}.rs"),
                generated(NEW_FILE_LINES, "new"),
            );
            made += 1;
            created.store(made, Ordering::Relaxed);
            if made > KEEP_CREATED {
                let old = made - KEEP_CREATED - 1;
                let _ = std::fs::remove_file(scratch.path_of(&format!("scratch/new_{old}.rs")));
            }
        }

        // Two shapes, alternating, and each is here for a different bound.
        //
        // A file put back to the bytes the index holds **leaves the diff**
        // entirely, and its cached diff is evicted. The next bulk rewrite
        // brings it back.
        //
        // A sparsely edited file is one hunk every `SPARSE_EVERY` lines rather
        // than one hunk of the whole file, which is what puts several hunks on
        // one screen. Without it the fixture's files are a single thousand-row
        // hunk each, a screenful is inside one or two of them, and the claim
        // that the highlight cache is bounded by the viewport is tested three
        // entries away from a bound of forty.
        if at % REVERT_EVERY == 0 && files > 1 {
            let target = 1 + (at / REVERT_EVERY) % (files - 1);
            let path = format!("src/mod_{target}.rs");
            if (at / REVERT_EVERY) % 2 == 0 {
                scratch.write(&path, generated(lines, "before"));
            } else {
                scratch.write(&path, sparse(lines, SPARSE_EVERY));
            }
        }

        // The bulk event: every file changes at once, so for the whole settle
        // margin nothing can be proved unchanged. `SPEC.md` §10 measured this
        // as the shell's worst case.
        if at > 0 && at % BULK_EVERY == 0 {
            scratch.rewrite_all(files, lines, at / BULK_EVERY);
            scratch.write("src/mod_0.rs", sparse(lines, SPARSE_EVERY));
        }

        round += 1;
        rounds.store(round, Ordering::Relaxed);
        std::thread::sleep(WRITE_PAUSE);
    }
}

/// A file whose lines match the index except every `every`th one.
///
/// The two sides are taken from [`generated`] rather than written here, so a
/// sparse file is byte-identical to the fixture wherever it is unchanged. Any
/// other spelling would show up as a diff of the whole file and produce the one
/// enormous hunk this exists to avoid.
fn sparse(lines: usize, every: usize) -> String {
    let before = generated(lines, "before");
    let after = generated(lines, "after");
    before
        .lines()
        .zip(after.lines())
        .enumerate()
        .map(|(at, (before, after))| {
            let line = if (at + 1) % every == 0 { after } else { before };
            format!("{line}\n")
        })
        .collect()
}

/// Rounds between each phase of [`workload`].
///
/// The pause is what keeps the writer slower than the loop reading it: a
/// harness that outran the frame path would grow the watcher's channel and the
/// gate would blame the product for a backlog the test manufactured.
const WRITE_PAUSE: Duration = Duration::from_millis(50);
const COLD_EVERY: usize = 4;
// Churn, and the reason it moved is in `DEFAULT_FILES` above: paths ever
// changed is the fixture size plus write rounds over this, so this is the term
// that keeps I3's situation guard satisfiable now that the pinned list has
// widened the working set. Two rather than three for margin on a slow runner,
// where fewer rounds fit in the window: macOS managed 104, and at 2 that is
// still ~72 paths against a high-water mark of 23.
const CREATE_EVERY: usize = 2;
const KEEP_CREATED: u64 = 6;
const REVERT_EVERY: usize = 13;
const BULK_EVERY: usize = 100;
const NEW_FILE_LINES: usize = 40;

/// Lines between the edits in a [`sparse`] file.
///
/// Above `2 * CONTEXT + 1`, or two edits share a hunk and the file has fewer
/// hunks than it looks like: the same constraint `Scratch::sparse_edits`
/// documents.
const SPARSE_EVERY: usize = 8;

/// What the reader does, on a cycle, so no sample is taken in the cheapest
/// state.
///
/// `SPEC.md` §7 twice: a budget measured at one position is measured at its
/// cheapest one, and a gate that settles before it measures has measured the
/// cheapest state. A soak that never scrolled would hold one hunk on screen
/// for a day and prove nothing about the cache that holds them.
///
/// [`DEEP`] is the case the I2b audit left to this issue: a reader sitting far
/// inside one large hunk is what makes a single highlight entry heavy, because
/// it accumulates a parse checkpoint every `CHECKPOINT_STRIDE` lines.
fn scripted(frames: u64, body: usize) -> Option<Action> {
    let page = isize::try_from(body.max(1)).unwrap_or(1);
    match frames % 40 {
        7 => Some(Action::Scroll(page)),
        15 => Some(Action::Scroll(-page)),
        23 => Some(Action::Bottom),
        27 => Some(Action::Scroll(DEEP)),
        35 => Some(Action::Top),
        // Follow was disengaged by the scrolls above, so this re-engages it and
        // jumps to the newest change, exactly as `f` does.
        39 => Some(Action::ToggleFollow),
        _ => None,
    }
}

/// Rows into one hunk that the scripted reader scrolls.
const DEEP: isize = 400;

/// Panes the run is drawn into, cycled so the layout is exercised at both ends
/// of I6's range rather than at one comfortable width.
const AREAS: [(u16, u16); 3] = [(80, 24), (40, 24), (120, 40)];
const RESIZE_EVERY: u64 = 97;

/// What the header calls the worktree.
const NAME: &str = "soak";

/// Run the soak and report what it did.
fn soak(scratch: &Scratch, files: usize, lines: usize, window: Duration) -> Report {
    let (tx, rx) = mpsc::channel::<Vec<String>>();
    let root = scratch.root().to_path_buf();

    // The product's own shape: the watcher owns its repository on its own
    // thread, because `gix::Repository` is `Send` and not `Sync`, and it is
    // detached because nothing can wake a blocked `next_tick` except a `Stop`.
    // See `vigia::run`.
    std::thread::spawn(move || {
        let worktree = Worktree::discover(&root).expect("discover for the watch thread");
        let mut watcher = worktree
            .watch(WatchOptions::default())
            .expect("arm the watch");
        while let Some(tick) = watcher.next_tick() {
            if tx.send(tick.paths).is_err() {
                return;
            }
        }
    });

    let stop = AtomicBool::new(false);
    let rounds = AtomicU64::new(0);
    let created = AtomicU64::new(0);

    std::thread::scope(|scope| {
        scope.spawn(|| workload(scratch, files, lines, &stop, &rounds, &created));
        let report = drive(scratch, files, window, &rx, &rounds, &created);
        stop.store(true, Ordering::Relaxed);
        report
    })
}

/// The frame loop: everything `vigia::run` does between waking and drawing.
fn drive(
    scratch: &Scratch,
    fixture_files: usize,
    window: Duration,
    rx: &mpsc::Receiver<Vec<String>>,
    rounds: &AtomicU64,
    created: &AtomicU64,
) -> Report {
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("the first walk");

    let mut app = App::new();
    let mut highlighter = Highlighter::new();

    // **The warmer, because `run` spawns one and this harness claims to be `run`
    // with the terminal taken out.** It compiles grammars ahead of the reader,
    // and what it leaves behind is `syntect`'s compiled-pattern cache, which is
    // the one thing in the process that grows as more grammars are touched and
    // is never evicted.
    //
    // That is a **plateau rather than drift**, the same shape `RETAINED_HUNKS`
    // already argued for: a bigger constant is a higher level, and drift
    // compares a window against itself so it cannot see a level. Which is
    // exactly why it has to be in here rather than reasoned about in a comment
    // alone — a claim that something cannot drift is worth more when the gate
    // that would notice has actually been run with it present.
    //
    // Joined rather than detached, unlike in `run`: the warm is bounded and
    // finishes in well under a second, and a soak that started sampling while a
    // one-off startup cost was still landing would put it in the first quarter's
    // median and read it as drift in the wrong direction.
    highlighter
        .warm_ahead(
            worktree.workdir().to_path_buf(),
            frame
                .files()
                .iter()
                .take(vigia_core::WARM_FILES)
                .map(|change| change.path.clone())
                .collect(),
        )
        .join()
        .expect("the warmer thread");

    // The third retained cache, and the one this run exists to bound now: it is
    // the only one that deliberately outlives the diff, so a soak that left it
    // out would be measuring I3 against two thirds of what the process keeps.
    let mut history = History::new();
    let theme = Theme::default();
    let mut area = Rect::new(0, 0, AREAS[0].0, AREAS[0].1);
    let mut buffer = Buffer::empty(area);
    let mut view = View::default();

    let count = samples_for(window);
    let interval = window / count as u32;
    let started = Instant::now();

    let mut samples = Vec::with_capacity(count);
    let (mut frames, mut full_frames, mut ticks, mut failed) = (0u64, 0u64, 0u64, 0u64);
    let mut last_error = None;
    let mut body = Body::default();
    let mut closest_hunk_bound = None;

    while samples.len() < count {
        let deadline = started + interval * (samples.len() as u32 + 1);
        let now = Instant::now();
        if now >= deadline {
            samples.push(Sample {
                at: started.elapsed(),
                rss: rss_bytes().unwrap_or(0),
                fds: open_files(),
                tracked_diffs: frame.tracked(),
                tracked_spans: frame.tracked_spans(),
                tracked_hunks: highlighter.tracked(),
                tracked_history: history.tracked(),
                files: frame.files().len(),
                body: body.diff,
            });
            continue;
        }

        match rx.recv_timeout(deadline - now) {
            // Nothing arrived before the next sample was due, which is only
            // possible if the writer stopped. The floors below catch it.
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => {
                panic!(
                    "the watch thread ended after {frames} frames, so the rest of this window would measure a process with nothing to do"
                )
            }
            Ok(paths) => {
                ticks += 1;
                // Sampled on the wake, before the walk, exactly where
                // `vigia::run` samples it. I10 is a claim about a store fed one
                // tick at a time, so a soak feeding it any other way would bound
                // something the product never builds.
                history.record(paths.iter().map(String::as_str), Instant::now());
                // Advance first, follow second: the path is looked up in the
                // file list, and before the walk that list is the previous
                // frame's. `vigia::run` says why.
                match frame.advance() {
                    Ok(()) => {
                        if let Some(path) = paths.last() {
                            app.follow(path, &frame);
                        }
                    }
                    Err(e) => {
                        failed += 1;
                        last_error = Some(e.to_string());
                    }
                }
            }
        }

        if let Some(action) = scripted(frames, body.diff) {
            let chrome = app.chrome(NAME, None);
            let height = diff_height(area, &chrome, frame.files().len());
            if let Err(e) = app.apply(action, &mut frame, height) {
                failed += 1;
                last_error = Some(e.to_string());
            }
        }

        if frames > 0 && frames % RESIZE_EVERY == 0 {
            let (width, height) = AREAS[(frames / RESIZE_EVERY) as usize % AREAS.len()];
            area = Rect::new(0, 0, width, height);
            buffer = Buffer::empty(area);
        }

        // Both status readouts, in the order `vigia::run` performs them, and
        // they belong in a soak for a different reason than they belong in a
        // budget gate. I9 asks what one frame costs; I3 asks what a **week** of
        // them retains, and these are the two newest things on the frame path
        // that allocate: a syscall's buffer and a hundred and twenty-eight
        // durations that a percentile copies and sorts every frame. A soak that
        // drove a screen without them would report drift for a process nobody
        // runs.
        //
        // `record_frame` is at the bottom of the loop, where the frame ends.
        let frame_began = Instant::now();
        app.sample_memory();
        let chrome = app.chrome(NAME, None);
        body = body_layout(area, &chrome, frame.files().len());
        match app.view(&mut frame, &mut highlighter, &history, body) {
            Ok(fresh) => {
                view = fresh;
                // Every hunk that put a line on this screen, which is what the
                // highlighter was asked for, plus the ones it is allowed to keep
                // for a reader who scrolls back. One more than the headers drawn,
                // because the top of the screen can sit inside a hunk whose
                // header is above it, and never more: a hunk with no line on
                // screen is never asked for at all.
                //
                // `RETAINED_HUNKS` is a constant added to a per-frame number, not
                // slack. It is the exact size of the retired queue (#45), so the
                // bound still moves with the screen and still cannot be satisfied
                // by a cache bounded by the session: deleting the sweep reports
                // hunks in the hundreds against a bound in single figures, the
                // same way it did before the queue existed.
                let bound = 1
                    + RETAINED_HUNKS
                    + view
                        .rows
                        .iter()
                        .filter(|row| matches!(row, Row::Hunk { .. }))
                        .count();
                let held = highlighter.tracked();
                if closest_hunk_bound.is_none_or(|(worst, at)| held * at >= worst * bound) {
                    closest_hunk_bound = Some((held, bound));
                }
            }
            Err(e) => {
                failed += 1;
                last_error = Some(e.to_string());
            }
        }
        // Drawn rather than collected, because the renderer is the half of the
        // shell that holds a buffer, and a soak that stopped short of it would
        // leave the one allocation per frame nobody measured.
        render(&mut buffer, area, &view, &theme, &chrome);
        app.record_frame(frame_began.elapsed());
        if view.rows.len() == body.diff {
            full_frames += 1;
        }
        frames += 1;
    }

    Report {
        window,
        samples,
        frames,
        full_frames,
        ticks,
        rounds: rounds.load(Ordering::Relaxed),
        created: created.load(Ordering::Relaxed),
        fixture_files,
        failed,
        last_error,
        closest_hunk_bound,
        frame: frame.stats(),
        highlight: highlighter.stats(),
        history: history.stats(),
    }
}

/// Floors below which this soak proved nothing.
///
/// Deliberately far under what the default window produces on this machine
/// (about 250 frames in fifteen seconds), because their job is to catch a run
/// that stopped rather than to describe a healthy one. A bound over a process
/// that drew forty frames is a bound over nothing, and it would pass.
const MIN_FRAMES: u64 = 40;
const MIN_TICKS: u64 = 40;
const MIN_ROUNDS: u64 = 50;

impl Report {
    /// Every claim I3 makes that this process can see.
    ///
    /// Non-vacuity first, because every bound below is satisfied by a soak that
    /// did nothing at all.
    fn gate(&self) {
        assert!(
            self.frames >= MIN_FRAMES && self.ticks >= MIN_TICKS,
            "I3: {} frames from {} ticks over {:?}, under the {MIN_FRAMES} and \
             {MIN_TICKS} this gate needs, so the bounds below describe a monitor \
             that was not running",
            self.frames,
            self.ticks,
            self.window
        );
        assert!(
            self.rounds >= MIN_ROUNDS,
            "I3: the writer completed {} rounds, under {MIN_ROUNDS}, so nothing \
             was changing under the frame path",
            self.rounds
        );
        assert!(
            self.created > KEEP_CREATED,
            "I3: {} files were created and none was old enough to be deleted \
             again, so no path ever left the diff",
            self.created
        );
        assert!(
            self.highlight.parsed > 0,
            "I3: nothing was highlighted, so this soak measured the frame path \
             with the syntax parser missing"
        );
        // Both paths, not just the expensive one. A workload that rewrites every
        // file inside the settle margin never lets a fingerprint be trusted, so
        // it recomputes every diff and never exercises the cache it is here to
        // bound. That is what this run did before `COLD_EVERY` existed.
        assert!(
            self.frame.reused > 0,
            "I3: {} diffs were computed and none reused over {} frames, so every \
             file was inside the settle margin for the whole window and the \
             reuse path was never taken",
            self.frame.computed,
            self.frames
        );
        // And the viewport bound has to have been holding more than one thing,
        // or "bounded by the screen" is indistinguishable from "holds one hunk".
        let hunks = self.max_tracked_hunks();
        assert!(
            hunks > 1,
            "I3: the highlight cache never held more than {hunks} hunk parse, so \
             a bound of one screen is being tested against a screen showing one \
             hunk"
        );
        assert!(
            self.full_frames * 2 > self.frames,
            "I3: only {} of {} frames filled the body, so the screen was mostly \
             empty and the viewport bound below is not being tested",
            self.full_frames,
            self.frames
        );
        assert!(
            self.failed * 20 <= self.frames,
            "I3: {} of {} frames failed, over the 5% a workload that deletes \
             files under the walk should produce (last: {})",
            self.failed,
            self.frames,
            self.last_error.as_deref().unwrap_or("none recorded")
        );
        assert!(
            self.samples.iter().all(|sample| sample.rss > 0),
            "I3: {} of {} samples read no RSS at all, so the drift below is \
             computed from a series this platform never filled in",
            self.samples.iter().filter(|s| s.rss == 0).count(),
            self.samples.len()
        );

        // The four retained caches, each against the thing that is supposed to
        // bound it. `Frame` keeps two, both bounded by the current diff;
        // `Highlighter` is bounded by the screen, which is the stronger claim;
        // `History` is bounded by a fixed cap, which is I10 and is the only one
        // of the four that has to keep holding a path *after* it has left the
        // diff.
        for (at, sample) in self.samples.iter().enumerate() {
            assert!(
                sample.tracked_diffs <= sample.files,
                "I3: sample {at} at {:?} held {} diffs for {} changed files, so \
                 the frame path is keeping diffs for paths that stopped being \
                 changed",
                sample.at,
                sample.tracked_diffs,
                sample.files
            );
            assert!(
                sample.tracked_spans <= sample.files,
                "I3: sample {at} at {:?} held {} heights for {} changed files, so \
                 the span cache is bounded by the session rather than by the \
                 diff (#101 made it outlive the tick; the migration in \
                 `Frame::advance` is what is supposed to bound it)",
                sample.at,
                sample.tracked_spans,
                sample.files
            );
            assert!(
                sample.tracked_history <= HISTORY_PATHS,
                "I10: sample {at} at {:?} tracked {} paths against a cap of \
                 {HISTORY_PATHS}, so glance history is bounded by what the \
                 session did rather than by anything fixed",
                sample.at,
                sample.tracked_history
            );
        }

        self.gate_history();

        let (held, bound) = self
            .closest_hunk_bound
            .expect("no frame collected a view, which the frame floor above rules out");
        assert!(
            held <= bound,
            "I3: the highlight cache held {held} hunk parses on a screen that \
             could have asked for {bound}, so it is bounded by something other \
             than the viewport"
        );

        // **And the span bound above cannot be satisfied by an empty map.**
        // `tracked_spans <= files` holds trivially against a frame that keeps no
        // span at all, which is precisely what the pre-#101 code did: it cleared
        // the map on every tick. A bound is only evidence when something reached
        // it, which is the rule I10's own gate is written against one cache over.
        assert!(
            self.max_tracked_spans() > 0,
            "I3: no sample held a single height across {} frames, so the span \
             cache is being dropped rather than bounded and the bound above says \
             nothing",
            self.frames
        );

        assert!(
            self.frame.evicted > 0 && self.highlight.evicted > 0,
            "I3: {} diffs and {} hunk parses were evicted, so nothing ever left \
             either cache and both bounds above hold for a reason that is not \
             the code",
            self.frame.evicted,
            self.highlight.evicted
        );

        // And the claim the two bounds are really making: the caches follow the
        // diff, not the session. A run whose paths never churned would satisfy
        // every line above while growing forever in the one case I3 is about.
        let tracked = self.max_tracked_diffs() as u64;
        assert!(
            self.paths() >= 2 * tracked,
            "I3: {} distinct paths were ever changed against a high-water mark of \
             {tracked} tracked diffs, so this window never churned enough paths \
             for 'bounded by the diff' to differ from 'bounded by the session'",
            self.paths()
        );

        self.gate_descriptors();
        self.gate_drift();
    }

    /// File handles, which I3 names alongside RSS.
    fn gate_descriptors(&self) {
        let counts: Vec<usize> = self.samples.iter().filter_map(|s| s.fds).collect();
        let complete = counts.len() == self.samples.len();

        // On the one platform that can answer, not answering is a broken reader
        // rather than an absent feature, so it fails instead of printing. A
        // passing CI run is otherwise indistinguishable from one where this
        // metric quietly collected nothing, because the note below is invisible
        // unless a test fails or `--nocapture` is on.
        #[cfg(target_os = "linux")]
        assert!(
            complete,
            "I3: {} of {} samples could not read /proc/self/fd",
            counts.len(),
            self.samples.len()
        );

        if !complete {
            println!(
                "note: file handles are not gated on this platform; \
                 {} of {} samples could read a count",
                counts.len(),
                self.samples.len()
            );
            return;
        }

        // Its own fraction rather than [`warmup`]'s measured plateau, and the
        // reason both exist is on [`FD_WARMUP_FRACTION`].
        let warm = (((counts.len() as f64 * FD_WARMUP_FRACTION).ceil()) as usize).max(1);
        let baseline = counts[..warm].iter().copied().max().unwrap_or(0);
        let peak = counts[warm..].iter().copied().max().unwrap_or(0);
        assert!(
            peak <= baseline + FD_HEADROOM,
            "I3: descriptors went from {baseline} after warmup to {peak}, over \
             the {FD_HEADROOM} of headroom a transient open needs, so something \
             is holding handles open"
        );
    }

    /// I10's eviction, which a short window cannot reach.
    ///
    /// The bound itself is asserted per sample in [`Report::gate`] and always
    /// holds. **That assertion on its own is decorative**, and this is what
    /// makes it mean something: a store nothing ever filled satisfies a cap the
    /// way an empty room satisfies a fire code.
    ///
    /// Neither of I10's two eviction rules is reachable in a short run. The cap
    /// is [`HISTORY_PATHS`] and the default window invents about eighty paths;
    /// the time rule is [`vigia_core::HISTORY_WINDOW`] and the default window is
    /// shorter than it. So this refuses to assert rather than passing on a
    /// question it never asked, exactly as [`Report::gate_drift`] does and for
    /// exactly the reason `SPEC.md` §7 gives about gates that cannot say no.
    ///
    /// The scheduled runs reach both comfortably: four hours at this rate is
    /// tens of thousands of paths and a window turned over more than a hundred
    /// times. And the *deterministic* proof of both rules is
    /// `crates/vigia-core/tests/history.rs`, which drives ten thousand paths in
    /// every `cargo test` rather than waiting for a long window to happen to
    /// produce them. This gate is the one that says the same thing about the
    /// real process.
    fn gate_history(&self) {
        let pressure = self.paths() > HISTORY_PATHS as u64;
        let aged = self.window > HISTORY_WINDOW;
        if !pressure && !aged {
            println!(
                "note: I10's eviction is not gated by a {:?} window: {} distinct \
                 paths changed against a cap of {HISTORY_PATHS}, and the window \
                 is under the {HISTORY_WINDOW:?} a path needs to age out. Run \
                 with {SECS}={} to enforce it.",
                self.window,
                self.paths(),
                HISTORY_WINDOW.as_secs() + 1
            );
            return;
        }

        assert!(
            self.history.evicted_by_cap > 0 || self.history.evicted_by_window > 0,
            "I10: {} path samples were recorded over {} distinct paths in a {:?} \
             window and nothing was ever evicted, so the store grew with the \
             session and the per-sample cap above held for a reason that is not \
             the code",
            self.history.recorded,
            self.paths(),
            self.window
        );
    }

    /// RSS drift, which is the half that has to be scheduled.
    fn gate_drift(&self) {
        if self.window < GATED_WINDOW {
            println!(
                "note: the drift gate is not applied to a {:?} window, which is \
                 under the {GATED_WINDOW:?} it needs to have anything but warmup \
                 in it (SPEC.md §7). Run with {SECS}={} to enforce it.",
                self.window,
                GATED_WINDOW.as_secs()
            );
            return;
        }

        let drift = drift(&self.series()).unwrap_or_else(|| {
            panic!(
                "I3: {} samples over {:?} produced no verdict, so the window was \
                 gated and measured nothing",
                self.samples.len(),
                self.window
            )
        });
        // The quarters and the gradient travel with the failure, because the
        // first question asked of a breach is whether it was a trend or a step
        // and neither is legible from a ratio. The ends are `quarters[0]` and
        // `quarters[3]`, so they are stated once rather than twice.
        //
        // **And the baseline travels with it too**, because the second question
        // is where the comparison started: the day-long window failed at 13.26%
        // on a process whose back half was flat to 0.02 MiB/h, and the whole of
        // that was baseline placement. A breach that does not say which rule
        // placed its baseline sends the next reader to the series to work it
        // out, which is what #126 cost.
        assert!(
            drift.ratio < DRIFT_BUDGET,
            "I3: RSS drifted {:.2}% over {:?}, over the {:.0}% budget: quarters \
             {:.2}, {:.2}, {:.2}, {:.2} MiB at {:+.2} MiB/h, peak {:.2} MiB over \
             {} frames, from a baseline {} samples in ({}) that {}",
            drift.ratio * 100.0,
            self.window,
            DRIFT_BUDGET * 100.0,
            mib(drift.quarters[0]),
            mib(drift.quarters[1]),
            mib(drift.quarters[2]),
            mib(drift.quarters[3]),
            drift.slope,
            mib(self.samples.iter().map(|s| s.rss).max().unwrap_or(0)),
            self.frames,
            drift.warm,
            span(drift.at),
            match drift.baseline_rule {
                Baseline::Floor => "the series had already settled by",
                Baseline::Measured => "a measured plateau put there",
                Baseline::Unsettled =>
                    "the floor put there, because nothing settled inside half \
                     the window: this run either never reached a plateau or is \
                     leaking, and the quarters above say which",
                Baseline::TooShort =>
                    "the floor put there, because the plateau this run does \
                     have arrives too late to leave the gate two ends",
                Baseline::RoseLate =>
                    "the floor put there, because this run was already flat \
                     when it started: what climbed afterwards is memory it \
                     took and kept, not a warmup",
                Baseline::Trough =>
                    "the floor put there, because a measured baseline landed \
                     in a dip and reported worse than the whole window",
            },
        );
    }
}

/// What is in a directory right now, by name.
fn listing(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .filter_map(|entry| Some(entry.ok()?.file_name().to_string_lossy().into_owned()))
        .collect()
}

#[test]
fn resources_are_flat_and_nothing_is_retained() {
    let root = std::env::temp_dir().join(format!("vigia-soak-{}", std::process::id()));
    let private = root.join("temp");
    let worktree = root.join("worktree");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&private).expect("create the private temp directory");
    std::fs::create_dir_all(&worktree).expect("create the worktree directory");

    // Before, as well as after. An empty-at-the-end assertion over a directory
    // that was never empty, or never pointed at, passes for the wrong reason.
    assert!(
        listing(&private).is_empty(),
        "the private temp directory was not empty before the run: {:?}",
        listing(&private)
    );

    let exe = std::env::current_exe().expect("this test binary's own path");
    let output = Command::new(&exe)
        .args([CHILD_TEST, "--exact", "--ignored", "--nocapture"])
        .args(["--test-threads", "1"])
        .env(CHILD, "1")
        .env(PRIVATE, &private)
        .env(WORKTREE, &worktree)
        // All three, because the platforms disagree about which one names the
        // temp directory and `std::env::temp_dir` reads a different one on each.
        .env("TMPDIR", &private)
        .env("TMP", &private)
        .env("TEMP", &private)
        .output()
        .expect("run the soak child");

    let report = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.status.success(),
        "the soak failed ({}):\n{report}",
        output.status
    );
    // **What the report says is an invariant too, and the parent is the only
    // place that can hold it.** `SPEC.md` §7 states in bold that the four
    // quarter medians and the gradient with its span are carried beside the
    // ratio, and nothing read this string until now: deleting either from
    // `Report::print` left the whole suite green, which is the shape
    // `CLAUDE.md` calls a wish. The statistic is gated by `mod statistic`; this
    // is the line that says it reaches a reader.
    for expected in [
        "soak: rss quarters ",
        " MiB/h over ",
        "soak: rss baseline ",
        // Where the baseline was taken, and what the fraction would have said.
        // `SPEC.md` §7 states both, and a measured baseline nobody can see the
        // placement of is the same wish the quarters were before this loop
        // existed.
        "soak: rss warmup ",
        " floor instead ",
    ] {
        assert!(
            report.contains(expected),
            "the soak's report carries no {expected:?}, so a number \
             `SPEC.md` §7 says is printed is not being printed:\n{report}"
        );
    }
    // Only on the way out, so a passing run in `cargo test` says its numbers
    // once rather than interleaved with every other test in the workspace.
    println!("{report}");

    let left = listing(&private);
    assert!(
        left.is_empty(),
        "I3: zero temp files retained, and the run left {} in its own temp \
         directory: {left:?}",
        left.len()
    );

    // Left behind on failure on purpose: the worktree is the evidence.
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
#[ignore = "the parent owns the private temp directory this needs"]
fn soak_child() {
    assert!(
        std::env::var_os(CHILD).is_some(),
        "{CHILD} is not set, so this was run directly. It is the body of \
         `resources_are_flat_and_nothing_is_retained`, which builds the private \
         temp directory it measures; run that instead."
    );

    let private = PathBuf::from(std::env::var_os(PRIVATE).expect("the parent sets the temp dir"));
    let want = std::fs::canonicalize(&private).expect("canonicalise the private temp dir");
    let got = std::fs::canonicalize(std::env::temp_dir()).expect("canonicalise the temp dir");
    assert_eq!(
        got, want,
        "the temp directory was not redirected into {want:?}, so the \
         retained-temp-file gate would be asserting about a directory nothing \
         under test can write to"
    );

    let worktree = PathBuf::from(std::env::var_os(WORKTREE).expect("the parent sets the worktree"));
    let files = env_var(FILES, DEFAULT_FILES);
    let lines = env_var(LINES, DEFAULT_LINES);
    let window = Duration::from_secs(env_var(SECS, DEFAULT_SECS));

    let scratch = Scratch::in_dir(&worktree, "soak");
    scratch.fill_large_diff(files, lines);
    println!(
        "soak: fixture {files} files x {lines} lines at {:?}",
        scratch.root()
    );

    // The fixture is built by real `git`, which inherits this process's
    // environment and therefore its temp directory. Checked here rather than
    // only at the end, so fixture leftovers are never reported as a leak in the
    // code under test.
    assert!(
        listing(&private).is_empty(),
        "building the fixture left {:?} in the temp directory, which would be \
         charged to the soak below",
        listing(&private)
    );

    let report = soak(&scratch, files, lines, window);
    report.print();
    report.gate();
}

/// The soak workflow, at `.github/workflows/soak.yml`.
const WORKFLOW: &str = "../../.github/workflows/soak.yml";

/// Every setting in the workflow, by key, ignoring comments.
///
/// Deliberately every one and not the first: `find_map` over a key that occurs
/// twice pins the copy nearest the top of the file and says nothing about the
/// rest, which for `timeout-minutes:` means a second job can carry any number
/// at all. `SPEC.md` §7's rule about bounds applies to a text scan too.
/// A trailing `#` comment is stripped, because a setting is what YAML reads and
/// not what a reader wrote beside it: `timeout-minutes: 330 # was
/// needs.plan.outputs.timeout` is a pinned 330, and it satisfied an earlier
/// version of this scan that only looked for the substring anywhere in the
/// line.
fn workflow_settings(source: &str, key: &str) -> Vec<String> {
    source
        .lines()
        .map(str::trim)
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| {
            let set_to = line.strip_prefix(key)?;
            Some(
                set_to
                    .split_once(" #")
                    .map_or(set_to, |(value, _)| value)
                    .trim()
                    .to_owned(),
            )
        })
        .collect()
}

/// The workflow's jobs, by name, each with the block of text that is its own.
///
/// A job starts at two spaces of indent under `jobs:` and runs until the next
/// one. Enough structure to say *which* job a setting belongs to, which is the
/// difference between "the derived timeout is in this file somewhere" and "the
/// job that soaks uses it".
fn workflow_jobs(source: &str) -> Vec<(String, String)> {
    let after = match source.split_once("\njobs:\n") {
        Some((_, after)) => after,
        None => return Vec::new(),
    };
    let mut jobs: Vec<(String, String)> = Vec::new();
    for line in after.lines() {
        let named = line
            .strip_prefix("  ")
            .filter(|rest| !rest.starts_with(' ') && !rest.starts_with('#'))
            .and_then(|rest| rest.strip_suffix(':'));
        match named {
            Some(name) => jobs.push((name.to_owned(), String::new())),
            None => {
                if let Some((_, block)) = jobs.last_mut() {
                    block.push_str(line);
                    block.push('\n');
                }
            }
        }
    }
    jobs
}

/// Whether a job block declares a dependency on `job`.
///
/// **Every spelling YAML allows, because the point is what the runner will do
/// and the runner accepts all of them.** `needs: plan`, `needs: [plan]`,
/// `needs: [plan, build]`, `needs: ["plan"]` and the block sequence
/// (`needs:` then `  - plan`) are one declaration written five ways. The first
/// version of this check listed three of them literally and would have gone red
/// the first time a second dependency was added, reporting it as a missing one:
/// the same mistake, one level down, that keying the soak job on `needs: plan`
/// made in the first place.
fn declares_need(block: &str, job: &str) -> bool {
    let inline = workflow_settings(block, "needs:").into_iter().any(|on| {
        on.trim_matches(['[', ']'].as_slice())
            .split(',')
            .any(|name| name.trim().trim_matches(['"', '\''].as_slice()) == job)
    });
    // The block-sequence form leaves the key's own value empty and puts the
    // names on the lines under it.
    let sequence = block
        .lines()
        .map(str::trim)
        .skip_while(|line| *line != "needs:")
        .skip(1)
        .take_while(|line| line.starts_with("- "))
        .any(|line| {
            line.trim_start_matches("- ")
                .trim_matches(['"', '\''].as_slice())
                == job
        });
    inline || sequence
}

/// The workflow's text, and the path it came from.
fn workflow() -> (PathBuf, String) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(WORKFLOW);
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read the soak workflow at {}: {e}", path.display()));
    (path, source)
}

/// The scheduled job must not kill the window its own input offers.
///
/// `.github/workflows/soak.yml` advertises `seconds: 86400` and then pinned
/// `timeout-minutes: 330`, which is 5.5 hours: the dispatch that I3's day-long
/// budget needs would have been terminated by *this repository's* timeout
/// before the platform's six-hour cap was even reached, on an uncapped runner
/// exactly as on a hosted one. Nothing caught it, and the only thing that would
/// have said so is a run that had already burned five and a half hours.
///
/// The obvious repair is to derive the timeout from the window in the field
/// itself, and **the expression language cannot**:
/// `${{ fromJSON(inputs.seconds) / 60 + 90 }}` is rejected at parse time with
/// `Unexpected symbol: '/'`, which fails the whole workflow rather than the one
/// field, so the daily soak would have stopped running. Arithmetic lives in
/// `bash` instead, in a `plan` job whose outputs both fields read.
///
/// This is the **plumbing** half: each setting reads the output that matches
/// it, and no *other* timeout in the file is large enough to matter. That the
/// plan job's arithmetic is right is
/// [`the_plan_job_gives_every_window_room_to_finish`], and the two halves are
/// separate because either alone passes while the defect is present. An
/// earlier version asserted only that both settings named
/// `needs.plan.outputs`, and seven mutations survived it, including
/// `timeout=330` written straight into the plan script.
///
/// **What it does not prove**: nothing here parses YAML. A parser is a
/// dependency `SPEC.md` does not name, and the expression language only exists
/// on the runner, so a syntactically broken workflow still reaches CI.
#[test]
fn the_soak_workflow_cannot_kill_the_window_it_offers() {
    let (path, source) = workflow();

    // The window and the timeout each read the output that carries it, rather
    // than merely reading *some* output: swapping the two is a mutation the
    // weaker form could not see.
    //
    // **Per job, not per file.** Asserting that the derived expression appears
    // *somewhere* is satisfied by it appearing on the wrong job: swapping the
    // soak's `timeout-minutes` with the plan job's `5` leaves both settings
    // present, the file scan green, and the daily soak killed after five
    // minutes. Which job carries which number is the entire claim.
    // **An empty split passes a `for` loop, so the split is asserted first.**
    // Round one's version scanned the whole file and could not go vacuous,
    // because an empty list failed its `any`. Per-job resolution traded that
    // away: a `jobs:` line with a trailing space, or a comment after it, or
    // CRLF endings, and this finds nothing and asserts nothing. `plan_script`
    // already sets the precedent by checking its own extraction landed.
    let jobs = workflow_jobs(&source);
    assert!(
        !jobs.is_empty(),
        "no jobs were found in {}, so everything below this line would pass \
         without asserting anything",
        path.display()
    );

    let mut soaking = 0;
    for (job, block) in &jobs {
        // **The job that soaks is the one that runs the soak**, which is a
        // property of what it does rather than of what it depends on. Keying
        // on `needs: plan` instead reads as the same thing and is not: it
        // misses the equally valid `needs: [plan]` and then reports the miss
        // as a bad timeout, and it would force a future summary job that also
        // depends on `plan` down a branch demanding it carry a soak window.
        // Through the setting scan rather than a raw `contains` either, so a
        // comment mentioning the key could not be read as the key. No comment
        // in this file does today; the scan costs nothing and the raw form was
        // demonstrated to misfire on one.
        let soaks = !workflow_settings(block, "VIGIA_SOAK_SECS:").is_empty();
        soaking += usize::from(soaks);
        let timeouts = workflow_settings(block, "timeout-minutes:");
        assert_eq!(
            timeouts.len(),
            1,
            "job `{job}` in {} declares {} timeouts. None is worse than two \
             here: a job with no `timeout-minutes` inherits GitHub's own 360 \
             minutes, which is a pinned number nobody wrote down and is this \
             issue exactly. Two, and the one that fires is whichever the \
             runner read first",
            path.display(),
            timeouts.len()
        );
        let set_to = &timeouts[0];

        if soaks {
            // `starts_with`, not `contains`: the value has to *be* the
            // expression, not merely mention it. A trailing comment naming the
            // output satisfied the weaker form.
            assert!(
                set_to.starts_with("${{") && set_to.contains("needs.plan.outputs.timeout"),
                "job `{job}` soaks and its `timeout-minutes` is {set_to:?}, \
                 which is not the timeout the plan job computed from the \
                 window. That is the whole of this issue: a pinned timeout \
                 beside a window it never heard about, which killed the \
                 86400-second dispatch the `seconds` input advertises at 5.5 \
                 hours"
            );
            // It reads three of that job's outputs, so it has to declare the
            // dependency: without `needs:`, `needs.plan.outputs.*` resolves
            // empty and `fromJSON('')` throws on the runner, which is a failure
            // nothing here would have predicted.
            assert!(
                declares_need(block, "plan"),
                "job `{job}` reads `needs.plan.outputs` and does not declare \
                 `needs: plan`, so the outputs it reads resolve to nothing and \
                 the run fails on the runner rather than here"
            );

            // Every planned number, not only the timeout. The platform list is
            // the third, and it is the one the `runner` input travels through:
            // pinning `os:` back to a literal matrix leaves that input inert
            // while the file stays valid and every other assertion here holds,
            // and an inert `runner` is the one thing standing between this
            // repository and I3's twenty-four hours.
            for (key, output) in [
                ("VIGIA_SOAK_SECS:", "needs.plan.outputs.seconds"),
                ("os:", "needs.plan.outputs.os"),
            ] {
                assert!(
                    workflow_settings(block, key)
                        .iter()
                        .any(|set_to| set_to.starts_with("${{") && set_to.contains(output)),
                    "job `{job}` reads the planned timeout but its `{key}` does \
                     not come from `{output}`, so the plan job and the job that \
                     soaks have stopped describing the same run"
                );
            }
        } else {
            // Everything else may pin a literal, provided it is short enough
            // that it could not be holding a soak window.
            let minutes: u64 = set_to.parse().unwrap_or_else(|_| {
                panic!(
                    "job `{job}` does not soak and its `timeout-minutes` is \
                     {set_to:?}, which is neither a plain number nor anything \
                     this gate knows how to judge"
                )
            });
            assert!(
                minutes <= 30,
                "job `{job}` does not soak and pins `timeout-minutes: \
                 {minutes}`, which is long enough to be holding a soak window. \
                 A job that only runs a shell script may pin a small number; \
                 anything larger belongs on the job that soaks, derived"
            );
        }
    }

    // And exactly one job soaked, so the branch carrying every assertion above
    // was actually taken: with none, every job takes the literal branch and the
    // file passes as a workflow that soaks nothing. Exactly one rather than at
    // least one, because the
    // concurrency group at the top of this workflow exists to stop two soaks
    // sharing a runner's memory pressure, and two soaking jobs inside one
    // workflow would be that same mistake a level up.
    assert_eq!(
        soaking,
        1,
        "{soaking} of the {} jobs in {} run a soak; the assertions above \
         describe exactly one",
        jobs.len(),
        path.display()
    );
}

/// The plan job's shell script, taken out of the workflow by indentation.
///
/// Crude on purpose: a YAML parser is a dependency `SPEC.md` does not name, and
/// there is exactly one `run: |` block in this file. The assertion below is
/// what stops that from being a silent assumption.
fn plan_script(path: &Path, source: &str) -> String {
    let after = source
        .split_once("        run: |\n")
        .unwrap_or_else(|| panic!("no `run: |` block in {}", path.display()))
        .1;
    let script: String = after
        .lines()
        .take_while(|line| line.trim().is_empty() || line.starts_with("          "))
        .map(|line| format!("{}\n", line.strip_prefix("          ").unwrap_or(line)))
        .collect();
    assert!(
        script.contains("GITHUB_OUTPUT"),
        "the extracted script never writes to GITHUB_OUTPUT, so the block taken \
         was not the one that plans the window:\n{script}"
    );
    script
}

/// What the plan script did: whether it succeeded, what it wrote to
/// `$GITHUB_OUTPUT`, and what it said.
struct Planned {
    ok: bool,
    emitted: String,
    /// **Both streams.** The script writes its refusals to stdout, because a
    /// `::error::` workflow command has to go there to become an annotation,
    /// which is also what `ci.yml` does. An assertion printing only stderr
    /// therefore printed nothing for every refusal the script can produce.
    said: String,
}

/// Run the plan script with these inputs, or `None` where there is no `bash`.
///
/// The scratch directory is created **after the probe below answers and before
/// the script runs**, which is the only ordering available: `$GITHUB_OUTPUT`
/// has to exist before the script writes to it, so it cannot wait for that
/// spawn. What it must not do is precede the probe, which is what it used to
/// do: every run on a machine with no usable `bash` then leaked a directory, in
/// the one file whose headline invariant is that nothing is retained.
fn run_plan(script: &str, seconds: &str, runner: &str) -> Option<Planned> {
    run_plan_on(script, seconds, runner, "false")
}

/// The same, with the daily cron's flag under the caller's control.
fn run_plan_on(script: &str, seconds: &str, runner: &str, daily: &str) -> Option<Planned> {
    // A counter rather than anything derived from the inputs: two labels of the
    // same length would otherwise share a directory, and two of the hostile
    // ones below are both thirteen characters.
    static RUN: AtomicU64 = AtomicU64::new(0);
    let scratch = std::env::temp_dir().join(format!(
        "vigia-plan-{}-{}",
        std::process::id(),
        RUN.fetch_add(1, Ordering::Relaxed)
    ));

    let out = scratch.join("output");

    let mut command = Command::new("bash");
    command
        .arg("-c")
        .arg(script)
        .env("SOAK_SECONDS", seconds)
        .env("SOAK_RUNNER", runner)
        .env("SOAK_DAILY", daily)
        .env("GITHUB_OUTPUT", &out);

    // Probe before building anything for it to write into, and **probe for a
    // `bash` that works rather than for one that exists**.
    //
    // Spawning is not the question. `windows-latest` puts WSL's launcher on
    // `PATH` as `bash.exe`, which starts perfectly and then reports "Windows
    // Subsystem for Linux has no installed distributions" and exits non-zero,
    // so a probe that only asked whether the process started said yes and three
    // tests then failed on the runner with WSL's setup instructions as their
    // panic message. Locally the same three pass, because Git Bash comes first
    // on `PATH` there. Checking the exit status covers both that and a missing
    // `bash`, and a `bash` too broken to run `exit 0` is one these tests are
    // right to skip.
    let usable = Command::new("bash")
        .arg("-c")
        .arg("exit 0")
        .output()
        .is_ok_and(|probe| probe.status.success());
    if !usable {
        return None;
    }
    std::fs::create_dir_all(&scratch).expect("a scratch directory for GITHUB_OUTPUT");
    std::fs::write(&out, "").expect("an empty GITHUB_OUTPUT");

    let run = command.output();
    let planned = run.ok().map(|run| Planned {
        ok: run.status.success(),
        emitted: std::fs::read_to_string(&out).expect("read back GITHUB_OUTPUT"),
        said: format!(
            "{}{}",
            String::from_utf8_lossy(&run.stdout),
            String::from_utf8_lossy(&run.stderr)
        ),
    });
    let _ = std::fs::remove_dir_all(&scratch);
    planned
}

/// Printed once where `bash` is missing, rather than passing quietly.
fn no_bash(what: &str) {
    println!(
        "note: {what} is not gated here, because no working `bash` answered. It \
         runs on Linux and macOS in CI, and locally wherever one is on PATH; on \
         a Windows runner `bash` is WSL's launcher with no distribution behind \
         it, so this skips there and the Linux leg is what holds it."
    );
}

/// A dispatch input cannot corrupt the file the plan job writes its answers to.
///
/// `$GITHUB_OUTPUT` is a line-oriented `key=value` file, so **anything that
/// puts a second line under one key breaks the step**, and the runner's error
/// for that names nothing useful. The `runner` label is the one free-form value
/// in the job, so it is the one that has to be handled.
///
/// It is **validated rather than escaped**, and the reason is portability
/// rather than taste. Escaping arbitrary text into JSON correctly is what `jq`
/// is for, and `jq` ships on the hosted images but on neither stock macOS nor
/// stock Git Bash. Requiring it here would have failed `cargo test --workspace`
/// on two of three tier-1 targets, which is a steep price for accepting a
/// runner label nobody would ever type. Every real label is letters, digits,
/// dot, dash and underscore.
#[test]
fn a_runner_label_cannot_corrupt_the_plan_job_output() {
    let (path, source) = workflow();
    let script = plan_script(&path, &source);

    // An ordinary label plans normally.
    let Some(good) = run_plan(&script, "600", "ubuntu-latest") else {
        no_bash("the plan job's handling of a runner label");
        return;
    };
    assert!(
        good.ok,
        "the plan script refused an ordinary runner label:\n{}",
        good.said
    );
    assert_eq!(
        good.emitted.lines().count(),
        3,
        "an ordinary runner label produced {} lines of GITHUB_OUTPUT rather \
         than the three keys the job declares:\n{}",
        good.emitted.lines().count(),
        good.emitted
    );
    assert!(
        good.emitted.contains("os=[\"ubuntu-latest\"]"),
        "the platform list is not the single label it was given:\n{}",
        good.emitted
    );

    // And every shape that could write outside its own value is refused, out
    // loud, before anything is emitted.
    for hostile in ["ubuntu-latest\nx=y", "ubuntu\"], [\"x", "a b"] {
        let Some(bad) = run_plan(&script, "600", hostile) else {
            no_bash("the plan job's refusal of a hostile runner label");
            return;
        };
        assert!(
            !bad.ok,
            "the plan script accepted the runner label {hostile:?} and wrote:\n{}",
            bad.emitted
        );
        assert!(
            bad.emitted.trim().is_empty(),
            "the plan script refused the runner label {hostile:?} and still \
             wrote to GITHUB_OUTPUT, so a refusal is not the same as writing \
             nothing:\n{}",
            bad.emitted
        );
        assert!(
            bad.said.contains("::error::"),
            "the plan script refused the runner label {hostile:?} without an \
             annotation anyone would see:\n{}",
            bad.said
        );
    }
}

/// Which platforms each trigger soaks on, run rather than read.
///
/// **The producer side of the same hole three audit rounds closed on the
/// consumer side.** The soak job is now gated into reading
/// `needs.plan.outputs.os`, so pinning a literal matrix there fails. Nothing
/// looked at what the plan job *puts* in that output: narrowing the default
/// list to one label deletes macOS and Windows from every weekly run and every
/// manual dispatch, with the whole suite green and the workflow valid.
///
/// `SPEC.md` §7 is what this holds in place: "the soak is two tiers, like every
/// other budget here", weekly on all three tier-1 targets, and a leak is
/// usually not platform-specific but that is a reason to sample all three
/// rarely rather than one of them often.
#[test]
fn the_plan_job_soaks_the_platforms_each_trigger_asks_for() {
    let (path, source) = workflow();
    let script = plan_script(&path, &source);

    let platforms = |planned: &Planned| -> String {
        planned
            .emitted
            .lines()
            .find_map(|line| line.strip_prefix("os="))
            .unwrap_or_else(|| panic!("no `os=` in the plan output:\n{}", planned.emitted))
            .to_owned()
    };

    // **The flag has to name a cron this workflow actually runs on.** The three
    // legs below drive `SOAK_DAILY` directly, so they gate the script's
    // branches and not the trigger that sets the flag: change either the
    // schedule or the string it is compared against, and the daily cron takes
    // the weekly leg with everything here still green.
    let daily_cron = workflow_settings(&source, "SOAK_DAILY:")
        .into_iter()
        .next()
        .expect("the plan job sets SOAK_DAILY");
    let schedules = workflow_settings(&source, "- cron:");
    assert!(
        schedules
            .iter()
            .any(|cron| daily_cron.contains(cron.trim_matches('"'))),
        "`SOAK_DAILY` is {daily_cron:?}, which compares against no cron this \
         workflow is scheduled on ({schedules:?}), so the branch below fires \
         for no trigger or for every one"
    );

    // The daily cron: Linux alone, because three hosted runners every day is
    // not a proportionate way to find a leak that is rarely platform-specific.
    let Some(daily) = run_plan_on(&script, "14400", "", "true") else {
        no_bash("the plan job's platform list");
        return;
    };
    assert!(daily.ok, "the daily plan failed:\n{}", daily.said);
    assert_eq!(
        platforms(&daily),
        "[\"ubuntu-latest\"]",
        "the daily cron no longer soaks Linux alone"
    );

    // Everything else, which is the weekly cron and every manual dispatch: all
    // three tier-1 targets.
    let weekly = run_plan_on(&script, "14400", "", "false").expect("bash was there a moment ago");
    assert!(weekly.ok, "the weekly plan failed:\n{}", weekly.said);
    for target in ["ubuntu-latest", "macos-latest", "windows-latest"] {
        assert!(
            platforms(&weekly).contains(target),
            "the weekly run no longer soaks {target}, and it is a tier-1 \
             target: {}",
            platforms(&weekly)
        );
    }

    // And a named runner replaces the list rather than joining it, or the
    // uncapped machine I3's day needs would soak beside two capped ones.
    let named =
        run_plan_on(&script, "14400", "self-hosted", "false").expect("bash was there a moment ago");
    assert!(named.ok, "the named-runner plan failed:\n{}", named.said);
    assert_eq!(
        platforms(&named),
        "[\"self-hosted\"]",
        "a named runner did not replace the platform matrix"
    );
}

/// The plan job's arithmetic, run rather than read.
///
/// **[`the_soak_workflow_cannot_kill_the_window_it_offers`] checks the plumbing
/// and cannot see this**, which is the whole reason they are separate: with only the plumbing asserted, writing
/// `timeout=330` straight into the plan script restores the original defect
/// verbatim and the suite stays green. So do the one thing that settles it and
/// execute the script, with `GITHUB_OUTPUT` pointed at a scratch file, then
/// read back the number it computed.
///
/// The property is not "the timeout equals seconds / 60 + 90" — that is the
/// current arithmetic and pinning it would tax the next improvement the way the
/// first version of the sibling gate did. It is that **every window the input
/// offers gets a timeout long enough to finish in**, with room for a cold
/// release build on top. `SPEC.md` §7's shape: assert the bound, not the
/// formula.
///
/// Skips with a printed reason where `bash` is missing rather than passing
/// quietly, the way [`Report::gate_descriptors`] handles `/proc`.
#[test]
fn the_plan_job_gives_every_window_room_to_finish() {
    let (path, source) = workflow();

    let script = plan_script(&path, &source);

    // Every window a reader might reasonably dispatch, including the two the
    // file names in prose and the one I3's budget is written against.
    for seconds in [1u64, 600, 900, 14400, 86400] {
        let Some(planned) = run_plan(&script, &seconds.to_string(), "") else {
            no_bash("the plan job's arithmetic");
            return;
        };
        assert!(
            planned.ok,
            "the plan script refused a {seconds}s window:\n{}",
            planned.said
        );

        let emitted = planned.emitted;
        let value = |key: &str| -> u64 {
            emitted
                .lines()
                .find_map(|line| line.strip_prefix(key))
                .unwrap_or_else(|| panic!("no `{key}` in the plan output:\n{emitted}"))
                .trim()
                .parse()
                .unwrap_or_else(|e| panic!("`{key}` is not a number in:\n{emitted} ({e})"))
        };

        assert_eq!(
            value("seconds="),
            seconds,
            "the plan job changed the window it was given, so the soak would \
             run for a length nobody asked for"
        );

        // The bound, in seconds, against the window plus a cold build. Ninety
        // minutes is what the file allows; anything at or over the window
        // itself would already be a real improvement on the pinned 330, so the
        // floor is deliberately under the current headroom rather than equal
        // to it.
        let timeout = value("timeout=") * 60;
        assert!(
            timeout >= seconds + 1800,
            "a {seconds}s window was planned under a {timeout}s timeout, which \
             leaves {}s for checkout, build and fixture. A timeout that cannot \
             hold the window it was computed from is exactly the defect this \
             job exists to make impossible, and at 86400 it is the one that \
             shipped",
            timeout.saturating_sub(seconds)
        );
    }
}

mod statistic {
    //! The verdict, tested as the pure function it is.

    use super::*;

    /// Mebibytes, because RSS is quoted in them everywhere else here.
    fn mb(value: f64) -> u64 {
        (value * MIB) as u64
    }

    /// A process that is flat, jittering by a page either way.
    fn flat(len: usize) -> Vec<u64> {
        (0..len)
            .map(|at| mb(100.0) + (at as u64 % 3) * 4096)
            .collect()
    }

    /// A process handing memory back, steadily.
    ///
    /// Shared by the two tests that assert on the ends of the same shrink: the
    /// ratio is clamped to zero and the gradient is not. They are only about
    /// one series while one series builds them both.
    fn shrinking() -> Vec<u64> {
        (0..288).map(|at| mb(200.0 - 0.2 * at as f64)).collect()
    }

    /// The leak the gate is calibrated on, sized just over the budget rather
    /// than absurdly over it.
    ///
    /// 288 samples growing 0.05% each is 14.4% end to end. **0.05% is the
    /// calibration**, which is the reason this is a function and not a literal
    /// in two tests: [`a_linear_leak_is_caught`] owns the verdict and
    /// [`a_series_that_never_settles_keeps_the_fraction_floor`] owns where the
    /// baseline came from, and a rate retuned in one of them and not the other
    /// would leave two tests disagreeing about what "just over budget" means.
    fn leaking() -> Vec<u64> {
        (0..288)
            .map(|at| mb(100.0 * (1.0 + 0.0005 * at as f64)))
            .collect()
    }

    /// A process that climbs hard and reaches its plateau inside the floor.
    ///
    /// The allocator's own shape, and the case that decides [`WARMUP_FRACTION`]:
    /// first sample to last it climbs 140%. Shared by the
    /// test that says it is not drift and the one that says [`warmup`] finds
    /// nothing to measure in it, which are two claims about one series.
    fn plateaus_early() -> Vec<u64> {
        let climb = 288 * 8 / 100;
        (0..288)
            .map(|at| {
                if at < climb {
                    mb(50.0 + 70.0 * (at as f64 / climb as f64))
                } else {
                    mb(120.0)
                }
            })
            .collect()
    }

    /// A strictly increasing series, so no two samples share a value.
    ///
    /// **`step` is load bearing and the two callers must agree on it**, which is
    /// why it is a parameter of one function rather than a literal in each. At
    /// one page (4096) the whole climb is under 2% of the level, [`warmup`]
    /// finds a plateau at sample 60, and the 228 post-warmup samples leave a
    /// remainder of **zero** — at which the two slicings those callers exist to
    /// tell apart are the same four windows and both go vacuous. At three pages
    /// nothing settles inside [`WARMUP_CEILING`], the baseline falls back to the
    /// floor, and the remainder of 3 their guards demand comes back.
    fn ramp(step: u64) -> Vec<u64> {
        (0..288).map(|at| mb(60.0) + at as u64 * step).collect()
    }

    /// The step this module's ramps take: see [`ramp`].
    const RAMP_STEP: u64 = 12288;

    /// The interval a full-length run samples at.
    ///
    /// 288 samples across 24 hours, which is `SPEC.md`'s "every 5 min" and the
    /// cadence [`MAX_SAMPLES`] exists to hold. Using the real one means the
    /// MiB/h figures below are the magnitudes a real report prints rather than
    /// numbers scaled by whatever a test picked.
    const SAMPLE: Duration = Duration::from_secs(300);

    /// A bare RSS series against the clock it would have been sampled on.
    ///
    /// [`drift`] takes elapsed times because a gradient per *hour* needs them;
    /// every series below is built as values first because that is the half
    /// each test is about.
    ///
    /// Not `timed`, which `support::timed` already owns in this crate with an
    /// unrelated contract — it times a closure, and `budgets.rs` calls it by
    /// that bare name. One word meaning two things across two test binaries is
    /// exactly what that helper's own doc warns about for timers.
    fn sampled(series: &[u64]) -> Vec<(Duration, u64)> {
        series
            .iter()
            .enumerate()
            .map(|(at, &rss)| (SAMPLE * at as u32, rss))
            .collect()
    }

    /// The verdict over a series that is long enough to have one.
    ///
    /// Nine tests wanted the same three-part incantation, so the sentence
    /// explaining why 288 samples is enough is written once rather than nine
    /// times. The one test that asserts [`drift`] returns `None` cannot use it
    /// and calls [`sampled`] directly, which is the point of keeping both.
    fn verdict(series: &[u64]) -> Drift {
        drift(&sampled(series)).expect("288 samples is enough to have a verdict")
    }

    #[test]
    fn a_flat_series_does_not_drift() {
        let drift = verdict(&flat(288));
        assert!(
            drift.ratio < DRIFT_BUDGET,
            "a flat series drifted by {:.2}%, so the gate cannot report 'no drift' \
             and every green soak run means nothing",
            drift.ratio * 100.0
        );
    }

    /// The leak the gate exists for, sized just over the budget rather than
    /// absurdly over it: a test that ramps by 10x proves only that the
    /// arithmetic has a sign.
    ///
    /// 288 samples growing 0.05% each is 14.4% end to end. The warmup discards
    /// 29, so the baseline is the median of samples 29..93 and the settled
    /// figure is the median of 224..288, which is about 9.5% apart.
    ///
    /// **29 is [`WARMUP_FRACTION`]'s floor rather than [`warmup`]'s plateau, and
    /// the difference matters to a reader of this test.** A series climbing to
    /// its last sample never settles, so the detector reports
    /// [`Baseline::Unsettled`] and hands the gate the whole window. That the
    /// figure above survived #126 unchanged is the point rather than a
    /// coincidence, and
    /// [`a_series_that_never_settles_keeps_the_fraction_floor`] is what asserts
    /// the mechanism behind it.
    #[test]
    fn a_linear_leak_is_caught() {
        let drift = verdict(&leaking());
        assert!(
            drift.ratio > DRIFT_BUDGET,
            "a series that grew 14% over the window reported {:.2}% drift, under \
             the {:.0}% budget, so a leak of this shape ships",
            drift.ratio * 100.0,
            DRIFT_BUDGET * 100.0
        );
    }

    /// What an allocator plateau looks like, and it must not read as a leak.
    ///
    /// The climb is over inside the first 8% of the window, so the warmup
    /// swallows it whole. This is the case that decides `WARMUP_FRACTION`: with
    /// no warmup at all, the same series climbs 140% from its first sample to its
    /// last. It still decides it,
    /// because a series already at its level by the floor has nothing for
    /// [`warmup`] to measure and takes the floor:
    /// [`a_series_flat_from_the_floor_takes_the_floor`] asserts that half.
    ///
    /// **It is also the only fixture here that can tell where the gradient was
    /// fitted**, and it did not always assert on one. Every other series in
    /// this module is linear from end to end, so fitting through the warmup
    /// and fitting past it give the same answer and a mutation swapping one
    /// for the other survived them all. This one is a step followed by a
    /// plateau: past the warmup it is flat, through it, steeply positive.
    #[test]
    fn growth_that_stops_inside_the_warmup_is_not_drift() {
        let drift = verdict(&plateaus_early());
        assert!(
            drift.ratio < DRIFT_BUDGET,
            "a process that reached its plateau in the first 8% of the window \
             reported {:.2}% drift, so every soak fails for a reason that is not \
             a leak",
            drift.ratio * 100.0
        );
        assert!(
            drift.slope.abs() < 0.05,
            "the same plateau reported {:+.2} MiB/h, so the gradient was fitted \
             through the warmup climb the medians discard and the report would \
             call an allocator filling up a trend",
            drift.slope
        );
    }

    /// Too short to have two ends is not a pass. A gate that answers "fine" from
    /// four samples is worse than one that is absent, because it looks like
    /// coverage.
    #[test]
    fn a_series_too_short_to_have_two_ends_reports_nothing() {
        for len in 0..=(MIN_END * 4) {
            let series = sampled(&flat(len));
            assert_eq!(
                drift(&series),
                None,
                "{len} samples produced a verdict, and each end of it holds fewer \
                 than {MIN_END} samples"
            );
        }
        assert!(
            drift(&sampled(&flat(MIN_END * 4 + 1))).is_some(),
            "the shortest series that does have two ends of {MIN_END} was refused, \
             so the guard rejects series it should answer"
        );
    }

    /// The gate's two numbers are the outer two quarters, and they are pinned
    /// here against the rule rather than against the implementation.
    ///
    /// [`Drift::quarters`] carries four now, and the obvious way to cut four
    /// out of a series — `len * k / 4` — puts the last one's left edge a sample
    /// earlier on any length that is not a multiple of four. A 288-sample run
    /// leaves 259 after the warmup, so that is the ordinary case rather than an
    /// edge. This re-derives the ends from `SPEC.md` §7's rule — discard the
    /// warmup, take the median of a quarter at each end — and compares.
    ///
    /// **It takes two fixtures, and each one hides what the other catches.**
    ///
    /// On a plain monotone ramp, `[..q]`/`[end-q..]` and `end * k / 4` report
    /// the *same* medians: widening a 64-sample window to 65 moves the nearest
    /// rank from index 31 to 32 at the same time as it prepends a value, and on
    /// sorted data the two shifts cancel exactly. The first version of this
    /// test used a ramp and passed against both slicings, proving nothing.
    ///
    /// Adding a spike just before the boundary breaks that cancellation — and
    /// then hides the *other* mutation, a window sliding one sample off the
    /// end, because a spike that outranks everything keeps the median at the
    /// same rank however the window shifts under it. So both shapes run, and
    /// each end is re-derived from `SPEC.md` §7's rule rather than from the
    /// code under test.
    ///
    /// **[`RAMP_STEP`] is load bearing rather than arbitrary**, and [`ramp`]
    /// carries why: at one page the measured baseline lands where the remainder
    /// is zero and both slicings agree, which would make this whole test
    /// vacuous. The rule is asserted rather than assumed, two lines down.
    #[test]
    fn the_gated_ends_are_the_first_and_last_quarter_medians() {
        let ramp = ramp(RAMP_STEP);
        // The same, plus an outlier at post-warmup index 194: the sample
        // `end * 3 / 4` includes in the last quarter and `end - quarter` does
        // not.
        let spiked = {
            let mut spiked = ramp.clone();
            spiked[223] = mb(500.0);
            spiked
        };

        for (shape, series) in [("a ramp", &ramp), ("a ramp with an outlier", &spiked)] {
            let drift = verdict(series);

            let (warm, rule) = warmup(series);
            assert_eq!(
                (warm, rule),
                (warmup_floor(series.len()), Baseline::Unsettled),
                "over {shape}, the baseline no longer comes from the floor, and \
                 the remainder guard below is what that silently turns vacuous"
            );
            let rest = &series[warm..];
            let quarter = rest.len() / 4;
            assert_eq!(
                rest.len() % 4,
                3,
                "this fixture is meant to be a length the two slicings disagree \
                 on, and {} post-warmup samples leaves a remainder of {}",
                rest.len(),
                rest.len() % 4
            );

            assert_eq!(
                drift.quarters[0],
                median(&rest[..quarter]).expect("a quarter of the post-warmup samples"),
                "over {shape}, the first quarter's median is not the baseline \
                 the gate compares from"
            );
            assert_eq!(
                drift.quarters[3],
                median(&rest[rest.len() - quarter..])
                    .expect("a quarter of the post-warmup samples"),
                "over {shape}, the last quarter's median is not the settled \
                 figure the gate compares to, so the ends have moved and the \
                 number I3 gates on moved with them"
            );
            assert_eq!(
                (drift.baseline(), drift.settled()),
                (drift.quarters[0], drift.quarters[3]),
                "over {shape}, the accessors and the array disagree, so the \
                 report and the gate can print different numbers for one run"
            );
        }
    }

    /// The middle pair is measured inward from the ends, and this is what says
    /// so.
    ///
    /// [`the_gated_ends_are_the_first_and_last_quarter_medians`] pins indices 0
    /// and 3 only, and the climb test asserts the four rise on a ramp, which
    /// every candidate slicing does. So the `end * k / 4` cutting that
    /// [`Drift::quarters`] spends a paragraph rejecting **survived the whole
    /// module**, and so did a version widening the middle pair to `[q, 3q)`.
    /// A documented decision with nothing that fails when it is violated is
    /// what `CLAUDE.md` calls a wish.
    ///
    /// **A plain ramp is the right fixture here, and adding an outlier made
    /// half of this test vacuous.** The ends need one because both of their
    /// edges move together: widening the last quarter from 64 samples to 65
    /// prepends a value *and* advances the nearest rank, and on sorted data
    /// those cancel exactly. Each middle window moves at only **one** edge, so
    /// there is nothing to cancel and a ramp separates the two slicings on its
    /// own. A spike added here for symmetry landed inside the naive third
    /// quarter and shifted its rank by exactly the sample the two windows
    /// differ by, making `quarters[2]` identical under both and leaving one
    /// live assertion where the doc claimed two.
    ///
    /// [`RAMP_STEP`] rather than one page, for the reason [`ramp`] carries: at
    /// one page the measured baseline lands where the remainder is zero and
    /// both slicings agree.
    #[test]
    fn the_middle_quarters_are_measured_inward_from_the_ends() {
        // No outlier: see above.
        let series = ramp(RAMP_STEP);
        let (warm, rule) = warmup(&series);
        assert_eq!(
            (warm, rule),
            (warmup_floor(series.len()), Baseline::Unsettled),
            "the baseline no longer comes from the floor, and the remainder \
             guard below is what that silently turns vacuous"
        );
        // **The same guard the ends test carries, and for a sharper reason.**
        // Where the post-warmup length divides by four, the inward cut and the
        // `end * k / 4` cut are the *same four windows*, so this test would
        // pass against the slicing it exists to reject. A change to
        // `WARMUP_FRACTION` or to the fixture length is all it would take.
        let remainder = (series.len() - warm) % 4;
        assert_eq!(
            remainder,
            3,
            "this fixture leaves {} post-warmup samples, a remainder of \
             {remainder}, and at a remainder of zero the two slicings this test \
             tells apart are the same four windows",
            series.len() - warm
        );

        let quarter = (series.len() - warm) / 4;
        let drift = verdict(&series);
        let rest = &series[warm..];

        assert_eq!(
            drift.quarters[1],
            median(&rest[quarter..quarter * 2]).expect("a quarter of the samples"),
            "the second quarter is not measured a quarter's width in from the \
             start, so the four are being cut at `end * k / 4` and the rule \
             `Drift::quarters` documents is not the rule running"
        );
        let end = rest.len();
        assert_eq!(
            drift.quarters[2],
            median(&rest[end - quarter * 2..end - quarter]).expect("a quarter of the samples"),
            "the third quarter is not measured a quarter's width in from the \
             end, so the middle pair has drifted off the ends it is supposed to \
             be anchored to"
        );
    }

    /// The divisor every number in the report is quoted through is a mebibyte.
    ///
    /// One line, because without it nothing can catch a wrong one: this
    /// module's [`mb`] multiplies by `MIB` and the file's [`mib`] divides by
    /// it, so the two cancel and every MiB assertion here passes for *any*
    /// value of the constant.
    ///
    /// **It pins this file's divisor and not the shell's**, which is worth
    /// saying because the name first put on it claimed otherwise. The status
    /// bar keeps a private `MIB` of its own in `crates/vigia/src/render.rs` and
    /// an integration test cannot reach it, so the divergence [`MIB`]'s doc
    /// worries about is half covered: a wrong constant *here* now fails, and a
    /// wrong constant *there* still would not. Saying so beats a name that
    /// implies both.
    #[test]
    fn the_report_is_quoted_in_mebibytes() {
        assert_eq!(
            mib(1024 * 1024),
            1.0,
            "MIB is not 2^20, so every RSS figure in this report is quoted in \
             units nothing else uses"
        );
    }

    /// A gradient carries the span it was divided by, and the span follows the
    /// baseline that was actually used.
    ///
    /// See [`Drift::span`]. Without it the four figures `SPEC.md` §10 asks a
    /// reader to compare are not comparable, because the same wander reads
    /// +364 MiB/h over fifteen seconds and +0.92 over an hour.
    ///
    /// **Two fixtures, because one of them cannot see half the claim.** A flat
    /// series takes the floor, so on it the floor and the measured baseline are
    /// the same number and a gradient still fitted from the floor would pass.
    /// The recorded day is the case where they differ by 68 samples: fitting
    /// past the fraction rather than past the plateau puts nearly six hours of
    /// climb back into a slope printed as the settled one, which is the figure
    /// §10 asks a reader to compare between runs.
    #[test]
    fn the_gradient_is_reported_with_the_span_it_was_fitted_over() {
        for (shape, series) in [
            ("a flat series", flat(288)),
            ("the recorded day", day_long()),
        ] {
            let drift = verdict(&series);
            assert_eq!(
                drift.span,
                SAMPLE * (series.len() - drift.warm) as u32 - SAMPLE,
                "over {shape}, the span is not the elapsed time past the \
                 baseline the gate used, so it describes a different window \
                 from the gradient it is printed beside"
            );
            // **And the gradient itself, not only its span.** The two are
            // separate expressions over what is meant to be one slice, so a
            // slope fitted from the fraction while the span is taken from the
            // plateau leaves both fields present, plausible, and describing
            // different windows. Mutation confirmed it: pointing `slope` at
            // the floor left the whole suite green until this line existed.
            //
            // A least-squares gradient is invariant to shifting its clock, so
            // re-sampling the tail from zero is the same fit the report did.
            assert_eq!(
                drift.slope,
                mib_per_hour(&sampled(&series[drift.warm..])),
                "over {shape}, the gradient is not fitted past the baseline the \
                 gate used"
            );
        }

        // And the two fixtures above genuinely disagree, or the second one is
        // saying no more than the first.
        let day = day_long();
        assert_ne!(
            warmup(&day).0,
            warmup_floor(day.len()),
            "the recorded day now takes the floor, so both fixtures here are \
             floor cases and neither can tell a gradient fitted from the \
             fraction from one fitted past the plateau"
        );
    }

    /// The gradient, in the units the report prints and against a series whose
    /// answer is known by construction.
    ///
    /// **The magnitude as well as the sign**, because the answer has to be
    /// right in both: `SPEC.md` §10 has one hour of this code at +0.92 MiB/h
    /// and fifteen minutes of it at −0.70, and a statistic that only got the
    /// direction right could not tell either of those from the +81.3 an
    /// injected leak produced.
    #[test]
    fn a_known_linear_climb_reports_its_slope_in_mib_per_hour() {
        const RATE: f64 = 2.0;
        let series: Vec<u64> = (0..288)
            .map(|at| {
                let hours = (SAMPLE * at as u32).as_secs_f64() / 3600.0;
                mb(100.0 + RATE * hours)
            })
            .collect();

        let drift = verdict(&series);
        assert!(
            (drift.slope - RATE).abs() < 0.01,
            "a series climbing exactly {RATE:.2} MiB/h was reported at \
             {:+.4} MiB/h, and the disagreement this statistic exists to settle \
             is 1.62 MiB/h wide",
            drift.slope
        );
    }

    /// The sign the ratio deliberately throws away is the one the gradient has
    /// to keep, and both halves are asserted in one place so the asymmetry is a
    /// gate rather than a comment.
    ///
    /// **The ratio half is here rather than in a test of its own**, which it
    /// used to have: `a_series_that_gave_memory_back_reports_no_drift_rather_
    /// than_a_negative_one` opened with this exact assertion over this exact
    /// fixture, so once this test existed the older one could not fail while
    /// this one passed. A gate that is a strict subset of another is a gate
    /// nobody will maintain and everybody will count.
    #[test]
    fn a_series_that_gave_memory_back_reports_a_negative_slope_where_the_ratio_reports_zero() {
        let drift = verdict(&shrinking());

        assert_eq!(
            drift.ratio, 0.0,
            "a shrinking series reported {drift:?}, and the gate's ratio is \
             clamped so that a process giving memory back cannot pass for the \
             wrong reason"
        );
        assert!(
            drift.slope < 0.0,
            "the same shrinking series reported {:+.2} MiB/h, so the gradient is \
             clamped too and `SPEC.md` §10's −0.70 could never have been stated",
            drift.slope
        );
    }

    /// The degenerate series, driven straight at [`mib_per_hour`].
    ///
    /// Its guard is unreachable through [`drift`], which only ever hands it
    /// eight or more samples at strictly increasing instants, so deleting it
    /// leaves every other test in this file green. That is the tell
    /// `SPEC.md` §7 names, and a doc comment promising behaviour nothing
    /// exercises is what `CLAUDE.md` calls a wish. Cheaper to reach past
    /// `drift` and assert them than to narrow the function.
    #[test]
    fn a_series_with_nothing_to_fit_through_reports_no_gradient() {
        for (what, samples) in [
            ("an empty series", Vec::new()),
            ("one sample", vec![(Duration::from_secs(1), mb(100.0))]),
            (
                "every sample at the same instant",
                vec![
                    (Duration::ZERO, mb(100.0)),
                    (Duration::ZERO, mb(700.0)),
                    (Duration::ZERO, mb(300.0)),
                ],
            ),
        ] {
            assert_eq!(
                mib_per_hour(&samples),
                0.0,
                "{what} reported a gradient. There is no line through it, and \
                 the alternative to saying so is a division by zero variance \
                 that arrives as an infinity and reads as a catastrophic leak"
            );
        }
    }

    /// A statistic that cannot report "no trend" has not been tested, which is
    /// the same rule `SPEC.md` §7 applies to the drift gate itself.
    #[test]
    fn a_flat_series_reports_a_slope_of_about_zero() {
        let drift = verdict(&flat(288));
        assert!(
            drift.slope.abs() < 0.05,
            "a series jittering by a page around a constant reported \
             {:+.2} MiB/h, which is a fifteenth of the residual §10 could not \
             call, so the instrument's own noise would swamp the answer",
            drift.slope
        );
    }

    /// The shape two endpoints round off.
    ///
    /// `SPEC.md` §10's hour reported 2.18% drift, comfortably inside a 5%
    /// budget, and the thing that made it a question at all was that its four
    /// quarter medians rose monotonically. A run reporting only the ends is
    /// indistinguishable from one that stepped once and went flat.
    #[test]
    fn the_quarter_medians_show_a_climb_that_the_two_ends_alone_round_off() {
        let series: Vec<u64> = (0..288).map(|at| mb(25.5 + 0.0025 * at as f64)).collect();
        let drift = verdict(&series);

        assert!(
            drift.ratio < DRIFT_BUDGET,
            "this fixture is meant to pass the gate and still be a trend, and it \
             reported {:.2}% against a {:.0}% budget",
            drift.ratio * 100.0,
            DRIFT_BUDGET * 100.0
        );
        assert!(
            drift.quarters.windows(2).all(|pair| pair[0] < pair[1]),
            "a monotonically climbing series reported quarters {:?}, so the \
             middle pair is not being measured where the ends are",
            drift.quarters
        );
    }

    /// The RSS series of the first full-window soak, in KiB, in sample order.
    ///
    /// Reference machine, release, 100 x 500 fixture, **86,400 seconds** from
    /// 2026-08-05 22:22, 288 samples at a five-minute cadence, 1,850,935 frames.
    /// The reading is on [#47](https://github.com/breferrari/vigia/issues/47) and
    /// the series is preserved on
    /// [#126](https://github.com/breferrari/vigia/issues/126); both were checked
    /// byte for byte against `target/soak-24h-2026-08-05.log` before this was
    /// copied out of it, and that log lives in a gitignored directory one
    /// `cargo clean` from gone, which is why it is here.
    ///
    /// **It is the only fixture in this module that was not invented**, and that
    /// is its whole value: every other series here is a shape someone chose,
    /// which means every one of them can be chosen to agree with the code. This
    /// one was produced by the process the gate is about, and the gate failed on
    /// it.
    const DAY_LONG: [u32; 288] = [
        28768, 28552, 29140, 28488, 26764, 28256, 28672, 28864, 29288, 28916, 28068, 27308, 27056,
        29632, 30732, 29816, 29888, 29068, 30100, 30488, 30596, 31136, 30668, 32016, 31012, 30448,
        29872, 31252, 32696, 28848, 31332, 30132, 31540, 31084, 31368, 31696, 31348, 31784, 32696,
        30448, 31324, 31592, 32828, 32048, 29752, 30696, 31556, 30748, 30932, 31528, 31056, 30984,
        32724, 31028, 30712, 28772, 31752, 31188, 31736, 32148, 30976, 30984, 32660, 30796, 33064,
        32380, 31908, 30280, 31216, 31788, 33104, 32684, 32536, 32688, 31976, 31420, 31304, 32088,
        32812, 31608, 32448, 31916, 32468, 32056, 31840, 32344, 32364, 32248, 32876, 31436, 34124,
        35520, 34568, 34464, 35256, 35456, 35208, 34664, 35756, 36176, 34944, 34676, 35352, 35336,
        34844, 35304, 36092, 34604, 34552, 34432, 34404, 35272, 33128, 35004, 36316, 35992, 35580,
        36252, 35048, 35400, 34872, 34820, 35884, 34880, 35812, 34840, 36448, 34820, 35640, 36736,
        35960, 35808, 34912, 36236, 35420, 35592, 35592, 35564, 36016, 36920, 36792, 35832, 35324,
        37036, 36344, 35740, 36288, 36016, 34724, 35964, 35908, 35152, 35532, 35748, 35976, 36420,
        36520, 35792, 35260, 35828, 35708, 36272, 35088, 36480, 34752, 36312, 36712, 34764, 35712,
        36676, 35304, 35828, 36100, 35960, 34892, 35368, 35708, 35448, 35420, 35948, 35404, 35332,
        34728, 36384, 36060, 35068, 35208, 34580, 35192, 36280, 35408, 36044, 35288, 35592, 33524,
        35784, 35236, 36532, 35900, 36060, 36388, 35608, 34544, 36280, 36420, 36608, 36436, 36036,
        36052, 35464, 35864, 36120, 35136, 35860, 35664, 36180, 36128, 36540, 36496, 36380, 35172,
        36148, 35960, 36556, 34980, 35900, 35536, 36964, 34376, 36300, 36240, 34576, 35380, 35612,
        35520, 35716, 36564, 35968, 36120, 34736, 35804, 36272, 35312, 36344, 35616, 36416, 35488,
        35572, 35452, 35648, 36236, 37188, 35112, 36104, 37132, 35948, 36172, 35852, 36476, 36288,
        33876, 35892, 35668, 36252, 34608, 35860, 36792, 36556, 36288, 36240, 37124, 37096, 35476,
        37100, 35912, 35708, 36656, 35816, 37192, 36496, 36076, 33888, 35748, 35532, 35800, 36232,
        35332, 36232,
    ];

    /// [`DAY_LONG`] as the byte counts the statistic takes.
    fn day_long() -> Vec<u64> {
        DAY_LONG.iter().map(|&kib| u64::from(kib) * 1024).collect()
    }

    /// The recorded day, re-derived: it breached through a window fraction and
    /// does not through a measured plateau.
    ///
    /// **Both halves, because either alone is satisfiable by a mistake.** A test
    /// asserting only that the day passes now is passed by deleting the gate;
    /// one asserting only that the fraction failed is passed by any code at all,
    /// since the fraction is not what runs. Together they say the thing
    /// [#126](https://github.com/breferrari/vigia/issues/126) claims: that one
    /// series, unchanged, is a breach under one baseline rule and is not under
    /// the other, so what changed is where the comparison starts and not what it
    /// tolerates.
    ///
    /// This is also the re-derivation
    /// [#47](https://github.com/breferrari/vigia/issues/47) is waiting on, which
    /// is why it is a test rather than a paragraph: the run does not need
    /// repeating to re-derive its own statistic, and a number recovered by hand
    /// off a printed series is a number nobody can check again.
    #[test]
    fn the_recorded_day_long_run_is_green_through_a_measured_baseline() {
        let series = day_long();
        let drift = verdict(&series);

        assert_eq!(
            drift.baseline_rule,
            Baseline::Measured,
            "the recorded day is the run that made a measured baseline \
             necessary, and its baseline was placed by {:?} at {} samples in \
             rather than by a measured plateau",
            drift.baseline_rule,
            drift.warm
        );
        assert!(
            drift.from_floor >= DRIFT_BUDGET,
            "the recorded day reported {:.2}% from the {:.0}% floor, inside the \
             {:.0}% budget, so the breach #126 exists to explain is not in this \
             fixture and the assertion below proves nothing",
            drift.from_floor * 100.0,
            WARMUP_FRACTION * 100.0,
            DRIFT_BUDGET * 100.0
        );
        assert!(
            drift.ratio < DRIFT_BUDGET,
            "the recorded day reported {:.2}% through a baseline measured {} \
             samples in, over the {:.0}% budget, on a process whose own back half \
             was flat: quarters {:?}",
            drift.ratio * 100.0,
            drift.warm,
            DRIFT_BUDGET * 100.0,
            drift.quarters
        );
    }

    /// A rise that flattens is warmup; a step that never comes back is not.
    ///
    /// **The property the whole detector stands on**, and the one
    /// [`WARMUP_CEILING`] names the failure of: a baseline that walks forward
    /// until the series stops moving would launder any leak that arrives all at
    /// once and then holds, which is a monitor allocating something it never
    /// frees.
    ///
    /// It holds because a step is flat before it, so [`rose_from_the_start`]
    /// refuses to measure at all and the floor stands. The rolling median's own
    /// lag helps too and is not what carries it: the lag covers a step and fails
    /// for a rise wider than half the window. So this asserts the same figure
    /// the fraction reports rather than merely "over budget" — a step must be
    /// *unchanged* by the measured baseline, not just survive it.
    #[test]
    fn a_step_that_never_comes_back_is_not_warmup() {
        // **The rule each case exercises is named, because they are not all the
        // same one and a bare "over budget" hid that.** A step is flat before
        // it, so `rose_from_the_start` refuses to measure at all and the floor
        // stands: that is the clause under test here. The 200 case additionally
        // sits past the ceiling, and asserting only the ratio there let it
        // stand in as evidence about the rolling median's width when it is
        // evidence about the ceiling.
        for (at, after, rule) in [
            (72, 36.0, Baseline::RoseLate),
            (100, 36.0, Baseline::RoseLate),
            (144, 36.0, Baseline::RoseLate),
            (200, 36.0, Baseline::RoseLate),
            (100, 31.8, Baseline::RoseLate),
        ] {
            let series: Vec<u64> = (0..288)
                .map(|sample| if sample < at { mb(30.0) } else { mb(after) })
                .collect();
            let drift = verdict(&series);
            assert_eq!(
                (drift.warm, drift.baseline_rule),
                (warmup_floor(series.len()), rule),
                "a step to {after:.1} MiB at sample {at} had its baseline placed \
                 at {} by {:?}, where the run was flat before the step and has \
                 no warmup to measure",
                drift.warm,
                drift.baseline_rule
            );
            assert!(
                drift.ratio >= DRIFT_BUDGET,
                "a step to {after:.1} MiB at sample {at} reported {:.2}% drift, \
                 inside the {:.0}% budget, so the measured baseline walked past \
                 it and a leak that arrives at once is now called warmup",
                drift.ratio * 100.0,
                DRIFT_BUDGET * 100.0
            );
            assert_eq!(
                drift.ratio,
                drift.from_floor,
                "the same step reported {:.2}% through the measured baseline and \
                 {:.2}% through the floor: a step is supposed to be *unchanged* \
                 by where the warmup ends, and this one has been eroded",
                drift.ratio * 100.0,
                drift.from_floor * 100.0
            );
        }
    }

    /// A platform that never reported RSS gets no verdict, rather than a ratio
    /// against nothing.
    ///
    /// **Both ends, because guarding one alone leaves the other open.** `drift`
    /// refuses when either the gated baseline or the settled figure is zero.
    /// Only the baseline was guarded until a mutation showed the settled end
    /// mattered more: `ends_ratio` clamps a fall to zero, so a vanished series
    /// read as the flattest process ever measured. A ratio whose
    /// denominator is zero is an infinity that passes or fails by luck, and
    /// `SPEC.md` §7's rule is that a gate which cannot say "no drift" has not
    /// been tested.
    #[test]
    fn a_series_that_reported_no_memory_has_no_verdict() {
        for (shape, series) in [
            ("a platform that answered zero throughout", vec![0u64; 288]),
            (
                "a platform that answered zero until the last quarter",
                (0..288)
                    .map(|at| if at < 216 { 0 } else { mb(100.0) })
                    .collect::<Vec<u64>>(),
            ),
            // The one that passes rather than refusing, if only the baseline is
            // guarded: a fall is clamped to zero, so this reported 0.00% drift.
            (
                "a platform that stopped answering half way through",
                (0..288)
                    .map(|at| if at < 144 { mb(100.0) } else { 0 })
                    .collect::<Vec<u64>>(),
            ),
        ] {
            assert_eq!(
                drift(&sampled(&series)),
                None,
                "{shape} produced a verdict, and the ratio behind it divides by \
                 a baseline of zero"
            );
        }
    }

    /// A series that never settles keeps the floor, and therefore keeps the gate
    /// it had before any of this.
    ///
    /// **The safety argument for [`PLATEAU_BAND`] in one test.** The band's error
    /// direction is only benign if failing to find a plateau degrades to the
    /// fraction rather than to nothing, and a leak is the case where that
    /// matters: [`leaking`] never plateaus by construction, so it has to come
    /// out at the floor with the slicing the floor would have used.
    ///
    /// **The verdict itself belongs to [`a_linear_leak_is_caught`] and is
    /// deliberately not repeated here.** The two share [`leaking`], so asserting
    /// `ratio > DRIFT_BUDGET` in both would make the older test a strict subset
    /// of this one, and a gate that cannot fail while another passes is a gate
    /// nobody maintains and everybody counts. That is the same rule
    /// [`a_series_that_gave_memory_back_reports_a_negative_slope_where_the_ratio_reports_zero`]
    /// applies to its own fixture. One series, two disjoint claims: that one is
    /// the verdict, this one is where the baseline came from.
    #[test]
    fn a_series_that_never_settles_keeps_the_fraction_floor() {
        let leaking = leaking();
        let drift = verdict(&leaking);

        assert_eq!(
            (drift.warm, drift.baseline_rule),
            (warmup_floor(leaking.len()), Baseline::Unsettled),
            "a series climbing to the last sample reported a plateau, so the \
             detector believes a leak is a warmup"
        );
    }

    /// A rise the process made *after* it had settled is not warmup, however
    /// gradual it is.
    ///
    /// **The hole the plateau detector opens if it only looks at the end of the
    /// climb**, and the one the round-1 adversarial pass found: "flat, then a
    /// rise, then flat" has the same tail as "warmup, then flat", and a rise
    /// wider than half the rolling window is absorbed whole. `SPEC.md` §7's "a
    /// step is not absorbed" covered the instantaneous case only, and the width
    /// at which absorption begins is `quarter / 2`, which is a *twentieth* of a
    /// day-long run.
    ///
    /// Both repros are the shipped rule's own measurements before
    /// [`rose_from_the_start`] existed. The second is the worst *shape* found and
    /// it is a near miss rather than a breach: it reported 5.00% against a 5%
    /// budget where the whole window read 37.00%, so it fired by four
    /// ten-thousandths. The one that genuinely passed is
    /// [`a_page_of_noise_in_a_flat_head_does_not_buy_a_warmup`], at 4.77%.
    #[test]
    fn a_rise_after_the_process_settled_is_not_warmup() {
        for (flat_until, rise_over, to) in [(86, 86, 120.0), (60, 102, 137.0)] {
            let series: Vec<u64> = (0..288)
                .map(|at| {
                    if at < flat_until {
                        mb(100.0)
                    } else if at < flat_until + rise_over {
                        mb(100.0 + (to - 100.0) * (at - flat_until) as f64 / rise_over as f64)
                    } else {
                        mb(to)
                    }
                })
                .collect();
            let drift = verdict(&series);

            assert_eq!(
                (drift.warm, drift.baseline_rule),
                (warmup_floor(series.len()), Baseline::RoseLate),
                "a run flat for its first {flat_until} samples and then rising to \
                 {to:.0} MiB had its baseline measured at {}, so the rise was \
                 taken for a warmup",
                drift.warm
            );
            assert!(
                drift.ratio >= DRIFT_BUDGET,
                "the same run reported {:.2}% against the {:.0}% budget, so a \
                 process that took {:.0}% more memory and kept it ships",
                drift.ratio * 100.0,
                DRIFT_BUDGET * 100.0,
                to - 100.0
            );
        }
    }

    /// A page of drift in the flat head does not make the rest of a leak a
    /// warmup.
    ///
    /// **The attack that defeated a bare "did it rise at all" test**, and the
    /// reason [`RISE_SHARE`] measures a share rather than a direction. Prepend a
    /// single 4 KiB step to the flat head of the leak that
    /// [`a_rise_after_the_process_settled_is_not_warmup`] rejects, and the whole
    /// prefix reads as a climb: the baseline moved to sample 83 and a **29.5%**
    /// leak reported **4.77%** against a 5% budget, which is worse than the rule
    /// this one replaced. One page in 288 samples is the smallest change a
    /// series can express.
    ///
    /// Both spellings run, so the fixture cannot quietly stop being an attack:
    /// without the step it must be refused, with it it must still be refused,
    /// and the two must report the same figure.
    #[test]
    fn a_page_of_noise_in_a_flat_head_does_not_buy_a_warmup() {
        for step in [0, 4096] {
            let series: Vec<u64> = (0..288)
                .map(|at| match at {
                    at if at < 36 => mb(100.0),
                    at if at < 60 => mb(100.0) + step,
                    at if at < 120 => {
                        mb(100.0) + step + (29.5 * MIB) as u64 * (at - 60) as u64 / 60
                    }
                    _ => mb(129.5) + step,
                })
                .collect();
            let drift = verdict(&series);

            assert_eq!(
                (drift.warm, drift.baseline_rule),
                (warmup_floor(series.len()), Baseline::RoseLate),
                "a {step}-byte step in the flat head moved the baseline to \
                 sample {} by {:?}, so a leak bought itself a warmup with the \
                 smallest change a series can express",
                drift.warm,
                drift.baseline_rule
            );
            assert!(
                drift.ratio >= DRIFT_BUDGET,
                "the same 29.5% leak reported {:.2}% with a {step}-byte step in \
                 its head, inside the {:.0}% budget",
                drift.ratio * 100.0,
                DRIFT_BUDGET * 100.0
            );
        }
    }

    /// The recorded day clears the ceiling by more than its own jitter.
    ///
    /// **A margin asserted as a number, because a margin nobody measured is how
    /// this gate would fail on the weather.** The day's baseline lands at 97 and
    /// the ceiling refuses past 136, so the clearance is 39 samples. At the 50%
    /// ceiling this rule first shipped with it was **eleven**, and an audit round
    /// found that adding less noise than the series' own sample-to-sample jitter
    /// crossed it on 93 of 300 seeds: the flagship fixture was a third of a coin
    /// flip away from a red build, and nothing said so.
    ///
    /// Deterministic noise rather than a random source, for the reason
    /// [`workload`] gives about its own counter: a failing seed has to be
    /// repeatable. The amplitude is half a mebibyte, against a series whose
    /// neighbouring samples differ by more than two.
    #[test]
    fn the_recorded_day_clears_the_ceiling_under_its_own_jitter() {
        let day = day_long();
        let quarter = day.len() / 4;
        let ceiling = (day.len() as f64 * WARMUP_CEILING) as usize;

        for seed in 0..64u64 {
            // A cheap deterministic hash, so every seed shakes the series
            // differently and any failure names the seed that produced it.
            let noisy: Vec<u64> = day
                .iter()
                .enumerate()
                .map(|(at, &rss)| {
                    let mixed = (at as u64)
                        .wrapping_mul(6_364_136_223_846_793_005)
                        .wrapping_add(seed);
                    let swing = ((mixed >> 33) % (MIB as u64)) as i64 - (MIB as i64 / 2);
                    rss.saturating_add_signed(swing)
                })
                .collect();
            let (at, rule) = warmup(&noisy);
            assert_eq!(
                rule,
                Baseline::Measured,
                "seed {seed} shook the recorded day by half a mebibyte and its \
                 baseline was placed by {rule:?} at {at} rather than measured, \
                 so the flagship fixture sits on a cliff edge"
            );
            assert!(
                at + quarter / 2 <= ceiling,
                "seed {seed} put the day's baseline at {at}, and with the \
                 rolling median's lag that is {} against a ceiling of {ceiling}",
                at + quarter / 2
            );
        }

        let clean = warmup(&day).0;
        assert!(
            ceiling - (clean + quarter / 2) >= quarter / 2,
            "the recorded day clears the ceiling by {} samples, under the half \
             window of lag the walk itself carries: a margin thinner than the \
             instrument's own resolution is not a margin",
            ceiling - (clean + quarter / 2)
        );
    }

    /// A measured baseline never reports more drift than the whole window.
    ///
    /// **The invariant that makes moving the baseline safe in the other
    /// direction.** Dropping a warmup climb raises the first quarter's median
    /// and lowers the ratio, so a baseline that raises it did not drop a climb:
    /// it landed in a trough. Before [`Baseline::Trough`] existed, a process
    /// flat at 100 MiB with a dip to 95 across samples 104 to 140 reported
    /// **5.26%** and failed, on a series whose first and last samples are the
    /// same number.
    ///
    /// Swept rather than sampled, over every dip depth, width and position that
    /// fits, because the shape that violated it was not one anybody guessed.
    ///
    /// **It asserts the ordering and not the budget**, deliberately. A dip
    /// landing inside the baseline's *own* first quarter lowers `quarters[0]`
    /// under either rule, so some of these shapes breach 5% through the fraction
    /// too. That is the fraction rule's behaviour and it predates
    /// [#126](https://github.com/breferrari/vigia/issues/126); asserting a budget
    /// here would be this branch taking responsibility for it, and would fail
    /// for a reason that has nothing to do with a measured baseline. The claim
    /// this branch owes is the one below: measuring never makes it *worse*.
    #[test]
    fn a_measured_baseline_never_reports_more_drift_than_the_whole_window() {
        let mut dipped = 0;
        for warmed in [false, true] {
            for depth in [99.0, 97.0, 95.0, 90.0] {
                for width in [12, 24, 36, 48, 72] {
                    for start in (0..288 - width).step_by(24) {
                        let series: Vec<u64> = (0..288)
                            .map(|at| {
                                // Optionally on top of a genuine warmup, so the
                                // sweep also covers series that reach the measured
                                // baseline at all rather than only ones
                                // `rose_from_the_start` turns away.
                                let base = if warmed && at < 40 {
                                    80.0 + 20.0 * at as f64 / 40.0
                                } else {
                                    100.0
                                };
                                if (start..start + width).contains(&at) {
                                    mb(base.min(depth))
                                } else {
                                    mb(base)
                                }
                            })
                            .collect();
                        let drift = verdict(&series);
                        assert!(
                            drift.ratio <= drift.from_floor,
                            "a flat run dipping to {depth} MiB over samples \
                         {start}..{} reported {:.2}% through a baseline measured \
                         {} samples in, against {:.2}% over the whole window: \
                         moving the baseline made the drift worse, which is a \
                         baseline in a trough",
                            start + width,
                            drift.ratio * 100.0,
                            drift.warm,
                            drift.from_floor * 100.0
                        );
                        dipped += usize::from(drift.baseline_rule == Baseline::Trough);
                    }
                }
            }
        }
        assert!(
            dipped > 0,
            "no dip in the sweep reached `Baseline::Trough`, so the clamp in \
             `drift` is unreachable and every assertion above holds for a reason \
             that is not the code under test"
        );
    }

    /// The warmup may never take more of the run than [`WARMUP_CEILING`].
    ///
    /// A plateau that only arrives late is not evidence the climb before it was
    /// warmup; it is a window shorter than its own warmup, which `SPEC.md` §7
    /// says is measuring warmup. So the ceiling refuses it and the whole climb
    /// is measured, which is the loud outcome rather than the quiet one.
    ///
    /// **Swept rather than sampled, and the sweep is what caught the ceiling
    /// bounding a different number from the one it documents.** A single 70%
    /// fixture passed while a climb ending anywhere from 51% to 63% was still
    /// `Measured`: the rolling median lags the true settle point by half its own
    /// width, so `at` names a position 12.5% of the run earlier than where the
    /// series flattened, and comparing `at` alone bought 63% under a constant
    /// that then said 50%. `warmup` adds the lag back.
    ///
    /// **Both sides of the boundary, at the boundary**, because a sweep that
    /// starts well past it is passed by a rule that refuses everything, and one
    /// that stops well before it is passed by a rule that believes everything.
    /// The pair below sits either side of [`WARMUP_CEILING`] exactly.
    #[test]
    fn a_plateau_that_arrives_past_the_ceiling_is_not_believed() {
        let climbing_until = |pct: usize| -> Vec<u64> {
            let late = 288 * pct / 100;
            (0..288)
                .map(|at| {
                    if at < late {
                        mb(20.0 + 20.0 * (at as f64 / late as f64))
                    } else {
                        mb(40.0)
                    }
                })
                .collect()
        };

        let ceiling_pct = (WARMUP_CEILING * 100.0) as usize;
        for pct in (ceiling_pct + 1)..=(ceiling_pct + 15) {
            let series = climbing_until(pct);
            let drift = verdict(&series);
            assert_eq!(
                (drift.warm, drift.baseline_rule),
                (warmup_floor(series.len()), Baseline::Unsettled),
                "a series still climbing at {pct}% of the window had its \
                 baseline measured at sample {}, past the {:.0}% ceiling",
                drift.warm,
                WARMUP_CEILING * 100.0
            );
            assert!(
                drift.ratio > DRIFT_BUDGET,
                "the same series reported {:.2}% drift at {pct}%, inside the \
                 budget, so a process that spends most of a run climbing passes",
                drift.ratio * 100.0
            );
        }

        // And the sample immediately under it, or "refuses everything" would
        // satisfy every line above. At the boundary rather than comfortably
        // inside it: a fixture at 40% is passed by any ceiling from 41 upwards
        // and so pins nothing about where this one is.
        let drift = verdict(&climbing_until(ceiling_pct));
        assert_eq!(
            drift.baseline_rule,
            Baseline::Measured,
            "a climb finishing at exactly the {ceiling_pct}% ceiling was refused \
             too, so the ceiling is not a boundary and every assertion above \
             would hold against a rule that never measures anything"
        );
    }

    /// A series already at its level by the floor keeps the floor, rather than
    /// having a baseline invented for it.
    ///
    /// The common case, and the one every per-commit run takes: nothing to
    /// measure, so nothing moves. Asserted because the alternative failure is
    /// silent — a detector that crept forward on a flat series would narrow
    /// every short window's evidence and no existing test would notice, since
    /// they all assert on the ratio and a flat series has none.
    #[test]
    fn a_series_flat_from_the_floor_takes_the_floor() {
        for (shape, series) in [
            ("a flat series", flat(288)),
            ("a climb that stops inside the floor", plateaus_early()),
        ] {
            let (warm, rule) = warmup(&series);
            assert_eq!(
                (warm, rule),
                (warmup_floor(series.len()), Baseline::Floor),
                "{shape} had its baseline placed at sample {warm} by {rule:?}, \
                 where nothing about it needed measuring"
            );
        }
    }

    /// A measured baseline never turns a verdict into a refusal.
    ///
    /// **This is the one that bit during design.** [`WARMUP_CEILING`] bounds the
    /// warmup at half a long run and still leaves a short one able to detect a
    /// plateau that takes the quarters below [`MIN_END`] — nine samples is such
    /// a length — at which point [`drift`] answers `None` where the fraction
    /// answers a number. A gate that goes quiet is worse than one that is absent
    /// for the same reason `SPEC.md` §7 gives about four samples: it looks like
    /// coverage. [`drift`]'s retry is what holds it, and this also asserts the
    /// retry **fires**: a [`Baseline::TooShort`] nothing ever produces is a
    /// branch that cannot be wrong and a variant that is only a name.
    ///
    /// Driven over every length the sampler can produce a verdict near, and over
    /// prefixes of the real day, because a synthetic series is exactly the thing
    /// that would be built to have a plateau in the right place.
    ///
    /// **A climbing shape is here because the two tame ones missed it.** A flat
    /// series and the day's own opening both settle by the floor at these
    /// lengths, so the clamp is never reached and deleting it left this test
    /// green while the real fifteen-second soak went red: 12 samples climbing
    /// from 20.9 to 28 MiB is what the per-commit run actually produces, and
    /// that is the shape that detects late enough to lose its ends. A pure
    /// function whose only failing witness is a fifteen-second integration run
    /// is the case `SPEC.md` §7 says the pure test has to cover.
    #[test]
    fn a_measured_warmup_never_turns_a_verdict_into_a_refusal() {
        let day = day_long();
        let agree = |shape: &str, series: &[u64]| {
            let floored = quarter_medians(&series[warmup_floor(series.len())..]);
            assert_eq!(
                drift(&sampled(series)).is_some(),
                floored.is_some_and(|quarters| quarters[0] != 0),
                "over {} samples of {shape}, a measured baseline and the floor \
                 disagree about whether there is a verdict at all",
                series.len()
            );
        };

        for len in (0..=64).chain([96, 144, 192, 288]) {
            agree("a flat series", &flat(len));
            agree("the recorded day", &day[..len.min(day.len())]);
        }

        // **And every place the plateau could land, at every short length.**
        // Guessing a shape does not work here: the clamp is only reachable in a
        // narrow band, where the detection sits at or under half the run *and*
        // leaves under `4 * MIN_END` samples behind it, which needs a run under
        // sixteen samples whose plateau starts around 40% in. A hand-picked
        // climbing series missed it, and the fifteen-second integration soak
        // caught it **once and then did not**, because its series is whatever
        // the machine produced that minute. So sweep the space instead of
        // sampling it: 33 lengths by every plateau position is a second of
        // arithmetic and does not depend on anyone's intuition about where the
        // edge is.
        let climb_to = |len: usize, plateau: usize| -> Vec<u64> {
            (0..len)
                .map(|at| mb(20.0 + 8.0 * at.min(plateau) as f64 / plateau.max(1) as f64))
                .collect()
        };
        let mut retried = 0;
        for len in 8..=40 {
            for plateau in 0..len {
                let series = climb_to(len, plateau);
                agree(
                    &format!("a climb plateauing at {plateau} of {len}"),
                    &series,
                );
                retried += usize::from(
                    drift(&sampled(&series))
                        .is_some_and(|drift| drift.baseline_rule == Baseline::TooShort),
                );
            }
        }

        // The sweep above passes trivially if the retry never runs, because
        // every series then takes a branch that existed before it did.
        assert!(
            retried > 0,
            "no series in the sweep reached `Baseline::TooShort`, so the retry \
             in `drift` is unreachable and the property above holds for a reason \
             that is not the code under test"
        );
    }

    /// What the measured baseline absorbed is reported, on every run.
    ///
    /// [`Drift::from_floor`] is the answer to the objection [`WARMUP_CEILING`]
    /// records: a detector that adapts to a slow leak would hide it. It can only
    /// be that answer if it is the *same statistic* over the *same series* from
    /// the fraction, so this re-derives it rather than trusting the field, over
    /// the one fixture here where the two genuinely differ.
    #[test]
    fn the_report_carries_what_the_measured_baseline_absorbed() {
        let series = day_long();
        let drift = verdict(&series);
        let floor = quarter_medians(&series[warmup_floor(series.len())..])
            .expect("288 samples has two ends at the floor");

        assert_eq!(
            drift.from_floor,
            ends_ratio(floor),
            "the reported whole-window figure is not the ends-ratio taken from \
             the floor, so the number a reader compares the gated one against \
             describes some third window"
        );
        assert!(
            drift.from_floor > drift.ratio,
            "on the recorded day the floor's figure ({:.2}%) is not above the \
             gated one ({:.2}%), so this fixture no longer shows the difference \
             the line exists to print",
            drift.from_floor * 100.0,
            drift.ratio * 100.0
        );
    }
}
