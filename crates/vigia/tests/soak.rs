//! I3, gated over a soak.

#[path = "../../vigia-core/tests/support/mod.rs"]
mod support;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use vigia::{
    Action, App, Body, Glyphs, Pointing, Row, Theme, View, body_layout, diff_height, render,
};
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
const DEFAULT_SECS: u64 = 15;

/// Fixture the default window can actually exercise.
const DEFAULT_FILES: usize = 20;
const DEFAULT_LINES: usize = 200;

/// Samples across the window, whatever its length.
const MAX_SAMPLES: usize = 288;
const MIN_SAMPLES: usize = 12;

/// Below this window the drift gate reports its numbers and does not assert.
const GATED_WINDOW: Duration = Duration::from_secs(600);

/// File descriptors a sample may exceed the baseline by.
const FD_HEADROOM: usize = 16;

/// Samples discarded before the baseline the gate compares from, as a fraction
/// of the run.
const WARMUP_FRACTION: f64 = 0.10;

/// Samples [`Report::gate_descriptors`] discards before its baseline.
const FD_WARMUP_FRACTION: f64 = 0.10;

/// How close a rolling quarter median sits to the settled level before the
/// series counts as plateaued.
const PLATEAU_BAND: f64 = 0.01;

/// I3's budget: RSS drift over the window.
const DRIFT_BUDGET: f64 = 0.05;

/// Samples each end of the comparison needs before there is a verdict at all.
const MIN_END: usize = 2;

/// Bytes in a mebibyte, and seconds in an hour.
const MIB: f64 = 1024.0 * 1024.0;
const SECS_PER_HOUR: f64 = 3600.0;

/// A byte count as the MiB every number in the report is quoted in.
fn mib(bytes: u64) -> f64 {
    bytes as f64 / MIB
}

/// An elapsed time, in whichever unit puts a significant figure on the page.
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
    quarters: [u64; 4],
    /// `(settled - baseline) / baseline`, and never negative: a process that
    /// gave memory back has not drifted, and reporting that as a signed number
    /// invites a threshold that passes for the wrong reason.
    ratio: f64,
    /// Samples discarded before [`Drift::baseline`], which is always
    /// [`WARMUP_FRACTION`]'s prefix: see [`settled_at`].
    warm: usize,
    /// Where the series settled, in samples, or `None` for a run that never
    /// came inside the band. **Reported, never gated**: [`settled_at`].
    settled: Option<usize>,
    /// The same position as an elapsed time.
    settled_at: Option<Duration>,
    /// The same ends-ratio taken from [`Drift::settled`] instead of the floor.
    /// **Reported, never gated.**
    settled_ratio: Option<f64>,
    /// Least-squares gradient over the post-warmup series, in MiB per hour.
    slope: f64,
    /// Elapsed time the gradient was fitted across, which is its lever arm.
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
fn ends_ratio(quarters: [u64; 4]) -> f64 {
    quarters[3].saturating_sub(quarters[0]) as f64 / quarters[0] as f64
}

/// The least a run may discard before its baseline: see [`WARMUP_FRACTION`].
fn warmup_floor(len: usize) -> usize {
    (len as f64 * WARMUP_FRACTION).ceil() as usize
}

/// Where the series settled, as a **reported** diagnostic and never as a gate.
fn settled_at(values: &[u64]) -> Option<usize> {
    let quarter = values.len() / 4;
    let level = quarter_medians(values).map(|whole| whole[3])?;
    if level == 0 {
        return None;
    }

    let settled = |at: usize| {
        median(&values[at..at + quarter])
            .is_some_and(|m| m.abs_diff(level) as f64 / (level as f64) < PLATEAU_BAND)
    };
    let last = values.len() - quarter;
    let mut at = last;
    while at > 0 && settled(at - 1) {
        at -= 1;
    }
    // Nothing before the final window was ever in band, so there is no plateau
    // to report rather than one that happens to sit at the end.
    (at < last).then_some(at)
}

