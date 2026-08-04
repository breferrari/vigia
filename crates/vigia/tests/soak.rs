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
/// measuring warmup. Ten minutes leaves a minute of warmup and about four
/// minutes at each end of the comparison.
const GATED_WINDOW: Duration = Duration::from_secs(600);

/// File descriptors a sample may exceed the baseline by.
///
/// Generous because a *leak* is per-frame and reaches thousands within seconds,
/// while a transient open is a handful: the gap between the two failure modes
/// is wide enough that the threshold does not have to be precise. Linux only,
/// where `/proc/self/fd` makes it free; elsewhere it reports unavailable rather
/// than passing quietly.
const FD_HEADROOM: usize = 16;

/// Fraction of the samples discarded before the baseline is taken.
///
/// Every process climbs to an allocator plateau before it is flat, and that
/// climb is not a leak. Measuring from the first sample would read it as one,
/// and the threshold needed to tolerate it would be wide enough to wave a real
/// leak through.
const WARMUP_FRACTION: f64 = 0.10;

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
/// `{:.2}h` alone renders every window under thirty-six seconds as `0.00h`,
/// which is the whole band [`Drift::span`] exists to explain: `+903 MiB/h over
/// 0.00h` reads as a division by zero rather than as a very short lever arm.
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
    /// post-warmup samples is the case a 288-sample run actually produces — and
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
    /// Measured on the reference machine, same code, same statistic, five
    /// windows: 15s reports **+364.71 MiB/h**, 600s **+7.89**, 900s **−0.70**
    /// and **+1.94**, 3600s **+0.92**. Nothing is wrong with any of those
    /// numbers; the 15-second one is a fifth of a minute of lever arm and says
    /// so once this is printed beside it.
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
    let warm = (samples.len() as f64 * WARMUP_FRACTION).ceil() as usize;
    let rest = samples.get(warm..)?;

    // A quarter at each end, so the two never overlap however short the series
    // is and the middle half is free to wander without deciding anything.
    let quarter = rest.len() / 4;
    if quarter < MIN_END {
        return None;
    }

    // Projected once, so the four windows are slices of one buffer rather than
    // four throwaway copies, and so `median` keeps the `&[u64]` signature its
    // own tests call it through.
    let values: Vec<u64> = rest.iter().map(|&(_, rss)| rss).collect();
    let end = values.len();
    // Inward from each end rather than `end * k / 4`: see [`Drift::quarters`].
    // The two never overlap, because `quarter` is a floor of a quarter, so
    // `4 * quarter <= end` and therefore `2 * quarter <= end - 2 * quarter`.
    let quarters = [
        median(&values[..quarter])?,
        median(&values[quarter..quarter * 2])?,
        median(&values[end - quarter * 2..end - quarter])?,
        median(&values[end - quarter..])?,
    ];
    // A baseline of zero means the platform did not report RSS at all, and a
    // ratio against it would be an infinity that passes or fails by luck.
    if quarters[0] == 0 {
        return None;
    }

    Some(Drift {
        quarters,
        ratio: quarters[3].saturating_sub(quarters[0]) as f64 / quarters[0] as f64,
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

        let warm = ((counts.len() as f64 * WARMUP_FRACTION).ceil() as usize).max(1);
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
        assert!(
            drift.ratio < DRIFT_BUDGET,
            "I3: RSS drifted {:.2}% over {:?}, over the {:.0}% budget: quarters \
             {:.2}, {:.2}, {:.2}, {:.2} MiB at {:+.2} MiB/h, peak {:.2} MiB over \
             {} frames",
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
    for expected in ["soak: rss quarters ", " MiB/h over ", "soak: rss baseline "] {
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
    for (job, block) in workflow_jobs(&source) {
        let soaks = block.contains("needs: plan");
        let timeouts = workflow_settings(&block, "timeout-minutes:");
        assert_eq!(
            timeouts.len(),
            1,
            "job `{job}` in {} declares {} timeouts; one job, one timeout, or \
             the one that fires is whichever the runner read first",
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
            assert!(
                workflow_settings(&block, "VIGIA_SOAK_SECS:")
                    .iter()
                    .any(|window| window.starts_with("${{")
                        && window.contains("needs.plan.outputs.seconds")),
                "job `{job}` reads the planned timeout but not the planned \
                 window, so the two describe different numbers again"
            );
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
/// The scratch directory is created **after** the spawn succeeds. Creating it
/// first leaked one per run on any machine without `bash`, in the one file
/// whose headline invariant is that nothing is retained.
fn run_plan(script: &str, seconds: &str, runner: &str) -> Option<Planned> {
    let scratch = std::env::temp_dir().join(format!(
        "vigia-plan-{}-{seconds}-{}",
        std::process::id(),
        runner.len()
    ));
    let out = scratch.join("output");

    let mut command = Command::new("bash");
    command
        .arg("-c")
        .arg(script)
        .env("SOAK_SECONDS", seconds)
        .env("SOAK_RUNNER", runner)
        .env("SOAK_DAILY", "false")
        .env("GITHUB_OUTPUT", &out);

    // Probe for `bash` before building anything for it to write into.
    if Command::new("bash")
        .arg("-c")
        .arg("exit 0")
        .output()
        .is_err()
    {
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
        "note: {what} is not gated here, because `bash` could not be run. It is \
         gated on every tier-1 target that has one, and on CI."
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
    for hostile in ["ubuntu-latest\nx=y", "ubuntu\"], [\"x", "a b", ""] {
        if hostile.is_empty() {
            continue;
        }
        let Some(bad) = run_plan(&script, "600", hostile) else {
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

/// The plan job's arithmetic, run rather than read.
///
/// **The sibling gate above checks the plumbing and cannot see this**, which is
/// the whole reason there are two: with only the plumbing asserted, writing
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
    #[test]
    fn a_linear_leak_is_caught() {
        let series: Vec<u64> = (0..288)
            .map(|at| mb(100.0 * (1.0 + 0.0005 * at as f64)))
            .collect();
        let drift = verdict(&series);
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
    /// no warmup at all, the same series drifts by 140%.
    ///
    /// **It is also the only fixture here that can tell where the gradient was
    /// fitted**, and it did not always assert on one. Every other series in
    /// this module is linear from end to end, so fitting through the warmup
    /// and fitting past it give the same answer and a mutation swapping one
    /// for the other survived them all. This one is a step followed by a
    /// plateau: past the warmup it is flat, through it, steeply positive.
    #[test]
    fn growth_that_stops_inside_the_warmup_is_not_drift() {
        let climb = 288 * 8 / 100;
        let series: Vec<u64> = (0..288)
            .map(|at| {
                if at < climb {
                    mb(50.0 + 70.0 * (at as f64 / climb as f64))
                } else {
                    mb(120.0)
                }
            })
            .collect();

        let drift = verdict(&series);
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
    #[test]
    fn the_gated_ends_are_the_first_and_last_quarter_medians() {
        // Strictly increasing by a page, so no two samples share a value.
        let ramp: Vec<u64> = (0..288).map(|at| mb(60.0) + at as u64 * 4096).collect();
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

            let warm = (series.len() as f64 * WARMUP_FRACTION).ceil() as usize;
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
    /// **The spikes here are carried for symmetry, not because they are
    /// load-bearing**, and the difference is worth stating in the one module
    /// whose docs exist to record which fixtures carry weight. The ends need an
    /// outlier because both of their edges move together: widening the last
    /// quarter from 64 samples to 65 prepends a value *and* advances the
    /// nearest rank, and on sorted data those cancel exactly. Each middle
    /// window moves at only **one** edge, so there is nothing to cancel and a
    /// plain ramp already separates the two slicings.
    #[test]
    fn the_middle_quarters_are_measured_inward_from_the_ends() {
        let mut series: Vec<u64> = (0..288).map(|at| mb(60.0) + at as u64 * 4096).collect();
        let warm = (series.len() as f64 * WARMUP_FRACTION).ceil() as usize;
        // **The same guard the ends test carries, and for a sharper reason.**
        // Where the post-warmup length divides by four, the inward cut and the
        // `end * k / 4` cut are the *same four windows*, so this test would
        // pass against the slicing it exists to reject. A change to
        // `WARMUP_FRACTION` or to the fixture length is all it would take.
        assert_eq!(
            (series.len() - warm) % 4,
            3,
            "this fixture leaves {} post-warmup samples, which divides by four, \
             and the two slicings this test tells apart are identical there",
            series.len() - warm
        );
        // Derived rather than written as 29: the spikes have to land on the
        // boundaries the slicings disagree about, and those move with the
        // warmup. `quarter * 2` is the sample `[q..2q)` excludes and
        // `[end/4..end/2)` includes; the other is just below the third
        // quarter's left edge.
        let quarter = (series.len() - warm) / 4;
        let end = series.len() - warm;
        series[warm + quarter * 2] = mb(500.0);
        series[warm + end - quarter * 2 - 1] = mb(900.0);

        let drift = verdict(&series);
        let rest = &series[warm..];

        assert_eq!(
            drift.quarters[1],
            median(&rest[quarter..quarter * 2]).expect("a quarter of the samples"),
            "the second quarter is not measured a quarter's width in from the \
             start, so the four are being cut at `end * k / 4` and the rule \
             `Drift::quarters` documents is not the rule running"
        );
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

    /// A gradient carries the span it was divided by.
    ///
    /// See [`Drift::span`]. Without it the four figures `SPEC.md` §10 asks a
    /// reader to compare are not comparable, because the same wander reads
    /// +364 MiB/h over fifteen seconds and +0.92 over an hour.
    #[test]
    fn the_gradient_is_reported_with_the_span_it_was_fitted_over() {
        let drift = verdict(&flat(288));
        let rest = 288 - (288.0 * WARMUP_FRACTION).ceil() as usize;
        assert_eq!(
            drift.span,
            SAMPLE * (rest as u32 - 1),
            "the span is not the post-warmup elapsed time, so it describes a \
             different window from the gradient it is printed beside"
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
}
