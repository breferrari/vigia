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

/// What a series of RSS samples did after it warmed up.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Drift {
    /// Median of the first quarter after the warmup.
    baseline: u64,
    /// Median of the last quarter.
    settled: u64,
    /// `(settled - baseline) / baseline`, and never negative: a process that
    /// gave memory back has not drifted, and reporting that as a signed number
    /// invites a threshold that passes for the wrong reason.
    ratio: f64,
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
fn drift(rss: &[u64]) -> Option<Drift> {
    let warm = (rss.len() as f64 * WARMUP_FRACTION).ceil() as usize;
    let rest = rss.get(warm..)?;

    // A quarter at each end, so the two never overlap however short the series
    // is and the middle half is free to wander without deciding anything.
    let quarter = rest.len() / 4;
    if quarter < MIN_END {
        return None;
    }

    let baseline = median(&rest[..quarter])?;
    let settled = median(&rest[rest.len() - quarter..])?;
    // A baseline of zero means the platform did not report RSS at all, and a
    // ratio against it would be an infinity that passes or fails by luck.
    if baseline == 0 {
        return None;
    }

    Some(Drift {
        baseline,
        settled,
        ratio: settled.saturating_sub(baseline) as f64 / baseline as f64,
    })
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

    fn rss(&self) -> Vec<u64> {
        self.samples.iter().map(|s| s.rss).collect()
    }

    fn print(&self) {
        let mb = |bytes: u64| bytes as f64 / (1024.0 * 1024.0);
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
        match drift(&self.rss()) {
            Some(drift) => println!(
                "soak: rss baseline {:.1} MiB, settled {:.1} MiB, drift {:.2}% \
                 (budget {:.0}%), min {:.1}, max {:.1}",
                mb(drift.baseline),
                mb(drift.settled),
                drift.ratio * 100.0,
                DRIFT_BUDGET * 100.0,
                mb(self.samples.iter().map(|s| s.rss).min().unwrap_or(0)),
                mb(self.samples.iter().map(|s| s.rss).max().unwrap_or(0)),
            ),
            None => println!(
                "soak: rss has no verdict from {} samples",
                self.samples.len()
            ),
        }
        println!(
            "soak: tracked diffs max {} of {} files max; tracked hunks max {} of \
             body {}, closest to its screen's own bound {:?}; paths ever changed {}",
            self.max_tracked_diffs(),
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
        let series: Vec<String> = self
            .samples
            .iter()
            .map(|s| (s.rss / 1024).to_string())
            .collect();
        println!("soak: rss KiB = {}", series.join(","));
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

        // The three retained caches, each against the thing that is supposed to
        // bound it. `Frame` is bounded by the current diff; `Highlighter` is
        // bounded by the screen, which is the stronger claim; `History` is
        // bounded by a fixed cap, which is I10 and is the only one of the three
        // that has to keep holding a path *after* it has left the diff.
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
        let series = self.rss();
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

        let drift = drift(&series).unwrap_or_else(|| {
            panic!(
                "I3: {} samples over {:?} produced no verdict, so the window was \
                 gated and measured nothing",
                series.len(),
                self.window
            )
        });
        let mb = |bytes: u64| bytes as f64 / (1024.0 * 1024.0);
        assert!(
            drift.ratio < DRIFT_BUDGET,
            "I3: RSS drifted {:.2}% over {:?}, over the {:.0}% budget: \
             {:.1} MiB after warmup against {:.1} MiB at the end, peak {:.1} MiB \
             over {} frames",
            drift.ratio * 100.0,
            self.window,
            DRIFT_BUDGET * 100.0,
            mb(drift.baseline),
            mb(drift.settled),
            mb(series.iter().copied().max().unwrap_or(0)),
            self.frames
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

mod statistic {
    //! The verdict, tested as the pure function it is.

    use super::*;

    /// Megabytes, because RSS is quoted in them everywhere else here.
    fn mb(value: f64) -> u64 {
        (value * 1024.0 * 1024.0) as u64
    }

    /// A process that is flat, jittering by a page either way.
    fn flat(len: usize) -> Vec<u64> {
        (0..len)
            .map(|at| mb(100.0) + (at as u64 % 3) * 4096)
            .collect()
    }

    #[test]
    fn a_flat_series_does_not_drift() {
        let series = flat(288);
        let drift = drift(&series).expect("288 samples is enough to have a verdict");
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
        let drift = drift(&series).expect("288 samples is enough to have a verdict");
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

        let drift = drift(&series).expect("288 samples is enough to have a verdict");
        assert!(
            drift.ratio < DRIFT_BUDGET,
            "a process that reached its plateau in the first 8% of the window \
             reported {:.2}% drift, so every soak fails for a reason that is not \
             a leak",
            drift.ratio * 100.0
        );
    }

    /// A shrinking process has not drifted, and the ratio has to say so without
    /// going negative: a signed number under a `<` threshold passes for the
    /// wrong reason.
    #[test]
    fn a_series_that_gave_memory_back_reports_no_drift_rather_than_a_negative_one() {
        let series: Vec<u64> = (0..288).map(|at| mb(200.0 - 0.2 * at as f64)).collect();
        let drift = drift(&series).expect("288 samples is enough to have a verdict");
        assert_eq!(
            drift.ratio, 0.0,
            "a shrinking series reported {:?}, which is not a drift",
            drift
        );
    }

    /// Too short to have two ends is not a pass. A gate that answers "fine" from
    /// four samples is worse than one that is absent, because it looks like
    /// coverage.
    #[test]
    fn a_series_too_short_to_have_two_ends_reports_nothing() {
        for len in 0..=(MIN_END * 4) {
            let series = flat(len);
            assert_eq!(
                drift(&series),
                None,
                "{len} samples produced a verdict, and each end of it holds fewer \
                 than {MIN_END} samples"
            );
        }
        assert!(
            drift(&flat(MIN_END * 4 + 1)).is_some(),
            "the shortest series that does have two ends of {MIN_END} was refused, \
             so the guard rejects series it should answer"
        );
    }
}