/// What `rss` drifted by, or `None` when the series cannot answer.
fn drift(samples: &[(Duration, u64)]) -> Option<Drift> {
    // Projected once, so every window below is a slice of one buffer rather
    // than a throwaway copy, and so `median` keeps the `&[u64]` signature its
    // own tests call it through.
    let values: Vec<u64> = samples.iter().map(|&(_, rss)| rss).collect();

    // **The gate's baseline, and the only one it has.** See [`settled_at`] for
    // why the measured plateau below is printed and never substituted here.
    let warm = warmup_floor(values.len());
    let rest = samples.get(warm..)?;
    let quarters = quarter_medians(&values[warm..])?;

    // A zero at either end means the platform stopped reporting RSS, and
    // neither end may be one. A zero *baseline* makes the ratio an infinity
    // that passes or fails by luck. A zero *settled* figure is worse, because
    // it passes quietly: [`ends_ratio`] clamps a fall to zero, so a run whose
    // reads vanish half way through reported 0.00% drift and looked like the
    // flattest process ever measured. Only the baseline was guarded until a
    // mutation went looking for the other end.
    if quarters[0] == 0 || quarters[3] == 0 {
        return None;
    }

    // The plateau, reported beside the verdict and never inside it. Its own
    // drift is taken through the same cut the gate uses, so the two figures a
    // reader compares are the same statistic over two baselines rather than two
    // statistics.
    let settled = settled_at(&values);
    let settled_ratio = settled
        .and_then(|at| quarter_medians(&values[at..]))
        .filter(|ends| ends[0] != 0)
        .map(ends_ratio);

    Some(Drift {
        quarters,
        ratio: ends_ratio(quarters),
        warm,
        settled,
        settled_at: settled.and_then(|at| samples.get(at).map(|&(at, _)| at)),
        settled_ratio,
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
fn rss_bytes() -> Option<u64> {
    vigia::memory::resident()
}

/// Descriptors this process holds open, where that is free to ask.
#[cfg(target_os = "linux")]
fn open_files() -> Option<usize> {
    Some(std::fs::read_dir("/proc/self/fd").ok()?.count())
}

#[cfg(not(target_os = "linux"))]
fn open_files() -> Option<usize> {
    None
}

/// How many samples a window of this length gets.
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
    closest_hunk_bound: Option<(usize, usize)>,
    frame: FrameStats,
    highlight: HighlightStats,
    history: HistoryStats,
}

impl Report {
    /// Distinct paths that were ever part of the diff.
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
                // The measured plateau, printed beside the gate's own figure
                // and never inside it.
                match (drift.settled, drift.settled_at, drift.settled_ratio) {
                    (Some(at), Some(elapsed), Some(ratio)) => println!(
                        "soak: rss plateau: settled at sample {} of {} ({} in), drift from there {:.2}% (reported, not gated)",
                        at,
                        self.samples.len(),
                        span(elapsed),
                        ratio * 100.0,
                    ),
                    // One prefix across both, because the parent asserts this
                    // line reaches a reader and an assertion that only matches
                    // the settled case would be satisfied by every long window
                    // and silently absent from every short one.
                    _ => {
                        println!("soak: rss plateau: none inside this window (reported, not gated)")
                    }
                }
                // The shape, and the gradient through it. `SPEC.md` §10's open
                // question about I3 is a *sign* disagreement between two runs,
                // and it was settled by hand off the series below the last time
                // anyone asked. Printing both is what makes the next long run
                // answer it from its own report.
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
const SPARSE_EVERY: usize = 8;

/// What the reader does, on a cycle, so no sample is taken in the cheapest
/// state.
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
    highlighter
        .warm_ahead(
            worktree.workdir().to_path_buf(),
            frame
                .files()
                .iter()
                .take(vigia_core::WARM_FILES)
                .map(|change| change.path.clone())
                .collect(),
            None,
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
            let chrome = app.chrome(NAME, None, Pointing::default(), 0, "");
            let height = diff_height(area, &chrome, frame.files().len(), frame.files().len());
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
        let frame_began = Instant::now();
        app.sample_memory();
        let chrome = app.chrome(NAME, None, Pointing::default(), 0, "");
        body = body_layout(area, &chrome, frame.files().len(), frame.files().len());
        match app.view(&mut frame, &mut highlighter, &history, body) {
            Ok(fresh) => {
                view = fresh;
                // Every hunk that put a line on this screen, which is what the
                // highlighter was asked for, plus the ones it is allowed to keep
                // for a reader who scrolls back. One more than the headers drawn,
                // because the top of the screen can sit inside a hunk whose
                // header is above it, and never more: a hunk with no line on
                // screen is never asked for at all.
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
        render(&mut buffer, area, &view, &theme, Glyphs::default(), &chrome);
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
const MIN_FRAMES: u64 = 40;
const MIN_TICKS: u64 = 40;
const MIN_ROUNDS: u64 = 50;

impl Report {
    /// Every claim I3 makes that this process can see.
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

        // Its own fraction rather than the RSS baseline's, and the reason both
        // exist is on [`FD_WARMUP_FRACTION`].
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
            "I3: RSS drifted {:.2}% over {:?}, over the {:.0}% budget: quarters {:.2}, {:.2}, {:.2}, {:.2} MiB at {:+.2} MiB/h, peak {:.2} MiB over {} frames, from the {:.0}% baseline at sample {}. {}",
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
            WARMUP_FRACTION * 100.0,
            drift.warm,
            match (drift.settled, drift.settled_ratio) {
                (Some(at), Some(ratio)) => format!(
                    "The series settled at sample {at} and drifts {:.2}% from there, so read this breach against that before calling it a leak",
                    ratio * 100.0
                ),
                _ => "The series never settled inside this window, so there is no plateau to read the breach against"
                    .to_owned(),
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
        // The measured plateau, which `SPEC.md` §7 says is printed beside the
        // gate's own figure. A number nobody can see is the same wish the
        // quarters were before this loop existed.
        "soak: rss plateau: ",
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

/// No message this file prints carries a run of collapsed indentation.
#[test]
fn no_message_in_this_file_prints_collapsed_indentation() {
    // `CARGO_MANIFEST_DIR` rather than `file!()`, which is relative to the
    // workspace root while a test runs from the crate: the same join
    // [`workflow`] already makes for the same reason.
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/soak.rs");
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read this file's own source at {}: {e}", path.display()));
    let exempt = ["run: |", "strip_prefix", "starts_with("];
    // Built rather than written, or this line is its own first offender.
    let run = " ".repeat(3);

    let offenders: Vec<(usize, &str)> = source
        .lines()
        .enumerate()
        .filter(|(_, line)| !exempt.iter().any(|k| line.contains(k)))
        .filter(|(_, line)| {
            // Between the first and last quote on the line, which is coarse and
            // enough: the literals this catches are all message text.
            match (line.find('"'), line.rfind('"')) {
                (Some(open), Some(close)) if close > open => line[open..close].contains(&run),
                _ => false,
            }
        })
        .map(|(at, line)| (at + 1, line.trim()))
        .collect();

    assert!(
        offenders.is_empty(),
        "{} message(s) in this file carry a run of three or more spaces, which is what a backslash continuation leaves behind when its literal is rewritten onto one line:
{}",
        offenders.len(),
        offenders
            .iter()
            .map(|(at, line)| format!("  {at}: {}", &line[..line.len().min(120)]))
            .collect::<Vec<_>>()
            .join("
")
    );
}

/// The soak workflow, at `.github/workflows/soak.yml`.
const WORKFLOW: &str = "../../.github/workflows/soak.yml";

/// Every setting in the workflow, by key, ignoring comments.
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
#[test]
fn the_soak_workflow_cannot_kill_the_window_it_offers() {
    let (path, source) = workflow();

    // The window and the timeout each read the output that carries it, rather
    // than merely reading *some* output: swapping the two is a mutation the
    // weaker form could not see.
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
    fn shrinking() -> Vec<u64> {
        (0..288).map(|at| mb(200.0 - 0.2 * at as f64)).collect()
    }

    /// The leak the gate is calibrated on, sized just over the budget rather
    /// than absurdly over it.
    fn leaking() -> Vec<u64> {
        (0..288)
            .map(|at| mb(100.0 * (1.0 + 0.0005 * at as f64)))
            .collect()
    }

    /// A process that climbs hard and reaches its plateau inside the floor.
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
    fn ramp(step: u64) -> Vec<u64> {
        (0..288).map(|at| mb(60.0) + at as u64 * step).collect()
    }

    /// The step this module's ramps take: see [`ramp`].
    const RAMP_STEP: u64 = 12288;

    /// The interval a full-length run samples at.
    const SAMPLE: Duration = Duration::from_secs(300);

    /// A bare RSS series against the clock it would have been sampled on.
    fn sampled(series: &[u64]) -> Vec<(Duration, u64)> {
        series
            .iter()
            .enumerate()
            .map(|(at, &rss)| (SAMPLE * at as u32, rss))
            .collect()
    }

    /// The verdict over a series that is long enough to have one.
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

            let warm = warmup_floor(series.len());
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
    #[test]
    fn the_middle_quarters_are_measured_inward_from_the_ends() {
        // No outlier: see above.
        let series = ramp(RAMP_STEP);
        let warm = warmup_floor(series.len());
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
            assert_eq!(
                drift.slope,
                mib_per_hour(&sampled(&series[drift.warm..])),
                "over {shape}, the gradient is not fitted past the baseline the \
                 gate used"
            );
        }
    }

    /// The gradient, in the units the report prints and against a series whose
    /// answer is known by construction.
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
    #[test]
    fn the_recorded_day_breaches_the_gate_and_the_report_explains_why() {
        let series = day_long();
        let drift = verdict(&series);

        assert!(
            drift.ratio >= DRIFT_BUDGET,
            "the recorded day reported {:.2}% from the {:.0}% floor, inside the {:.0}% budget, so the breach this whole issue exists to explain is not in this fixture and nothing below proves anything",
            drift.ratio * 100.0,
            WARMUP_FRACTION * 100.0,
            DRIFT_BUDGET * 100.0
        );
        assert_eq!(
            drift.warm,
            warmup_floor(series.len()),
            "the gate's baseline moved off the fraction, which is the one thing #126's ruling says it must not do"
        );

        let (Some(at), Some(settled)) = (drift.settled, drift.settled_ratio) else {
            panic!(
                "the recorded day reports no plateau at all, so a reader of its breach gets the 13.26% and nothing to read it against"
            );
        };
        assert!(
            settled < DRIFT_BUDGET,
            "the recorded day settles at sample {at} and reports {:.2}% from there, over the {:.0}% budget: the annotation is supposed to be what shows the breach is a baseline on a warmup ramp rather than a leak, and this one shows nothing",
            settled * 100.0,
            DRIFT_BUDGET * 100.0
        );
        assert!(
            at > drift.warm,
            "the plateau is reported at sample {at}, at or before the gate's own baseline at {}, so the two figures describe the same window and the annotation is empty",
            drift.warm
        );
    }

    /// The annotation must never talk a step down.
    #[test]
    fn the_report_never_annotates_a_step_away() {
        for (at, after) in [
            (72, 36.0),
            (100, 36.0),
            (144, 36.0),
            (200, 36.0),
            (100, 31.8),
        ] {
            let series: Vec<u64> = (0..288)
                .map(|sample| if sample < at { mb(30.0) } else { mb(after) })
                .collect();
            let drift = verdict(&series);

            assert!(
                drift.ratio >= DRIFT_BUDGET,
                "a step to {after:.1} MiB at sample {at} reported {:.2}% drift from the {:.0}% floor, inside the {:.0}% budget, so the gate this issue leaves untouched has stopped catching a step",
                drift.ratio * 100.0,
                WARMUP_FRACTION * 100.0,
                DRIFT_BUDGET * 100.0
            );
            if let Some(settled) = drift.settled_ratio {
                assert!(
                    settled >= DRIFT_BUDGET,
                    "a step to {after:.1} MiB at sample {at} breaches at {:.2}% and the report annotates it with {:.2}% from its plateau, inside the budget: a reader is being told a step is a warmup artifact, which is the one thing this annotation must never say",
                    drift.ratio * 100.0,
                    settled * 100.0
                );
            }
        }
    }

    /// A platform that never reported RSS gets no verdict, rather than a ratio
    /// against nothing.
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
}
